import type { Revision } from "../../contract";

export type TemporalSelection =
  | { kind: "live" }
  | { kind: "snapshot"; at: number }
  | { kind: "compare"; from: number; to: number };

export type TemporalRequest =
  | { mode: "live" }
  | { mode: "snapshot"; revision: string }
  | { mode: "compare"; from: string; to: string };

const STORAGE_KEY = "cairn.graph.temporal";

/** Translate a scrubber selection into the temporal request to run. Indices
 *  index into `timeline` as stored (newest-first). A collapsed range degrades
 *  to snapshot; an empty timeline or any out-of-range index falls back to live
 *  (the safe default when the scrubber and data are momentarily out of sync). */
export function selectionToRequest(
  sel: TemporalSelection,
  timeline: Revision[] | null,
): TemporalRequest {
  const at = (i: number): string | null =>
    timeline && i >= 0 && i < timeline.length ? timeline[i].id : null;

  if (sel.kind === "live") return { mode: "live" };
  if (sel.kind === "snapshot") {
    const rev = at(sel.at);
    return rev ? { mode: "snapshot", revision: rev } : { mode: "live" };
  }
  const from = at(sel.from);
  const to = at(sel.to);
  if (!from || !to) return { mode: "live" };
  if (from === to) return { mode: "snapshot", revision: to };
  return { mode: "compare", from, to };
}

export function loadTemporalOpen(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveTemporalOpen(open: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, open ? "1" : "0");
  } catch {
    // ignore (private mode / quota)
  }
}
