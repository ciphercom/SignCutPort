import { useCallback, useReducer } from "react";
import type { DesignObject, Sheet } from "./types";

interface Snapshot {
  objects: DesignObject[];
  sheet: Sheet;
}

export interface DocState {
  objects: DesignObject[];
  selection: string[];
  sheet: Sheet;
  past: Snapshot[];
  future: Snapshot[];
  dirty: boolean;
  filePath: string | null;
}

type Action =
  | { type: "set"; objects: DesignObject[]; selection?: string[]; before?: Snapshot | null }
  | { type: "select"; ids: string[] }
  | { type: "sheet"; sheet: Sheet; history?: boolean }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "load"; objects: DesignObject[]; sheet: Sheet; filePath: string | null }
  | { type: "saved"; filePath: string };

const LIMIT = 100;

function reducer(s: DocState, a: Action): DocState {
  switch (a.type) {
    case "set": {
      const valid = new Set(a.objects.map((o) => o.id));
      const selection = (a.selection ?? s.selection).filter((id) => valid.has(id));
      if (a.before === null) {
        // Transient update (during a drag): no history entry.
        return { ...s, objects: a.objects, selection };
      }
      const before = a.before ?? { objects: s.objects, sheet: s.sheet };
      return {
        ...s,
        objects: a.objects,
        selection,
        past: [...s.past, before].slice(-LIMIT),
        future: [],
        dirty: true,
      };
    }
    case "select":
      return { ...s, selection: a.ids };
    case "sheet":
      if (a.history === false) return { ...s, sheet: a.sheet };
      return {
        ...s,
        sheet: a.sheet,
        past: [...s.past, { objects: s.objects, sheet: s.sheet }].slice(-LIMIT),
        future: [],
        dirty: true,
      };
    case "undo": {
      const prev = s.past[s.past.length - 1];
      if (!prev) return s;
      return {
        ...s,
        objects: prev.objects,
        sheet: prev.sheet,
        past: s.past.slice(0, -1),
        future: [{ objects: s.objects, sheet: s.sheet }, ...s.future],
        selection: s.selection.filter((id) => prev.objects.some((o) => o.id === id)),
        dirty: true,
      };
    }
    case "redo": {
      const next = s.future[0];
      if (!next) return s;
      return {
        ...s,
        objects: next.objects,
        sheet: next.sheet,
        past: [...s.past, { objects: s.objects, sheet: s.sheet }],
        future: s.future.slice(1),
        selection: s.selection.filter((id) => next.objects.some((o) => o.id === id)),
        dirty: true,
      };
    }
    case "load":
      return {
        objects: a.objects,
        sheet: a.sheet,
        selection: [],
        past: [],
        future: [],
        dirty: false,
        filePath: a.filePath,
      };
    case "saved":
      return { ...s, dirty: false, filePath: a.filePath };
  }
}

export function useDocument(initialSheet: Sheet) {
  const [state, dispatch] = useReducer(reducer, {
    objects: [],
    selection: [],
    sheet: initialSheet,
    past: [],
    future: [],
    dirty: false,
    filePath: null,
  });

  const setObjects = useCallback(
    (objects: DesignObject[], opts?: { selection?: string[]; transient?: boolean; before?: Snapshot }) =>
      dispatch({
        type: "set",
        objects,
        selection: opts?.selection,
        before: opts?.transient ? null : opts?.before,
      }),
    [],
  );
  const select = useCallback((ids: string[]) => dispatch({ type: "select", ids }), []);
  const setSheet = useCallback((sheet: Sheet, history = true) => dispatch({ type: "sheet", sheet, history }), []);
  const undo = useCallback(() => dispatch({ type: "undo" }), []);
  const redo = useCallback(() => dispatch({ type: "redo" }), []);
  const load = useCallback(
    (objects: DesignObject[], sheet: Sheet, filePath: string | null) =>
      dispatch({ type: "load", objects, sheet, filePath }),
    [],
  );
  const markSaved = useCallback((filePath: string) => dispatch({ type: "saved", filePath }), []);

  return { state, setObjects, select, setSheet, undo, redo, load, markSaved };
}

export type DocApi = ReturnType<typeof useDocument>;

/** Document file format (.scport): plain JSON. */
export interface DocFile {
  format: "signcut-port";
  version: 1;
  sheet: Sheet;
  objects: DesignObject[];
}

export function serializeDoc(objects: DesignObject[], sheet: Sheet): string {
  const f: DocFile = { format: "signcut-port", version: 1, sheet, objects };
  return JSON.stringify(f);
}

export function parseDoc(text: string): DocFile {
  const f = JSON.parse(text) as DocFile;
  if (f.format !== "signcut-port" || !Array.isArray(f.objects)) throw new Error("Not a SignCut Port document");
  return f;
}
