import { fmtNum, t } from "../i18n";
import { useEffect, useState } from "react";
import type { DesignObject, Sheet, Units } from "../types";
import { Box, fmtLen, objectBox, parseLen, rotateObjects, scaleObjects, toUnits, unionBox } from "../geometry";

/** A numeric field that commits on Enter / blur. */
export function NumField(props: {
  label: string;
  value: number | null;
  onCommit: (v: number) => void;
  suffix?: string;
  step?: number;
  min?: number;
  max?: number;
  disabled?: boolean;
  digits?: number;
  title?: string;
}) {
  const fmt = (v: number | null) => (v === null || !isFinite(v) ? "" : fmtNum(+v.toFixed(props.digits ?? 2)));
  const [text, setText] = useState(fmt(props.value));
  const [focused, setFocused] = useState(false);
  useEffect(() => {
    if (!focused) setText(fmt(props.value));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.value, focused]);
  const commit = () => {
    const n = parseFloat(text.replace(",", "."));
    if (isFinite(n)) {
      let v = n;
      if (props.min !== undefined) v = Math.max(props.min, v);
      if (props.max !== undefined) v = Math.min(props.max, v);
      if (v !== props.value) props.onCommit(v);
      setText(fmt(v));
    } else setText(fmt(props.value));
  };
  return (
    <label className="field" title={props.title}>
      <span className="field-label">{props.label}</span>
      <span className="field-input">
        <input
          value={text}
          disabled={props.disabled}
          inputMode="decimal"
          onFocus={(e) => {
            setFocused(true);
            e.target.select();
          }}
          onBlur={() => {
            setFocused(false);
            commit();
          }}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") {
              setText(fmt(props.value));
              (e.target as HTMLInputElement).blur();
            }
            if (e.key === "ArrowUp" || e.key === "ArrowDown") {
              e.preventDefault();
              const n = parseFloat(text.replace(",", ".")) || 0;
              const st = (props.step ?? 1) * (e.shiftKey ? 10 : 1);
              let v = n + (e.key === "ArrowUp" ? st : -st);
              if (props.min !== undefined) v = Math.max(props.min, v);
              if (props.max !== undefined) v = Math.min(props.max, v);
              props.onCommit(v);
            }
            e.stopPropagation();
          }}
        />
        {props.suffix && <span className="suffix">{props.suffix}</span>}
      </span>
    </label>
  );
}

export function PropertiesPanel(props: {
  objects: DesignObject[];
  selection: string[];
  units: Units;
  sheetWidth: number;
  onChange: (objs: DesignObject[]) => void;
}) {
  const { objects, selection, units, sheetWidth } = props;
  const [lock, setLock] = useState(true);
  const sel = objects.filter((o) => selection.includes(o.id));
  const box: Box | null = unionBox(sel.map(objectBox));
  if (!box) {
    return (
      <div className="panel">
        <div className="panel-title">{t("Selection")}</div>
        <div className="muted small">{t("Nothing selected. Click an object, or drag a box around several.")}</div>
      </div>
    );
  }
  const apply = (updated: DesignObject[]) => {
    const m = new Map(updated.map((o) => [o.id, o]));
    props.onChange(objects.map((o) => m.get(o.id) ?? o));
  };
  const toU = (mm: number) => toUnits(mm, units);
  const fromU = (v: number) => parseLen(String(v), units) ?? 0;
  // Y is measured from the origin edge (bottom of the sheet) to the bottom of the selection.
  const yFromOrigin = sheetWidth - (box.y + box.h);
  const setPos = (x: number | null, y: number | null) => {
    const dx = x === null ? 0 : fromU(x) - box.x;
    const dy = y === null ? 0 : yFromOrigin - fromU(y);
    apply(sel.map((o) => ({ ...o, x: o.x + dx, y: o.y + dy })));
  };
  const setSize = (w: number | null, h: number | null) => {
    let fx = w === null ? 1 : fromU(w) / box.w;
    let fy = h === null ? 1 : fromU(h) / box.h;
    if (lock) {
      if (w === null) fx = fy;
      else fy = fx;
    }
    if (!isFinite(fx) || !isFinite(fy) || fx <= 0 || fy <= 0) return;
    apply(scaleObjects(sel, [box.x, box.y], fx, fy));
  };
  const single = sel.length === 1 ? sel[0] : null;
  const rot = single ? single.rot : 0;
  const center: [number, number] = [box.x + box.w / 2, box.y + box.h / 2];
  const scalePct = single ? Math.abs(single.sx) * 100 : null;

  return (
    <div className="panel">
      <div className="panel-title">
        {sel.length === 1 ? single!.name : t("{n} objects", { n: sel.length })}
      </div>
      <div className="grid2">
        <NumField label="X" value={toU(box.x)} onCommit={(v) => setPos(v, null)} suffix={units} title={t("Distance from the origin along the material")} />
        <NumField label="Y" value={toU(yFromOrigin)} onCommit={(v) => setPos(null, v)} suffix={units} title={t("Distance from the origin edge (bottom of the sheet)")} />
        <NumField label={t("W")} value={toU(box.w)} onCommit={(v) => setSize(v, null)} suffix={units} min={0.01} />
        <NumField label={t("H")} value={toU(box.h)} onCommit={(v) => setSize(null, v)} suffix={units} min={0.01} />
      </div>
      <div className="row gap">
        <label className="check">
          <input type="checkbox" checked={lock} onChange={(e) => setLock(e.target.checked)} /> {t("Keep proportions")}
        </label>
      </div>
      <div className="grid2">
        <NumField
          label={t("Rotate")}
          value={rot}
          suffix="°"
          digits={1}
          onCommit={(v) => apply(rotateObjects(sel, center, single ? v - rot : v))}
          title={single ? t("Absolute rotation") : t("Rotate the selection by this angle")}
        />
        {scalePct !== null ? (
          <NumField
            label={t("Scale")}
            value={scalePct}
            suffix="%"
            digits={1}
            min={0.1}
            onCommit={(v) => {
              const f = v / scalePct;
              apply(scaleObjects(sel, center, f, f));
            }}
          />
        ) : (
          <span />
        )}
      </div>
      {single?.source?.kind === "file" && single.source.path && (
        <div className="muted small ellipsis" title={single.source.path}>
          {single.source.path}
        </div>
      )}
    </div>
  );
}

export function SheetPanel(props: {
  sheet: Sheet;
  units: Units;
  maxWidth: number | null;
  onSheet: (s: Sheet) => void;
  onUnits: (u: Units) => void;
  filled: boolean;
  onFilled: (f: boolean) => void;
  objectsExtent: number;
}) {
  const { sheet, units } = props;
  const toU = (mm: number) => toUnits(mm, units, units === "in" ? 2 : 1);
  const fromU = (v: number) => parseLen(String(v), units) ?? 0;
  return (
    <div className="panel">
      <div className="panel-title">{t("Material")}</div>
      <div className="grid2">
        <NumField
          label={t("Width")}
          value={toU(sheet.width)}
          suffix={units}
          min={1}
          onCommit={(v) => props.onSheet({ ...sheet, width: fromU(v) })}
          title={t("Width of the vinyl roll / sheet across the cutter")}
        />
        <NumField
          label={t("Length")}
          value={toU(sheet.length)}
          suffix={units}
          min={1}
          onCommit={(v) => props.onSheet({ ...sheet, length: fromU(v) })}
          title={t("Length of material available along the feed direction")}
        />
      </div>
      {props.maxWidth !== null && sheet.width > props.maxWidth + 0.5 && (
        <div className="warn small">
          {t("Wider than the cutter's max. cutting width ({w}).", { w: `${fmtLen(props.maxWidth, units)} ${units}` })}
        </div>
      )}
      {props.objectsExtent > sheet.length && (
        <div className="warn small">{t("Objects extend past the material length.")}</div>
      )}
      <div className="row gap">
        <div className="seg">
          <button className={units === "mm" ? "on" : ""} onClick={() => props.onUnits("mm")}>
            mm
          </button>
          <button className={units === "in" ? "on" : ""} onClick={() => props.onUnits("in")}>
            in
          </button>
        </div>
        <div className="seg">
          <button className={props.filled ? "on" : ""} onClick={() => props.onFilled(true)} title={t("Filled view")}>
            {t("Filled")}
          </button>
          <button className={!props.filled ? "on" : ""} onClick={() => props.onFilled(false)} title={t("Wireframe view (cut lines)")}>
            {t("Outline")}
          </button>
        </div>
      </div>
    </div>
  );
}

export function ObjectsPanel(props: {
  objects: DesignObject[];
  selection: string[];
  units: Units;
  onSelect: (ids: string[]) => void;
  onToggleHidden: (id: string) => void;
  onFixFonts: (o: DesignObject) => void;
}) {
  const { objects, selection, units } = props;
  return (
    <div className="panel grow">
      <div className="panel-title">{t("Objects")}</div>
      {objects.length === 0 && <div className="muted small">{t("No objects yet.")}</div>}
      <ul className="obj-list">
        {[...objects].reverse().map((o) => {
          const b = objectBox(o);
          const colors = Array.from(new Set(o.paths.map((p) => p.color))).slice(0, 6);
          const missing = (o.fontIssues ?? []).filter((i) => i.kind === "missing-font");
          return (
            <li
              key={o.id}
              className={selection.includes(o.id) ? "sel" : ""}
              onClick={(e) => {
                if (e.shiftKey || e.metaKey)
                  props.onSelect(selection.includes(o.id) ? selection.filter((x) => x !== o.id) : [...selection, o.id]);
                else props.onSelect([o.id]);
              }}
            >
              <button
                className={`eye ${o.hidden ? "off" : ""}`}
                title={o.hidden ? t("Show (will be cut)") : t("Hide (won't be cut)")}
                onClick={(e) => {
                  e.stopPropagation();
                  props.onToggleHidden(o.id);
                }}
              >
                {o.hidden ? "◌" : "●"}
              </button>
              <div className="obj-main">
                <div className="obj-name">
                  {o.source?.kind === "text" ? "T  " : ""}
                  {o.name}
                </div>
                <div className="obj-meta">
                  {fmtLen(b.w, units)} × {fmtLen(b.h, units)} {units}
                  <span className="swatches">
                    {colors.map((c) => (
                      <i key={c} style={{ background: c }} />
                    ))}
                  </span>
                </div>
              </div>
              {missing.length > 0 && (
                <button
                  className="badge-warn"
                  title={t("Fonts were missing on import — click to fix")}
                  onClick={(e) => {
                    e.stopPropagation();
                    props.onFixFonts(o);
                  }}
                >
                  {t("Font")}
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
