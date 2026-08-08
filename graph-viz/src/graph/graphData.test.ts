import { describe, it, expect } from "vitest";
import { buildGraphData, nodeRadius, labelAlpha, type RFNode } from "./graphData";
import type { GraphNode } from "../contract/GraphNode";

const node = (path: string, over: Partial<GraphNode> = {}): GraphNode => ({
  path, title: "", degree: 0, tags: [], mtime_secs: 0, ...over,
});

describe("nodeRadius", () => {
  it("is sublinear in degree", () => {
    expect(nodeRadius(0)).toBeCloseTo(3);
    expect(nodeRadius(4)).toBeCloseTo(3 + 1.6 * 2);
  });
});

describe("labelAlpha", () => {
  it("ramps 0->1 between zoom 1.2 and 2.5", () => {
    expect(labelAlpha(1.0)).toBe(0);
    expect(labelAlpha(2.5)).toBe(1);
    expect(labelAlpha(1.85)).toBeCloseTo(0.5, 1);
  });
});

describe("buildGraphData", () => {
  it("derives label from title, falls back to stem, carries enriched fields", () => {
    const { graph } = buildGraphData(
      [node("dir/a.md", { title: "Alpha", degree: 3, tags: ["x"], mtime_secs: 99 }), node("dir/b.md")],
      [],
    );
    expect(graph.nodes[0]).toMatchObject({ id: "dir/a.md", label: "Alpha", degree: 3, tags: ["x"], mtime: 99 });
    expect(graph.nodes[1].label).toBe("b"); // stem of dir/b.md
  });

  it("drops edges whose endpoints are missing", () => {
    const { graph } = buildGraphData([node("a.md"), node("b.md")], [
      { from: "a.md", to: "b.md" },
      { from: "a.md", to: "ghost.md" },
    ]);
    expect(graph.links).toEqual([{ source: "a.md", target: "b.md" }]);
  });

  it("reuses node object identity across payloads so x/y persist", () => {
    const first = buildGraphData([node("a.md")], []);
    first.index.get("a.md")!.x = 42; // simulate react-force-graph placing the node
    const second = buildGraphData([node("a.md", { degree: 5 })], [], first.index);
    const reused: RFNode = second.graph.nodes[0];
    expect(reused).toBe(first.index.get("a.md")); // same object identity
    expect(reused.x).toBe(42); // position preserved
    expect(reused.degree).toBe(5); // fields refreshed
  });

  it("drops nodes absent from the new payload", () => {
    const first = buildGraphData([node("a.md"), node("b.md")], []);
    const second = buildGraphData([node("a.md")], [], first.index);
    expect(second.graph.nodes.map((n) => n.id)).toEqual(["a.md"]);
    expect(second.index.has("b.md")).toBe(false);
  });
});
