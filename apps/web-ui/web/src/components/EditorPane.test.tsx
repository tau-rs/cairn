import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { EditorPane } from "./EditorPane";
import { cairnStore } from "../app/cairnStore";

const bp = vi.hoisted(() => ({ value: "desktop" as "desktop" | "mobile" }));
vi.mock("./responsive/useBreakpoint", () => ({
  useBreakpoint: () => bp.value,
}));

function seedSplit() {
  cairnStore.setState({
    notePaths: ["a.md", "b.md"],
    openNotes: {
      "a.md": { contents: "alpha", dirty: false, saving: false },
      "b.md": { contents: "beta", dirty: false, saving: false },
    },
    panes: [
      { tabs: [{ path: "a.md", preview: false }], activePath: "a.md" },
      { tabs: [{ path: "b.md", preview: false }], activePath: "b.md" },
    ],
    activePane: 1,
    splitRatio: 0.5,
    activePath: "b.md",
    activeContents: "beta",
  });
}

function seedRevision() {
  cairnStore.setState({
    notePaths: ["a.md"],
    openNotes: {
      "a.md": { contents: "current body", dirty: false, saving: false },
    },
    panes: [{ tabs: [{ path: "a.md", preview: false }], activePath: "a.md" }],
    activePane: 0,
    splitRatio: 1,
    activePath: "a.md",
    activeContents: "current body",
    viewingRevision: { path: "a.md", revision: "r1", contents: "old body" },
  });
}

describe("EditorPane revision view", () => {
  it("renders the read-only RevisionView when a revision is being viewed", async () => {
    seedRevision();
    render(
      <MemoryRouter initialEntries={["/note/a.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    expect(await screen.findByText(/read-only/i)).toBeInTheDocument();
    expect(screen.getByText("old body")).toBeInTheDocument();
  });

  it("clears a stale revision view when the active note no longer matches", async () => {
    // viewingRevision belongs to a.md, but the active note is b.md — the
    // clear-on-switch effect must drop it so it can't leak across notes.
    cairnStore.setState({
      notePaths: ["a.md", "b.md"],
      openNotes: {
        "a.md": { contents: "alpha", dirty: false, saving: false },
        "b.md": { contents: "beta", dirty: false, saving: false },
      },
      panes: [{ tabs: [{ path: "b.md", preview: false }], activePath: "b.md" }],
      activePane: 0,
      splitRatio: 1,
      activePath: "b.md",
      activeContents: "beta",
      viewingRevision: { path: "a.md", revision: "r1", contents: "old body" },
    });
    render(
      <MemoryRouter initialEntries={["/note/b.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    await vi.waitFor(() =>
      expect(cairnStore.getState().viewingRevision).toBeNull(),
    );
    expect(screen.queryByText(/read-only/i)).toBeNull();
  });
});

describe("EditorPane split", () => {
  beforeEach(() => seedSplit());

  it("renders both panes' tab strips and a resize separator", () => {
    render(
      <MemoryRouter initialEntries={["/note/b.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    // stem("a.md") === "a", stem("b.md") === "b" — TabStrip uses stem() for aria-label
    expect(screen.getByLabelText("a")).toBeInTheDocument();
    expect(screen.getByLabelText("b")).toBeInTheDocument();
    expect(screen.getByRole("separator")).toBeInTheDocument();
  });

  it("renders only the first pane on mobile even when two are open", () => {
    bp.value = "mobile";
    render(
      <MemoryRouter initialEntries={["/note/b.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    expect(screen.getByLabelText("a")).toBeInTheDocument(); // pane 0 tab strip
    expect(screen.queryByLabelText("b")).not.toBeInTheDocument(); // pane 1 gone
    expect(screen.queryByRole("separator")).not.toBeInTheDocument();
    bp.value = "desktop";
  });
});
