// Shared types. Field names match the Rust structs (camelCase serde).

export interface PathData {
  d: string;
  color: string;
  filled?: boolean;
}

export interface FontIssue {
  requested: string;
  substitutedWith: string | null;
  kind: "missing-font" | "missing-glyph" | string;
  detail: string | null;
}

export interface ImportOptions {
  dpi?: number | null;
  fontSubstitutions?: Record<string, string>;
  resourcesDir?: string | null;
}

export interface ImportedDesign {
  name: string;
  paths: PathData[];
  widthMm: number;
  heightMm: number;
  offsetXMm: number;
  offsetYMm: number;
  docWidthMm: number;
  docHeightMm: number;
  textObjects: number;
  fontIssues: FontIssue[];
  warnings: string[];
}

export interface TextRequest {
  text: string;
  family: string;
  weight: number;
  italic: boolean;
  sizeMm: number;
  letterSpacingMm: number;
  lineHeight: number;
  align: "start" | "middle" | "end";
}

export interface FontFace {
  family: string;
  postScriptName: string;
  weight: number;
  italic: boolean;
  styleName: string;
  path: string | null;
}

export interface FontFamilyInfo {
  family: string;
  faces: FontFace[];
}

export interface ObjectSource {
  kind: "file" | "text";
  path?: string;
  options?: ImportOptions;
  text?: TextRequest;
}

export interface DesignObject {
  id: string;
  name: string;
  paths: PathData[];
  /** Local size (mm); local geometry spans [0,w]x[0,h]. */
  w: number;
  h: number;
  /** Centre on the sheet (mm). */
  x: number;
  y: number;
  /** Scale factors; negative = mirrored. */
  sx: number;
  sy: number;
  /** Rotation in degrees (clockwise on screen). */
  rot: number;
  hidden?: boolean;
  source?: ObjectSource;
  fontIssues?: FontIssue[];
}

export interface Sheet {
  /** Material (roll) width in mm = vertical extent on screen. */
  width: number;
  /** Material length in mm = horizontal extent on screen. */
  length: number;
}

export type Units = "mm" | "in";

// ---- machines

export interface ModelSummary {
  name: string;
  maxWidthMm: number;
}
export interface ManufacturerSummary {
  manufacturer: string;
  models: ModelSummary[];
}

export interface MachineCommands {
  initialise: string;
  start: string;
  end: string;
  toolUp: string;
  toolDown: string;
  pageFeed: string;
  afterCut: string;
  delimiter: string;
  terminator: string;
  velocity: string;
  force: string;
  selectPen: string;
}

export interface MachineProfile {
  manufacturer: string;
  model: string;
  maxWidthMm: number;
  maxLengthMm: number;
  language: string;
  xResolution: number;
  yResolution: number;
  useKnifeCompensation: boolean;
  defaultBladeOffset: number;
  defaultBaud: number;
  rts: boolean;
  cts: boolean;
  dtr: boolean;
  dsr: boolean;
  vendorId: number | null;
  productId: number | null;
  pens: number;
  toolNames: string[];
  minSpeed: number | null;
  maxSpeed: number | null;
  minForce: number | null;
  maxForce: number | null;
  supportsContourCut: boolean;
  swapAxis: boolean;
  commands: MachineCommands;
}

// ---- ports

export type Port =
  | { kind: "serial"; path: string; baud: number; flow: "none" | "hardware" | "software"; dtr: boolean }
  | { kind: "usb"; vendorId: number; productId: number; serial: string | null }
  | { kind: "tcp"; host: string; port: number }
  | { kind: "printer"; name: string }
  | { kind: "file"; path: string };

export interface PortInfo {
  port: Port;
  label: string;
  detail: string;
  likelyCutter: boolean;
}

// ---- cutting

export type SortMode = "none" | "nearest" | "bands";
export type Placement = "asPlaced" | "origin";
export type AfterCut = "returnToOrigin" | "feedPastJob" | "stay";

export interface LayerSettings {
  enabled: boolean;
  passes?: number | null;
  speed?: number | null;
  force?: number | null;
  overcut?: number | null;
  bladeOffset?: number | null;
  tool?: number | null;
  pauseBefore: boolean;
}

export interface CutSettings {
  materialWidth: number;
  bladeOffset: number;
  useBladeOffset: boolean;
  tangentialEmulation: boolean;
  overcut: number;
  passes: number;
  speed: number | null;
  force: number | null;
  tool: number;
  sort: SortMode;
  bandWidth: number;
  insideFirst: boolean;
  mirror: boolean;
  placement: Placement;
  margin: number;
  weedBorder: number | null;
  copies: number;
  copyGap: number;
  stackCopies: boolean;
  afterCut: AfterCut;
  feedExtra: number;
  curveTolerance: number;
  layers: [string, LayerSettings][];
}

export interface EncodeOptions {
  sendSpeedForce: boolean;
  swapXy: boolean | null;
  afterCut: AfterCut;
  feedExtra: number;
  sendPageFeed: boolean;
}

export interface JobObject {
  paths: { d: string; color: string }[];
  transform: [number, number, number, number, number, number];
}

export interface JobRequest {
  objects: JobObject[];
  settings: CutSettings;
  encode: EncodeOptions;
  manufacturer: string;
  model: string;
}

export interface PlanStats {
  paths: number;
  cutLengthMm: number;
  travelLengthMm: number;
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  warnings: string[];
}

export interface JobPreview {
  stats: PlanStats;
  cuts: [number, number][][];
  bytes: number;
  dataHead: string;
  pauses: string[];
}

export interface CutProgress {
  sent: number;
  total: number;
  chunk: number;
  chunks: number;
}

export interface CutStatus {
  state: "pause" | "done" | "error" | "cancelled";
  message: string;
}
