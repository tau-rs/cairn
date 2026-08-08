import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";
import type { GraphPayload, GraphDiffPayload } from "../client/daemonClient";

export type DiffClass = "added" | "removed" | "changed" | "unchanged";

export const DIFF_COLORS: Record<DiffClass, string> = {
  added: "#22c55e", // green
  removed: "#ef4444", // red (painted ghosted via alpha in GraphView)
  changed: "#f59e0b", // amber
  unchanged: "#cdd0e0",
};

export interface DiffGraph {
  nodes: (GraphNode & { diff: DiffClass })[];
  edges: (GraphEdge & { diff: DiffClass })[];
}

const edgeKey = (e: GraphEdge) => `${e.from}->${e.to}`;

// Overlay diff classification on the base graph (base = graph at `to`).
// Removed nodes/edges are absent from base, so re-add them as ghosts.
export function buildDiffGraph(base: GraphPayload, diff: GraphDiffPayload): DiffGraph {
  const nodeClass = new Map<string, DiffClass>();
  for (const x of diff.nodesChanged) nodeClass.set(x.path, "changed");
  for (const x of diff.nodesAdded) nodeClass.set(x.path, "added");

  const nodes: (GraphNode & { diff: DiffClass })[] = base.nodes.map((x) => ({
    ...x,
    diff: nodeClass.get(x.path) ?? "unchanged",
  }));
  for (const x of diff.nodesRemoved) nodes.push({ ...x, diff: "removed" });

  const edgeClass = new Map<string, DiffClass>();
  for (const e of diff.edgesAdded) edgeClass.set(edgeKey(e), "added");

  const baseEdgeKeys = new Set(base.edges.map(edgeKey));
  const edges: (GraphEdge & { diff: DiffClass })[] = base.edges.map((e) => ({
    ...e,
    diff: edgeClass.get(edgeKey(e)) ?? "unchanged",
  }));
  for (const e of diff.edgesAdded) {
    if (!baseEdgeKeys.has(edgeKey(e))) edges.push({ ...e, diff: "added" });
  }
  for (const e of diff.edgesRemoved) edges.push({ ...e, diff: "removed" });
  return { nodes, edges };
}
