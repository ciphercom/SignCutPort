import { memo, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { JSX, Ref } from "react";
import type { DesignObject, Sheet, Units } from "../types";
import {
  Box,
  boxContains,
  boxesIntersect,
  matToSvg,
  objectBox,
  objectMatrix,
  rotateObjects,
  scaleObjects,
  unionBox,
  fmtLen,
} from "../geometry";

const RULER = 22;
const HANDLE = 8;

export interface CanvasHandle {
  fit: () => void;
  zoomBy: (f: number) => void;
  zoom100: () => void;
  getZoom: () => number;
}

interface Props {
  objects: DesignObject[];
  selection: string[];
  sheet: Sheet;
  units: Units;
  filled: boolean;
  maxWidth: number | null;
  onSelect: (ids: string[]) => void;
  onChange: (objs: DesignObject[], opts: { transient?: boolean; before?: { objects: DesignObject[]; sheet: Sheet } }) => void;
  onEdit: (o: DesignObject) => void;
  onZoom?: (z: number) => void;
  handleRef?: Ref<CanvasHandle>;
}

type Drag =
  | { mode: "move"; start: [number, number]; orig: DesignObject[]; box: Box }
  | { mode: "scale"; start: [number, number]; orig: DesignObject[]; box: Box; anchor: [number, number]; corner: [number, number] }
  | { mode: "rotate"; start: [number, number]; orig: DesignObject[]; center: [number, number] }
  | { mode: "marquee"; start: [number, number]; cur: [number, number]; additive: boolean; base: string[] }
  | { mode: "pan"; startScreen: [number, number]; pan: [number, number] };

const ObjectLayer = memo(function ObjectLayer({
  objects,
  filled,
}: {
  objects: DesignObject[];
  filled: boolean;
}) {
  return (
    <g>
      {objects.map((o) =>
        o.hidden ? null : (
          <g key={o.id} transform={matToSvg(objectMatrix(o))}>
            {o.paths.map((p, i) => (
              <path
                key={i}
                d={p.d}
                fill={filled ? p.color : "none"}
                fillOpacity={filled ? 0.85 : 0}
                fillRule="evenodd"
                stroke={filled ? "rgba(0,0,0,0.55)" : p.color}
                strokeWidth={filled ? 0.6 : 1.2}
                vectorEffect="non-scaling-stroke"
              />
            ))}
          </g>
        ),
      )}
    </g>
  );
});

function niceStep(minPx: number, zoom: number) {
  const raw = minPx / zoom;
  const p = Math.pow(10, Math.floor(Math.log10(raw)));
  for (const m of [1, 2, 5, 10]) if (m * p >= raw) return m * p;
  return 10 * p;
}

export function Canvas(props: Props) {
  const { objects, selection, sheet, units, filled, onSelect, onChange, onEdit, onZoom, maxWidth } = props;
  const wrap = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [zoom, setZoom] = useState(2); // px per mm
  const [pan, setPan] = useState<[number, number]>([60, 60]);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [space, setSpace] = useState(false);
  const fitted = useRef(false);

  useLayoutEffect(() => {
    const el = wrap.current!;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  const contentBox = useMemo(() => {
    const b = unionBox(objects.map(objectBox));
    const sheetBox = { x: 0, y: 0, w: Math.max(sheet.length, 100), h: sheet.width };
    return b ? unionBox([b, sheetBox])! : sheetBox;
  }, [objects, sheet]);

  const fit = useCallback(() => {
    const vw = size.w - RULER - 60;
    const vh = size.h - RULER - 80;
    const z = Math.max(0.05, Math.min(vw / contentBox.w, vh / contentBox.h));
    setZoom(z);
    setPan([RULER + 30 - contentBox.x * z + (vw - contentBox.w * z) / 2, RULER + 30 - contentBox.y * z + (vh - contentBox.h * z) / 2]);
  }, [size, contentBox]);

  const zoomAt = useCallback(
    (f: number, sx?: number, sy?: number) => {
      const cx = sx ?? size.w / 2;
      const cy = sy ?? size.h / 2;
      setZoom((z) => {
        const nz = Math.min(200, Math.max(0.02, z * f));
        setPan((p) => [cx - ((cx - p[0]) / z) * nz, cy - ((cy - p[1]) / z) * nz]);
        return nz;
      });
    },
    [size],
  );

  useImperativeHandle(props.handleRef, () => ({
    fit,
    zoomBy: (f: number) => zoomAt(f),
    zoom100: () => {
      // 100% = real size on a ~110 dpi display.
      const target = 110 / 25.4;
      zoomAt(target / zoom);
    },
    getZoom: () => zoom,
  }));

  useEffect(() => {
    if (!fitted.current && size.w > 100) {
      fitted.current = true;
      fit();
    }
  }, [size, fit]);

  useEffect(() => onZoom?.(zoom), [zoom, onZoom]);

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.code === "Space" && !(e.target instanceof HTMLInputElement) && !(e.target instanceof HTMLTextAreaElement)) {
        setSpace(true);
        e.preventDefault();
      }
    };
    const up = (e: KeyboardEvent) => e.code === "Space" && setSpace(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);

  // Wheel: pinch / ctrl = zoom, otherwise pan (trackpad friendly).
  useEffect(() => {
    const el = wrap.current!;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = el.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey) {
        zoomAt(Math.exp(-e.deltaY * 0.01), e.clientX - r.left, e.clientY - r.top);
      } else {
        setPan((p) => [p[0] - e.deltaX, p[1] - e.deltaY]);
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomAt]);

  const toWorld = (sx: number, sy: number): [number, number] => [(sx - pan[0]) / zoom, (sy - pan[1]) / zoom];
  const toScreen = (x: number, y: number): [number, number] => [x * zoom + pan[0], y * zoom + pan[1]];

  const selected = useMemo(() => objects.filter((o) => selection.includes(o.id)), [objects, selection]);
  const selBox = useMemo(() => unionBox(selected.map(objectBox)), [selected]);

  const localPoint = (e: React.PointerEvent | React.MouseEvent): [number, number] => {
    const r = wrap.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };

  const hitObject = (wx: number, wy: number): DesignObject | null => {
    const pad = 3 / zoom;
    for (let i = objects.length - 1; i >= 0; i--) {
      const o = objects[i];
      if (!o.hidden && boxContains(objectBox(o), wx, wy, pad)) return o;
    }
    return null;
  };

  const handles = useMemo(() => {
    if (!selBox) return null;
    const [x0, y0] = toScreen(selBox.x, selBox.y);
    const [x1, y1] = toScreen(selBox.x + selBox.w, selBox.y + selBox.h);
    return {
      x0,
      y0,
      x1,
      y1,
      corners: [
        { s: [x0, y0], anchor: [selBox.x + selBox.w, selBox.y + selBox.h], c: [selBox.x, selBox.y], cursor: "nwse-resize" },
        { s: [x1, y0], anchor: [selBox.x, selBox.y + selBox.h], c: [selBox.x + selBox.w, selBox.y], cursor: "nesw-resize" },
        { s: [x1, y1], anchor: [selBox.x, selBox.y], c: [selBox.x + selBox.w, selBox.y + selBox.h], cursor: "nwse-resize" },
        { s: [x0, y1], anchor: [selBox.x + selBox.w, selBox.y], c: [selBox.x, selBox.y + selBox.h], cursor: "nesw-resize" },
      ] as { s: [number, number]; anchor: [number, number]; c: [number, number]; cursor: string }[],
      rot: [(x0 + x1) / 2, y0 - 26] as [number, number],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selBox, zoom, pan]);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button === 2) return;
    (e.target as Element).setPointerCapture?.(e.pointerId);
    const sp = localPoint(e);
    const wp = toWorld(sp[0], sp[1]);
    if (e.button === 1 || space) {
      setDrag({ mode: "pan", startScreen: sp, pan });
      return;
    }
    if (handles && selected.length) {
      if (Math.hypot(sp[0] - handles.rot[0], sp[1] - handles.rot[1]) <= HANDLE) {
        setDrag({ mode: "rotate", start: wp, orig: objects, center: [selBox!.x + selBox!.w / 2, selBox!.y + selBox!.h / 2] });
        return;
      }
      for (const c of handles.corners) {
        if (Math.abs(sp[0] - c.s[0]) <= HANDLE && Math.abs(sp[1] - c.s[1]) <= HANDLE) {
          setDrag({ mode: "scale", start: wp, orig: objects, box: selBox!, anchor: c.anchor, corner: c.c });
          return;
        }
      }
    }
    const hit = hitObject(wp[0], wp[1]);
    if (hit) {
      let sel = selection;
      if (e.shiftKey || e.metaKey) {
        sel = selection.includes(hit.id) ? selection.filter((x) => x !== hit.id) : [...selection, hit.id];
        onSelect(sel);
        if (!sel.includes(hit.id)) return;
      } else if (!selection.includes(hit.id)) {
        sel = [hit.id];
        onSelect(sel);
      }
      const box = unionBox(objects.filter((o) => sel.includes(o.id)).map(objectBox))!;
      setDrag({ mode: "move", start: wp, orig: objects, box });
      return;
    }
    const additive = e.shiftKey || e.metaKey;
    if (!additive) onSelect([]);
    setDrag({ mode: "marquee", start: wp, cur: wp, additive, base: additive ? selection : [] });
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!drag) return;
    const sp = localPoint(e);
    const wp = toWorld(sp[0], sp[1]);
    if (drag.mode === "pan") {
      setPan([drag.pan[0] + sp[0] - drag.startScreen[0], drag.pan[1] + sp[1] - drag.startScreen[1]]);
      return;
    }
    if (drag.mode === "marquee") {
      setDrag({ ...drag, cur: wp });
      return;
    }
    const sel = new Set(selection);
    const moving = drag.orig.filter((o) => sel.has(o.id));
    let updated: DesignObject[] = moving;
    if (drag.mode === "move") {
      let dx = wp[0] - drag.start[0];
      let dy = wp[1] - drag.start[1];
      if (e.shiftKey) {
        if (Math.abs(dx) > Math.abs(dy)) dy = 0;
        else dx = 0;
      }
      // Snap the selection's edges to the sheet edges.
      const t = 6 / zoom;
      const b = drag.box;
      const snapX = [0 - b.x, sheet.length - (b.x + b.w)];
      const snapY = [0 - b.y, sheet.width - (b.y + b.h), sheet.width / 2 - (b.y + b.h / 2)];
      for (const s of snapX) if (Math.abs(dx - s) < t) dx = s;
      for (const s of snapY) if (Math.abs(dy - s) < t) dy = s;
      updated = moving.map((o) => ({ ...o, x: o.x + dx, y: o.y + dy }));
    } else if (drag.mode === "scale") {
      const ax = drag.anchor[0];
      const ay = drag.anchor[1];
      const vx = drag.corner[0] - ax;
      const vy = drag.corner[1] - ay;
      let fx: number, fy: number;
      if (e.shiftKey) {
        fx = Math.abs(vx) > 1e-9 ? (wp[0] - ax) / vx : 1;
        fy = Math.abs(vy) > 1e-9 ? (wp[1] - ay) / vy : 1;
      } else {
        const f = ((wp[0] - ax) * vx + (wp[1] - ay) * vy) / (vx * vx + vy * vy || 1);
        fx = fy = f;
      }
      fx = Math.max(0.001, fx);
      fy = Math.max(0.001, fy);
      updated = scaleObjects(moving, [ax, ay], fx, fy);
    } else if (drag.mode === "rotate") {
      const [cx, cy] = drag.center;
      let a = (Math.atan2(wp[1] - cy, wp[0] - cx) - Math.atan2(drag.start[1] - cy, drag.start[0] - cx)) * (180 / Math.PI);
      if (e.shiftKey) a = Math.round(a / 15) * 15;
      updated = rotateObjects(moving, drag.center, a);
    }
    const byId = new Map(updated.map((o) => [o.id, o]));
    onChange(
      drag.orig.map((o) => byId.get(o.id) ?? o),
      { transient: true },
    );
  };

  const onPointerUp = () => {
    if (!drag) return;
    if (drag.mode === "marquee") {
      const r = {
        x: Math.min(drag.start[0], drag.cur[0]),
        y: Math.min(drag.start[1], drag.cur[1]),
        w: Math.abs(drag.cur[0] - drag.start[0]),
        h: Math.abs(drag.cur[1] - drag.start[1]),
      };
      if (r.w * zoom > 3 || r.h * zoom > 3) {
        const ids = objects.filter((o) => !o.hidden && boxesIntersect(r, objectBox(o))).map((o) => o.id);
        onSelect(Array.from(new Set([...drag.base, ...ids])));
      }
    } else if (drag.mode !== "pan") {
      const changed = drag.orig !== objects;
      if (changed) onChange(objects, { before: { objects: drag.orig, sheet } });
    }
    setDrag(null);
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    const sp = localPoint(e);
    const wp = toWorld(sp[0], sp[1]);
    const hit = hitObject(wp[0], wp[1]);
    if (hit) onEdit(hit);
  };

  // ------------------------------------------------------------ rulers/grid
  const unitMm = units === "in" ? 25.4 : 1;
  const major = niceStep(70, zoom / unitMm) * unitMm; // mm between labels
  const minor = major / 5;
  const [vx0, vy0] = toWorld(RULER, RULER);
  const [vx1, vy1] = toWorld(size.w, size.h);

  const grid = useMemo(() => {
    const lines: JSX.Element[] = [];
    const gStep = minor * zoom < 8 ? major : minor;
    const xs = Math.max(0, Math.ceil(vx0 / gStep) * gStep);
    const xe = Math.min(sheet.length, vx1);
    for (let x = xs; x <= xe + 1e-9; x += gStep) {
      const isMajor = Math.abs(x / major - Math.round(x / major)) < 1e-6;
      lines.push(<line key={`x${x}`} x1={x} y1={0} x2={x} y2={sheet.width} className={isMajor ? "grid-major" : "grid-minor"} />);
    }
    // Horizontal lines are measured from the origin edge (bottom of the sheet).
    const us = Math.max(0, Math.ceil((sheet.width - vy1) / gStep) * gStep);
    const ue = Math.min(sheet.width, sheet.width - vy0);
    for (let u = us; u <= ue + 1e-9; u += gStep) {
      const y = sheet.width - u;
      const isMajor = Math.abs(u / major - Math.round(u / major)) < 1e-6;
      lines.push(<line key={`y${u}`} x1={0} y1={y} x2={sheet.length} y2={y} className={isMajor ? "grid-major" : "grid-minor"} />);
    }
    return lines;
  }, [vx0, vx1, vy0, vy1, major, minor, zoom, sheet]);

  const rulerTicks = (horizontal: boolean) => {
    const els: JSX.Element[] = [];
    // Vertical ruler counts from the origin edge (bottom of the sheet) upward.
    const [a, b] = horizontal ? [vx0, vx1] : [sheet.width - vy1, sheet.width - vy0];
    const start = Math.floor(a / minor) * minor;
    for (let v = start; v <= b; v += minor) {
      const isMajor = Math.abs(v / major - Math.round(v / major)) < 1e-6;
      const s = horizontal ? v * zoom + pan[0] : (sheet.width - v) * zoom + pan[1];
      if (s < RULER) continue;
      const len = isMajor ? RULER : RULER * 0.3;
      if (horizontal) {
        els.push(<line key={v} x1={s} x2={s} y1={RULER} y2={RULER - len} />);
        if (isMajor)
          els.push(
            <text key={`t${v}`} x={s + 3} y={10}>
              {fmtLen(v, units, units === "in" ? 1 : 0)}
            </text>,
          );
      } else {
        els.push(<line key={v} y1={s} y2={s} x1={RULER} x2={RULER - len} />);
        if (isMajor)
          els.push(
            <text key={`t${v}`} x={10} y={s + 3} transform={`rotate(-90 10 ${s + 3})`}>
              {fmtLen(v, units, units === "in" ? 1 : 0)}
            </text>,
          );
      }
    }
    return els;
  };

  const cursor = drag?.mode === "pan" || space ? "grabbing" : "default";
  const [ox, oy] = toScreen(0, sheet.width);

  return (
    <div
      ref={wrap}
      className="canvas-wrap"
      style={{ cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      onContextMenu={(e) => e.preventDefault()}
    >
      <svg width={size.w} height={size.h} className="canvas-svg">
        <g transform={`translate(${pan[0]} ${pan[1]}) scale(${zoom})`}>
          <rect x={0} y={0} width={sheet.length} height={sheet.width} className="sheet" />
          {grid}
          {maxWidth && maxWidth < sheet.width && (
            <line x1={0} x2={sheet.length} y1={sheet.width - maxWidth} y2={sheet.width - maxWidth} className="max-width-line" vectorEffect="non-scaling-stroke" />
          )}
          <ObjectLayer objects={objects} filled={filled} />
        </g>
        {/* origin marker: the sheet's bottom-left = cutter home */}
        <g transform={`translate(${ox} ${oy})`} className="origin">
          <circle r={5} />
          <text x={8} y={16}>
            Origin (cutter home)
          </text>
        </g>
        <text x={ox + sheet.length * zoom - 4} y={oy + 16} className="sheet-caption" textAnchor="end">
          {fmtLen(sheet.length, units)} × {fmtLen(sheet.width, units)} {units} · material feeds →
        </text>
        {selected.map((o) => {
          const b = objectBox(o);
          const [x, y] = toScreen(b.x, b.y);
          return <rect key={o.id} x={x} y={y} width={b.w * zoom} height={b.h * zoom} className="sel-obj" />;
        })}
        {handles && (
          <g className="handles">
            <rect x={handles.x0} y={handles.y0} width={handles.x1 - handles.x0} height={handles.y1 - handles.y0} className="sel-box" />
            <line x1={handles.rot[0]} y1={handles.rot[1]} x2={handles.rot[0]} y2={handles.y0} className="rot-stem" />
            <circle cx={handles.rot[0]} cy={handles.rot[1]} r={HANDLE / 1.4} className="rot-handle" />
            {handles.corners.map((c, i) => (
              <rect key={i} x={c.s[0] - HANDLE / 2} y={c.s[1] - HANDLE / 2} width={HANDLE} height={HANDLE} className="handle" style={{ cursor: c.cursor }} />
            ))}
            {selBox && (
              <text x={handles.x0} y={handles.y0 - 8} textAnchor="start" className="dim-label">
                {fmtLen(selBox.w, units)} × {fmtLen(selBox.h, units)} {units}
              </text>
            )}
          </g>
        )}
        {drag?.mode === "marquee" && (() => {
          const [a0, a1] = toScreen(drag.start[0], drag.start[1]);
          const [b0, b1] = toScreen(drag.cur[0], drag.cur[1]);
          return <rect x={Math.min(a0, b0)} y={Math.min(a1, b1)} width={Math.abs(b0 - a0)} height={Math.abs(b1 - a1)} className="marquee" />;
        })()}
        {/* rulers */}
        <g className="ruler">
          <rect x={0} y={0} width={size.w} height={RULER} />
          {rulerTicks(true)}
        </g>
        <g className="ruler">
          <rect x={0} y={0} width={RULER} height={size.h} />
          {rulerTicks(false)}
        </g>
        <rect x={0} y={0} width={RULER} height={RULER} className="ruler-corner" />
        <text x={RULER / 2} y={14} textAnchor="middle" className="ruler-unit">
          {units}
        </text>
      </svg>
      {objects.length === 0 && (
        <div className="empty-hint">
          <div className="empty-title">Drop SVG, DXF or PLT files here</div>
          <div>or use Import (⌘I) · Text (⌘T)</div>
        </div>
      )}
    </div>
  );
}
