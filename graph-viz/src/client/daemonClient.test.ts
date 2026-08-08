import { describe, it, expect, vi, beforeEach } from "vitest";
import { getGraph, graphAt, graphDiff, vaultHistory, DaemonError } from "./daemonClient";

function mockFetch(status: number, body: unknown) {
  return vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  });
}

describe("daemonClient", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("getGraph posts get_graph with full scope and returns nodes/edges", async () => {
    const f = mockFetch(200, { type: "graph", nodes: [{ path: "a.md", title: "A", degree: 1, tags: [], mtime_secs: 1 }], edges: [{ from: "a.md", to: "b.md" }] });
    vi.stubGlobal("fetch", f);
    const g = await getGraph();
    expect(f).toHaveBeenCalledWith("/query", expect.objectContaining({ method: "POST" }));
    const sent = JSON.parse(f.mock.calls[0][1].body);
    expect(sent).toEqual({ type: "get_graph", scope: { type: "full" } });
    expect(g.nodes).toHaveLength(1);
    expect(g.edges[0]).toEqual({ from: "a.md", to: "b.md" });
  });

  it("graphAt posts graph_at with revision", async () => {
    const f = mockFetch(200, { type: "graph", nodes: [], edges: [] });
    vi.stubGlobal("fetch", f);
    await graphAt("HEAD~2");
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "graph_at", revision: "HEAD~2", scope: { type: "full" } });
  });

  it("graphDiff posts graph_diff with from/to and maps snake_case arms", async () => {
    const f = mockFetch(200, { type: "graph_diff", nodes_added: [1], nodes_removed: [2], nodes_changed: [3], edges_added: [4], edges_removed: [5] });
    vi.stubGlobal("fetch", f);
    const d = await graphDiff("old", "new");
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "graph_diff", from: "old", to: "new", scope: { type: "full" } });
    expect(d).toEqual({ nodesAdded: [1], nodesRemoved: [2], nodesChanged: [3], edgesAdded: [4], edgesRemoved: [5] });
  });

  it("vaultHistory posts vault_history and returns revisions", async () => {
    const f = mockFetch(200, { type: "history", revisions: [{ id: "a", message: "m", timestamp_secs: 1, author: "x" }] });
    vi.stubGlobal("fetch", f);
    const revs = await vaultHistory(50);
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "vault_history", limit: 50 });
    expect(revs).toHaveLength(1);
  });

  it("maps a non-2xx ContractError body to DaemonError", async () => {
    const f = mockFetch(400, { type: "invalid_request", message: "bad revspec" });
    vi.stubGlobal("fetch", f);
    await expect(graphAt("nope")).rejects.toMatchObject({ name: "DaemonError", kind: "invalid_request", message: "bad revspec" });
  });

  it("throws when the response arm is unexpected", async () => {
    const f = mockFetch(200, { type: "tags", tags: [] });
    vi.stubGlobal("fetch", f);
    await expect(getGraph()).rejects.toBeInstanceOf(DaemonError);
  });
});
