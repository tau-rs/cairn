export interface LocalGraphSettings {
  enabled: boolean;
  depth: number;
}

export const DEFAULT_LOCAL_GRAPH: LocalGraphSettings = {
  enabled: false,
  depth: 1,
};
export const DEPTH_RANGE = { min: 1, max: 3, step: 1 } as const;

const STORAGE_KEY = "cairn.graph.local";

/** BFS from `root` over UNDIRECTED edges to `depth` hops. Returns the reached
 *  nodes (incl. root, in input order) and the edges whose endpoints are both
 *  reached. root null / not in `nodes` → empty. */
export function localSubgraph(
  nodes: string[],
  edges: { from: string; to: string }[],
  root: string | null,
  depth: number,
): { nodes: string[]; edges: { from: string; to: string }[] } {
  if (!root || !nodes.includes(root)) return { nodes: [], edges: [] };

  const present = new Set(nodes);
  const adj = new Map<string, string[]>();
  const addAdj = (a: string, b: string) => {
    const list = adj.get(a);
    if (list) list.push(b);
    else adj.set(a, [b]);
  };
  for (const e of edges) {
    if (present.has(e.from) && present.has(e.to)) {
      addAdj(e.from, e.to);
      addAdj(e.to, e.from);
    }
  }

  const reached = new Set<string>([root]);
  let frontier = [root];
  for (let d = 0; d < depth && frontier.length; d++) {
    const next: string[] = [];
    for (const n of frontier) {
      for (const m of adj.get(n) ?? []) {
        if (!reached.has(m)) {
          reached.add(m);
          next.push(m);
        }
      }
    }
    frontier = next;
  }

  return {
    nodes: nodes.filter((n) => reached.has(n)),
    edges: edges.filter((e) => reached.has(e.from) && reached.has(e.to)),
  };
}

const clampDepth = (d: number): number =>
  Math.min(DEPTH_RANGE.max, Math.max(DEPTH_RANGE.min, Math.round(d)));

export function loadLocalGraph(): LocalGraphSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_LOCAL_GRAPH;
    const p = JSON.parse(raw) as Partial<LocalGraphSettings>;
    return {
      enabled: !!p.enabled,
      depth: clampDepth(typeof p.depth === "number" ? p.depth : 1),
    };
  } catch {
    return DEFAULT_LOCAL_GRAPH;
  }
}

export function saveLocalGraph(s: LocalGraphSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
