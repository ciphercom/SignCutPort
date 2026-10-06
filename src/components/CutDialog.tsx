import { useEffect, useMemo, useRef, useState } from "react";
import { api, dialogs, onCutProgress, onCutStatus } from "../api";
import type {
  CutProgress,
  CutSettings,
  EncodeOptions,
  JobObject,
  JobPreview,
  LayerSettings,
  MachineProfile,
  ManufacturerSummary,
  Port,
  PortInfo,
  Units,
} from "../types";
import { NumField } from "./Panels";
import { fmtLen, modelLabel } from "../geometry";
import { fmtInt, t, tb } from "../i18n";

export interface CutConfig {
  manufacturer: string;
  model: string;
  port: Port | null;
  settings: CutSettings;
  encode: EncodeOptions;
  testCutSize: number;
}

export const DEFAULT_SETTINGS: CutSettings = {
  materialWidth: 600,
  bladeOffset: 0.25,
  useBladeOffset: true,
  tangentialEmulation: true,
  overcut: 1,
  passes: 1,
  speed: null,
  force: null,
  tool: 1,
  sort: "nearest",
  bandWidth: 200,
  insideFirst: true,
  mirror: false,
  placement: "asPlaced",
  margin: 0,
  weedBorder: null,
  copies: 1,
  copyGap: 5,
  stackCopies: true,
  afterCut: "feedPastJob",
  feedExtra: 0,
  curveTolerance: 0.05,
  layers: [],
};

export const DEFAULT_ENCODE: EncodeOptions = {
  sendSpeedForce: true,
  swapXy: null,
  afterCut: "feedPastJob",
  feedExtra: 0,
  sendPageFeed: true,
};

function portKey(p: Port | null): string {
  if (!p) return "";
  switch (p.kind) {
    case "serial":
      return `serial:${p.path}`;
    case "usb":
      return `usb:${p.vendorId}:${p.productId}:${p.serial ?? ""}`;
    case "tcp":
      return "tcp";
    case "printer":
      return `printer:${p.name}`;
    case "file":
      return "file";
  }
}

export function MachineSelect(props: {
  machines: ManufacturerSummary[];
  manufacturer: string;
  model: string;
  onChange: (manufacturer: string, model: string) => void;
}) {
  const man = props.machines.find((m) => m.manufacturer === props.manufacturer);
  return (
    <div className="grid2">
      <label className="field">
        <span className="field-label">{t("Manufacturer")}</span>
        <select
          value={props.manufacturer}
          onChange={(e) => {
            const m = props.machines.find((x) => x.manufacturer === e.target.value);
            props.onChange(e.target.value, m?.models[0]?.name ?? "");
          }}
        >
          {props.machines.map((m) => (
            <option key={m.manufacturer} value={m.manufacturer}>
              {m.manufacturer}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span className="field-label">{t("Model")}</span>
        <select value={props.model} onChange={(e) => props.onChange(props.manufacturer, e.target.value)}>
          {(man?.models ?? []).map((m) => (
            <option key={m.name} value={m.name}>
              {m.name}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

export function CutDialog(props: {
  machines: ManufacturerSummary[];
  config: CutConfig;
  profile: MachineProfile | null;
  objects: JobObject[];
  colors: string[];
  sheetWidth: number;
  units: Units;
  onConfig: (c: CutConfig) => void;
  onClose: () => void;
  onLoadDrivers: () => void;
}) {
  const { config, profile, units } = props;
  const s = config.settings;
  const [ports, setPorts] = useState<PortInfo[]>([]);
  const [scanning, setScanning] = useState(false);
  const [preview, setPreview] = useState<JobPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<CutProgress | null>(null);
  const [sending, setSending] = useState<null | "job" | "test">(null);
  const [pauseMsg, setPauseMsg] = useState<string | null>(null);
  const [doneMsg, setDoneMsg] = useState<string | null>(null);
  const [anim, setAnim] = useState<number | null>(null);
  const [showData, setShowData] = useState(false);

  const setSettings = (p: Partial<CutSettings>) => props.onConfig({ ...config, settings: { ...s, ...p } });
  const setEncode = (p: Partial<EncodeOptions>) => props.onConfig({ ...config, encode: { ...config.encode, ...p } });

  const job = useMemo(
    () => ({
      objects: props.objects,
      settings: { ...s, materialWidth: props.sheetWidth },
      encode: { ...config.encode, afterCut: s.afterCut, feedExtra: s.feedExtra },
      manufacturer: config.manufacturer,
      model: config.model,
    }),
    [props.objects, s, props.sheetWidth, config.encode, config.manufacturer, config.model],
  );

  const scan = async () => {
    setScanning(true);
    try {
      const p = await api.portsList();
      setPorts(p);
      if (!config.port || (config.port.kind !== "tcp" && config.port.kind !== "file" && !p.some((x) => portKey(x.port) === portKey(config.port)))) {
        const best = p.find((x) => x.likelyCutter) ?? p[0];
        if (best) choosePort(best.port);
      }
    } catch (e) {
      setError(tb(e));
    } finally {
      setScanning(false);
    }
  };

  const choosePort = (p: Port) => {
    let port = p;
    if (p.kind === "serial" && profile) {
      const prev = config.port?.kind === "serial" && config.port.path === p.path ? config.port : null;
      port = prev ?? {
        ...p,
        baud: profile.defaultBaud || 9600,
        flow: profile.rts && profile.cts ? "hardware" : "none",
        dtr: profile.dtr,
      };
    }
    props.onConfig({ ...config, port });
  };

  useEffect(() => {
    scan();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live preview.
  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(async () => {
      try {
        const p = await api.jobPreview(job);
        if (!cancelled) {
          setPreview(p);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(tb(e));
      }
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [job]);

  // Send progress / status events.
  useEffect(() => {
    const unsub: (() => void)[] = [];
    onCutProgress((p) => setProgress(p)).then((u) => unsub.push(u));
    onCutStatus((st) => {
      if (st.state === "pause") {
        setPauseMsg(tb(st.message || t("Paused")));
        return;
      }
      setPauseMsg(null);
      setSending(null);
      if (st.state === "done") setDoneMsg(t("Sent to the cutter."));
      else if (st.state === "cancelled") setDoneMsg(t("Cancelled."));
      else setError(tb(st.message));
    }).then((u) => unsub.push(u));
    return () => unsub.forEach((u) => u());
  }, []);

  // Simulation.
  const raf = useRef<number>(0);
  useEffect(() => {
    if (anim === null || !preview) return;
    const total = preview.cuts.length;
    const step = Math.max(1, Math.ceil(total / 240));
    raf.current = window.setTimeout(() => {
      setAnim((a) => (a === null ? null : a + step >= total ? null : a + step));
    }, 16);
    return () => clearTimeout(raf.current);
  }, [anim, preview]);

  const needPort = () => {
    if (!config.port) {
      setError(t("Choose how the cutter is connected first."));
      return false;
    }
    return true;
  };

  const start = async () => {
    if (!needPort()) return;
    setError(null);
    setDoneMsg(null);
    if (config.port!.kind === "file") return exportPlt();
    setSending("job");
    setProgress(null);
    try {
      await api.jobStart(job, config.port!);
    } catch (e) {
      setSending(null);
      setError(tb(e));
    }
  };

  const exportPlt = async () => {
    const path = await dialogs.saveFile(t("Save plot file"), "cut.plt", ["plt", "hpgl"]);
    if (!path) return;
    try {
      const n = await api.jobExport(job, path);
      setDoneMsg(t("Saved {bytes} bytes to {path}", { bytes: fmtInt(n), path }));
    } catch (e) {
      setError(tb(e));
    }
  };

  const testCut = async () => {
    if (!needPort() || config.port!.kind === "file") return;
    setError(null);
    setDoneMsg(null);
    setSending("test");
    try {
      // Bottom-left of the sheet = near the cutter origin.
      await api.testCut(job, config.port!, config.testCutSize, [5, props.sheetWidth - 5 - config.testCutSize]);
    } catch (e) {
      setSending(null);
      setError(tb(e));
    }
  };

  const testFeed = async () => {
    if (!needPort() || config.port!.kind === "file") return;
    setError(null);
    setSending("test");
    try {
      await api.testFeed(config.manufacturer, config.model, config.port!, 150);
    } catch (e) {
      setSending(null);
      setError(tb(e));
    }
  };

  const layerFor = (c: string): LayerSettings =>
    s.layers.find(([k]) => k.toLowerCase() === c.toLowerCase())?.[1] ?? { enabled: true, pauseBefore: false };
  const setLayer = (c: string, l: Partial<LayerSettings>) => {
    const rest = s.layers.filter(([k]) => k.toLowerCase() !== c.toLowerCase());
    // Keep the dialog's colour order as the cutting order.
    const merged: [string, LayerSettings][] = props.colors.map((col) =>
      col.toLowerCase() === c.toLowerCase() ? [col, { ...layerFor(col), ...l }] : [col, layerFor(col)],
    );
    setSettings({ layers: [...merged, ...rest.filter(([k]) => !props.colors.some((x) => x.toLowerCase() === k.toLowerCase()))] });
  };
  const moveLayer = (c: string, dir: -1 | 1) => {
    const order = [...props.colors].sort((a, b) => layerIndex(a) - layerIndex(b));
    const i = order.indexOf(c);
    const j = i + dir;
    if (j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    setSettings({ layers: order.map((col) => [col, layerFor(col)] as [string, LayerSettings]) });
  };
  const layerIndex = (c: string) => {
    const i = s.layers.findIndex(([k]) => k.toLowerCase() === c.toLowerCase());
    return i < 0 ? 1000 + props.colors.indexOf(c) : i;
  };
  const orderedColors = [...props.colors].sort((a, b) => layerIndex(a) - layerIndex(b));

  const supportsSpeed = !!profile?.commands.velocity;
  const supportsForce = !!profile?.commands.force;
  const port = config.port;

  // ------------------------------------------------------------ preview svg
  const pv = useMemo(() => {
    if (!preview) return null;
    const W = Math.max(props.sheetWidth, preview.stats.maxY + 10);
    const maxX = Math.max(preview.stats.maxX, 50) + 20;
    const vb = `-10 ${-W - 10} ${maxX + 20} ${W + 30}`;
    return { W, maxX, vb };
  }, [preview, props.sheetWidth]);

  const shown = preview ? (anim === null ? preview.cuts.length : anim) : 0;
  const stats = preview?.stats;
  const n = preview?.cuts.length ?? 0;

  return (
    <div className="modal-bg">
      <div className="modal cut-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>{t("Cut")}</h2>
          <span className="muted">
            {modelLabel(config.manufacturer, config.model)}
            {profile ? ` · ${profile.language} · ${t("max. {w}", { w: `${fmtLen(profile.maxWidthMm, units, 0)} ${units}` })}` : ""}
          </span>
          <button className="close" onClick={props.onClose} disabled={!!sending} title={t("Close")}>
            ✕
          </button>
        </div>
        <div className="cut-body">
          <div className="cut-preview">
            <div className="preview-toolbar">
              <button onClick={() => setAnim(anim === null ? 0 : null)} disabled={!n}>
                {anim === null ? `▶ ${t("Simulate")}` : `■ ${t("Stop")}`}
              </button>
              <span className="muted small">
                {t("Cutter view: origin ● at bottom-left (front-right of the cutter), material feeds →. Numbers show cut order.")}
              </span>
            </div>
            <div className="preview-canvas">
              {pv && preview && (
                <svg viewBox={pv.vb} preserveAspectRatio="xMidYMid meet">
                  <rect x={0} y={-props.sheetWidth} width={pv.maxX} height={props.sheetWidth} className="pv-material" />
                  {profile && profile.maxWidthMm < pv.W && (
                    <line x1={0} x2={pv.maxX} y1={-profile.maxWidthMm} y2={-profile.maxWidthMm} className="pv-maxwidth" />
                  )}
                  {/* travel moves */}
                  <path
                    className="pv-travel"
                    d={(() => {
                      let d = "";
                      let last: [number, number] = [0, 0];
                      for (let i = 0; i < shown; i++) {
                        const c = preview.cuts[i];
                        d += `M${last[0]} ${-last[1]}L${c[0][0]} ${-c[0][1]}`;
                        last = c[c.length - 1];
                      }
                      return d;
                    })()}
                  />
                  {preview.cuts.slice(0, shown).map((c, i) => (
                    <polyline
                      key={i}
                      points={c.map(([x, y]) => `${x},${-y}`).join(" ")}
                      fill="none"
                      stroke={`hsl(${220 - (200 * i) / Math.max(1, n - 1)} 75% 45%)`}
                      strokeWidth={1.3}
                      vectorEffect="non-scaling-stroke"
                    />
                  ))}
                  {n <= 60 &&
                    preview.cuts.slice(0, shown).map((c, i) => (
                      <text key={`n${i}`} x={c[0][0]} y={-c[0][1]} className="pv-num">
                        {i + 1}
                      </text>
                    ))}
                  <circle cx={0} cy={0} r={Math.max(1.5, pv.W / 120)} className="pv-origin" />
                </svg>
              )}
            </div>
            <div className="cut-stats">
              {stats && (
                <>
                  <span>
                    <b>{stats.paths}</b> {t("paths")}
                  </span>
                  <span>
                    {t("cut")} <b>{fmtLen(stats.cutLengthMm / (units === "in" ? 1 : 1000), units === "in" ? "in" : "mm", 2)}</b>{" "}
                    {units === "in" ? "in" : "m"}
                  </span>
                  <span>
                    {t("travel")} <b>{fmtLen(stats.travelLengthMm / (units === "in" ? 1 : 1000), units === "in" ? "in" : "mm", 2)}</b>{" "}
                    {units === "in" ? "in" : "m"}
                  </span>
                  <span>
                    {t("uses")} <b>{fmtLen(Math.max(0, stats.maxX), units)}</b> × <b>{fmtLen(Math.max(0, stats.maxY), units)}</b> {units}
                  </span>
                  <span className="muted">{t("{bytes} bytes", { bytes: fmtInt(preview!.bytes) })}</span>
                </>
              )}
            </div>
            {stats?.warnings.map((w, i) => (
              <div className="warn small" key={i}>
                {tb(w)}
              </div>
            ))}
            {showData && preview && <pre className="plot-data">{preview.dataHead}</pre>}
          </div>

          <div className="cut-settings">
            <section>
              <h3>{t("Cutter")}</h3>
              <MachineSelect
                machines={props.machines}
                manufacturer={config.manufacturer}
                model={config.model}
                onChange={(m, mo) => props.onConfig({ ...config, manufacturer: m, model: mo })}
              />
              <button className="link small" onClick={props.onLoadDrivers}>
                {t("Load SignCut driver pack (drivers.pak / .xml)…")}
              </button>
            </section>

            <section>
              <h3>
                {t("Connection")}{" "}
                <button className="mini" onClick={scan} disabled={scanning}>
                  {scanning ? t("Scanning…") : `↻ ${t("Rescan")}`}
                </button>
              </h3>
              <label className="field">
                <span className="field-label">{t("Port")}</span>
                <select
                  value={portKey(port)}
                  onChange={(e) => {
                    const v = e.target.value;
                    if (v === "tcp") choosePort({ kind: "tcp", host: port?.kind === "tcp" ? port.host : "192.168.1.100", port: port?.kind === "tcp" ? port.port : 9100 });
                    else if (v === "file") choosePort({ kind: "file", path: "" });
                    else {
                      const p = ports.find((x) => portKey(x.port) === v);
                      if (p) choosePort(p.port);
                    }
                  }}
                >
                  <option value="" disabled>
                    {t("Choose…")}
                  </option>
                  {ports.map((p) => (
                    <option key={portKey(p.port)} value={portKey(p.port)}>
                      {p.likelyCutter ? "★ " : ""}
                      {p.label} {p.detail ? `— ${p.detail}` : ""}
                    </option>
                  ))}
                  {port?.kind === "serial" && !ports.some((p) => portKey(p.port) === portKey(port)) && (
                    <option value={portKey(port)}>{t("{port} (not connected)", { port: port.path })}</option>
                  )}
                  <option value="tcp">{t("Network (TCP/IP)…")}</option>
                  <option value="file">{t("Save to .plt file")}</option>
                </select>
              </label>
              {port?.kind === "serial" && (
                <div className="grid3">
                  <label className="field">
                    <span className="field-label">Baud</span>
                    <select value={port.baud} onChange={(e) => props.onConfig({ ...config, port: { ...port, baud: +e.target.value } })}>
                      {[1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200].map((b) => (
                        <option key={b} value={b}>
                          {b}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="field">
                    <span className="field-label">{t("Handshake")}</span>
                    <select value={port.flow} onChange={(e) => props.onConfig({ ...config, port: { ...port, flow: e.target.value as "none" } })}>
                      <option value="hardware">RTS/CTS</option>
                      <option value="software">XON/XOFF</option>
                      <option value="none">{t("None")}</option>
                    </select>
                  </label>
                  <label className="check">
                    <input type="checkbox" checked={port.dtr} onChange={(e) => props.onConfig({ ...config, port: { ...port, dtr: e.target.checked } })} /> DTR
                  </label>
                </div>
              )}
              {port?.kind === "tcp" && (
                <div className="grid2">
                  <label className="field">
                    <span className="field-label">{t("Host / IP")}</span>
                    <input value={port.host} onChange={(e) => props.onConfig({ ...config, port: { ...port, host: e.target.value } })} onKeyDown={(e) => e.stopPropagation()} />
                  </label>
                  <NumField label={t("Port")} value={port.port} digits={0} min={1} max={65535} onCommit={(v) => props.onConfig({ ...config, port: { ...port, port: Math.round(v) } })} />
                </div>
              )}
              {ports.length === 0 && !scanning && (
                <div className="muted small">
                  {t("No cutter found. VEVOR cutters connect as a USB-serial port (“/dev/cu.wchusbserial…” or “/dev/cu.usbserial…”). Check the cable and that the cutter is switched on, then rescan.")}
                </div>
              )}
            </section>

            <section>
              <h3>{t("Knife")}</h3>
              <div className="grid2">
                <NumField
                  label={t("Blade offset")}
                  value={s.useBladeOffset ? s.bladeOffset : 0}
                  suffix="mm"
                  step={0.05}
                  min={0}
                  max={2}
                  disabled={!s.useBladeOffset}
                  onCommit={(v) => setSettings({ bladeOffset: v })}
                  title={t("Drag-knife compensation. 45° blade ≈ 0.25 mm, 60° ≈ 0.5 mm. Use 0 for pens or cutters with built-in compensation.")}
                />
                <NumField label={t("Overcut")} value={s.overcut} suffix="mm" step={0.25} min={0} max={10} onCommit={(v) => setSettings({ overcut: v })} title={t("Extra cut past the start of closed shapes so corners close cleanly")} />
                <NumField label={t("Passes")} value={s.passes} digits={0} min={1} max={10} onCommit={(v) => setSettings({ passes: Math.round(v) })} />
                {profile && profile.pens > 1 ? (
                  <label className="field">
                    <span className="field-label">{t("Tool")}</span>
                    <select value={s.tool} onChange={(e) => setSettings({ tool: +e.target.value })}>
                      {Array.from({ length: profile.pens }, (_, i) => (
                        <option key={i} value={i + 1}>
                          {profile.toolNames[i] ? t(profile.toolNames[i]) : t("Tool {n}", { n: i + 1 })}
                        </option>
                      ))}
                    </select>
                  </label>
                ) : (
                  <span />
                )}
              </div>
              <label className="check">
                <input type="checkbox" checked={s.useBladeOffset} onChange={(e) => setSettings({ useBladeOffset: e.target.checked })} /> {t("Blade offset compensation")}
              </label>
              {(supportsSpeed || supportsForce) && (
                <>
                  <label className="check">
                    <input type="checkbox" checked={config.encode.sendSpeedForce} onChange={(e) => setEncode({ sendSpeedForce: e.target.checked })} /> {t("Send speed & force from software")}
                  </label>
                  {config.encode.sendSpeedForce && (
                    <div className="grid2">
                      {supportsSpeed && (
                        <NumField
                          label={`${t("Speed")}${profile?.maxSpeed ? ` (${profile.minSpeed ?? 1}–${profile.maxSpeed})` : ""}`}
                          value={s.speed}
                          digits={0}
                          min={0}
                          onCommit={(v) => setSettings({ speed: v || null })}
                          title={t("Leave empty to use the cutter's panel setting")}
                        />
                      )}
                      {supportsForce && (
                        <NumField
                          label={`${t("Force")}${profile?.maxForce ? ` (${profile.minForce ?? 1}–${profile.maxForce})` : ""}`}
                          value={s.force}
                          digits={0}
                          min={0}
                          onCommit={(v) => setSettings({ force: v || null })}
                          title={t("Leave empty to use the cutter's panel setting")}
                        />
                      )}
                    </div>
                  )}
                </>
              )}
              {profile && !supportsSpeed && !supportsForce && (
                <div className="muted small">{t("Speed and force are set on the cutter's control panel for this model.")}</div>
              )}
            </section>

            <section>
              <h3>{t("Layout")}</h3>
              <label className="field">
                <span className="field-label">{t("Position")}</span>
                <div className="seg">
                  <button className={s.placement === "asPlaced" ? "on" : ""} onClick={() => setSettings({ placement: "asPlaced" })} title={t("Cut exactly where it is on the sheet")}>
                    {t("As placed")}
                  </button>
                  <button className={s.placement === "origin" ? "on" : ""} onClick={() => setSettings({ placement: "origin" })} title={t("Move the job to the origin to save material")}>
                    {t("At origin")}
                  </button>
                </div>
              </label>
              {s.placement === "origin" && <NumField label={t("Margin")} value={s.margin} suffix="mm" min={0} onCommit={(v) => setSettings({ margin: v })} />}
              <label className="check">
                <input type="checkbox" checked={s.mirror} onChange={(e) => setSettings({ mirror: e.target.checked })} /> {t("Mirror (heat-transfer vinyl / window back-side)")}
              </label>
              <div className="grid2">
                <NumField label={t("Copies")} value={s.copies} digits={0} min={1} max={500} onCommit={(v) => setSettings({ copies: Math.round(v) })} />
                <NumField label={t("Gap")} value={s.copyGap} suffix="mm" min={0} onCommit={(v) => setSettings({ copyGap: v })} disabled={s.copies < 2} />
              </div>
              {s.copies > 1 && (
                <label className="check">
                  <input type="checkbox" checked={s.stackCopies} onChange={(e) => setSettings({ stackCopies: e.target.checked })} /> {t("Stack copies across the width")}
                </label>
              )}
              <div className="row gap">
                <label className="check">
                  <input type="checkbox" checked={s.weedBorder !== null} onChange={(e) => setSettings({ weedBorder: e.target.checked ? 3 : null })} /> {t("Weeding border")}
                </label>
                {s.weedBorder !== null && <NumField label="" value={s.weedBorder} suffix="mm" min={0} onCommit={(v) => setSettings({ weedBorder: v })} />}
              </div>
            </section>

            <section>
              <h3>{t("Cutting order")}</h3>
              <label className="field">
                <span className="field-label">{t("Sort")}</span>
                <select value={s.sort} onChange={(e) => setSettings({ sort: e.target.value as CutSettings["sort"] })}>
                  <option value="nearest">{t("Shortest travel")}</option>
                  <option value="bands">{t("Along the length in bands (long jobs)")}</option>
                  <option value="none">{t("Document order")}</option>
                </select>
              </label>
              {s.sort === "bands" && <NumField label={t("Band length")} value={s.bandWidth} suffix="mm" min={20} onCommit={(v) => setSettings({ bandWidth: v })} />}
              <label className="check">
                <input type="checkbox" checked={s.insideFirst} onChange={(e) => setSettings({ insideFirst: e.target.checked })} /> {t("Cut inner shapes first")}
              </label>
              <label className="check">
                <input type="checkbox" checked={s.tangentialEmulation} onChange={(e) => setSettings({ tangentialEmulation: e.target.checked })} /> {t("Turn blade before each cut (tangential emulation)")}
              </label>
            </section>

            {props.colors.length > 1 && (
              <section>
                <h3>{t("Colours")}</h3>
                <ul className="layer-list">
                  {orderedColors.map((c, i) => {
                    const l = layerFor(c);
                    return (
                      <li key={c}>
                        <input type="checkbox" checked={l.enabled} onChange={(e) => setLayer(c, { enabled: e.target.checked })} title={t("Cut this colour")} />
                        <i className="swatch" style={{ background: c }} />
                        <span className="mono small">{c}</span>
                        <label className="check small" title={t("Pause before this colour to change tool or material")}>
                          <input type="checkbox" checked={l.pauseBefore} disabled={i === 0} onChange={(e) => setLayer(c, { pauseBefore: e.target.checked })} /> {t("pause")}
                        </label>
                        <span className="grow" />
                        <button className="mini" onClick={() => moveLayer(c, -1)} disabled={i === 0}>
                          ↑
                        </button>
                        <button className="mini" onClick={() => moveLayer(c, 1)} disabled={i === orderedColors.length - 1}>
                          ↓
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            )}

            <section>
              <h3>{t("After cutting")}</h3>
              <select value={s.afterCut} onChange={(e) => setSettings({ afterCut: e.target.value as CutSettings["afterCut"] })}>
                <option value="returnToOrigin">{t("Go back to the beginning")}</option>
                <option value="feedPastJob">{t("End after job (feed to the end of the job)")}</option>
                <option value="stay">{t("Leave the head where it stops")}</option>
              </select>
              {s.afterCut === "feedPastJob" && <NumField label={t("Extra feed")} value={s.feedExtra} suffix="mm" min={0} onCommit={(v) => setSettings({ feedExtra: v })} />}
              {profile?.commands.pageFeed && (
                <label className="check">
                  <input type="checkbox" checked={config.encode.sendPageFeed} onChange={(e) => setEncode({ sendPageFeed: e.target.checked })} /> {t("Send page-feed command")} (<code>{profile.commands.pageFeed}</code>) — {t("SignCut always does")}
                </label>
              )}
            </section>

            <details>
              <summary>{t("Advanced")}</summary>
              <label className="field">
                <span className="field-label">{t("Axis order")}</span>
                <select
                  value={config.encode.swapXy === null ? "auto" : config.encode.swapXy ? "swap" : "normal"}
                  onChange={(e) => setEncode({ swapXy: e.target.value === "auto" ? null : e.target.value === "swap" })}
                >
                  <option value="auto">{t("Automatic (from driver)")}</option>
                  <option value="normal">{t("X = feed, Y = carriage")}</option>
                  <option value="swap">{t("Swapped")}</option>
                </select>
              </label>
              <NumField label={t("Curve precision")} value={s.curveTolerance} suffix="mm" step={0.01} min={0.005} max={1} digits={3} onCommit={(v) => setSettings({ curveTolerance: v })} />
              <NumField label={t("Test cut size")} value={config.testCutSize} suffix="mm" min={5} max={100} onCommit={(v) => props.onConfig({ ...config, testCutSize: v })} />
              <label className="check">
                <input type="checkbox" checked={showData} onChange={(e) => setShowData(e.target.checked)} /> {t("Show plot data")}
              </label>
            </details>
          </div>
        </div>

        {error && <div className="error banner">{error}</div>}
        {doneMsg && !error && <div className="ok banner">{doneMsg}</div>}

        <div className="modal-foot">
          <div className="row gap">
            <button onClick={testFeed} disabled={!!sending || port?.kind === "file"} title={t("Feed the material forward and back to check tracking")}>
              {t("Test feed")}
            </button>
            <button onClick={testCut} disabled={!!sending || port?.kind === "file"} title={t("Cut a small square with a triangle near the origin")}>
              {t("Test cut")}
            </button>
            <button onClick={exportPlt} disabled={!!sending || !n}>
              {t("Save .plt…")}
            </button>
          </div>
          <div className="row gap">
            <button onClick={props.onClose} disabled={!!sending}>
              {t("Close")}
            </button>
            <button className="primary big" onClick={start} disabled={!!sending || !n}>
              {port?.kind === "file" ? t("Save plot file") : t("Cut")}
            </button>
          </div>
        </div>

        {sending && (
          <div className="sending">
            <div className="sending-box">
              <h3>{pauseMsg ? t("Paused") : sending === "test" ? t("Sending test…") : t("Cutting…")}</h3>
              {pauseMsg ? (
                <>
                  <p>{pauseMsg}</p>
                  <div className="row gap">
                    <button onClick={() => api.jobResume(false)}>{t("Cancel job")}</button>
                    <button className="primary" onClick={() => api.jobResume(true)}>
                      {t("Continue")}
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className="progress">
                    <div style={{ width: `${progress ? (100 * progress.sent) / Math.max(1, progress.total) : 0}%` }} />
                  </div>
                  <div className="muted small">
                    {progress ? t("{sent} / {total} bytes", { sent: fmtInt(progress.sent), total: fmtInt(progress.total) }) : t("Connecting…")}
                  </div>
                  <button onClick={() => api.jobCancel()}>{t("Stop sending")}</button>
                  <div className="muted small">{t("Stopping only halts data still in the computer; press pause/reset on the cutter to stop the head.")}</div>
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
