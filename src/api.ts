// Typed wrappers around the Tauri backend. When the UI runs in a plain
// browser (UI development, screenshots) a small mock backend is used.

import type {
  CutProgress,
  CutStatus,
  FontFamilyInfo,
  ImportedDesign,
  ImportOptions,
  JobPreview,
  JobRequest,
  MachineProfile,
  ManufacturerSummary,
  Port,
  PortInfo,
  TextRequest,
} from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  const { mockInvoke } = await import("./mock/mock");
  return mockInvoke(cmd, args) as Promise<T>;
}

export const api = {
  fontsList: () => call<FontFamilyInfo[]>("fonts_list"),
  fontsAddFile: (path: string) => call<string[]>("fonts_add_file", { path }),
  importFile: (path: string, options?: ImportOptions) =>
    call<ImportedDesign>("import_file", { path, options: options ?? null }),
  textRender: (request: TextRequest) => call<ImportedDesign>("text_render", { request }),
  supportedExtensions: () => call<string[]>("supported_extensions"),
  machinesList: () => call<ManufacturerSummary[]>("machines_list"),
  machineProfile: (manufacturer: string, model: string) =>
    call<MachineProfile>("machine_profile", { manufacturer, model }),
  driversLoad: (path: string) => call<number>("drivers_load", { path }),
  driversDetectSignCut: () => call<{ found: boolean; modelsLoaded: number }>("drivers_detect_signcut"),
  portsList: () => call<PortInfo[]>("ports_list"),
  jobPreview: (job: JobRequest) => call<JobPreview>("job_preview", { job }),
  jobExport: (job: JobRequest, path: string) => call<number>("job_export", { job, path }),
  jobStart: (job: JobRequest, port: Port) => call<number>("job_start", { job, port }),
  jobResume: (proceed: boolean) => call<void>("job_resume", { proceed }),
  jobCancel: () => call<void>("job_cancel"),
  testCut: (job: JobRequest, port: Port, size: number, position: [number, number]) =>
    call<void>("test_cut", { job, port, size, position }),
  testFeed: (manufacturer: string, model: string, port: Port, mm: number) =>
    call<void>("test_feed_cmd", { manufacturer, model, port, mm }),
  docSave: (path: string, content: string) => call<void>("doc_save", { path, content }),
  docLoad: (path: string) => call<string>("doc_load", { path }),
  startupFiles: () => call<string[]>("startup_files"),
};

export async function onCutProgress(cb: (p: CutProgress) => void) {
  return listen<CutProgress>("cut-progress", cb);
}
export async function onCutStatus(cb: (s: CutStatus) => void) {
  return listen<CutStatus>("cut-status", cb);
}
export async function onFontsReady(cb: () => void) {
  return listen<null>("fonts-ready", () => cb());
}

async function listen<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (e) => cb(e.payload));
  }
  const { mockListen } = await import("./mock/mock");
  return mockListen(event, cb as (p: unknown) => void);
}

/** Subscribe to files dropped onto the window. */
export async function onFileDrop(cb: (paths: string[]) => void, hover?: (over: boolean) => void) {
  if (!isTauri) return () => {};
  const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  return getCurrentWebviewWindow().onDragDropEvent((e) => {
    if (e.payload.type === "drop") {
      hover?.(false);
      cb(e.payload.paths);
    } else if (e.payload.type === "enter" || e.payload.type === "over") hover?.(true);
    else hover?.(false);
  });
}

export const dialogs = {
  async openFiles(title: string, extensions: string[], multiple = true): Promise<string[]> {
    if (!isTauri) return ["/demo/sample.svg"];
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({
      title,
      multiple,
      filters: [{ name: title, extensions }],
    });
    if (!r) return [];
    return Array.isArray(r) ? r : [r];
  },
  async saveFile(title: string, defaultPath: string, extensions: string[]): Promise<string | null> {
    if (!isTauri) return `/demo/${defaultPath}`;
    const { save } = await import("@tauri-apps/plugin-dialog");
    return save({ title, defaultPath, filters: [{ name: title, extensions }] });
  },
  async message(text: string, kind: "info" | "warning" | "error" = "info") {
    if (!isTauri) {
      window.alert(text);
      return;
    }
    const { message } = await import("@tauri-apps/plugin-dialog");
    await message(text, { kind, title: "SignCut Port" });
  },
  async confirm(text: string, okLabel = "OK"): Promise<boolean> {
    if (!isTauri) return window.confirm(text);
    const { ask } = await import("@tauri-apps/plugin-dialog");
    return ask(text, { title: "SignCut Port", okLabel, cancelLabel: "Cancel" });
  },
};
