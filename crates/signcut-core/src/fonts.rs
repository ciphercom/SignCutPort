//! Font discovery and robust font-name resolution.
//!
//! The original SignCut silently substitutes fonts it cannot match, which is
//! why SVGs with custom fonts come out "wonky" on macOS. Here we:
//!
//! * load every font location macOS uses (system, user, network, Adobe Fonts
//!   activated through Creative Cloud, fonts registered via CoreText), plus
//!   legacy `.dfont` files and resource-fork font suitcases;
//! * match names the way design tools write them: family name, PostScript
//!   name (`MyFont-BoldItalic`, as Illustrator writes), "Family Style"
//!   combinations (`My Font Bold`, as Inkscape writes), case/space/hyphen
//!   insensitive;
//! * never substitute silently: every miss is reported back to the UI so the
//!   user can pick a replacement or load the font file.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use fontdb;
use fontdb::{Database, Family, Query, Source, Stretch, Style, Weight, ID};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFace {
    pub family: String,
    pub post_script_name: String,
    pub weight: u16,
    pub italic: bool,
    pub style_name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFamilyInfo {
    pub family: String,
    pub faces: Vec<FontFace>,
}

/// A font request that could not be satisfied.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontIssue {
    /// The family (or list) as requested by the document.
    pub requested: String,
    /// What was used instead, if anything.
    pub substituted_with: Option<String>,
    /// "missing-font" or "missing-glyph".
    pub kind: String,
    /// For missing glyphs: the characters affected.
    pub detail: Option<String>,
}

pub struct FontLibrary {
    db: Arc<Database>,
    index: Arc<NameIndex>,
}

impl Clone for FontLibrary {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            index: self.index.clone(),
        }
    }
}

#[derive(Default)]
struct NameIndex {
    /// normalized family name -> face ids
    family: HashMap<String, Vec<ID>>,
    /// normalized postscript name -> face id
    postscript: HashMap<String, ID>,
    /// canonical (display) family name for each face
    display_family: HashMap<ID, String>,
}

/// Lowercase and strip everything that is not alphanumeric, so
/// "My Font", "my-font", "MyFont" and "'My_Font'" all compare equal.
pub fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

const STYLE_WORDS: &[(&str, Option<u16>, Option<bool>)] = &[
    ("thin", Some(100), None),
    ("hairline", Some(100), None),
    ("extralight", Some(200), None),
    ("ultralight", Some(200), None),
    ("light", Some(300), None),
    ("book", Some(400), None),
    ("regular", Some(400), None),
    ("normal", Some(400), None),
    ("roman", Some(400), None),
    ("plain", Some(400), None),
    ("medium", Some(500), None),
    ("semibold", Some(600), None),
    ("demibold", Some(600), None),
    ("demi", Some(600), None),
    ("bold", Some(700), None),
    ("extrabold", Some(800), None),
    ("ultrabold", Some(800), None),
    ("heavy", Some(900), None),
    ("black", Some(900), None),
    ("italic", None, Some(true)),
    ("oblique", None, Some(true)),
    ("it", None, Some(true)),
];

/// Decompose a token like "semibolditalic" fully into style words.
fn decompose_style_token(tok: &str) -> Option<(Option<u16>, Option<bool>)> {
    if tok.is_empty() {
        return None;
    }
    let mut rest = tok;
    let mut weight = None;
    let mut italic = None;
    while !rest.is_empty() {
        // Longest style word that is a suffix of `rest`.
        let (w, wt, it) = STYLE_WORDS
            .iter()
            .filter(|(w, _, _)| rest.ends_with(w))
            .max_by_key(|(w, _, _)| w.len())?;
        if weight.is_none() {
            weight = *wt;
        }
        if it.is_some() {
            italic = *it;
        }
        rest = &rest[..rest.len() - w.len()];
    }
    Some((weight, italic))
}

/// Split "MyFont-BoldItalic" / "My Font Bold Italic" into ("myfont", weight, italic).
fn split_style_suffix(name: &str) -> Option<(String, Option<u16>, Option<bool>)> {
    let parts: Vec<String> = name
        .split(|c: char| c == ' ' || c == '-' || c == '_' || c == ',')
        .map(normalize_name)
        .filter(|s| !s.is_empty())
        .collect();
    let mut weight = None;
    let mut italic = None;
    let mut cut = parts.len();
    while cut > 1 {
        match decompose_style_token(&parts[cut - 1]) {
            Some((w, it)) => {
                if w.is_some() {
                    weight = w;
                }
                if it.is_some() {
                    italic = it;
                }
                cut -= 1;
            }
            None => break,
        }
    }
    if cut < parts.len() {
        return Some((parts[..cut].concat(), weight, italic));
    }
    // Concatenated without separators, e.g. "MyFontBold".
    let lower = normalize_name(name);
    for (w, wt, it) in STYLE_WORDS.iter().filter(|(w, _, _)| w.len() > 2) {
        if lower.len() > w.len() + 2 && lower.ends_with(w) {
            return Some((lower[..lower.len() - w.len()].to_string(), *wt, *it));
        }
    }
    None
}

impl FontLibrary {
    /// An empty library (useful for tests).
    pub fn empty() -> Self {
        Self::from_db(Database::new())
    }

    /// Load all fonts installed on this computer.
    pub fn system() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        for dir in extra_font_dirs() {
            db.load_fonts_dir(&dir);
            load_legacy_mac_fonts_in_dir(&mut db, &dir, 0);
        }
        for dir in standard_font_dirs() {
            load_legacy_mac_fonts_in_dir(&mut db, &dir, 0);
        }
        #[cfg(target_os = "macos")]
        macos::load_coretext_registered_fonts(&mut db);
        set_generic_families(&mut db);
        Self::from_db(db)
    }

    pub fn from_db(db: Database) -> Self {
        let index = Arc::new(build_index(&db));
        Self {
            db: Arc::new(db),
            index,
        }
    }

    pub fn db(&self) -> &Arc<Database> {
        &self.db
    }

    /// Add a font file (ttf/otf/ttc/dfont/suitcase). Returns the families added.
    pub fn add_font_file(&mut self, path: &Path) -> Result<Vec<String>, String> {
        let before: std::collections::HashSet<ID> = self.db.faces().map(|f| f.id).collect();
        let db = Arc::make_mut(&mut self.db);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "ttf" | "otf" | "ttc" | "otc" => db
                .load_font_file(path)
                .map_err(|e| format!("{}: {e}", path.display()))?,
            _ => {
                if load_legacy_mac_font(db, path) == 0 {
                    // Last resort: maybe it is an sfnt with an odd extension.
                    let data = std::fs::read(path).map_err(|e| e.to_string())?;
                    db.load_font_data(data);
                }
            }
        }
        let added: Vec<String> = self
            .db
            .faces()
            .filter(|f| !before.contains(&f.id))
            .filter_map(|f| f.families.first().map(|x| x.0.clone()))
            .collect();
        if added.is_empty() {
            return Err(format!(
                "No usable outline font found in {} (Type 1 / bitmap fonts are not supported)",
                path.display()
            ));
        }
        self.index = Arc::new(build_index(&self.db));
        Ok(dedup_sorted(added))
    }

    /// Load every font file in `dir` (the app's own font library).
    pub fn add_dir(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        Arc::make_mut(&mut self.db).load_fonts_dir(dir);
        self.index = Arc::new(build_index(&self.db));
    }

    /// Families provided by font files located inside `dir`, per file name.
    pub fn families_in_dir(&self, dir: &Path) -> Vec<(String, Vec<String>)> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        for f in self.db.faces() {
            let p = match &f.source {
                Source::File(p) => p.clone(),
                #[allow(unreachable_patterns)]
                Source::SharedFile(p, _) => p.clone(),
                _ => continue,
            };
            if !(p.starts_with(&canon) || p.starts_with(dir)) {
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let fam = self.index.display_family.get(&f.id).cloned().unwrap_or_default();
            let e = map.entry(name).or_default();
            if !e.contains(&fam) {
                e.push(fam);
            }
        }
        map.into_iter().collect()
    }

    /// Add font bytes (e.g. an @font-face embedded in an SVG).
    pub fn add_font_data(&mut self, data: Vec<u8>) -> Vec<String> {
        let before: std::collections::HashSet<ID> = self.db.faces().map(|f| f.id).collect();
        Arc::make_mut(&mut self.db).load_font_data(data);
        let added: Vec<String> = self
            .db
            .faces()
            .filter(|f| !before.contains(&f.id))
            .filter_map(|f| f.families.first().map(|x| x.0.clone()))
            .collect();
        self.index = Arc::new(build_index(&self.db));
        dedup_sorted(added)
    }

    /// All font families, sorted case-insensitively.
    pub fn families(&self) -> Vec<FontFamilyInfo> {
        let mut map: BTreeMap<String, FontFamilyInfo> = BTreeMap::new();
        for f in self.db.faces() {
            let Some(fam) = self.index.display_family.get(&f.id) else {
                continue;
            };
            let entry = map
                .entry(fam.to_lowercase())
                .or_insert_with(|| FontFamilyInfo {
                    family: fam.clone(),
                    faces: vec![],
                });
            let path = match &f.source {
                Source::File(p) => Some(p.display().to_string()),
                #[allow(unreachable_patterns)]
                Source::SharedFile(p, _) => Some(p.display().to_string()),
                _ => None,
            };
            if entry
                .faces
                .iter()
                .any(|x| x.post_script_name == f.post_script_name && !f.post_script_name.is_empty())
            {
                continue;
            }
            entry.faces.push(FontFace {
                family: fam.clone(),
                post_script_name: f.post_script_name.clone(),
                weight: f.weight.0,
                italic: f.style != Style::Normal,
                style_name: style_name(f.weight.0, f.style != Style::Normal),
                path,
            });
        }
        let mut v: Vec<FontFamilyInfo> = map.into_values().collect();
        for fam in &mut v {
            fam.faces.sort_by_key(|f| (f.italic, f.weight));
        }
        v
    }

    /// Resolve a single requested name to a face, using all matching strategies.
    pub fn resolve(&self, requested: &str, weight: u16, italic: bool) -> Option<ID> {
        resolve_in(&self.db, &self.index, requested, weight, italic)
    }

    pub fn family_of(&self, id: ID) -> Option<String> {
        self.index.display_family.get(&id).cloned()
    }

    /// Build a usvg font resolver that uses our matching rules, applies the
    /// user's substitutions and records every miss in `issues`.
    pub fn usvg_resolver(
        &self,
        substitutions: HashMap<String, String>,
        issues: Arc<Mutex<Vec<FontIssue>>>,
    ) -> usvg::FontResolver<'static> {
        let index = self.index.clone();
        let issues_fb = issues.clone();
        let index_fb = self.index.clone();
        let subs: HashMap<String, String> = substitutions
            .into_iter()
            .map(|(k, v)| (normalize_name(&k), v))
            .collect();
        let select_font: usvg::FontSelectionFn<'static> = Box::new(move |font, db| {
            let weight = font.weight();
            let italic = !matches!(font.style(), usvg::FontStyle::Normal);
            let mut requested_names = Vec::new();
            for fam in font.families() {
                match fam {
                    usvg::FontFamily::Named(name) => {
                        requested_names.push(name.clone());
                        let target = subs.get(&normalize_name(name)).cloned().unwrap_or(name.clone());
                        if let Some(id) = resolve_in(db, &index, &target, weight, italic) {
                            return Some(id);
                        }
                    }
                    generic => {
                        // A generic family is an explicit request for "any font
                        // of that kind": not an error.
                        let fam = match generic {
                            usvg::FontFamily::Serif => Family::Serif,
                            usvg::FontFamily::SansSerif => Family::SansSerif,
                            usvg::FontFamily::Cursive => Family::Cursive,
                            usvg::FontFamily::Fantasy => Family::Fantasy,
                            _ => Family::Monospace,
                        };
                        if let Some(id) = db.query(&Query {
                            families: &[fam],
                            weight: Weight(weight),
                            stretch: Stretch::Normal,
                            style: if italic { Style::Italic } else { Style::Normal },
                        }) {
                            return Some(id);
                        }
                    }
                }
            }
            // Nothing matched: fall back, but report it.
            let fallback = db.query(&Query {
                families: &[Family::SansSerif, Family::Serif],
                weight: Weight(weight),
                stretch: Stretch::Normal,
                style: if italic { Style::Italic } else { Style::Normal },
            });
            let fallback = fallback.or_else(|| db.faces().next().map(|f| f.id));
            let issue = FontIssue {
                requested: requested_names.join(", "),
                substituted_with: fallback.and_then(|id| index.display_family.get(&id).cloned()),
                kind: "missing-font".into(),
                detail: None,
            };
            let mut g = issues.lock().unwrap();
            if !g.contains(&issue) {
                g.push(issue);
            }
            fallback
        });
        let default_fallback = usvg::FontResolver::default_fallback_selector();
        let select_fallback: usvg::FallbackSelectionFn<'static> =
            Box::new(move |c, used, db| {
                let id = default_fallback(c, used, db);
                if !c.is_whitespace() && !c.is_control() {
                    let base = used.first().and_then(|u| index_fb.display_family.get(u).cloned());
                    let with = id.and_then(|i| index_fb.display_family.get(&i).cloned());
                    let mut g = issues_fb.lock().unwrap();
                    if let Some(existing) = g.iter_mut().find(|i| {
                        i.kind == "missing-glyph" && i.requested == base.clone().unwrap_or_default()
                    }) {
                        let d = existing.detail.get_or_insert_with(String::new);
                        if !d.contains(c) {
                            d.push(c);
                        }
                    } else {
                        g.push(FontIssue {
                            requested: base.unwrap_or_default(),
                            substituted_with: with,
                            kind: "missing-glyph".into(),
                            detail: Some(c.to_string()),
                        });
                    }
                }
                id
            });
        usvg::FontResolver {
            select_font,
            select_fallback,
        }
    }
}

fn dedup_sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

pub fn style_name(weight: u16, italic: bool) -> String {
    let w = match weight {
        0..=150 => "Thin",
        151..=250 => "ExtraLight",
        251..=350 => "Light",
        351..=450 => "Regular",
        451..=550 => "Medium",
        551..=650 => "SemiBold",
        651..=750 => "Bold",
        751..=850 => "ExtraBold",
        _ => "Black",
    };
    match (w, italic) {
        ("Regular", true) => "Italic".into(),
        (w, true) => format!("{w} Italic"),
        (w, false) => w.into(),
    }
}

fn build_index(db: &Database) -> NameIndex {
    let mut idx = NameIndex::default();
    for f in db.faces() {
        // Prefer the English family name, the first otherwise.
        let display = f
            .families
            .iter()
            .find(|(_, l)| *l == fontdb::Language::English_UnitedStates)
            .or_else(|| f.families.first())
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| f.post_script_name.clone());
        idx.display_family.insert(f.id, display);
        for (name, _) in &f.families {
            idx.family.entry(normalize_name(name)).or_default().push(f.id);
        }
        if !f.post_script_name.is_empty() {
            idx.postscript.insert(normalize_name(&f.post_script_name), f.id);
        }
    }
    idx
}

/// CSS Fonts Level 4 §5.2 weight matching: lower key = better match.
/// For a desired weight of 400–500, weights up to 500 are tried first
/// (ascending), then *lighter* ones (descending), then heavier ones; below
/// 400 lighter weights win, above 500 heavier weights win. This is what
/// WebKit/CoreText do, e.g. "normal" on a Light+Bold family picks Light.
fn css_weight_key(desired: u16, available: u16) -> (u8, u16) {
    let (d, a) = (desired, available);
    if a == d {
        return (0, 0);
    }
    if (400..=500).contains(&d) {
        if a > d && a <= 500 {
            (1, a - d)
        } else if a < d {
            (2, d - a)
        } else {
            (3, a - d)
        }
    } else if d < 400 {
        if a < d {
            (1, d - a)
        } else {
            (2, a - d)
        }
    } else if a > d {
        (1, a - d)
    } else {
        (2, d - a)
    }
}

fn best_face(db: &Database, ids: &[ID], weight: u16, italic: Option<bool>) -> Option<ID> {
    ids.iter()
        .filter_map(|id| db.face(*id))
        .min_by_key(|f| {
            // Prefer normal width, then the requested style (italic/oblique
            // count as equivalent), then CSS weight matching.
            let stretch_pen = u8::from(f.stretch != Stretch::Normal);
            let it = f.style != Style::Normal;
            let style_pen = match italic {
                Some(want) => u8::from(want != it),
                None => u8::from(it),
            };
            (stretch_pen, style_pen, css_weight_key(weight, f.weight.0))
        })
        .map(|f| f.id)
}

fn resolve_in(db: &Database, idx: &NameIndex, requested: &str, weight: u16, italic: bool) -> Option<ID> {
    // Font family lists may carry quotes.
    let req = requested.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if req.is_empty() {
        return None;
    }
    let n = normalize_name(req);
    // 1. Family name.
    if let Some(ids) = idx.family.get(&n) {
        if let Some(id) = best_face(db, ids, weight, Some(italic)) {
            return Some(id);
        }
    }
    // 2. Exact PostScript name ("MyFont-BoldItalic").
    if let Some(id) = idx.postscript.get(&n) {
        return Some(*id);
    }
    // 3. "Family Style" / "Family-Style" combinations.
    if let Some((fam, w, it)) = split_style_suffix(req) {
        if let Some(ids) = idx.family.get(&fam) {
            let w = w.unwrap_or(weight);
            let it = it.or(Some(italic));
            if let Some(id) = best_face(db, ids, w, it) {
                return Some(id);
            }
        }
        // PostScript families often drop spaces: "MyFont" for "My Font".
        if let Some((_, ids)) = idx.family.iter().find(|(k, _)| **k == fam) {
            return best_face(db, ids, w.unwrap_or(weight), it.or(Some(italic)));
        }
    }
    // 4. PostScript name with "-Regular"/"-Roman" etc. appended.
    for suffix in ["regular", "roman", "book", "normal", "plain"] {
        if let Some(id) = idx.postscript.get(&format!("{n}{suffix}")) {
            return Some(*id);
        }
    }
    None
}

fn set_generic_families(db: &mut Database) {
    let has = |db: &Database, name: &str| {
        let n = normalize_name(name);
        db.faces()
            .any(|f| f.families.iter().any(|(fam, _)| normalize_name(fam) == n))
    };
    let pick = |db: &Database, cands: &[&str]| cands.iter().find(|c| has(db, c)).map(|s| s.to_string());
    if let Some(f) = pick(db, &["Helvetica", "Helvetica Neue", "Arial", "DejaVu Sans", "Liberation Sans", "Noto Sans"]) {
        db.set_sans_serif_family(f);
    }
    if let Some(f) = pick(db, &["Times", "Times New Roman", "DejaVu Serif", "Liberation Serif", "Noto Serif"]) {
        db.set_serif_family(f);
    }
    if let Some(f) = pick(db, &["Menlo", "Courier", "Courier New", "DejaVu Sans Mono", "Liberation Mono"]) {
        db.set_monospace_family(f);
    }
    if let Some(f) = pick(db, &["Apple Chancery", "Snell Roundhand", "Comic Sans MS", "URW Chancery L"]) {
        db.set_cursive_family(f);
    }
    if let Some(f) = pick(db, &["Papyrus", "Impact", "Comic Sans MS"]) {
        db.set_fantasy_family(f);
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn standard_font_dirs() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Network/Library/Fonts"),
    ];
    if let Some(h) = home() {
        v.push(h.join("Library/Fonts"));
    }
    v
}

/// Locations fontdb does not scan by itself.
fn extra_font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        // Adobe Fonts (Creative Cloud) activated fonts.
        v.push(h.join("Library/Application Support/Adobe/CoreSync/plugins/livetype/.r"));
        v.push(h.join("Library/Application Support/Adobe/CoreSync/plugins/livetype/r"));
        // Common font managers.
        v.push(h.join("Library/Application Support/FontBase/fonts"));
        v.push(h.join("Library/Application Support/RightFont"));
        // Linux user fonts (dev/test convenience).
        v.push(h.join(".fonts"));
        v.push(h.join(".local/share/fonts"));
    }
    v.push(PathBuf::from("/Library/Application Support/Adobe/Fonts"));
    v.retain(|p| p.is_dir());
    v
}

/// Recursively find `.dfont` files and extension-less suitcase files.
fn load_legacy_mac_fonts_in_dir(db: &mut Database, dir: &Path, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            load_legacy_mac_fonts_in_dir(db, &p, depth + 1);
        } else if ft.is_file() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase());
            match ext.as_deref() {
                Some("dfont") => {
                    load_legacy_mac_font(db, &p);
                }
                None | Some("suit") => {
                    if cfg!(target_os = "macos") {
                        load_legacy_mac_font(db, &p);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Copy a font file into the app's font library folder `dir` so it is
/// available on every start. Legacy `.dfont`/suitcase files are converted to
/// one `.ttf`/`.otf` per face. Returns the files written.
pub fn install_font_file(src: &Path, dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "font".into());
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let data = std::fs::read(src).map_err(|e| format!("Cannot read {}: {e}", src.display()))?;
    let is_sfnt = |d: &[u8]| ttf_parser::fonts_in_collection(d).is_some() || ttf_parser::Face::parse(d, 0).is_ok();
    let mut written = Vec::new();
    if matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") || is_sfnt(&data) {
        if !is_sfnt(&data) {
            return Err(format!("{} is not a valid font file", src.display()));
        }
        let ext = if ext.is_empty() { "ttf".to_string() } else { ext };
        let dst = dir.join(format!("{stem}.{ext}"));
        std::fs::write(&dst, &data).map_err(|e| e.to_string())?;
        written.push(dst);
    } else {
        let mut faces = extract_sfnt_resources(&data);
        if faces.is_empty() {
            let rsrc = PathBuf::from(format!("{}/..namedfork/rsrc", src.display()));
            if let Ok(r) = std::fs::read(rsrc) {
                faces = extract_sfnt_resources(&r);
            }
        }
        for (i, f) in faces.into_iter().enumerate() {
            if ttf_parser::Face::parse(&f, 0).is_err() {
                continue;
            }
            let ext = if f.starts_with(b"OTTO") { "otf" } else { "ttf" };
            let dst = dir.join(format!("{stem}-{i}.{ext}"));
            std::fs::write(&dst, &f).map_err(|e| e.to_string())?;
            written.push(dst);
        }
        if written.is_empty() {
            return Err(format!(
                "No usable outline font in {} (PostScript Type 1 and bitmap fonts are not supported)",
                src.display()
            ));
        }
    }
    Ok(written)
}

/// Load sfnt resources from a `.dfont` (data-fork resource file) or a classic
/// font suitcase (resource fork). Returns the number of faces loaded.
pub fn load_legacy_mac_font(db: &mut Database, path: &Path) -> usize {
    let mut count = 0;
    let mut candidates = vec![std::fs::read(path).ok()];
    // Resource fork (macOS only; harmless elsewhere).
    let rsrc = PathBuf::from(format!("{}/..namedfork/rsrc", path.display()));
    candidates.push(std::fs::read(rsrc).ok());
    for data in candidates.into_iter().flatten() {
        for sfnt in extract_sfnt_resources(&data) {
            if ttf_parser::Face::parse(&sfnt, 0).is_ok() {
                db.load_font_data(sfnt);
                count += 1;
            }
        }
        if count > 0 {
            break;
        }
    }
    count
}

/// Parse a Mac resource file and return all `sfnt` resources.
pub fn extract_sfnt_resources(data: &[u8]) -> Vec<Vec<u8>> {
    fn be32(d: &[u8], o: usize) -> Option<usize> {
        d.get(o..o + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn be16(d: &[u8], o: usize) -> Option<usize> {
        d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    }
    let mut out = Vec::new();
    let (Some(data_off), Some(map_off), Some(data_len), Some(map_len)) =
        (be32(data, 0), be32(data, 4), be32(data, 8), be32(data, 12))
    else {
        return out;
    };
    if data_off + data_len > data.len() || map_off + map_len > data.len() || map_len < 30 {
        return out;
    }
    let Some(type_list_off) = be16(data, map_off + 24) else {
        return out;
    };
    let tl = map_off + type_list_off;
    let Some(n_types) = be16(data, tl).map(|n| (n + 1) & 0xffff) else {
        return out;
    };
    for t in 0..n_types {
        let e = tl + 2 + t * 8;
        let Some(ty) = data.get(e..e + 4) else { break };
        let (Some(n_res), Some(ref_off)) = (be16(data, e + 4), be16(data, e + 6)) else {
            break;
        };
        if ty != b"sfnt" {
            continue;
        }
        for r in 0..=n_res {
            let re = tl + ref_off + r * 12;
            let Some(attr_off) = be32(data, re + 4) else { break };
            let off = data_off + (attr_off & 0x00ff_ffff);
            let Some(len) = be32(data, off) else { break };
            if let Some(bytes) = data.get(off + 4..off + 4 + len) {
                out.push(bytes.to_vec());
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod macos {
    use fontdb::Database;
    use std::collections::HashSet;

    /// Ask CoreText for every font file it knows about (this includes fonts
    /// activated by font managers from arbitrary folders) and load the ones
    /// fontdb has not already seen.
    pub fn load_coretext_registered_fonts(db: &mut Database) {
        let known: HashSet<std::path::PathBuf> = db
            .faces()
            .filter_map(|f| match &f.source {
                fontdb::Source::File(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        let collection = core_text::font_collection::create_for_all_families();
        let Some(descs) = collection.get_descriptors() else {
            return;
        };
        let mut seen = HashSet::new();
        for i in 0..descs.len() {
            let Some(d) = descs.get(i) else { continue };
            if let Some(path) = d.font_path() {
                if known.contains(&path) || !seen.insert(path.clone()) {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase());
                match ext.as_deref() {
                    Some("ttf" | "otf" | "ttc" | "otc") => {
                        let _ = db.load_font_file(&path);
                    }
                    _ => {
                        super::load_legacy_mac_font(db, &path);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(normalize_name("'My Font-Bold'"), "myfontbold");
    }

    #[test]
    fn split_styles() {
        assert_eq!(
            split_style_suffix("MyFont-BoldItalic"),
            Some(("myfont".into(), Some(700), Some(true)))
        );
        assert_eq!(
            split_style_suffix("Open Sans SemiBold"),
            Some(("opensans".into(), Some(600), None))
        );
        assert_eq!(split_style_suffix("Lobster"), None);
    }

    #[test]
    fn resource_fork_parse_empty() {
        assert!(extract_sfnt_resources(&[0u8; 10]).is_empty());
    }

    #[test]
    fn install_rejects_garbage() {
        let dir = std::env::temp_dir().join(format!("scp-fonts-{}", std::process::id()));
        let src = dir.join("junk.ttf");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&src, b"not a font").unwrap();
        assert!(install_font_file(&src, &dir.join("lib")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn install_and_load_system_font_copy() {
        let Some(src) = ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
        else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("scp-fontlib-{}", std::process::id()));
        let files = install_font_file(&src, &dir).unwrap();
        assert_eq!(files.len(), 1);
        let mut lib = FontLibrary::empty();
        lib.add_dir(&dir);
        let fams = lib.families_in_dir(&dir);
        assert_eq!(fams.len(), 1, "{fams:?}");
        assert!(lib.resolve(&fams[0].1[0], 400, false).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn css_weight_matching() {
        // "normal" on a Light + Bold family (e.g. Noteworthy) -> Light.
        let pick = |want: u16, have: &[u16]| *have.iter().min_by_key(|a| css_weight_key(want, **a)).unwrap();
        assert_eq!(pick(400, &[300, 700]), 300);
        assert_eq!(pick(400, &[500, 300]), 500);
        assert_eq!(pick(500, &[400, 600]), 400);
        assert_eq!(pick(700, &[300, 600]), 600);
        assert_eq!(pick(700, &[300, 800]), 800);
        assert_eq!(pick(300, &[400, 200]), 200);
        assert_eq!(pick(300, &[400, 700]), 400);
    }

    #[test]
    fn normal_weight_prefers_light_over_bold_face() {
        let dir = std::path::Path::new("/usr/share/fonts/opentype/inter");
        let (l, b) = (dir.join("Inter-Light.otf"), dir.join("Inter-Bold.otf"));
        if !l.exists() || !b.exists() {
            return;
        }
        let mut db = Database::new();
        db.load_font_file(&l).unwrap();
        db.load_font_file(&b).unwrap();
        let lib = FontLibrary::from_db(db);
        let id = lib.resolve("Inter", 400, false).unwrap();
        assert_eq!(lib.db().face(id).unwrap().weight.0, 300);
        let id = lib.resolve("Inter", 700, false).unwrap();
        assert_eq!(lib.db().face(id).unwrap().weight.0, 700);
    }
}
