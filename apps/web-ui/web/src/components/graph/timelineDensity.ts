import type { Revision } from "../../contract";
import type { TemporalSelection } from "./temporalControls";

/** Time-proportional activity histogram: bucket `revisions` into `bucketCount`
 *  equal time intervals spanning the oldest→newest timestamp. Fixed length
 *  regardless of commit count, so it never overflows the track. */
export function timelineBuckets(
  revisions: Revision[],
  bucketCount = 12,
): { count: number }[] {
  if (revisions.length === 0) return [];
  const times = revisions.map((r) => Number(r.timestamp_secs));
  // times[0] is safe: empty input already returned above.
  let min = times[0];
  let max = times[0];
  for (const t of times) {
    if (t < min) min = t;
    if (t > max) max = t;
  }
  const span = max - min;
  const buckets = Array.from({ length: bucketCount }, () => ({ count: 0 }));
  for (const t of times) {
    const frac = span === 0 ? 0 : (t - min) / span;
    const idx = Math.min(bucketCount - 1, Math.floor(frac * bucketCount));
    buckets[idx].count += 1;
  }
  return buckets;
}

function fmtDate(secs: number): string {
  // UTC ISO date — deterministic across locales.
  return new Date(secs * 1000).toISOString().slice(0, 10);
}

/** Human-readable "where am I" text for the scrubber banner. Out-of-range
 *  indices degrade to Live (the safe default when scrubber and data desync). */
export function describeSelection(
  selection: TemporalSelection,
  timeline: Revision[],
): { state: string; detail: string } {
  const at = (i: number): Revision | null =>
    i >= 0 && i < timeline.length ? timeline[i] : null;
  const live = { state: "Live", detail: "current vault" };
  if (selection.kind === "live") return live;
  if (selection.kind === "snapshot") {
    const r = at(selection.at);
    return r
      ? {
          state: "Viewing vault as of",
          detail: `${fmtDate(r.timestamp_secs)} — ${r.message}`,
        }
      : live;
  }
  const from = at(selection.from);
  const to = at(selection.to);
  return from && to
    ? {
        state: "Comparing",
        detail: `${fmtDate(from.timestamp_secs)} → ${fmtDate(to.timestamp_secs)}`,
      }
    : live;
}
