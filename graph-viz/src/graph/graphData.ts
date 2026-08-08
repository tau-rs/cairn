import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";

export interface RFNode {
  id: string;
  label: string;
  degree: number;
  tags: string[];
  mtime: number;
  // Mutated in place by react-force-graph-2d; preserved via object reuse.
  x?: number;
  y?: number;
  fx?: number;
  fy?: number;
}

export interface RFLink {
  source: string;
  target: string;
}

export interface RFGraph {
  nodes: RFNode[];
  links: RFLink[];
}

// Sublinear node sizing (ported from cairn-ui graphData.ts).
export function nodeRadius(degree: number): number {
  return 3 + 1.6 * Math.sqrt(degree);
}

// Zoom-adaptive label opacity (ported from cairn-ui graphData.ts).
export function labelAlpha(zoom: number): number {
  const lo = 1.2;
  const hi = 2.5;
  if (zoom <= lo) return 0;
  if (zoom >= hi) return 1;
  return (zoom - lo) / (hi - lo);
}

function stem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.[^.]+$/, "");
}

// Build react-force-graph data. When `prev` is supplied, reuse node objects by
// path so react-force-graph keeps x/y/vx/vy — the graph morphs instead of
// re-exploding when scrubbing/diffing.
export function buildGraphData(
  nodes: GraphNode[],
  edges: GraphEdge[],
  prev?: Map<string, RFNode>,
): { graph: RFGraph; index: Map<string, RFNode> } {
  const index = new Map<string, RFNode>();
  const out: RFNode[] = [];
  for (const n of nodes) {
    const node: RFNode = prev?.get(n.path) ?? { id: n.path, label: "", degree: 0, tags: [], mtime: 0 };
    node.label = n.title || stem(n.path);
    node.degree = n.degree;
    node.tags = n.tags;
    node.mtime = n.mtime_secs;
    index.set(n.path, node);
    out.push(node);
  }
  const links: RFLink[] = edges
    .filter((e) => index.has(e.from) && index.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));
  return { graph: { nodes: out, links }, index };
}
