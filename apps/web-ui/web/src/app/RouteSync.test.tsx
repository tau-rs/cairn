import { describe, it, expect, beforeEach } from "vitest";
import { render, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { RouteSync } from "./RouteSync";
import { cairnStore } from "./cairnStore";

beforeEach(() => {
  localStorage.clear();
  // Seed a known, ready (restore-complete) state without running full init().
  cairnStore.setState({
    cairnPath: "/mock",
    ready: true,
    notePaths: ["index.md", "ideas.md"],
    activePath: null,
    activeTag: null,
    searchResults: null,
    searchSnippets: null,
    openNotes: {},
    panes: [{ tabs: [], activePath: null }],
    activePane: 0,
  });
});

describe("RouteSync", () => {
  it("opens the note named in the URL", async () => {
    render(
      <MemoryRouter initialEntries={["/note/ideas.md"]}>
        <RouteSync />
      </MemoryRouter>,
    );
    await waitFor(() =>
      expect(cairnStore.getState().activePath).toBe("ideas.md"),
    );
  });

  it("stays inert until init finishes (ready === false)", async () => {
    cairnStore.setState({ ready: false });
    render(
      <MemoryRouter initialEntries={["/note/ideas.md"]}>
        <RouteSync />
      </MemoryRouter>,
    );
    // give effects a tick; nothing should open while the restore is in flight
    await new Promise((r) => setTimeout(r, 0));
    expect(cairnStore.getState().activePath).toBeNull();
  });
});
