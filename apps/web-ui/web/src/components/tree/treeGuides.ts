/** Indent-guide geometry + per-level "spine" computation for the folder tree.
 *
 *  The tree draws a vertical guide line per nesting level. A folder's spine runs
 *  down to its *last direct child* and stops (a straight └), and never bleeds
 *  onto rows that belong to a different folder — so each level's line respects
 *  folder boundaries. The "active path" (root → open note) lights its spine in
 *  the accent color; everything else stays grey.
 */

export interface Guide {
  /** Draw a full-height vertical line in this column (an ancestor's spine
   *  continues past this row because that ancestor has a younger sibling). */
  show: boolean;
  /** That spine is part of the active path. */
  accent: boolean;
}

export interface GuideMark {
  /** Left offset in px (center of the column). */
  x: number;
  variant: "line" | "tick";
  accent: boolean;
  /** A connector line that stops at the row's vertical center (└) — used for the
   *  last child so the spine ends instead of running on. */
  toCenter?: boolean;
}

export const GUIDE_COL = 16;
export const GUIDE_BASE = 8;

/** Left padding for a row's content at a given depth (root = 0). */
export function indentPad(depth: number): number {
  return GUIDE_BASE + depth * GUIDE_COL;
}

/** Is `path` the active note or one of its ancestor folders? */
export function onActivePath(path: string, activePath: string | null): boolean {
  if (!activePath) return false;
  return activePath === path || activePath.startsWith(path + "/");
}

/** Pass-through guide columns for the children of a node. Root nodes (depth 0)
 *  contribute none — their children's connector is column 0. Otherwise the
 *  node's own connector column becomes a pass-through that continues iff the
 *  node has a younger sibling (`!isLast`), tinted if the node sits on the path. */
export function childGuides(
  ancestorGuides: Guide[],
  depth: number,
  isLast: boolean,
  parentOnPath: boolean,
): Guide[] {
  if (depth === 0) return [];
  return [...ancestorGuides, { show: !isLast, accent: parentOnPath }];
}

/** The guide marks (vertical lines + the connector tick) to draw for one row. */
export function rowGuides(
  ancestorGuides: Guide[],
  depth: number,
  isLast: boolean,
  parentOnPath: boolean,
): GuideMark[] {
  const marks: GuideMark[] = [];
  ancestorGuides.forEach((g, level) => {
    if (g.show) {
      marks.push({
        x: GUIDE_BASE + level * GUIDE_COL + GUIDE_COL / 2,
        variant: "line",
        accent: g.accent,
      });
    }
  });
  if (depth >= 1) {
    const x = GUIDE_BASE + (depth - 1) * GUIDE_COL + GUIDE_COL / 2;
    marks.push({ x, variant: "line", accent: parentOnPath, toCenter: isLast });
    marks.push({ x, variant: "tick", accent: parentOnPath });
  }
  return marks;
}
