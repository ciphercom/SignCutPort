import { fmtNum } from "./i18n";
import type { DesignObject, JobObject } from "./types";

export type Mat = [number, number, number, number, number, number];
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const identity: Mat = [1, 0, 0, 1, 0, 0];

/** Returns m1 ∘ m2 (apply m2 first). */
export function mul(m1: Mat, m2: Mat): Mat {
  const [a1, b1, c1, d1, e1, f1] = m1;
  const [a2, b2, c2, d2, e2, f2] = m2;
  return [
    a1 * a2 + c1 * b2,
    b1 * a2 + d1 * b2,
    a1 * c2 + c1 * d2,
    b1 * c2 + d1 * d2,
    a1 * e2 + c1 * f2 + e1,
    b1 * e2 + d1 * f2 + f1,
  ];
}

export function apply(m: Mat, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

export function objectMatrix(o: DesignObject): Mat {
  const r = (o.rot * Math.PI) / 180;
  const cos = Math.cos(r);
  const sin = Math.sin(r);
  // T(x,y) · R · S · T(-w/2,-h/2)
  const s: Mat = [o.sx, 0, 0, o.sy, (-o.w / 2) * o.sx, (-o.h / 2) * o.sy];
  const rt: Mat = [cos, sin, -sin, cos, o.x, o.y];
  return mul(rt, s);
}

export function objectBox(o: DesignObject): Box {
  const m = objectMatrix(o);
  const pts = [
    apply(m, 0, 0),
    apply(m, o.w, 0),
    apply(m, o.w, o.h),
    apply(m, 0, o.h),
  ];
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}

export function unionBox(boxes: Box[]): Box | null {
  if (!boxes.length) return null;
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const b of boxes) {
    x0 = Math.min(x0, b.x);
    y0 = Math.min(y0, b.y);
    x1 = Math.max(x1, b.x + b.w);
    y1 = Math.max(y1, b.y + b.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

export function boxContains(b: Box, x: number, y: number, pad = 0) {
  return x >= b.x - pad && x <= b.x + b.w + pad && y >= b.y - pad && y <= b.y + b.h + pad;
}

export function boxesIntersect(a: Box, b: Box) {
  return a.x <= b.x + b.w && a.x + a.w >= b.x && a.y <= b.y + b.h && a.y + a.h >= b.y;
}

export function matToSvg(m: Mat) {
  return `matrix(${m.map((v) => +v.toFixed(6)).join(" ")})`;
}

/** Scale objects about an anchor point (sheet mm). */
export function scaleObjects(
  objs: DesignObject[],
  anchor: [number, number],
  fx: number,
  fy: number,
): DesignObject[] {
  return objs.map((o) => {
    const quarter = Math.round((((o.rot % 180) + 180) % 180) / 90) % 2 === 1;
    const [ox, oy] = quarter ? [fy, fx] : [fx, fy];
    return {
      ...o,
      x: anchor[0] + (o.x - anchor[0]) * fx,
      y: anchor[1] + (o.y - anchor[1]) * fy,
      sx: o.sx * ox,
      sy: o.sy * oy,
    };
  });
}

export function rotateObjects(objs: DesignObject[], center: [number, number], deg: number): DesignObject[] {
  const r = (deg * Math.PI) / 180;
  const c = Math.cos(r);
  const s = Math.sin(r);
  return objs.map((o) => {
    const dx = o.x - center[0];
    const dy = o.y - center[1];
    return {
      ...o,
      x: center[0] + dx * c - dy * s,
      y: center[1] + dx * s + dy * c,
      rot: normDeg(o.rot + deg),
    };
  });
}

export function normDeg(d: number) {
  let r = d % 360;
  if (r > 180) r -= 360;
  if (r <= -180) r += 360;
  return Math.abs(r) < 1e-9 ? 0 : r;
}

/** Mirror objects across the vertical (axis "x") or horizontal axis through `center`. */
export function flipObjects(objs: DesignObject[], axis: "h" | "v", center: [number, number]): DesignObject[] {
  return objs.map((o) => {
    if (axis === "h") {
      // Reflect across a vertical line: x' = 2cx - x; rotation negates; mirror local x.
      return { ...o, x: 2 * center[0] - o.x, rot: normDeg(-o.rot), sx: -o.sx };
    }
    return { ...o, y: 2 * center[1] - o.y, rot: normDeg(-o.rot), sy: -o.sy };
  });
}

export function toJobObject(o: DesignObject): JobObject {
  return { paths: o.paths.map((p) => ({ d: p.d, color: p.color })), transform: objectMatrix(o) };
}

export const MM_PER_IN = 25.4;
export function fmtLen(mm: number, units: "mm" | "in", digits?: number) {
  if (units === "in") return fmtNum(mm / MM_PER_IN, digits ?? 3);
  return fmtNum(mm, digits ?? 1);
}
/** Millimetres converted to display units as a number, rounded to `digits`. */
export function toUnits(mm: number, units: "mm" | "in", digits?: number): number {
  const v = units === "in" ? mm / MM_PER_IN : mm;
  const d = digits ?? (units === "in" ? 3 : 2);
  return Math.round(v * 10 ** d) / 10 ** d;
}

export function parseLen(v: string, units: "mm" | "in"): number | null {
  const n = parseFloat(String(v).replace(",", "."));
  if (!isFinite(n)) return null;
  return units === "in" ? n * MM_PER_IN : n;
}

/**
 * Pack boxes along the roll: columns across the material width starting at
 * the cutter origin (bottom of the sheet), then advancing along the length.
 */
export function arrange(objs: DesignObject[], sheetWidth: number, gap: number, margin: number): DesignObject[] {
  const items = objs
    .map((o) => ({ o, b: objectBox(o) }))
    .sort((a, b) => b.b.w - a.b.w || b.b.h - a.b.h);
  let colX = margin;
  let colW = 0;
  let y = sheetWidth - margin; // bottom edge, moving up
  const out = new Map<string, DesignObject>();
  for (const { o, b } of items) {
    if (y - b.h < margin - 1e-6 && colW > 0) {
      colX += colW + gap;
      colW = 0;
      y = sheetWidth - margin;
    }
    const top = y - b.h;
    const dx = colX - b.x;
    const dy = top - b.y;
    out.set(o.id, { ...o, x: o.x + dx, y: o.y + dy });
    y = top - gap;
    colW = Math.max(colW, b.w);
  }
  return objs.map((o) => out.get(o.id) ?? o);
}

let idCounter = 0;
export function newId() {
  idCounter += 1;
  return `o${Date.now().toString(36)}${idCounter}`;
}

/** "VEVOR KH-720" for manufacturer "VEVOR" / model "VEVOR KH-720". */
export function modelLabel(manufacturer: string, model: string) {
  return model.toLowerCase().startsWith(manufacturer.toLowerCase()) ? model : `${manufacturer} ${model}`;
}
