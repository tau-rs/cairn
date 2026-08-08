import { describe, it, expect } from "vitest";
import { buildDiffGraph, DIFF_COLORS } from "./diffStyle";
import type { GraphNode } from "../contract/GraphNode";

const n = (path: string): GraphNode => ({ path, title: path, degree: 0, tags: [], mtime_secs: 0 });

describe("buildDiffGraph", () => {
  it("classifies added, changed, unchanged from the base, and re-adds removed as ghosts", () => {
    const base = {
      nodes: [n("keep.md"), n("added.md"), n("changed.md")],
      edges: [{ from: "keep.md", to: "changed.md" }],
    };
    const diff = {
      nodesAdded: [n("added.md")],
      nodesChanged: [n("changed.md")],
      nodesRemoved: [n("gone.md")],
      edgesAdded: [{ from: "keep.md", to: "added.md" }],
      edgesRemoved: [{ from: "keep.md", to: "gone.md" }],
    };
    const g = buildDiffGraph(base, diff);
    const cls = Object.fromEntries(g.nodes.map((x) => [x.path, x.diff]));
    expect(cls).toEqual({ "keep.md": "unchanged", "added.md": "added", "changed.md": "changed", "gone.md": "removed" });

    const ecls = g.edges.map((e) => `${e.from}->${e.to}:${e.diff}`);
    expect(ecls).toContain("keep.md->changed.md:unchanged");
    expect(ecls).toContain("keep.md->added.md:added");
    expect(ecls).toContain("keep.md->gone.md:removed");
  });

  it("exposes a color per class", () => {
    expect(DIFF_COLORS.added).toBeTruthy();
    expect(DIFF_COLORS.removed).toBeTruthy();
    expect(DIFF_COLORS.changed).toBeTruthy();
    expect(DIFF_COLORS.unchanged).toBeTruthy();
  });
});
