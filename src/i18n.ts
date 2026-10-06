// Minimal i18n: English source strings are the keys (gettext style), so a
// missing translation falls back to readable English. `npm run check-i18n`
// verifies that every translated string has a German translation.

import { useSyncExternalStore } from "react";
import { de } from "./locales/de";

export type Lang = "en" | "de";
export type LangSetting = Lang | "system";

const dictionaries: Record<Lang, Record<string, string>> = { en: {}, de };

export const LANGUAGES: { value: LangSetting; label: string }[] = [
  { value: "system", label: "System" },
  { value: "en", label: "English" },
  { value: "de", label: "Deutsch" },
];

const LS_KEY = "signcut-port.lang";

function systemLang(): Lang {
  const langs = typeof navigator !== "undefined" ? (navigator.languages ?? [navigator.language]) : [];
  for (const l of langs) {
    const code = l.toLowerCase().slice(0, 2);
    if (code === "de") return "de";
    if (code === "en") return "en";
  }
  return "en";
}

function readSetting(): LangSetting {
  try {
    const v = localStorage.getItem(LS_KEY);
    if (v === "en" || v === "de" || v === "system") return v;
  } catch {
    /* ignore */
  }
  return "system";
}

let setting: LangSetting = readSetting();
let current: Lang = setting === "system" ? systemLang() : setting;
const listeners = new Set<() => void>();

export function getLang(): Lang {
  return current;
}
export function getLangSetting(): LangSetting {
  return setting;
}
export function setLangSetting(s: LangSetting) {
  setting = s;
  current = s === "system" ? systemLang() : s;
  try {
    localStorage.setItem(LS_KEY, s);
  } catch {
    /* ignore */
  }
  document.documentElement.lang = current;
  listeners.forEach((l) => l());
}

/** Re-render the calling component when the language changes. */
export function useLang(): Lang {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => current,
  );
}

/** Translate `s`, replacing `{name}` placeholders with `params`. */
export function t(s: string, params?: Record<string, string | number>): string {
  let out = dictionaries[current][s] ?? s;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      out = out.split(`{${k}}`).join(typeof v === "number" ? fmtNum(v) : v);
    }
  }
  return out;
}

/** Format a number with the language's decimal separator. */
export function fmtNum(n: number, digits?: number): string {
  const s = digits === undefined ? String(n) : n.toFixed(digits);
  return current === "de" ? s.replace(".", ",") : s;
}

/** Locale-aware integer with thousands separators. */
export function fmtInt(n: number): string {
  return n.toLocaleString(current === "de" ? "de-DE" : "en-US");
}

if (typeof document !== "undefined") document.documentElement.lang = current;

// ---------------------------------------------------------------------------
// Messages produced by the Rust backend are English; translate the known ones.

type Rule = [RegExp, string];

const backendRules: Rule[] = [
  [/^Nothing to cut$/, "Nothing to cut"],
  [
    /^Part of the job lies outside the material \(before the origin\); it will be clipped by the cutter\.$/,
    "Part of the job lies outside the material (before the origin); it will be clipped by the cutter.",
  ],
  [/^Job is ([\d.]+) mm wide but the material is only ([\d.]+) mm\.$/, "Job is {1} mm wide but the material is only {2} mm."],
  [/^Job exceeds the cutter's maximum cutting width of ([\d.]+) mm\.$/, "Job exceeds the cutter's maximum cutting width of {1} mm."],
  [/^Change tool or material for colour (.+)$/, "Change tool or material for colour {1}"],
  [
    /^(\d+) embedded bitmap image\(s\) ignored \(only vector outlines can be cut\)$/,
    "{1} embedded bitmap image(s) ignored (only vector outlines can be cut)",
  ],
  [/^Gradient\/pattern fills are cut as plain outlines$/, "Gradient/pattern fills are cut as plain outlines"],
  [/^Using embedded font\(s\): (.+)$/, "Using embedded font(s): {1}"],
  [/^No cuttable vector paths found in the file\.(.*)$/, "No cuttable vector paths found in the file.{1}"],
  [/^\.(\w+) files are not supported directly yet\..*$/s, ".{1} files are not supported directly yet. Export as SVG from your design program — SignCut Port keeps your installed fonts."],
  [/^Unsupported file type: \.(.*)$/, "Unsupported file type: .{1}"],
  [/^Invalid SVG: (.*)$/s, "Invalid SVG: {1}"],
  [/^Invalid DXF: (.*)$/s, "Invalid DXF: {1}"],
  [/^Compressed SVG \(\.svgz\) is not supported; please save as plain SVG$/, "Compressed SVG (.svgz) is not supported; please save as plain SVG"],
  [
    /^DXF text entities are skipped; convert text to curves in your CAD program or use the Text tool$/,
    "DXF text entities are skipped; convert text to curves in your CAD program or use the Text tool",
  ],
  [/^Cannot read (.+?): (.*)$/s, "Cannot read {1}: {2}"],
  [/^Text is empty$/, "Text is empty"],
  [/^No usable outline font (?:found )?in (.+?) \(.*not supported\)$/, "No usable outline font in {1} (PostScript Type 1 and bitmap fonts are not supported)"],
  [/^(.+) is not a valid font file$/, "{1} is not a valid font file"],
  [/^Cannot open (.+?): (.*)$/s, "Cannot open {1}: {2}"],
  [/^Cannot connect to (.+?): (.*)$/s, "Cannot connect to {1}: {2}"],
  [/^Cannot resolve (.+?)(?:: .*)?$/s, "Cannot resolve {1}"],
  [/^USB cutter (\S+) is not connected$/, "USB cutter {1} is not connected"],
  [
    /^Cannot claim the USB interface \((.*)\)\..*$/s,
    "Cannot claim the USB interface ({1}). If the cutter was added as a printer in macOS, choose its printer queue instead, or remove it from System Settings › Printers.",
  ],
  [/^The USB device has no bulk output endpoint$/, "The USB device has no bulk output endpoint"],
  [/^USB write failed: (.*)$/s, "USB write failed: {1}"],
  [/^Write failed: (.*)$/s, "Write failed: {1}"],
  [/^Cancelled$/, "Cancelled"],
  [/^Printing to (.+?) failed: (.*)$/s, "Printing to {1} failed: {2}"],
  [/^Cannot run lp: (.*)$/s, "Cannot run lp: {1}"],
  [/^A job is already being sent$/, "A job is already being sent"],
  [/^Unknown cutter (.*)$/, "Unknown cutter {1}"],
  [/^Cannot save (.+?): (.*)$/s, "Cannot save {1}: {2}"],
  [/^No driver definitions found in archive$/, "No driver definitions found in archive"],
  [/^not a <Driver> file$/, "not a <Driver> file"],
  [/^App data folder is not available$/, "App data folder is not available"],
  [/^Invalid font file name$/, "Invalid font file name"],
  [/^Not a SignCut Port document$/, "Not a SignCut Port document"],
];

/** Translate a message from the backend (falls back to the original text). */
export function tb(msg: unknown): string {
  const s = String(msg).replace(/^Error: /, "");
  // Multi-line / joined messages: translate each sentence group separately.
  for (const [re, tpl] of backendRules) {
    const m = s.match(re);
    if (m) {
      let out = t(tpl);
      m.slice(1).forEach((g, i) => {
        out = out.split(`{${i + 1}}`).join(g ?? "");
      });
      return out;
    }
  }
  return s;
}

/** All translatable templates used by `tb` (for the completeness check). */
export const BACKEND_TEMPLATES = backendRules.map(([, tpl]) => tpl);
