/** Case-insensitive subsequence match. Returns a score (higher = better; rewards
 *  contiguous runs and word-start/prefix matches) or null if `query` is not a
 *  subsequence of `text`. An empty query scores a neutral 0 (matches all). */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  if (q === "") return 0;
  const t = text.toLowerCase();

  let qi = 0;
  let score = 0;
  let prev = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      const consecutive = ti === prev + 1;
      let bonus = 1;
      if (ti === 0 || /[^a-z0-9]/.test(t[ti - 1])) bonus += 3; // word start
      if (ti === 0) bonus += 2; // prefix
      if (consecutive) bonus += 5; // strong consecutive-run bonus
      score += bonus;
      prev = ti;
      qi++;
    }
  }
  return qi === q.length ? score : null;
}

/** Score each item by its searchable text, drop non-matches, sort by score desc
 *  then text asc. Empty query → all items in their original order. */
export function filterItems<T>(
  items: T[],
  query: string,
  text: (item: T) => string,
): T[] {
  if (query.trim() === "") return items;
  return items
    .map((item) => ({ item, score: fuzzyScore(query, text(item)) }))
    .filter((x): x is { item: T; score: number } => x.score !== null)
    .sort(
      (a, b) => b.score - a.score || text(a.item).localeCompare(text(b.item)),
    )
    .map((x) => x.item);
}
