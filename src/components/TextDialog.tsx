import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { FontFamilyInfo, ImportedDesign, TextRequest, Units } from "../types";
import { NumField } from "./Panels";
import { fmtLen } from "../geometry";

export function FontPicker(props: {
  fonts: FontFamilyInfo[];
  value: string;
  onChange: (family: string) => void;
  sample?: string;
  height?: number;
}) {
  const [q, setQ] = useState("");
  const list = useMemo(() => {
    const ql = q.trim().toLowerCase();
    const l = ql ? props.fonts.filter((f) => f.family.toLowerCase().includes(ql)) : props.fonts;
    return l.slice(0, 400);
  }, [q, props.fonts]);
  const selRef = useRef<HTMLLIElement>(null);
  useEffect(() => selRef.current?.scrollIntoView({ block: "nearest" }), [props.value]);
  return (
    <div className="font-picker">
      <input placeholder={`Search ${props.fonts.length} installed fonts…`} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      <ul style={{ height: props.height ?? 240 }}>
        {list.map((f) => (
          <li
            key={f.family}
            ref={f.family === props.value ? selRef : undefined}
            className={f.family === props.value ? "sel" : ""}
            onClick={() => props.onChange(f.family)}
          >
            <span className="font-name">{f.family}</span>
            <span className="font-sample" style={{ fontFamily: `"${f.family}"` }}>
              {props.sample || "Aa Bb 123"}
            </span>
            <span className="muted small">{f.faces.length > 1 ? `${f.faces.length} styles` : ""}</span>
          </li>
        ))}
        {list.length === 0 && <li className="muted">No font matches “{q}”.</li>}
      </ul>
    </div>
  );
}

const DEFAULT_REQ: TextRequest = {
  text: "Your text",
  family: "Helvetica",
  weight: 400,
  italic: false,
  sizeMm: 50,
  letterSpacingMm: 0,
  lineHeight: 1.2,
  align: "start",
};

export function TextDialog(props: {
  fonts: FontFamilyInfo[];
  initial?: TextRequest;
  units: Units;
  onCancel: () => void;
  onDone: (req: TextRequest, design: ImportedDesign) => void;
  onLoadFont: () => void;
}) {
  const [req, setReq] = useState<TextRequest>(() => {
    if (props.initial) return props.initial;
    const fam =
      props.fonts.find((f) => f.family === "Helvetica")?.family ??
      props.fonts.find((f) => f.family === "Arial")?.family ??
      props.fonts[0]?.family ??
      "Helvetica";
    return { ...DEFAULT_REQ, family: fam };
  });
  const [design, setDesign] = useState<ImportedDesign | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const family = props.fonts.find((f) => f.family === req.family);

  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(async () => {
      if (!req.text.trim()) {
        setDesign(null);
        return;
      }
      setBusy(true);
      try {
        const d = await api.textRender(req);
        if (!cancelled) {
          setDesign(d);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setBusy(false);
      }
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [req]);

  const upd = (p: Partial<TextRequest>) => setReq((r) => ({ ...r, ...p }));
  const glyphIssues = design?.fontIssues ?? [];

  return (
    <div className="modal-bg" onMouseDown={props.onCancel}>
      <div className="modal text-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>{props.initial ? "Edit text" : "Add text"}</h2>
          <span className="muted small">Text is converted to cut outlines using the exact installed font.</span>
        </div>
        <div className="text-body">
          <div className="text-left">
            <textarea
              autoFocus
              value={req.text}
              rows={3}
              onChange={(e) => upd({ text: e.target.value })}
              onKeyDown={(e) => e.stopPropagation()}
              style={{ fontFamily: `"${req.family}"`, fontWeight: req.weight, fontStyle: req.italic ? "italic" : "normal" }}
            />
            <FontPicker fonts={props.fonts} value={req.family} onChange={(f) => upd({ family: f })} sample={req.text.split("\n")[0].slice(0, 24)} />
            <button className="link" onClick={props.onLoadFont}>
              Font not listed? Load a font file…
            </button>
          </div>
          <div className="text-right">
            <label className="field">
              <span className="field-label">Style</span>
              <select
                value={`${req.weight}|${req.italic ? 1 : 0}`}
                onChange={(e) => {
                  const [w, i] = e.target.value.split("|");
                  upd({ weight: +w, italic: i === "1" });
                }}
              >
                {(family?.faces.length ? family.faces : [{ weight: 400, italic: false, styleName: "Regular" }]).map((f) => (
                  <option key={`${f.weight}|${f.italic ? 1 : 0}`} value={`${f.weight}|${f.italic ? 1 : 0}`}>
                    {f.styleName}
                  </option>
                ))}
              </select>
            </label>
            <NumField label="Font size" value={req.sizeMm} suffix="mm" min={0.5} onCommit={(v) => upd({ sizeMm: v })} title="Em size; the canvas shows the real letter height" />
            <NumField label="Letter spacing" value={req.letterSpacingMm} suffix="mm" step={0.5} onCommit={(v) => upd({ letterSpacingMm: v })} />
            <NumField label="Line height" value={req.lineHeight} suffix="×" step={0.1} min={0.5} onCommit={(v) => upd({ lineHeight: v })} />
            <label className="field">
              <span className="field-label">Align</span>
              <div className="seg">
                {(["start", "middle", "end"] as const).map((a) => (
                  <button key={a} className={req.align === a ? "on" : ""} onClick={() => upd({ align: a })}>
                    {a === "start" ? "Left" : a === "middle" ? "Center" : "Right"}
                  </button>
                ))}
              </div>
            </label>
          </div>
        </div>
        <div className="text-preview">
          {design ? (
            <svg viewBox={`-2 -2 ${design.widthMm + 4} ${design.heightMm + 4}`} preserveAspectRatio="xMidYMid meet">
              {design.paths.map((p, i) => (
                <path key={i} d={p.d} fill="#222" fillRule="evenodd" />
              ))}
            </svg>
          ) : (
            <span className="muted">{busy ? "Rendering…" : "Type some text"}</span>
          )}
          {design && (
            <div className="preview-size">
              {fmtLen(design.widthMm, props.units)} × {fmtLen(design.heightMm, props.units)} {props.units}
            </div>
          )}
        </div>
        {glyphIssues.length > 0 && (
          <div className="warn small">
            {glyphIssues.map((g, i) =>
              g.kind === "missing-glyph" ? (
                <div key={i}>
                  “{g.requested}” has no glyphs for “{g.detail}” — {g.substitutedWith ? `taken from ${g.substitutedWith}` : "skipped"}.
                </div>
              ) : (
                <div key={i}>Font “{g.requested}” not found — using {g.substitutedWith}.</div>
              ),
            )}
          </div>
        )}
        {error && <div className="error small">{error}</div>}
        <div className="modal-foot">
          <span />
          <div className="row gap">
            <button onClick={props.onCancel}>Cancel</button>
            <button className="primary" disabled={!design} onClick={() => design && props.onDone(req, design)}>
              {props.initial ? "Update" : "Add to sheet"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
