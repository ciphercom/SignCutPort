import { useEffect, useMemo, useRef, useState } from "react";
import type { ManufacturerSummary, Units } from "../types";
import { searchModels, shortModelName, sortCatalog } from "../machines";
import { fmtLen } from "../geometry";
import { t } from "../i18n";

interface Row {
  manufacturer: string;
  name: string;
  maxWidthMm: number;
}

/**
 * Cutter selection: a sorted manufacturer list to browse, plus a search box
 * that searches every model of every manufacturer ("vevor 720", "kh720").
 */
export function MachinePicker(props: {
  machines: ManufacturerSummary[];
  manufacturer: string;
  model: string;
  units: Units;
  onChange: (manufacturer: string, model: string) => void;
}) {
  const sorted = useMemo(() => sortCatalog(props.machines), [props.machines]);
  const [browse, setBrowse] = useState(props.manufacturer);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(-1);
  const listRef = useRef<HTMLUListElement>(null);

  // Follow selection changes from outside (e.g. settings restored).
  useEffect(() => setBrowse(props.manufacturer), [props.manufacturer]);

  const searching = query.trim().length > 0;
  const rows: Row[] = useMemo(() => {
    if (searching) {
      return searchModels(sorted, query, 300, browse).map((h) => ({
        manufacturer: h.manufacturer,
        name: h.model.name,
        maxWidthMm: h.model.maxWidthMm,
      }));
    }
    const maker = sorted.find((m) => m.manufacturer === browse);
    return (maker?.models ?? []).map((m) => ({ manufacturer: browse, name: m.name, maxWidthMm: m.maxWidthMm }));
  }, [sorted, browse, query, searching]);

  useEffect(() => setActive(-1), [query, browse]);

  // Keep the selected (or keyboard-highlighted) row visible.
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(active >= 0 ? `[data-i="${active}"]` : ".sel");
    el?.scrollIntoView({ block: "nearest" });
  }, [active, rows, props.model]);

  const choose = (r: Row) => {
    props.onChange(r.manufacturer, r.name);
    setBrowse(r.manufacturer);
    setQuery("");
  };

  const isSelected = (r: Row) => r.manufacturer === props.manufacturer && r.name === props.model;

  return (
    <div className="machine-picker">
      <div className="grid2">
        <label className="field">
          <span className="field-label">{t("Manufacturer")}</span>
          <select
            value={browse}
            onChange={(e) => {
              setBrowse(e.target.value);
              setQuery("");
            }}
          >
            {sorted.map((m) => (
              <option key={m.manufacturer} value={m.manufacturer}>
                {m.manufacturer} ({m.models.length})
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Search")}</span>
          <input
            type="search"
            value={query}
            placeholder={t("e.g. “vevor 720”")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(rows.length - 1, a + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(0, a - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                const r = rows[active >= 0 ? active : 0];
                if (r) choose(r);
              } else if (e.key === "Escape" && query) {
                e.preventDefault();
                setQuery("");
              }
            }}
          />
        </label>
      </div>
      <ul className="model-list" ref={listRef} role="listbox" aria-label={t("Model")}>
        {rows.map((r, i) => (
          <li
            key={`${i}:${r.manufacturer}/${r.name}`}
            data-i={i}
            role="option"
            aria-selected={isSelected(r)}
            className={`${isSelected(r) ? "sel" : ""} ${i === active ? "active" : ""}`}
            onClick={() => choose(r)}
          >
            <span className="model-name">{shortModelName(r.manufacturer, r.name)}</span>
            {searching && <span className="model-maker">{r.manufacturer}</span>}
            <span className="model-width">
              {fmtLen(r.maxWidthMm, props.units, props.units === "in" ? 1 : 0)} {props.units}
            </span>
          </li>
        ))}
        {searching && rows.length === 0 && <li className="muted empty">{t("No cutter matches “{q}”.", { q: query })}</li>}
      </ul>
    </div>
  );
}
