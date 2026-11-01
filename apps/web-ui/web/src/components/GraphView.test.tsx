import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GraphView } from "./GraphView";
import { cairnStore } from "../app/cairnStore";
import type { GraphNode, Revision } from "../contract";

const gnode = (p: string): GraphNode => ({
  path: p,
  title: p,
  degree: 0,
  tags: [],
  mtime_secs: 0,
});
const TL: Revision[] = [
  {
    id: "r2",
    message: "add b",
    timestamp_secs: 20,
    author: "x",
    summary: null,
    name: null,
  },
  {
    id: "r1",
    message: "init",
    timestamp_secs: 10,
    author: "x",
    summary: null,
    name: null,
  },
];

// react-force-graph-2d only mounts once the container has a measured size,
// which it never does in jsdom, so these tests exercise the chrome (overlay,
// controls) without the canvas.
function setup(over = {}) {
  const props = {
    nodes: [] as GraphNode[],
    edges: [] as { from: string; to: string }[],
    tagsByNote: {} as Record<string, string[]>,
    activePath: null as string | null,
    onOpenNote: vi.fn(),
    suggestions: null,
    onLoadSuggestions: vi.fn(),
    ...over,
  };
  render(<GraphView {...props} />);
  return props;
}

describe("GraphView", () => {
  // The scrubber's open state persists to localStorage; clear it so opening it
  // in one test doesn't leave it open for the next.
  beforeEach(() => localStorage.clear());

  it("shows a loading overlay while the graph loads", () => {
    setup({ loading: true });
    expect(
      screen.getByRole("status", { name: /loading graph/i }),
    ).toBeInTheDocument();
  });
  it("shows no loading overlay when not loading", () => {
    setup({ loading: false });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows the cap banner when the global graph exceeds the node cap", () => {
    // 1501 > GLOBAL_NODE_CAP (1500) → capByDegree truncates in the global view.
    const many = Array.from({ length: 1501 }, (_, i) => gnode(`n${i}.md`));
    setup({ nodes: many, edges: [] });
    expect(
      screen.getByText(/most-connected of 1501 notes/i),
    ).toBeInTheDocument();
  });

  it("shows no cap banner under the node cap", () => {
    setup({ nodes: [gnode("a.md"), gnode("b.md")], edges: [] });
    expect(screen.queryByText(/most-connected of/i)).toBeNull();
  });

  it("keeps the temporal toggle enabled with no note open", () => {
    vi.spyOn(cairnStore.getState(), "loadVaultTimeline").mockResolvedValue();
    setup({ nodes: [gnode("a.md")], activePath: null });
    expect(
      screen.getByRole("button", { name: /graph history/i }),
    ).not.toBeDisabled();
  });

  it("renders the compare diff counts and +added/−removed delta in the scrubber banner", async () => {
    vi.spyOn(cairnStore.getState(), "loadVaultTimeline").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadDiff").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(
      () => {},
    );
    // Seed a base snapshot (drives the "N notes · M links" count) and a diff
    // (drives the "+added / −removed" delta): added = nodes_added(2) +
    // edges_added(2) = 4; removed = nodes_removed(1) + edges_removed(0) = 1.
    const snapshot = {
      nodes: [gnode("a.md"), gnode("b.md"), gnode("c.md")],
      edges: [
        { from: "a.md", to: "b.md" },
        { from: "b.md", to: "c.md" },
      ],
    };
    const diff = {
      nodes_added: [gnode("b.md"), gnode("c.md")],
      nodes_removed: [gnode("z.md")],
      nodes_changed: [],
      edges_added: [
        { from: "a.md", to: "b.md" },
        { from: "b.md", to: "c.md" },
      ],
      edges_removed: [],
    };
    cairnStore.setState({ temporal: { timeline: TL, snapshot, diff } });
    setup({ nodes: [gnode("a.md")], activePath: null });
    await userEvent.click(
      screen.getByRole("button", { name: /graph history/i }),
    );
    await userEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    expect(screen.getByText(/3 notes · 2 links/)).toBeInTheDocument();
    expect(screen.getByText("+4")).toBeInTheDocument();
    expect(screen.getByText("−1")).toBeInTheDocument();
  });

  it("shows the scrubber when temporal is opened, even with no note active", async () => {
    vi.spyOn(cairnStore.getState(), "loadVaultTimeline").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(
      () => {},
    );
    cairnStore.setState({
      temporal: { timeline: TL, snapshot: null, diff: null },
    });
    setup({ nodes: [gnode("a.md")], activePath: null });
    await userEvent.click(
      screen.getByRole("button", { name: /graph history/i }),
    );
    expect(screen.getByRole("button", { name: /live/i })).toBeInTheDocument();
  });
});
