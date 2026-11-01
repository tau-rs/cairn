import { stem } from "../../client/wikilink";
import type { GraphNode, GraphEdge, SuggestedEdge } from "../../contract";

export type GraphState = "appeared" | "disappeared" | "unchanged" | "changed";

export interface GNode {
  id: string;
  label: string;
  degree: number;
  state?: GraphState;
  // Present only on the live (server-fed) build path; the string-based
  // buildGraphData and the temporal buildCompareGraphData leave these unset.
  tags?: string[];
  mtimeSecs?: number;
  // Local-inject seam: set only on "suggested-only" ghost nodes injected by
  // buildSuggestedNodes (a suggestion's far endpoint that isn't a real node in
  // the current local view). Real builders leave it unset. Drives the dashed
  // ghost paint and keeps these nodes out of adjacency/degree.
  suggested?: true;
}
export interface GLink {
  source: string;
  target: string;
  state?: GraphState;
  // Overlay seam: undefined ≡ a real (explicit) edge — the three real-edge
  // builders leave it unset. "suggested" links carry engine similarity data.
  kind?: "real" | "suggested";
  weight?: number; // suggested-only: 0..1 similarity ranking → opacity/width
  why?: string | null; // suggested-only: provenance, shown via linkLabel tooltip
}

/** Build force-graph data: degree = count of links touching the node
 *  (undirected); label = stem(path); links filtered to edges whose endpoints
 *  both exist. */
export function buildGraphData(
  nodes: string[],
  edges: { from: string; to: string }[],
): { nodes: GNode[]; links: GLink[] } {
  const ids = new Set(nodes);
  const links: GLink[] = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));

  const degree = new Map<string, number>();
  for (const id of nodes) degree.set(id, 0);
  for (const l of links) {
    degree.set(l.source, (degree.get(l.source) ?? 0) + 1);
    degree.set(l.target, (degree.get(l.target) ?? 0) + 1);
  }

  const gnodes: GNode[] = nodes.map((id) => ({
    id,
    label: stem(id),
    degree: degree.get(id) ?? 0,
  }));
  return { nodes: gnodes, links };
}

/** Live-path builder: like buildGraphData but consumes enriched GraphNode[] and
 *  carries the SERVER degree, frontmatter tags, and coerced `mtimeSecs` onto each
 *  GNode. Degree is the server's undirected count over the full returned graph —
 *  not recomputed from these (possibly capped/filtered) links — so hub size still
 *  reflects true connectivity. Links are filtered to present endpoints. */
export function buildGraphDataFromNodes(
  nodes: GraphNode[],
  edges: { from: string; to: string }[],
): { nodes: GNode[]; links: GLink[] } {
  const ids = new Set(nodes.map((n) => n.path));
  const links: GLink[] = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));

  const gnodes: GNode[] = nodes.map((n) => ({
    id: n.path,
    label: stem(n.path),
    degree: n.degree,
    tags: n.tags,
    mtimeSecs: Number(n.mtime_secs),
  }));
  return { nodes: gnodes, links };
}

/** Symmetric adjacency for hover-neighbor highlighting. Pass STRING-keyed links
 *  (react-force-graph mutates link.source/target into node objects, so build
 *  this from the original string links, not the mutated array). */
export function buildAdjacency(links: GLink[]): Map<string, Set<string>> {
  const adj = new Map<string, Set<string>>();
  const add = (a: string, b: string) => {
    if (!adj.has(a)) adj.set(a, new Set());
    adj.get(a)!.add(b);
  };
  for (const l of links) {
    add(l.source, l.target);
    add(l.target, l.source);
  }
  return adj;
}

/** Undirected pair key so a↔b compares equal regardless of direction. */
const pairKey = (a: string, b: string) => (a < b ? `${a}|${b}` : `${b}|${a}`);

/** Map engine SuggestedEdge[] → suggested GLink[] for overlay rendering.
 *  Drops any edge whose endpoint is not a visible node (suggestions never
 *  introduce nodes), and dedupes (undirected) against real links and against
 *  earlier suggestions — so a suggestion duplicating an explicit link, or a
 *  reciprocal duplicate, is suppressed. */
export function buildSuggestedLinks(
  suggestions: SuggestedEdge[],
  visibleNodeIds: Set<string>,
  realLinks: GLink[],
  injectedIds?: Set<string>,
): GLink[] {
  // An endpoint "renders" if it's a visible real node or (local-inject mode
  // only) a ghost node being injected. Omitting injectedIds keeps the global /
  // vault drop path byte-for-byte identical.
  const present = (id: string) =>
    visibleNodeIds.has(id) || (injectedIds?.has(id) ?? false);
  const seen = new Set<string>();
  for (const l of realLinks) seen.add(pairKey(l.source, l.target));
  const out: GLink[] = [];
  for (const s of suggestions) {
    if (!present(s.from) || !present(s.to)) continue;
    const k = pairKey(s.from, s.to);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push({
      source: s.from,
      target: s.to,
      kind: "suggested",
      weight: s.weight,
      why: s.why,
    });
  }
  return out;
}

/** LOCAL-mode inject seam (companion to buildSuggestedLinks). Returns the far
 *  suggested endpoints that are NOT already visible, as "suggested-only" ghost
 *  GNode[] flagged `suggested`. A node is injected only when it is the missing
 *  endpoint of a suggestion whose OTHER endpoint IS visible (an anchor) — a
 *  suggestion between two invisible nodes injects nothing. Deduped, so two
 *  suggestions to the same missing node yield one node. Global / vault never
 *  calls this (it keeps dropping); the ghosts carry degree 0 and are kept out
 *  of adjacency so they never pollute hover-highlight, degree, or the cap. */
export function buildSuggestedNodes(
  suggestions: SuggestedEdge[],
  visibleNodeIds: Set<string>,
): GNode[] {
  const seen = new Set<string>();
  const out: GNode[] = [];
  const inject = (id: string) => {
    if (visibleNodeIds.has(id) || seen.has(id)) return;
    seen.add(id);
    out.push({ id, label: stem(id), degree: 0, suggested: true });
  };
  for (const s of suggestions) {
    const fromVis = visibleNodeIds.has(s.from);
    const toVis = visibleNodeIds.has(s.to);
    // Skip when both visible (nothing to inject) or both missing (no anchor).
    if (fromVis === toVis) continue;
    if (!fromVis) inject(s.from);
    if (!toVis) inject(s.to);
  }
  return out;
}

/** Node radius from degree — sublinear so hubs are bigger but not huge. */
export function nodeRadius(degree: number): number {
  return 3 + 1.6 * Math.sqrt(degree);
}

/** Label opacity from the current zoom scale — hidden when zoomed out, ramps to
 *  1 as you zoom in. */
export function labelAlpha(zoom: number): number {
  const lo = 1.2;
  const hi = 2.5;
  if (zoom <= lo) return 0;
  if (zoom >= hi) return 1;
  return (zoom - lo) / (hi - lo);
}

const edgeKey = (e: { from: string; to: string }) => `${e.from}|${e.to}`;

/** Force-graph data for COMPARE mode: the `to` graph (base) styled by the
 *  from→to diff. Added nodes/edges = "appeared"; removed = "disappeared" and
 *  re-injected (they are absent from the base `to` graph); nodes present in both
 *  revisions whose metadata shifted (degree/tags) = "changed"; everything else =
 *  "unchanged". Degree is undirected over base ∪ removed edges. The contract
 *  carries `nodes_changed` (nodes) but no `edges_changed`, so links only ever
 *  take appeared/disappeared/unchanged. */
export function buildCompareGraphData(
  base: { nodes: GraphNode[]; edges: GraphEdge[] },
  diff: {
    nodes_added: GraphNode[];
    nodes_removed: GraphNode[];
    nodes_changed: GraphNode[];
    edges_added: GraphEdge[];
    edges_removed: GraphEdge[];
  },
): { nodes: GNode[]; links: GLink[] } {
  const appearedNodes = new Set(diff.nodes_added.map((n) => n.path));
  const changedNodes = new Set(diff.nodes_changed.map((n) => n.path));
  const appearedEdges = new Set(diff.edges_added.map(edgeKey));

  // Node id list: base nodes + injected removed ghosts (dedup by path).
  // A node can't be both appeared (only in `to`) and changed (in both), but
  // appeared wins if the engine ever double-reports.
  const ids: string[] = [];
  const stateOf = new Map<string, GraphState>();
  for (const n of base.nodes) {
    ids.push(n.path);
    stateOf.set(
      n.path,
      appearedNodes.has(n.path)
        ? "appeared"
        : changedNodes.has(n.path)
          ? "changed"
          : "unchanged",
    );
  }
  for (const n of diff.nodes_removed) {
    if (!stateOf.has(n.path)) {
      ids.push(n.path);
      stateOf.set(n.path, "disappeared");
    }
  }
  const present = new Set(ids);

  // Edges: base ∪ removed (removed are absent from base). Filter to present
  // endpoints so a ghost edge only shows if both endpoints render.
  const seen = new Set<string>();
  const rawLinks: GLink[] = [];
  const push = (e: { from: string; to: string }, state: GraphState) => {
    const k = edgeKey(e);
    if (seen.has(k)) return;
    if (!present.has(e.from) || !present.has(e.to)) return;
    seen.add(k);
    rawLinks.push({ source: e.from, target: e.to, state });
  };
  for (const e of base.edges)
    push(e, appearedEdges.has(edgeKey(e)) ? "appeared" : "unchanged");
  for (const e of diff.edges_removed) push(e, "disappeared");

  const degree = new Map<string, number>();
  for (const id of ids) degree.set(id, 0);
  for (const l of rawLinks) {
    degree.set(l.source, (degree.get(l.source) ?? 0) + 1);
    degree.set(l.target, (degree.get(l.target) ?? 0) + 1);
  }

  const nodes: GNode[] = ids.map((id) => ({
    id,
    label: stem(id),
    degree: degree.get(id) ?? 0,
    state: stateOf.get(id),
  }));
  return { nodes, links: rawLinks };
}
