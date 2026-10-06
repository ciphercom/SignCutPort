import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, dialogs, isTauri, onFileDrop, onFontsReady } from "./api";
import { useDocument, serializeDoc, parseDoc } from "./store";
import type {
  DesignObject,
  FontFamilyInfo,
  ImportedDesign,
  ImportOptions,
  MachineProfile,
  ManufacturerSummary,
  TextRequest,
  Units,
} from "./types";
import { Canvas, CanvasHandle } from "./components/Canvas";
import { ObjectsPanel, PropertiesPanel, SheetPanel } from "./components/Panels";
import { TextDialog } from "./components/TextDialog";
import { FontIssuesDialog } from "./components/FontIssuesDialog";
import { FontsDialog } from "./components/FontsDialog";
import { CutConfig, CutDialog, DEFAULT_ENCODE, DEFAULT_SETTINGS } from "./components/CutDialog";
import { Icon } from "./components/Icons";
import { arrange, flipObjects, modelLabel, newId, objectBox, rotateObjects, toJobObject, unionBox } from "./geometry";

const LS_KEY = "signcut-port.config.v1";

interface Persisted {
  cut: CutConfig;
  units: Units;
  filled: boolean;
  sheetLength: number;
  sheetWidth: number | null;
}

function loadPersisted(): Persisted {
  const def: Persisted = {
    cut: {
      manufacturer: "VEVOR",
      model: "VEVOR KH-720",
      port: null,
      settings: DEFAULT_SETTINGS,
      encode: DEFAULT_ENCODE,
      testCutSize: 20,
    },
    units: "mm",
    filled: true,
    sheetLength: 1000,
    sheetWidth: null,
  };
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return def;
    const p = JSON.parse(raw) as Partial<Persisted>;
    return {
      ...def,
      ...p,
      cut: {
        ...def.cut,
        ...p.cut,
        settings: { ...DEFAULT_SETTINGS, ...p.cut?.settings, layers: [] },
        encode: { ...DEFAULT_ENCODE, ...p.cut?.encode },
      },
    };
  } catch {
    return def;
  }
}

const IMPORT_EXT = ["svg", "dxf", "plt", "hpgl", "hpg"];
const FONT_EXT = ["ttf", "otf", "ttc", "otc", "dfont"];

export default function App() {
  const persisted = useMemo(loadPersisted, []);
  const [cutCfg, setCutCfg] = useState<CutConfig>(persisted.cut);
  const [units, setUnits] = useState<Units>(persisted.units);
  const [filled, setFilled] = useState(persisted.filled);
  const doc = useDocument({ width: persisted.sheetWidth ?? 630, length: persisted.sheetLength });
  const { state } = doc;
  const [machines, setMachines] = useState<ManufacturerSummary[]>([]);
  const [profile, setProfile] = useState<MachineProfile | null>(null);
  const [fonts, setFonts] = useState<FontFamilyInfo[]>([]);
  const [textDlg, setTextDlg] = useState<{ edit?: DesignObject } | null>(null);
  const [fontDlg, setFontDlg] = useState<DesignObject | null>(null);
  const [cutOpen, setCutOpen] = useState(false);
  const [fontsOpen, setFontsOpen] = useState(false);
  const [toast, setToast] = useState<{ text: string; kind: "info" | "warn" | "error" } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [dropHover, setDropHover] = useState(false);
  const [zoom, setZoom] = useState(1);
  const canvasRef = useRef<CanvasHandle>(null);
  const clipboard = useRef<DesignObject[]>([]);
  const stateRef = useRef(state);
  stateRef.current = state;

  const notify = useCallback((text: string, kind: "info" | "warn" | "error" = "info") => {
    setToast({ text, kind });
  }, []);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), toast.kind === "error" ? 9000 : 5000);
    return () => clearTimeout(t);
  }, [toast]);

  // Persist preferences.
  useEffect(() => {
    const p: Persisted = {
      cut: { ...cutCfg, settings: { ...cutCfg.settings, layers: [] } },
      units,
      filled,
      sheetLength: state.sheet.length,
      sheetWidth: state.sheet.width,
    };
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(p));
    } catch {
      /* ignore */
    }
  }, [cutCfg, units, filled, state.sheet]);

  // Startup: machines (+ installed SignCut drivers), fonts, files passed on launch.
  useEffect(() => {
    (async () => {
      try {
        const sc = await api.driversDetectSignCut();
        if (sc.found) notify(`Using the driver pack from your SignCut Pro 2 installation (${sc.modelsLoaded} models).`);
      } catch {
        /* optional */
      }
      const m = await api.machinesList();
      setMachines(m);
      const files = await api.startupFiles();
      for (const f of files) await openPath(f);
    })().catch((e) => notify(String(e), "error"));
    const loadFonts = () => api.fontsList().then(setFonts).catch(() => {});
    loadFonts();
    let unsub: (() => void) | undefined;
    onFontsReady(loadFonts).then((u) => (unsub = u));
    return () => unsub?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Machine profile follows the selection; default material width = max cut width.
  const prevModel = useRef<string | null>(null);
  useEffect(() => {
    if (!cutCfg.model) return;
    api
      .machineProfile(cutCfg.manufacturer, cutCfg.model)
      .then((p) => {
        setProfile(p);
        const key = `${cutCfg.manufacturer}/${cutCfg.model}`;
        if (prevModel.current !== null && prevModel.current !== key) {
          // A different cutter: adopt its defaults.
          doc.setSheet({ ...stateRef.current.sheet, width: p.maxWidthMm }, false);
          setCutCfg((c) => ({
            ...c,
            settings: {
              ...c.settings,
              bladeOffset: p.defaultBladeOffset || c.settings.bladeOffset,
              useBladeOffset: p.useKnifeCompensation,
              speed: null,
              force: null,
              tool: 1,
            },
            port:
              c.port?.kind === "serial"
                ? { ...c.port, baud: p.defaultBaud || c.port.baud, flow: p.rts && p.cts ? "hardware" : "none", dtr: p.dtr }
                : c.port,
          }));
        } else if (persisted.sheetWidth === null && prevModel.current === null) {
          doc.setSheet({ ...stateRef.current.sheet, width: p.maxWidthMm }, false);
        }
        prevModel.current = key;
      })
      .catch((e) => notify(String(e), "error"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cutCfg.manufacturer, cutCfg.model]);

  // ------------------------------------------------------------- objects
  const placeNew = useCallback((d: ImportedDesign, extra: Partial<DesignObject>): DesignObject => {
    const s = stateRef.current;
    const box = unionBox(s.objects.map(objectBox));
    const margin = 5;
    const x0 = box ? box.x + box.w + 10 : margin;
    const w = d.widthMm;
    const h = d.heightMm;
    const y0 = Math.max(margin, s.sheet.width - margin - h);
    return {
      id: newId(),
      name: d.name,
      paths: d.paths,
      w,
      h,
      x: x0 + w / 2,
      y: y0 + h / 2,
      sx: 1,
      sy: 1,
      rot: 0,
      fontIssues: d.fontIssues,
      ...extra,
    };
  }, []);

  const addObjects = useCallback(
    (objs: DesignObject[]) => {
      const s = stateRef.current;
      doc.setObjects([...s.objects, ...objs], { selection: objs.map((o) => o.id) });
      const maxX = Math.max(...objs.map((o) => objectBox(o).x + objectBox(o).w));
      if (maxX > s.sheet.length) doc.setSheet({ ...s.sheet, length: Math.ceil((maxX + 20) / 100) * 100 }, false);
    },
    [doc],
  );

  const importPaths = useCallback(
    async (paths: string[]) => {
      const created: DesignObject[] = [];
      for (const path of paths) {
        setBusy(`Importing ${path.split("/").pop()}…`);
        try {
          const d = await api.importFile(path);
          const o = placeNew(d, { source: { kind: "file", path } });
          created.push(o);
          stateRef.current = { ...stateRef.current, objects: [...stateRef.current.objects, o] };
          if (d.warnings.length) notify(d.warnings.join(" "), "warn");
          if (d.fontIssues.some((i) => i.kind === "missing-font")) setFontDlg(o);
        } catch (e) {
          notify(`${path.split("/").pop()}: ${e}`, "error");
        }
      }
      setBusy(null);
      if (created.length) {
        const s = state;
        doc.setObjects([...s.objects, ...created], { selection: created.map((o) => o.id) });
        const maxX = Math.max(...created.map((o) => objectBox(o).x + objectBox(o).w));
        if (maxX > s.sheet.length) doc.setSheet({ ...s.sheet, length: Math.ceil((maxX + 20) / 100) * 100 }, false);
      }
    },
    [doc, placeNew, notify, state],
  );

  const openPath = async (path: string) => {
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    if (FONT_EXT.includes(ext)) {
      await importFonts([path]);
      return;
    }
    if (path.toLowerCase().endsWith(".scport")) {
      try {
        const f = parseDoc(await api.docLoad(path));
        doc.load(f.objects, f.sheet, path);
        setTimeout(() => canvasRef.current?.fit(), 50);
      } catch (e) {
        notify(String(e), "error");
      }
    } else await importPaths([path]);
  };

  const reimport = async (o: DesignObject, subs: Record<string, string>) => {
    if (o.source?.kind !== "file" || !o.source.path) return;
    const options: ImportOptions = { ...o.source.options, fontSubstitutions: subs };
    try {
      const d = await api.importFile(o.source.path, options);
      // Keep placement: same centre, keep scale relative to the new size.
      const upd: DesignObject = {
        ...o,
        paths: d.paths,
        w: d.widthMm,
        h: d.heightMm,
        fontIssues: d.fontIssues,
        source: { ...o.source, options },
      };
      doc.setObjects(state.objects.map((x) => (x.id === o.id ? upd : x)));
      setFontDlg(null);
      const left = d.fontIssues.filter((i) => i.kind === "missing-font").length;
      notify(left ? `${left} font(s) still missing.` : "Re-imported with the chosen fonts.", left ? "warn" : "info");
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const importFonts = async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const r = await api.fontsImport(paths);
      if (r.families.length) notify(`Imported ${r.families.join(", ")} — available from now on.`);
      if (r.errors.length) notify(r.errors.join(" "), "error");
    } catch (e) {
      notify(String(e), "error");
    }
    setFonts(await api.fontsList());
  };

  const loadFontFile = async () => {
    await importFonts(await dialogs.openFiles("Font files", FONT_EXT));
  };

  const selected = state.objects.filter((o) => state.selection.includes(o.id));
  const selBox = unionBox(selected.map(objectBox));

  const update = (objs: DesignObject[]) => {
    const m = new Map(objs.map((o) => [o.id, o]));
    doc.setObjects(state.objects.map((o) => m.get(o.id) ?? o));
  };

  const actions = {
    import: async () => {
      const paths = await dialogs.openFiles("Import designs", IMPORT_EXT);
      if (paths.length) importPaths(paths);
    },
    open: async () => {
      const [p] = await dialogs.openFiles("Open document", ["scport"], false);
      if (p) openPath(p);
    },
    save: async (as = false) => {
      let path = state.filePath;
      if (!path || as) path = await dialogs.saveFile("Save document", "Untitled.scport", ["scport"]);
      if (!path) return;
      if (!path.endsWith(".scport")) path += ".scport";
      try {
        await api.docSave(path, serializeDoc(state.objects, state.sheet));
        doc.markSaved(path);
        notify(`Saved ${path.split("/").pop()}`);
      } catch (e) {
        notify(String(e), "error");
      }
    },
    newDoc: async () => {
      if (state.dirty && !(await dialogs.confirm("Discard the current layout?", "Discard"))) return;
      doc.load([], state.sheet, null);
    },
    remove: () => {
      if (!selected.length) return;
      doc.setObjects(state.objects.filter((o) => !state.selection.includes(o.id)), { selection: [] });
    },
    duplicate: () => {
      if (!selected.length) return;
      const copies = selected.map((o) => ({ ...o, id: newId(), x: o.x + 10, y: o.y - 10 }));
      doc.setObjects([...state.objects, ...copies], { selection: copies.map((o) => o.id) });
    },
    copy: () => {
      clipboard.current = selected.map((o) => ({ ...o }));
    },
    paste: () => {
      if (!clipboard.current.length) return;
      const copies = clipboard.current.map((o) => ({ ...o, id: newId(), x: o.x + 10, y: o.y - 10 }));
      clipboard.current = copies;
      doc.setObjects([...state.objects, ...copies], { selection: copies.map((o) => o.id) });
    },
    rotate: (deg: number) => {
      if (!selBox) return;
      update(rotateObjects(selected, [selBox.x + selBox.w / 2, selBox.y + selBox.h / 2], deg));
    },
    flip: (axis: "h" | "v") => {
      if (!selBox) return;
      update(flipObjects(selected, axis, [selBox.x + selBox.w / 2, selBox.y + selBox.h / 2]));
    },
    align: (how: "left" | "right" | "top" | "bottom" | "hcenter" | "vcenter") => {
      if (!selBox) return;
      // A single object aligns to the material, several align to each other.
      const ref = selected.length > 1 ? selBox : { x: 0, y: 0, w: state.sheet.length, h: state.sheet.width };
      update(
        selected.map((o) => {
          const b = objectBox(o);
          let dx = 0,
            dy = 0;
          if (how === "left") dx = ref.x - b.x;
          if (how === "right") dx = ref.x + ref.w - (b.x + b.w);
          if (how === "top") dy = ref.y - b.y;
          if (how === "bottom") dy = ref.y + ref.h - (b.y + b.h);
          if (how === "hcenter") dx = ref.x + ref.w / 2 - (b.x + b.w / 2);
          if (how === "vcenter") dy = ref.y + ref.h / 2 - (b.y + b.h / 2);
          return { ...o, x: o.x + dx, y: o.y + dy };
        }),
      );
    },
    toOrigin: () => {
      const objs = selected.length ? selected : state.objects;
      const b = unionBox(objs.map(objectBox));
      if (!b) return;
      const dx = 5 - b.x;
      const dy = state.sheet.width - 5 - (b.y + b.h);
      update(objs.map((o) => ({ ...o, x: o.x + dx, y: o.y + dy })));
    },
    arrange: () => {
      const objs = (selected.length > 1 ? selected : state.objects).filter((o) => !o.hidden);
      if (!objs.length) return;
      update(arrange(objs, state.sheet.width, 5, 5));
      notify("Arranged to use as little material as possible.");
    },
    selectAll: () => doc.select(state.objects.map((o) => o.id)),
    nudge: (dx: number, dy: number) => {
      if (selected.length) update(selected.map((o) => ({ ...o, x: o.x + dx, y: o.y + dy })));
    },
    cut: () => {
      if (!state.objects.some((o) => !o.hidden)) {
        notify("Nothing to cut — import a design or add text first.", "warn");
        return;
      }
      setCutOpen(true);
    },
  };
  const act = useRef(actions);
  act.current = actions;

  // Keyboard shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT") return;
      if (textDlg || fontDlg || cutOpen || fontsOpen) return;
      const a = act.current;
      const cmd = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();
      const step = e.shiftKey ? 10 : 1;
      let handled = true;
      if (cmd && k === "z" && !e.shiftKey) doc.undo();
      else if ((cmd && k === "z" && e.shiftKey) || (cmd && k === "y")) doc.redo();
      else if (cmd && k === "i") a.import();
      else if (cmd && k === "o") a.open();
      else if (cmd && k === "s") a.save(e.shiftKey);
      else if (cmd && k === "n") a.newDoc();
      else if (cmd && k === "d") a.duplicate();
      else if (cmd && k === "c") a.copy();
      else if (cmd && k === "v") a.paste();
      else if (cmd && k === "a") a.selectAll();
      else if (cmd && k === "t") setTextDlg({});
      else if (cmd && (k === "p" || k === "k")) a.cut();
      else if (cmd && k === "0") canvasRef.current?.fit();
      else if (cmd && (k === "=" || k === "+")) canvasRef.current?.zoomBy(1.25);
      else if (cmd && k === "-") canvasRef.current?.zoomBy(0.8);
      else if (k === "backspace" || k === "delete") a.remove();
      else if (k === "arrowleft") a.nudge(-step, 0);
      else if (k === "arrowright") a.nudge(step, 0);
      else if (k === "arrowup") a.nudge(0, -step);
      else if (k === "arrowdown") a.nudge(0, step);
      else if (k === "r" && !cmd) a.rotate(e.shiftKey ? -90 : 90);
      else if (k === "escape") doc.select([]);
      else handled = false;
      if (handled) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [doc, textDlg, fontDlg, cutOpen, fontsOpen]);

  // Drag & drop from Finder.
  const importRef = useRef(openPath);
  importRef.current = openPath;
  useEffect(() => {
    let un: (() => void) | undefined;
    onFileDrop(
      async (paths) => {
        for (const p of paths) await importRef.current(p);
      },
      setDropHover,
    ).then((u) => (un = u));
    return () => un?.();
  }, []);

  // Window title.
  useEffect(() => {
    if (!isTauri) return;
    const name = state.filePath ? state.filePath.split("/").pop() : "Untitled";
    import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
      getCurrentWindow().setTitle(`${name}${state.dirty ? " — Edited" : ""} · SignCut Port`),
    );
  }, [state.filePath, state.dirty]);

  const onTextDone = (req: TextRequest, d: ImportedDesign) => {
    if (textDlg?.edit) {
      const o = textDlg.edit;
      // Keep the visual letter height: scale so the new outline matches the old scale.
      const upd: DesignObject = { ...o, name: d.name, paths: d.paths, w: d.widthMm, h: d.heightMm, source: { kind: "text", text: req }, fontIssues: d.fontIssues };
      doc.setObjects(state.objects.map((x) => (x.id === o.id ? upd : x)));
    } else {
      addObjects([placeNew(d, { source: { kind: "text", text: req } })]);
    }
    setTextDlg(null);
  };

  const visible = state.objects.filter((o) => !o.hidden);
  const colors = useMemo(() => Array.from(new Set(visible.flatMap((o) => o.paths.map((p) => p.color.toLowerCase())))), [visible]);
  const jobObjects = useMemo(() => visible.map(toJobObject), [visible]);
  const extent = unionBox(state.objects.map(objectBox));
  const hasSel = selected.length > 0;

  return (
    <div className="app">
      <header className="toolbar">
        <div className="tb-group">
          <button onClick={actions.import} title="Import SVG, DXF or PLT (⌘I)">
            <Icon name="import" /> Import
          </button>
          <button onClick={() => setTextDlg({})} title="Add text with any installed font (⌘T)">
            <Icon name="text" /> Text
          </button>
          <button onClick={() => setFontsOpen(true)} title="Fonts: import font files into SignCut Port">
            <Icon name="font" /> Fonts
          </button>
        </div>
        <div className="tb-group">
          <button className="icon" onClick={doc.undo} disabled={!state.past.length} title="Undo (⌘Z)">
            <Icon name="undo" />
          </button>
          <button className="icon" onClick={doc.redo} disabled={!state.future.length} title="Redo (⇧⌘Z)">
            <Icon name="redo" />
          </button>
        </div>
        <div className="tb-group">
          <button className="icon" onClick={actions.duplicate} disabled={!hasSel} title="Duplicate (⌘D)">
            <Icon name="duplicate" />
          </button>
          <button className="icon" onClick={() => actions.rotate(90)} disabled={!hasSel} title="Rotate 90° (R)">
            <Icon name="rotate" />
          </button>
          <button className="icon" onClick={() => actions.flip("h")} disabled={!hasSel} title="Mirror horizontally">
            <Icon name="flipH" />
          </button>
          <button className="icon" onClick={() => actions.flip("v")} disabled={!hasSel} title="Mirror vertically">
            <Icon name="flipV" />
          </button>
          <button className="icon" onClick={actions.remove} disabled={!hasSel} title="Delete (⌫)">
            <Icon name="trash" />
          </button>
        </div>
        <div className="tb-group">
          <button className="icon" onClick={() => actions.align("left")} disabled={!hasSel} title="Align left">
            <Icon name="alignLeft" />
          </button>
          <button className="icon" onClick={() => actions.align("hcenter")} disabled={!hasSel} title="Align centre">
            <Icon name="alignCenter" />
          </button>
          <button className="icon" onClick={() => actions.align("top")} disabled={!hasSel} title="Align top">
            <Icon name="alignTop" />
          </button>
          <button className="icon" onClick={() => actions.align("bottom")} disabled={!hasSel} title="Align bottom (origin side)">
            <Icon name="alignBottom" />
          </button>
          <button onClick={actions.arrange} disabled={!state.objects.length} title="Pack objects to save material">
            <Icon name="arrange" /> Arrange
          </button>
          <button onClick={actions.toOrigin} disabled={!state.objects.length} title="Move to the cutter origin">
            <Icon name="origin" /> To origin
          </button>
        </div>
        <div className="tb-group">
          <button className="icon" onClick={() => canvasRef.current?.zoomBy(0.8)} title="Zoom out (⌘−)">
            −
          </button>
          <span className="zoom-label" onClick={() => canvasRef.current?.zoom100()} title="Click for real size">
            {Math.round((zoom / (110 / 25.4)) * 100)}%
          </span>
          <button className="icon" onClick={() => canvasRef.current?.zoomBy(1.25)} title="Zoom in (⌘+)">
            +
          </button>
          <button onClick={() => canvasRef.current?.fit()} title="Fit (⌘0)">
            Fit
          </button>
        </div>
        <div className="tb-spacer" />
        <button className="machine-chip" onClick={() => setCutOpen(true)} title="Cutter and connection">
          <span className="dot" data-on={cutCfg.port ? "1" : "0"} />
          <span>
            <b>{cutCfg.model || "Choose cutter"}</b>
            <small>
              {cutCfg.port
                ? cutCfg.port.kind === "serial"
                  ? cutCfg.port.path.replace("/dev/", "")
                  : cutCfg.port.kind === "usb"
                    ? "USB"
                    : cutCfg.port.kind === "tcp"
                      ? `${cutCfg.port.host}:${cutCfg.port.port}`
                      : cutCfg.port.kind === "printer"
                        ? cutCfg.port.name
                        : "Save to file"
                : "not connected"}
            </small>
          </span>
        </button>
        <button className="primary cut-btn" onClick={actions.cut} title="Cut (⌘P)">
          <Icon name="cut" /> Cut
        </button>
      </header>

      <div className="main">
        <aside className="sidebar left">
          <ObjectsPanel
            objects={state.objects}
            selection={state.selection}
            units={units}
            onSelect={doc.select}
            onToggleHidden={(id) => doc.setObjects(state.objects.map((o) => (o.id === id ? { ...o, hidden: !o.hidden } : o)))}
            onFixFonts={setFontDlg}
          />
        </aside>
        <section className="canvas-area">
          <Canvas
            handleRef={canvasRef}
            objects={state.objects}
            selection={state.selection}
            sheet={state.sheet}
            units={units}
            filled={filled}
            maxWidth={profile?.maxWidthMm ?? null}
            onSelect={doc.select}
            onChange={(objs, opts) => doc.setObjects(objs, opts)}
            onEdit={(o) => (o.source?.kind === "text" ? setTextDlg({ edit: o }) : o.fontIssues?.length ? setFontDlg(o) : undefined)}
            onZoom={setZoom}
          />
          {dropHover && <div className="drop-overlay">Drop to import</div>}
        </section>
        <aside className="sidebar right">
          <PropertiesPanel objects={state.objects} selection={state.selection} units={units} sheetWidth={state.sheet.width} onChange={(objs) => doc.setObjects(objs)} />
          <SheetPanel
            sheet={state.sheet}
            units={units}
            maxWidth={profile?.maxWidthMm ?? null}
            onSheet={(s) => doc.setSheet(s)}
            onUnits={setUnits}
            filled={filled}
            onFilled={setFilled}
            objectsExtent={extent ? extent.x + extent.w : 0}
          />
          <div className="panel">
            <div className="panel-title">Cutter</div>
            <div className="small">
              <b>{modelLabel(cutCfg.manufacturer, cutCfg.model)}</b>
            </div>
            {profile && (
              <div className="muted small">
                Max width {profile.maxWidthMm} mm · {profile.language} · blade offset {cutCfg.settings.useBladeOffset ? `${cutCfg.settings.bladeOffset} mm` : "off"}
              </div>
            )}
            <button onClick={() => setCutOpen(true)}>Cutter &amp; connection…</button>
          </div>
        </aside>
      </div>

      {busy && <div className="busy">{busy}</div>}
      {toast && (
        <div className={`toast ${toast.kind}`} onClick={() => setToast(null)}>
          {toast.text}
        </div>
      )}

      {textDlg && (
        <TextDialog
          fonts={fonts}
          units={units}
          initial={textDlg.edit?.source?.text}
          onCancel={() => setTextDlg(null)}
          onDone={onTextDone}
          onLoadFont={loadFontFile}
        />
      )}
      {fontsOpen && <FontsDialog onClose={() => setFontsOpen(false)} onImport={loadFontFile} systemFamilies={fonts.length} />}
      {fontDlg && (
        <FontIssuesDialog
          object={state.objects.find((o) => o.id === fontDlg.id) ?? fontDlg}
          fonts={fonts}
          onClose={() => setFontDlg(null)}
          onLoadFont={loadFontFile}
          onReimport={(subs) => reimport(state.objects.find((o) => o.id === fontDlg.id) ?? fontDlg, subs)}
        />
      )}
      {cutOpen && (
        <CutDialog
          machines={machines}
          config={cutCfg}
          profile={profile}
          objects={jobObjects}
          colors={colors}
          sheetWidth={state.sheet.width}
          units={units}
          onConfig={setCutCfg}
          onClose={() => setCutOpen(false)}
          onLoadDrivers={async () => {
            const [p] = await dialogs.openFiles("SignCut drivers", ["pak", "xml", "zip"], false);
            if (!p) return;
            try {
              const n = await api.driversLoad(p);
              setMachines(await api.machinesList());
              notify(`Loaded ${n} cutter models.`);
            } catch (e) {
              notify(String(e), "error");
            }
          }}
        />
      )}
    </div>
  );
}
