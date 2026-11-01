export interface ColorGroup {
  kind: "path" | "tag";
  query: string;
  color: string;
}

const STORAGE_KEY = "cairn.graph.groups";

const isValid = (g: unknown): g is ColorGroup =>
  typeof g === "object" &&
  g !== null &&
  ((g as ColorGroup).kind === "path" || (g as ColorGroup).kind === "tag") &&
  typeof (g as ColorGroup).query === "string" &&
  typeof (g as ColorGroup).color === "string";

export function loadColorGroups(): ColorGroup[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isValid);
  } catch {
    return [];
  }
}

export function saveColorGroups(groups: ColorGroup[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(groups));
  } catch {
    // ignore (private mode / quota)
  }
}

/** First group that matches the node → the group itself; else null.
 *  path: case-insensitive substring of the path. tag: exact (case-insensitive)
 *  membership in the note's tags (the real engine preserves tag case, so compare
 *  case-insensitively). */
export function matchGroup(
  path: string,
  tags: string[],
  groups: ColorGroup[],
): ColorGroup | null {
  const lowerPath = path.toLowerCase();
  const lowerTags = tags.map((t) => t.toLowerCase());
  for (const g of groups) {
    const q = g.query.trim().toLowerCase();
    if (!q) continue;
    if (g.kind === "path" ? lowerPath.includes(q) : lowerTags.includes(q)) {
      return g;
    }
  }
  return null;
}

/** First group that matches the node → its color; else null. */
export function matchGroupColor(
  path: string,
  tags: string[],
  groups: ColorGroup[],
): string | null {
  return matchGroup(path, tags, groups)?.color ?? null;
}
