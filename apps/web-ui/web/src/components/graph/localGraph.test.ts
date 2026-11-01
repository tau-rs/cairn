import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  DEFAULT_LOCAL_GRAPH,
  localSubgraph,
  loadLocalGraph,
  saveLocalGraph,
} from "./localGraph";

const nodes = ["a", "b", "c", "d", "x"];
// a-b, b-c, c-d (chain); x isolated. Edges are directional in the data but
// neighbors are undirected.
const edges = [
  { from: "a", to: "b" },
  { from: "c", to: "b" }, // incoming to b — undirected reach
  { from: "c", to: "d" },
];

describe("localSubgraph", () => {
  it("depth 0 → just the root", () => {
    expect(localSubgraph(nodes, edges, "b", 0)).toEqual({
      nodes: ["b"],
      edges: [],
    });
  });
  it("depth 1 → root + direct neighbors (both directions)", () => {
    const s = localSubgraph(nodes, edges, "b", 1);
    expect(s.nodes.sort()).toEqual(["a", "b", "c"]);
    expect(s.edges).toEqual([
      { from: "a", to: "b" },
      { from: "c", to: "b" },
    ]);
  });
  it("depth 2 → two hops", () => {
    const s = localSubgraph(nodes, edges, "a", 2);
    expect(s.nodes.sort()).toEqual(["a", "b", "c"]); // a→b→c
  });
  it("returns empty when the root is absent or null", () => {
    expect(localSubgraph(nodes, edges, "missing", 2)).toEqual({
      nodes: [],
      edges: [],
    });
    expect(localSubgraph(nodes, edges, null, 2)).toEqual({
      nodes: [],
      edges: [],
    });
  });
  it("keeps each node once even via multiple paths", () => {
    const s = localSubgraph(nodes, edges, "b", 3);
    expect(new Set(s.nodes).size).toBe(s.nodes.length);
  });
});

describe("loadLocalGraph / saveLocalGraph", () => {
  beforeEach(() => localStorage.clear());
  it("returns the default when empty or corrupt", () => {
    expect(loadLocalGraph()).toEqual(DEFAULT_LOCAL_GRAPH);
    localStorage.setItem("cairn.graph.local", "{bad");
    expect(loadLocalGraph()).toEqual(DEFAULT_LOCAL_GRAPH);
  });
  it("clamps depth to 1..3 and coerces enabled to bool", () => {
    localStorage.setItem(
      "cairn.graph.local",
      JSON.stringify({ enabled: 1, depth: 99 }),
    );
    expect(loadLocalGraph()).toEqual({ enabled: true, depth: 3 });
  });
  it("round-trips and swallows storage errors", () => {
    saveLocalGraph({ enabled: true, depth: 2 });
    expect(loadLocalGraph()).toEqual({ enabled: true, depth: 2 });
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveLocalGraph(DEFAULT_LOCAL_GRAPH)).not.toThrow();
    spy.mockRestore();
  });
});
