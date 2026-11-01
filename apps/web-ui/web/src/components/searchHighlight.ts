export interface SearchSnippet {
  snippet: string;
  highlights: [number, number][]; // [start, end) offsets within `snippet`
}

export interface SnippetSegment {
  text: string;
  match: boolean;
}

/** Slice `snippet` into alternating plain/matched segments. Ranges are clamped to
 *  [0, len], invalid/empty dropped, sorted, and overlapping/adjacent merged. No
 *  highlights → a single plain segment (or [] for an empty snippet). */
export function splitSnippet(
  snippet: string,
  highlights: [number, number][],
): SnippetSegment[] {
  const len = snippet.length;
  const norm = highlights
    .map(
      ([s, e]) =>
        [Math.max(0, Math.min(s, len)), Math.max(0, Math.min(e, len))] as [
          number,
          number,
        ],
    )
    .filter(([s, e]) => e > s)
    .sort((a, b) => a[0] - b[0]);

  const merged: [number, number][] = [];
  for (const [s, e] of norm) {
    const last = merged[merged.length - 1];
    if (last && s <= last[1]) last[1] = Math.max(last[1], e);
    else merged.push([s, e]);
  }

  const segments: SnippetSegment[] = [];
  let cursor = 0;
  for (const [s, e] of merged) {
    if (s > cursor)
      segments.push({ text: snippet.slice(cursor, s), match: false });
    segments.push({ text: snippet.slice(s, e), match: true });
    cursor = e;
  }
  if (cursor < len)
    segments.push({ text: snippet.slice(cursor), match: false });
  return segments;
}
