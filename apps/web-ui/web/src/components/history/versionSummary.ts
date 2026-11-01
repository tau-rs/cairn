import type { Revision } from "../../contract/Revision";

// Fallback for revisions the engine committed without a `summary`: parse the
// deterministic label template, which elides zero sides —
//   Edit "Q3 Roadmap" § Goals (+112/−8 words)
//   Edit "A" (+112 words)
//   Edit "A" (−8 words)
// Accepts U+2212 minus (the template) and ASCII hyphen (defensive).
const BOTH_RE = /\(\+(\d+)\/[−-](\d+) words\)/;
const ADDED_RE = /\(\+(\d+) words\)/;
const REMOVED_RE = /\([−-](\d+) words\)/;

export function versionWordDelta(
  r: Revision,
): { added: number; removed: number } | null {
  if (r.summary)
    return { added: r.summary.words_added, removed: r.summary.words_removed };
  const both = BOTH_RE.exec(r.message);
  if (both) return { added: Number(both[1]), removed: Number(both[2]) };
  const added = ADDED_RE.exec(r.message);
  if (added) return { added: Number(added[1]), removed: 0 };
  const removed = REMOVED_RE.exec(r.message);
  if (removed) return { added: 0, removed: Number(removed[1]) };
  return null;
}
