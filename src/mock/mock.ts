// Browser-only mock of the Tauri backend, for UI development and screenshots.
import fixture from "./fixture.json";
import type { ImportedDesign, JobPreview, JobRequest, PortInfo } from "../types";

const listeners = new Map<string, ((p: unknown) => void)[]>();

export function mockListen(event: string, cb: (p: unknown) => void) {
  const l = listeners.get(event) ?? [];
  l.push(cb);
  listeners.set(event, l);
  return Promise.resolve(() => {
    listeners.set(event, (listeners.get(event) ?? []).filter((x) => x !== cb));
  });
}

function emit(event: string, payload: unknown) {
  for (const cb of listeners.get(event) ?? []) cb(payload);
}

const ports: PortInfo[] = [
  {
    port: { kind: "serial", path: "/dev/cu.wchusbserial1410", baud: 9600, flow: "hardware", dtr: false },
    label: "/dev/cu.wchusbserial1410",
    detail: "USB Serial (1a86:7523)",
    likelyCutter: true,
  },
  {
    port: { kind: "tcp", host: "192.168.1.50", port: 9100 },
    label: "Network (TCP/IP)…",
    detail: "",
    likelyCutter: false,
  },
];

/** Flatten our normalized path data (M/L/C/Z) for the mock preview. */
function flatten(d: string, m: number[]): [number, number][][] {
  const out: [number, number][][] = [];
  const nums = d.match(/[MLCZ]|-?\d*\.?\d+(?:e[-+]?\d+)?/gi) ?? [];
  let cur: [number, number][] = [];
  let i = 0;
  const tf = (x: number, y: number): [number, number] => [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
  let px = 0,
    py = 0,
    sx = 0,
    sy = 0;
  while (i < nums.length) {
    const c = nums[i++];
    if (c === "M") {
      if (cur.length > 1) out.push(cur);
      px = +nums[i++];
      py = +nums[i++];
      sx = px;
      sy = py;
      cur = [tf(px, py)];
    } else if (c === "L") {
      px = +nums[i++];
      py = +nums[i++];
      cur.push(tf(px, py));
    } else if (c === "C") {
      const [x1, y1, x2, y2, x3, y3] = nums.slice(i, i + 6).map(Number);
      i += 6;
      for (let k = 1; k <= 8; k++) {
        const t = k / 8,
          u = 1 - t;
        cur.push(
          tf(
            u * u * u * px + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3,
            u * u * u * py + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3,
          ),
        );
      }
      px = x3;
      py = y3;
    } else if (c === "Z" || c === "z") {
      cur.push(tf(sx, sy));
      out.push(cur);
      cur = [];
    }
  }
  if (cur.length > 1) out.push(cur);
  return out;
}

function preview(job: JobRequest): JobPreview {
  const W = job.settings.materialWidth;
  const cuts: [number, number][][] = [];
  for (const o of job.objects) {
    for (const p of o.paths) {
      for (const pl of flatten(p.d, o.transform)) cuts.push(pl.map(([x, y]) => [x, W - y]));
    }
  }
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity,
    len = 0;
  for (const c of cuts)
    c.forEach(([x, y], k) => {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
      if (k) len += Math.hypot(x - c[k - 1][0], y - c[k - 1][1]);
    });
  return {
    stats: { paths: cuts.length, cutLengthMm: len, travelLengthMm: len * 0.3, minX, minY, maxX, maxY, warnings: [] },
    cuts,
    bytes: Math.round(len * 3),
    dataHead: ";:H A L0 ECN U U1200,800 D1240,812 D1290,860 …",
    pauses: [],
  };
}

export async function mockInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case "fonts_list":
      return fixture.fonts;
    case "machines_list":
      return fixture.machines;
    case "machine_profile":
      return { ...fixture.profile, manufacturer: args?.manufacturer, model: args?.model };
    case "ports_list":
      return ports;
    case "import_file":
      return fixture.heart as ImportedDesign;
    case "text_render":
      return fixture.text as ImportedDesign;
    case "supported_extensions":
      return ["svg", "plt", "hpgl", "dxf"];
    case "drivers_detect_signcut":
      return { found: false, modelsLoaded: 0 };
    case "startup_files":
      return [];
    case "job_preview":
      return preview(args!.job as JobRequest);
    case "job_start":
    case "test_cut":
    case "test_feed_cmd": {
      let sent = 0;
      const total = 48000;
      const t = setInterval(() => {
        sent = Math.min(total, sent + 4000);
        emit("cut-progress", { sent, total, chunk: 0, chunks: 1 });
        if (sent >= total) {
          clearInterval(t);
          emit("cut-status", { state: "done", message: "" });
        }
      }, 120);
      return total;
    }
    default:
      return null;
  }
}
