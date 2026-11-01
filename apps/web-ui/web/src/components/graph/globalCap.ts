import type { GraphNode, GraphEdge } from "../../contract";

export const GLOBAL_NODE_CAP = 1500;

export interface CapResult {
  nodes: GraphNode[];
  edges: GraphEdge[];
  total: number;
  truncated: boolean;
}

/** Cap a global graph to the `limit` highest-degree nodes. Ties broken by path
 *  for determinism. Edges with a dropped endpoint are removed. */
export function capByDegree(
  nodes: GraphNode[],
  edges: GraphEdge[],
  limit: number = GLOBAL_NODE_CAP,
): CapResult {
  const total = nodes.length;
  if (total <= limit) return { nodes, edges, total, truncated: false };

  const kept = [...nodes]
    .sort((a, b) => b.degree - a.degree || a.path.localeCompare(b.path))
    .slice(0, limit);
  const keptIds = new Set(kept.map((x) => x.path));
  const keptEdges = edges.filter(
    (e) => keptIds.has(e.from) && keptIds.has(e.to),
  );
  return { nodes: kept, edges: keptEdges, total, truncated: true };
}
