//! Tauri backend: exposes the signcut-core library to the UI.

use serde::{Deserialize, Serialize};
use signcut_core::drivers::{load_driver_source, Catalog, MachineProfile, ManufacturerSummary};
use signcut_core::encode::{encode, test_cut_paths, test_feed, EncodeOptions};
use signcut_core::fonts::{FontFamilyInfo, FontLibrary};
use signcut_core::geometry::Affine;
use signcut_core::import::{self, text::TextRequest, ImportOptions, ImportedDesign};
use signcut_core::output::{self, Port, PortInfo, SendControl};
use signcut_core::plan::{plan, CutSettings, JobObject, Op, Placement, PlanStats};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Where SignCut Pro 2 keeps its driver pack on macOS.
const SIGNCUT_DRIVERS: &str = "/Applications/SignCutPro2.app/Contents/Resources/drivers.pak";

#[derive(Default)]
struct JobControl {
    cancel: Arc<AtomicBool>,
    /// Some(true) = resume, Some(false) = cancel, None = waiting.
    resume: Arc<(Mutex<Option<bool>>, Condvar)>,
}

struct AppState {
    fonts: Mutex<Option<FontLibrary>>,
    /// The app's own font library (fonts imported once by the user).
    fonts_dir: Mutex<Option<PathBuf>>,
    catalog: Mutex<Catalog>,
    job: Mutex<Option<JobControl>>,
}

impl AppState {
    fn fonts(&self) -> FontLibrary {
        let mut g = self.fonts.lock().unwrap();
        if g.is_none() {
            let mut lib = FontLibrary::system();
            if let Some(dir) = self.fonts_dir.lock().unwrap().clone() {
                lib.add_dir(&dir);
            }
            *g = Some(lib);
        }
        g.as_ref().unwrap().clone()
    }

    fn fonts_dir(&self) -> Res<PathBuf> {
        self.fonts_dir
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "App data folder is not available".to_string())
    }
}

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------- fonts

#[tauri::command]
async fn fonts_list(app: AppHandle) -> Res<Vec<FontFamilyInfo>> {
    blocking(move || Ok(app.state::<AppState>().fonts().families())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FontImportResult {
    families: Vec<String>,
    errors: Vec<String>,
}

/// Import font files into the app's own font library. They are copied, so
/// they stay available after restarts even if the originals are removed.
#[tauri::command]
async fn fonts_import(app: AppHandle, paths: Vec<String>) -> Res<FontImportResult> {
    blocking(move || {
        let st = app.state::<AppState>();
        let dir = st.fonts_dir()?;
        let mut lib = st.fonts();
        let mut families = Vec::new();
        let mut errors = Vec::new();
        for p in paths {
            match signcut_core::fonts::install_font_file(&PathBuf::from(&p), &dir) {
                Ok(files) => {
                    for f in files {
                        match lib.add_font_file(&f) {
                            Ok(mut fams) => families.append(&mut fams),
                            Err(e) => errors.push(e),
                        }
                    }
                }
                Err(e) => errors.push(e),
            }
        }
        *st.fonts.lock().unwrap() = Some(lib);
        families.sort();
        families.dedup();
        Ok(FontImportResult { families, errors })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportedFontFile {
    file: String,
    families: Vec<String>,
}

#[tauri::command]
async fn fonts_imported(app: AppHandle) -> Res<Vec<ImportedFontFile>> {
    blocking(move || {
        let st = app.state::<AppState>();
        let dir = st.fonts_dir()?;
        Ok(st
            .fonts()
            .families_in_dir(&dir)
            .into_iter()
            .map(|(file, families)| ImportedFontFile { file, families })
            .collect())
    })
    .await
}

#[tauri::command]
async fn fonts_remove(app: AppHandle, file: String) -> Res<()> {
    blocking(move || {
        let st = app.state::<AppState>();
        let dir = st.fonts_dir()?;
        // Only plain file names inside our own folder.
        if file.contains('/') || file.contains("..") {
            return Err("Invalid font file name".into());
        }
        std::fs::remove_file(dir.join(&file)).map_err(|e| e.to_string())?;
        // Rebuild the index without the removed font.
        *st.fonts.lock().unwrap() = None;
        let _ = st.fonts();
        Ok(())
    })
    .await
}

#[tauri::command]
fn fonts_folder(state: State<AppState>) -> Res<String> {
    let dir = state.fonts_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

// ---------------------------------------------------------------- import

#[tauri::command]
async fn import_file(app: AppHandle, path: String, options: Option<ImportOptions>) -> Res<ImportedDesign> {
    blocking(move || {
        let fonts = app.state::<AppState>().fonts();
        import::import_file(&PathBuf::from(&path), &fonts, &options.unwrap_or_default())
    })
    .await
}

#[tauri::command]
async fn text_render(app: AppHandle, request: TextRequest) -> Res<ImportedDesign> {
    blocking(move || {
        let fonts = app.state::<AppState>().fonts();
        import::text::render_text(&request, &fonts)
    })
    .await
}

#[tauri::command]
fn supported_extensions() -> Vec<&'static str> {
    import::SUPPORTED_EXTENSIONS.to_vec()
}

// ---------------------------------------------------------------- machines

#[tauri::command]
fn machines_list(state: State<AppState>) -> Vec<ManufacturerSummary> {
    state.catalog.lock().unwrap().summaries()
}

#[tauri::command]
fn machine_profile(state: State<AppState>, manufacturer: String, model: String) -> Res<MachineProfile> {
    state
        .catalog
        .lock()
        .unwrap()
        .profile(&manufacturer, &model)
        .ok_or_else(|| format!("Unknown cutter {manufacturer} {model}"))
}

#[tauri::command]
fn drivers_load(state: State<AppState>, path: String) -> Res<usize> {
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let drivers = load_driver_source(&data)?;
    Ok(state.catalog.lock().unwrap().merge(drivers))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignCutInstall {
    found: bool,
    models_loaded: usize,
}

/// If SignCut Pro 2 is installed, load its (possibly newer) driver pack.
#[tauri::command]
fn drivers_detect_signcut(state: State<AppState>) -> SignCutInstall {
    match std::fs::read(SIGNCUT_DRIVERS)
        .ok()
        .and_then(|d| load_driver_source(&d).ok())
    {
        Some(drivers) => SignCutInstall {
            found: true,
            models_loaded: state.catalog.lock().unwrap().merge(drivers),
        },
        None => SignCutInstall {
            found: false,
            models_loaded: 0,
        },
    }
}

// ---------------------------------------------------------------- ports

#[tauri::command]
async fn ports_list(app: AppHandle) -> Res<Vec<PortInfo>> {
    blocking(move || {
        let catalog = app.state::<AppState>().catalog.lock().unwrap().clone();
        let known = move |vid: u16, pid: u16| !catalog.find_by_usb(vid, pid).is_empty();
        Ok(output::list_ports(&known))
    })
    .await
}

// ---------------------------------------------------------------- jobs

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobRequest {
    objects: Vec<JobObject>,
    settings: CutSettings,
    encode: EncodeOptions,
    manufacturer: String,
    model: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobPreview {
    stats: PlanStats,
    /// Cut polylines in cutter mm, in cutting order.
    cuts: Vec<Vec<[f32; 2]>>,
    bytes: usize,
    /// Beginning of the plot data, for the curious.
    data_head: String,
    pauses: Vec<String>,
}

fn build(state: &AppState, req: &JobRequest) -> Res<(signcut_core::plan::Plan, signcut_core::encode::Encoded)> {
    let profile = state
        .catalog
        .lock()
        .unwrap()
        .profile(&req.manufacturer, &req.model)
        .ok_or_else(|| format!("Unknown cutter {} {}", req.manufacturer, req.model))?;
    let mut settings = req.settings.clone();
    if !profile.use_knife_compensation {
        settings.use_blade_offset = false;
    }
    let p = plan(&req.objects, &settings, Some(profile.max_width_mm));
    let mut eopts = req.encode.clone();
    eopts.after_cut = settings.after_cut;
    eopts.feed_extra = settings.feed_extra;
    let enc = encode(&p, &profile, &eopts);
    Ok((p, enc))
}

#[tauri::command]
async fn job_preview(app: AppHandle, job: JobRequest) -> Res<JobPreview> {
    blocking(move || {
        let st = app.state::<AppState>();
        let (p, enc) = build(&st, &job)?;
        let cuts = p
            .ops
            .iter()
            .filter_map(|o| match o {
                Op::Cut { pts, .. } => Some(pts.iter().map(|q| [q.x as f32, q.y as f32]).collect()),
                _ => None,
            })
            .collect();
        let all = enc.concat();
        Ok(JobPreview {
            stats: p.stats,
            cuts,
            bytes: enc.total_bytes,
            data_head: String::from_utf8_lossy(&all[..all.len().min(1500)]).to_string(),
            pauses: enc.pauses,
        })
    })
    .await
}

#[tauri::command]
async fn job_export(app: AppHandle, job: JobRequest, path: String) -> Res<usize> {
    blocking(move || {
        let st = app.state::<AppState>();
        let (_, enc) = build(&st, &job)?;
        let data = enc.concat();
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        Ok(data.len())
    })
    .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    sent: usize,
    total: usize,
    chunk: usize,
    chunks: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent {
    state: String, // "pause" | "done" | "error" | "cancelled"
    message: String,
}

fn spawn_send(app: AppHandle, port: Port, chunks: Vec<Vec<u8>>, pauses: Vec<String>) -> Res<()> {
    let st = app.state::<AppState>();
    let mut job = st.job.lock().unwrap();
    if job.is_some() {
        return Err("A job is already being sent".into());
    }
    let ctl = JobControl::default();
    let cancel = ctl.cancel.clone();
    let resume = ctl.resume.clone();
    *job = Some(ctl);
    drop(job);
    std::thread::spawn(move || {
        let n = chunks.len();
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        let mut sent_before = 0;
        let mut result: Result<(), String> = Ok(());
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                let msg = pauses.get(i - 1).cloned().unwrap_or_default();
                let _ = app.emit("cut-status", StatusEvent { state: "pause".into(), message: msg });
                let (lock, cv) = &*resume;
                let mut g = lock.lock().unwrap();
                *g = None;
                while g.is_none() {
                    g = cv.wait(g).unwrap();
                }
                if *g == Some(false) {
                    result = Err("Cancelled".into());
                    break;
                }
            }
            let app2 = app.clone();
            let mut progress = |sent: usize, _t: usize| {
                let _ = app2.emit(
                    "cut-progress",
                    ProgressEvent { sent: sent_before + sent, total, chunk: i, chunks: n },
                );
            };
            let mut ctl = SendControl { cancel: &cancel, progress: &mut progress };
            if let Err(e) = output::send(&port, chunk, &mut ctl) {
                result = Err(e);
                break;
            }
            sent_before += chunk.len();
        }
        let ev = match result {
            Ok(()) => StatusEvent { state: "done".into(), message: String::new() },
            Err(e) if e == "Cancelled" => StatusEvent { state: "cancelled".into(), message: e },
            Err(e) => StatusEvent { state: "error".into(), message: e },
        };
        *app.state::<AppState>().job.lock().unwrap() = None;
        let _ = app.emit("cut-status", ev);
    });
    Ok(())
}

#[tauri::command]
async fn job_start(app: AppHandle, job: JobRequest, port: Port) -> Res<usize> {
    let app2 = app.clone();
    let enc = blocking(move || {
        let st = app2.state::<AppState>();
        Ok(build(&st, &job)?.1)
    })
    .await?;
    let total = enc.total_bytes;
    spawn_send(app, port, enc.chunks, enc.pauses)?;
    Ok(total)
}

#[tauri::command]
fn job_resume(state: State<AppState>, proceed: bool) {
    if let Some(j) = state.job.lock().unwrap().as_ref() {
        let (lock, cv) = &*j.resume;
        *lock.lock().unwrap() = Some(proceed);
        cv.notify_all();
        if !proceed {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }
}

#[tauri::command]
fn job_cancel(state: State<AppState>) {
    if let Some(j) = state.job.lock().unwrap().as_ref() {
        j.cancel.store(true, Ordering::Relaxed);
        let (lock, cv) = &*j.resume;
        *lock.lock().unwrap() = Some(false);
        cv.notify_all();
    }
}

/// Cut a small test pattern at `position` (sheet mm, top-left).
#[tauri::command]
async fn test_cut(app: AppHandle, job: JobRequest, port: Port, size: f64, position: [f64; 2]) -> Res<()> {
    let mut job = job;
    job.objects = vec![JobObject {
        paths: test_cut_paths(size),
        transform: Affine::translate(position[0], position[1]),
    }];
    job.settings.copies = 1;
    job.settings.weed_border = None;
    job.settings.mirror = false;
    job.settings.placement = Placement::AsPlaced;
    job.settings.layers.clear();
    let app2 = app.clone();
    let enc = blocking(move || Ok(build(&app2.state::<AppState>(), &job)?.1)).await?;
    spawn_send(app, port, enc.chunks, enc.pauses)
}

#[tauri::command]
async fn test_feed_cmd(app: AppHandle, manufacturer: String, model: String, port: Port, mm: f64) -> Res<()> {
    let profile = app
        .state::<AppState>()
        .catalog
        .lock()
        .unwrap()
        .profile(&manufacturer, &model)
        .ok_or("Unknown cutter")?;
    let data = test_feed(&profile, mm);
    spawn_send(app, port, vec![data], vec![])
}

// ---------------------------------------------------------------- documents

#[tauri::command]
fn doc_save(path: String, content: String) -> Res<()> {
    std::fs::write(&path, content).map_err(|e| format!("Cannot save {path}: {e}"))
}

#[tauri::command]
fn doc_load(path: String) -> Res<String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Cannot open {path}: {e}"))
}

/// Paths of files passed on the command line / via "Open With".
#[tauri::command]
fn startup_files() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-') && std::path::Path::new(a).is_file())
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            fonts: Mutex::new(None),
            fonts_dir: Mutex::new(None),
            catalog: Mutex::new(Catalog::builtin()),
            job: Mutex::new(None),
        })
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                *app.state::<AppState>().fonts_dir.lock().unwrap() = Some(dir.join("fonts"));
            }
            // Index fonts in the background so the first import is fast.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let _ = handle.state::<AppState>().fonts();
                let _ = handle.emit("fonts-ready", ());
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fonts_list,
            fonts_import,
            fonts_imported,
            fonts_remove,
            fonts_folder,
            import_file,
            text_render,
            supported_extensions,
            machines_list,
            machine_profile,
            drivers_load,
            drivers_detect_signcut,
            ports_list,
            job_preview,
            job_export,
            job_start,
            job_resume,
            job_cancel,
            test_cut,
            test_feed_cmd,
            doc_save,
            doc_load,
            startup_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SignCut Port");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON shape produced by the UI (see src/components/CutDialog.tsx defaults).
    const UI_JOB: &str = r##"{
      "objects": [{"paths": [{"d": "M0 0L10 0L10 10L0 10Z", "color": "#000000"}],
                   "transform": [1, 0, 0, 1, 5, 600]}],
      "settings": {"materialWidth": 630, "bladeOffset": 0.25, "useBladeOffset": true, "tangentialEmulation": true,
        "overcut": 1, "passes": 1, "speed": null, "force": null, "tool": 1, "sort": "nearest", "bandWidth": 200,
        "insideFirst": true, "mirror": false, "placement": "asPlaced", "margin": 0, "weedBorder": null,
        "copies": 1, "copyGap": 5, "stackCopies": true, "afterCut": "returnToOrigin", "feedExtra": 50,
        "curveTolerance": 0.05, "layers": [["#000000", {"enabled": true, "pauseBefore": false}]]},
      "encode": {"sendSpeedForce": true, "swapXy": null, "afterCut": "feedPastJob", "feedExtra": 50, "sendPageFeed": false},
      "manufacturer": "VEVOR", "model": "VEVOR KH-720"
    }"##;

    #[test]
    fn ui_job_json_round_trip() {
        let job: JobRequest = serde_json::from_str(UI_JOB).unwrap();
        let state = AppState {
            fonts: Mutex::new(None),
            fonts_dir: Mutex::new(None),
            catalog: Mutex::new(Catalog::builtin()),
            job: Mutex::new(None),
        };
        let (plan, enc) = build(&state, &job).unwrap();
        assert_eq!(plan.stats.paths, 1);
        let out = String::from_utf8(enc.concat()).unwrap();
        assert!(out.starts_with(";:H A L0 ECN U "), "{out}");
        // Square at sheet y 600..610 on 630 mm material -> cutter Y 20..30 mm = 800..1200 units.
        eprintln!("{out}");
        // Every pen-down point lies within the blade-offset-expanded square.
        for tok in out.split(' ').filter(|t| t.starts_with('D') && t.contains(',')) {
            let (x, y) = tok[1..].split_once(',').unwrap();
            let (x, y): (i64, i64) = (x.parse().unwrap(), y.parse().unwrap());
            assert!((190..=610).contains(&x) && (790..=1210).contains(&y), "{tok}");
        }
        // Returns to the origin (the dialog setting wins over the encode default) and closes DMPL.
        assert!(out.contains(" U0,0 @"), "{out}");
        assert!(out.trim_end().ends_with("@"), "{out}");
    }

    #[test]
    fn ui_text_request_json() {
        let r: TextRequest = serde_json::from_str(
            r#"{"text":"Hi","family":"Helvetica","weight":700,"italic":false,"sizeMm":50,"letterSpacingMm":0,"lineHeight":1.2,"align":"start"}"#,
        )
        .unwrap();
        assert_eq!(r.weight, 700);
        assert_eq!(r.size_mm, 50.0);
    }
}
