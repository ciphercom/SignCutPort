import { t } from "../i18n";
import { useMemo, useState } from "react";
import type { DesignObject, FontFamilyInfo } from "../types";

function guess(requested: string, fonts: FontFamilyInfo[]): string {
  const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");
  const first = requested.split(",")[0].replace(/['"]/g, "").trim();
  const n = norm(first);
  // Strip style suffixes ("-Bold", " Italic").
  const base = norm(first.replace(/[-_ ](bold|italic|regular|light|medium|black|semibold|thin|heavy|oblique).*$/i, ""));
  const hit =
    fonts.find((f) => norm(f.family) === n) ??
    fonts.find((f) => norm(f.family) === base) ??
    fonts.find((f) => base.length > 3 && norm(f.family).startsWith(base)) ??
    fonts.find((f) => base.length > 3 && base.startsWith(norm(f.family)));
  return hit?.family ?? "";
}

export function FontIssuesDialog(props: {
  object: DesignObject;
  fonts: FontFamilyInfo[];
  onClose: () => void;
  onLoadFont: () => void;
  onReimport: (subs: Record<string, string>) => void;
}) {
  const missing = useMemo(
    () => (props.object.fontIssues ?? []).filter((i) => i.kind === "missing-font"),
    [props.object],
  );
  const glyphs = (props.object.fontIssues ?? []).filter((i) => i.kind === "missing-glyph");
  const [subs, setSubs] = useState<Record<string, string>>(() => {
    const s: Record<string, string> = { ...(props.object.source?.options?.fontSubstitutions ?? {}) };
    for (const m of missing) {
      // Substitutions are keyed by every family named in the request.
      const g = guess(m.requested, props.fonts);
      for (const name of m.requested.split(",").map((x) => x.trim()).filter(Boolean)) if (!s[name] && g) s[name] = g;
    }
    return s;
  });
  const canReimport = props.object.source?.kind === "file" && !!props.object.source.path;

  return (
    <div className="modal-bg" onMouseDown={props.onClose}>
      <div className="modal fonts-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>{t("Fonts in “{name}”", { name: props.object.name })}</h2>
        </div>
        <p className="small">
          {t("These fonts are used in the file but are not installed on this Mac. Instead of silently swapping them, SignCut Port lets you choose what to use — or import the original font and re-import the file.")}
        </p>
        <table className="fonts-table">
          <thead>
            <tr>
              <th>{t("Requested in file")}</th>
              <th>{t("Currently used")}</th>
              <th>{t("Use instead")}</th>
            </tr>
          </thead>
          <tbody>
            {missing.map((m) => {
              const names = m.requested.split(",").map((x) => x.trim()).filter(Boolean);
              const key = names[0];
              return (
                <tr key={m.requested}>
                  <td>
                    <b>{m.requested}</b>
                  </td>
                  <td className="muted">{m.substitutedWith ?? "—"}</td>
                  <td>
                    <select
                      value={subs[key] ?? ""}
                      onChange={(e) =>
                        setSubs((s) => {
                          const n = { ...s };
                          for (const name of names) n[name] = e.target.value;
                          return n;
                        })
                      }
                    >
                      <option value="">{t("(keep fallback)")}</option>
                      {props.fonts.map((f) => (
                        <option key={f.family} value={f.family}>
                          {f.family}
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {glyphs.length > 0 && (
          <div className="warn small">
            {glyphs.map((g, i) => (
              <div key={i}>
                {t("Characters “{chars}” are not in {font}; taken from {other}.", {
                  chars: g.detail ?? "",
                  font: g.requested || t("the font"),
                  other: g.substitutedWith ?? t("another font"),
                })}
              </div>
            ))}
          </div>
        )}
        {!canReimport && <div className="muted small">{t("This object was not imported from a file, so it cannot be re-imported.")}</div>}
        <div className="modal-foot">
          <button className="link" onClick={props.onLoadFont}>
            {t("Import a font file…")}
          </button>
          <div className="row gap">
            <button onClick={props.onClose}>{t("Keep as is")}</button>
            <button
              className="primary"
              disabled={!canReimport}
              onClick={() => {
                const clean = Object.fromEntries(Object.entries(subs).filter(([, v]) => v));
                props.onReimport(clean);
              }}
            >
              {t("Re-import")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
