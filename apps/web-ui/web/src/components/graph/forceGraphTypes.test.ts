import { describe, it, expect } from "vitest";
import { asGraphData } from "./forceGraphTypes";
import { buildGraphData } from "./graphData";

describe("asGraphData", () => {
  it("round-trips the build output shape (same ids + link count)", () => {
    const built = buildGraphData(
      ["a.md", "b.md"],
      [{ from: "a.md", to: "b.md" }],
    );
    const rf = asGraphData(built);
    expect(rf.nodes.map((n) => n.id)).toEqual(["a.md", "b.md"]);
    expect(rf.links).toHaveLength(1);
  });

  it("is a typed view, not a copy (the lib mutates nodes/links in place)", () => {
    const built = buildGraphData(["a.md"], []);
    const rf = asGraphData(built);
    expect(rf).toBe(built);
    expect(rf.nodes).toBe(built.nodes);
  });
});
