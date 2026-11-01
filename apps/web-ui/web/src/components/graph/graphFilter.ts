import type { GraphNode, GraphEdge } from "../../contract";
import { matchGroup, type ColorGroup } from "./colorGroups";

export interface FilterSettings {
  minDegree: number;
  hiddenGroupQueries: string[]; // group.query values whose nodes are hidden
  hideUngrouped: boolean; // hide nodes that match no group ("other" row)
}
export const DEFAULT_FILTER: FilterSettings = {
  minDegree: 0,
  hiddenGroupQueries: [],
  hideUngrouped: false,
};
const STORAGE_KEY = "cairn.graph.filter";

export function applyFilters(
  nodes: GraphNode[],
  edges: GraphEdge[],
  f: FilterSettings,
  groups: ColorGroup[],
): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const hidden = new Set(
    f.hiddenGroupQueries.map((q) => q.trim().toLowerCase()),
  );
  const visible = nodes.filter((node) => {
    if (node.degree < f.minDegree) return false;
    const g = matchGroup(node.path, node.tags, groups);
    if (g) return !hidden.has(g.query.trim().toLowerCase());
    return !f.hideUngrouped;
  });
  const ids = new Set(visible.map((n) => n.path));
  return {
    nodes: visible,
    edges: edges.filter((e) => ids.has(e.from) && ids.has(e.to)),
  };
}

export function loadFilter(): FilterSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_FILTER;
    const p = JSON.parse(raw) as Partial<FilterSettings>;
    return {
      minDegree:
        typeof p.minDegree === "number"
          ? Math.max(0, Math.round(p.minDegree))
          : 0,
      hiddenGroupQueries: Array.isArray(p.hiddenGroupQueries)
        ? p.hiddenGroupQueries.filter((x) => typeof x === "string")
        : [],
      hideUngrouped: !!p.hideUngrouped,
    };
  } catch {
    return DEFAULT_FILTER;
  }
}
export function saveFilter(f: FilterSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(f));
  } catch {
    // ignore
  }
}
