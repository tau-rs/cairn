import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import type { RFGraph } from "./graphData";

// Mock react-force-graph-2d: capture props, expose the color fn for assertions.
const captured: { props?: any } = {};
vi.mock("react-force-graph-2d", () => ({
  default: (props: any) => {
    captured.props = props;
    return null;
  },
}));

import GraphView from "./GraphView";
import { DIFF_COLORS } from "./diffStyle";

const graph: RFGraph = {
  nodes: [
    { id: "a.md", label: "A", degree: 2, tags: [], mtime: 0 },
    { id: "b.md", label: "B", degree: 0, tags: [], mtime: 0 },
  ],
  links: [{ source: "a.md", target: "b.md" }],
};

describe("GraphView", () => {
  it("forwards graphData to the force graph", () => {
    render(<GraphView graph={graph} />);
    expect(captured.props.graphData.nodes).toHaveLength(2);
    expect(captured.props.graphData.links).toHaveLength(1);
  });

  it("colors nodes by diff class when provided", () => {
    const diffByPath = new Map([["a.md", "added" as const]]);
    render(<GraphView graph={graph} diffByPath={diffByPath} />);
    const color = captured.props.nodeColor;
    expect(color(graph.nodes[0])).toBe(DIFF_COLORS.added);
    expect(color(graph.nodes[1])).toBe(DIFF_COLORS.unchanged);
  });
});
