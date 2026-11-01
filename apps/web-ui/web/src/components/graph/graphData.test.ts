import { describe, it, expect } from "vitest";
import {
  buildGraphData,
  buildGraphDataFromNodes,
  buildAdjacency,
  nodeRadius,
  labelAlpha,
  buildSuggestedLinks,
  buildSuggestedNodes,
} from "./graphData";
import type { GLink } from "./graphData";
import type { SuggestedEdge } from "../../contract";

describe("buildGraphData", () => {
  const nodes = ["a.md", "b.md", "c.md"];
  const edges = [
    { from: "a.md", to: "b.md" },
    { from: "a.md", to: "c.md" },
    { from: "a.md", to: "missing.md" }, // dropped: endpoint not in nodes
  ];
  it("labels nodes by stem and counts undirected degree", () => {
    const { nodes: gn } = buildGraphData(nodes, edges);
    const byId = Object.fromEntries(gn.map((n) => [n.id, n]));
    expect(byId["a.md"].label).toBe("a");
    expect(byId["a.md"].degree).toBe(2); // a–b, a–c (missing dropped)
    expect(byId["b.md"].degree).toBe(1);
    expect(byId["c.md"].degree).toBe(1);
  });
  it("drops links whose endpoint is not a known node", () => {
    const { links } = buildGraphData(nodes, edges);
    expect(links).toEqual([
      { source: "a.md", target: "b.md" },
      { source: "a.md", target: "c.md" },
    ]);
  });
});

describe("buildAdjacency", () => {
  it("builds symmetric neighbor sets", () => {
    const adj = buildAdjacency([
      { source: "a", target: "b" },
      { source: "a", target: "c" },
    ]);
    expect([...(adj.get("a") ?? [])].sort()).toEqual(["b", "c"]);
    expect([...(adj.get("b") ?? [])]).toEqual(["a"]);
    expect(adj.get("z")).toBeUndefined(); // isolated / unknown
  });
});

describe("nodeRadius", () => {
  it("is base at degree 0 and monotonic non-decreasing", () => {
    expect(nodeRadius(0)).toBe(3);
    expect(nodeRadius(1)).toBeGreaterThan(nodeRadius(0));
    expect(nodeRadius(9)).toBeGreaterThan(nodeRadius(4));
  });
});

describe("labelAlpha", () => {
  it("is 0 when zoomed out, 1 when zoomed in, clamped and monotonic", () => {
    expect(labelAlpha(1.0)).toBe(0);
    expect(labelAlpha(3.0)).toBe(1);
    const mid = labelAlpha(1.85);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
  });
});

import { buildCompareGraphData } from "./graphData";
import type { GraphNode, GraphEdge } from "../../contract";

const gn = (path: string): GraphNode => ({
  path,
  title: path,
  degree: 0,
  tags: [],
  mtime_secs: 0,
});
const ge = (from: string, to: string): GraphEdge => ({ from, to });

describe("buildCompareGraphData", () => {
  // base `to` = {a,b,c}, a-b, b-c ; diff: c appeared (+ b-c), x disappeared (+ a-x)
  const base = {
    nodes: [gn("a"), gn("b"), gn("c")],
    edges: [ge("a", "b"), ge("b", "c")],
  };
  const diff = {
    nodes_added: [gn("c")],
    nodes_removed: [gn("x")],
    // b persisted across revisions but its metadata shifted (degree/tags) →
    // engine reports it in nodes_changed carrying the `to`-revision values.
    nodes_changed: [gn("b")],
    edges_added: [ge("b", "c")],
    edges_removed: [ge("a", "x")],
  };

  it("labels appeared / disappeared / changed / unchanged and injects removed nodes", () => {
    const { nodes, links } = buildCompareGraphData(base, diff);
    const byId = Object.fromEntries(nodes.map((n) => [n.id, n.state]));
    expect(byId).toEqual({
      a: "unchanged",
      b: "changed",
      c: "appeared",
      x: "disappeared",
    });
    const linkState = links.map((l) => [l.source, l.target, l.state]);
    expect(linkState).toContainEqual(["a", "b", "unchanged"]);
    expect(linkState).toContainEqual(["b", "c", "appeared"]);
    expect(linkState).toContainEqual(["a", "x", "disappeared"]);
  });

  it("marks a metadata-only delta as changed without disturbing appeared/removed", () => {
    const { nodes } = buildCompareGraphData(base, diff);
    const changed = nodes.filter((n) => n.state === "changed").map((n) => n.id);
    expect(changed).toEqual(["b"]);
  });

  it("emits no changed links (contract carries nodes_changed only)", () => {
    const { links } = buildCompareGraphData(base, diff);
    expect(links.every((l) => l.state !== "changed")).toBe(true);
  });
});

describe("buildGraphDataFromNodes", () => {
  const gnode = (
    path: string,
    degree: number,
    tags: string[],
    mtime: number,
  ): GraphNode => ({ path, title: path, degree, tags, mtime_secs: mtime });

  it("carries server degree, tags and coerced mtimeSecs onto GNode", () => {
    const nodes = [
      gnode("a.md", 5, ["topic"], 1700000000),
      gnode("b.md", 2, [], 1600000000),
    ];
    const edges = [{ from: "a.md", to: "b.md" }];
    const { nodes: gn, links } = buildGraphDataFromNodes(nodes, edges);
    const byId = Object.fromEntries(gn.map((n) => [n.id, n]));
    // degree is the SERVER value, not recomputed from the (filtered) links.
    expect(byId["a.md"].degree).toBe(5);
    expect(byId["a.md"].tags).toEqual(["topic"]);
    expect(byId["a.md"].mtimeSecs).toBe(1700000000);
    expect(byId["a.md"].label).toBe("a");
    expect(byId["b.md"].degree).toBe(2);
    expect(links).toEqual([{ source: "a.md", target: "b.md" }]);
  });

  it("drops links whose endpoint is not a present node", () => {
    const nodes = [gnode("a.md", 1, [], 0)];
    const edges = [{ from: "a.md", to: "gone.md" }];
    expect(buildGraphDataFromNodes(nodes, edges).links).toEqual([]);
  });
});

describe("buildSuggestedLinks", () => {
  const sug = (
    from: string,
    to: string,
    weight = 0.5,
    why: string | null = null,
  ): SuggestedEdge => ({
    from,
    to,
    weight,
    why,
  });
  const visible = new Set(["a.md", "b.md", "c.md"]);

  it("maps a suggestion to a suggested GLink carrying weight and why", () => {
    const out = buildSuggestedLinks(
      [sug("a.md", "b.md", 0.8, "shared: x")],
      visible,
      [],
    );
    expect(out).toEqual([
      {
        source: "a.md",
        target: "b.md",
        kind: "suggested",
        weight: 0.8,
        why: "shared: x",
      },
    ]);
  });

  it("drops an edge whose endpoint is not a visible node (one missing)", () => {
    const out = buildSuggestedLinks([sug("a.md", "z.md")], visible, []);
    expect(out).toEqual([]);
  });

  it("drops an edge when both endpoints are missing", () => {
    const out = buildSuggestedLinks([sug("y.md", "z.md")], visible, []);
    expect(out).toEqual([]);
  });

  it("suppresses a suggestion that duplicates a real link (undirected)", () => {
    const real: GLink[] = [{ source: "b.md", target: "a.md" }];
    const out = buildSuggestedLinks([sug("a.md", "b.md")], visible, real);
    expect(out).toEqual([]);
  });

  it("dedupes duplicate suggestions among themselves (undirected)", () => {
    const out = buildSuggestedLinks(
      [sug("a.md", "b.md"), sug("b.md", "a.md")],
      visible,
      [],
    );
    expect(out).toHaveLength(1);
  });

  it("passes null why through untouched", () => {
    const out = buildSuggestedLinks([sug("a.md", "c.md")], visible, []);
    expect(out[0].why).toBeNull();
  });

  it("returns [] for empty suggestions or empty visible set", () => {
    expect(buildSuggestedLinks([], visible, [])).toEqual([]);
    expect(buildSuggestedLinks([sug("a.md", "b.md")], new Set(), [])).toEqual(
      [],
    );
  });

  it("keeps an edge whose far endpoint is being injected (inject mode)", () => {
    const injected = new Set(["z.md"]);
    const out = buildSuggestedLinks(
      [sug("a.md", "z.md", 0.7, "shared: y")],
      visible,
      [],
      injected,
    );
    expect(out).toEqual([
      {
        source: "a.md",
        target: "z.md",
        kind: "suggested",
        weight: 0.7,
        why: "shared: y",
      },
    ]);
  });

  it("still drops an edge whose far endpoint is missing AND not injected", () => {
    const injected = new Set(["q.md"]);
    const out = buildSuggestedLinks(
      [sug("a.md", "z.md")],
      visible,
      [],
      injected,
    );
    expect(out).toEqual([]);
  });
});

describe("buildSuggestedNodes", () => {
  const sug = (
    from: string,
    to: string,
    weight = 0.5,
    why: string | null = null,
  ): SuggestedEdge => ({ from, to, weight, why });
  const visible = new Set(["a.md", "b.md", "c.md"]);

  it("returns [] when both endpoints are already visible", () => {
    expect(buildSuggestedNodes([sug("a.md", "b.md")], visible)).toEqual([]);
  });

  it("injects the missing far endpoint as a suggested-only GNode", () => {
    const out = buildSuggestedNodes([sug("a.md", "z.md")], visible);
    expect(out).toEqual([
      { id: "z.md", label: "z", degree: 0, suggested: true },
    ]);
  });

  it("injects the missing endpoint regardless of suggestion direction", () => {
    const out = buildSuggestedNodes([sug("z.md", "a.md")], visible);
    expect(out).toEqual([
      { id: "z.md", label: "z", degree: 0, suggested: true },
    ]);
  });

  it("does not inject when neither endpoint is visible (no anchor)", () => {
    expect(buildSuggestedNodes([sug("y.md", "z.md")], visible)).toEqual([]);
  });

  it("dedupes two suggestions pointing at the same missing node", () => {
    const out = buildSuggestedNodes(
      [sug("a.md", "z.md"), sug("b.md", "z.md")],
      visible,
    );
    expect(out).toHaveLength(1);
    expect(out[0].id).toBe("z.md");
  });

  it("returns [] for empty suggestions", () => {
    expect(buildSuggestedNodes([], visible)).toEqual([]);
  });
});
