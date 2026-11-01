import { describe, it, expect } from "vitest";
import { capByDegree } from "./globalCap";
import type { GraphNode } from "../../contract";

const n = (path: string, degree: number): GraphNode => ({
  path,
  title: path,
  degree,
  tags: [],
  mtime_secs: 0,
});

describe("capByDegree", () => {
  it("returns everything untruncated under the limit", () => {
    const nodes = [n("a", 1), n("b", 0)];
    const edges = [{ from: "a", to: "b" }];
    const r = capByDegree(nodes, edges, 10);
    expect(r.truncated).toBe(false);
    expect(r.total).toBe(2);
    expect(r.nodes).toHaveLength(2);
  });
  it("keeps the top-N by degree and drops edges to dropped nodes", () => {
    const nodes = [n("hub", 5), n("mid", 2), n("leaf", 0)];
    const edges = [
      { from: "hub", to: "mid" },
      { from: "mid", to: "leaf" },
    ];
    const r = capByDegree(nodes, edges, 2);
    expect(r.truncated).toBe(true);
    expect(r.total).toBe(3);
    expect(r.nodes.map((x) => x.path).sort()).toEqual(["hub", "mid"]);
    expect(r.edges).toEqual([{ from: "hub", to: "mid" }]); // mid→leaf dropped
  });
  it("does not truncate when the total exactly equals the limit", () => {
    const nodes = [n("a", 0), n("b", 0)];
    const r = capByDegree(nodes, [], 2);
    expect(r.truncated).toBe(false);
    expect(r.nodes).toHaveLength(2);
  });
  it("keeps the highest-degree nodes regardless of input order", () => {
    // Input is ASCENDING by degree, so a broken/absent sort keeps the wrong
    // (low-degree) node and this assertion fails.
    const nodes = [n("low", 1), n("high", 9)];
    const r = capByDegree(nodes, [], 1);
    expect(r.nodes.map((x) => x.path)).toEqual(["high"]);
  });
  it("breaks degree ties by path for determinism", () => {
    const nodes = [n("b", 3), n("a", 3)];
    const r = capByDegree(nodes, [], 1);
    expect(r.nodes.map((x) => x.path)).toEqual(["a"]); // "a" < "b"
  });
});
