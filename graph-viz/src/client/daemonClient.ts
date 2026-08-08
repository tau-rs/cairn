import type { Query } from "../contract/Query";
import type { QueryResponse } from "../contract/QueryResponse";
import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";
import type { Revision } from "../contract/Revision";

export interface GraphPayload {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface GraphDiffPayload {
  nodesAdded: GraphNode[];
  nodesRemoved: GraphNode[];
  nodesChanged: GraphNode[];
  edgesAdded: GraphEdge[];
  edgesRemoved: GraphEdge[];
}

export class DaemonError extends Error {
  constructor(
    public kind: string,
    message: string,
  ) {
    super(message);
    this.name = "DaemonError";
  }
}

async function runQuery(q: Query): Promise<QueryResponse> {
  const res = await fetch("/query", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(q),
  });
  const body = (await res.json()) as Record<string, unknown>;
  if (!res.ok) {
    const message = (body.message ?? body.what ?? "request failed") as string;
    throw new DaemonError((body.type as string) ?? "internal", message);
  }
  return body as QueryResponse;
}

function expectArm<T extends QueryResponse["type"]>(
  r: QueryResponse,
  arm: T,
): Extract<QueryResponse, { type: T }> {
  if (r.type !== arm) {
    throw new DaemonError("internal", `expected "${arm}" response, got "${r.type}"`);
  }
  return r as Extract<QueryResponse, { type: T }>;
}

export async function getGraph(): Promise<GraphPayload> {
  const r = expectArm(await runQuery({ type: "get_graph", scope: { type: "full" } }), "graph");
  return { nodes: r.nodes, edges: r.edges };
}

export async function graphAt(revision: string): Promise<GraphPayload> {
  const r = expectArm(await runQuery({ type: "graph_at", revision, scope: { type: "full" } }), "graph");
  return { nodes: r.nodes, edges: r.edges };
}

export async function graphDiff(from: string, to: string): Promise<GraphDiffPayload> {
  const r = expectArm(await runQuery({ type: "graph_diff", from, to, scope: { type: "full" } }), "graph_diff");
  return {
    nodesAdded: r.nodes_added,
    nodesRemoved: r.nodes_removed,
    nodesChanged: r.nodes_changed,
    edgesAdded: r.edges_added,
    edgesRemoved: r.edges_removed,
  };
}

export async function vaultHistory(limit?: number): Promise<Revision[]> {
  const r = expectArm(await runQuery({ type: "vault_history", limit: limit ?? null }), "history");
  return r.revisions;
}
