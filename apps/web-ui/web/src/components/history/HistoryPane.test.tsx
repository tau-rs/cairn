import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { HistoryPane } from "./HistoryPane";
import { cairnStore } from "../../app/cairnStore";

beforeEach(async () => {
  localStorage.clear();
  await cairnStore.getState().init();
});

function renderPane() {
  return render(
    <MemoryRouter>
      <HistoryPane />
    </MemoryRouter>,
  );
}

describe("HistoryPane", () => {
  it("loads and renders history for the active note (empty for fixtures)", async () => {
    await cairnStore.getState().openNote("index.md");
    renderPane();
    // The dev MockClient fixtures seed no revisions → empty state.
    await waitFor(() =>
      expect(screen.getByText(/No versions yet/i)).toBeInTheDocument(),
    );
  });

  it("mounts with the restore confirm dialog closed", async () => {
    await cairnStore.getState().openNote("index.md");
    renderPane();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows a calm empty state instead of Loading when no note is open", async () => {
    // A prior test may leave a note open on this singleton store; force the
    // no-note-open state explicitly rather than relying on ordering.
    cairnStore.setState({
      activePath: null,
      history: null,
      historyPath: null,
      historyLoading: false,
    });
    renderPane();
    expect(
      screen.getByText(/open a note to see its versions/i),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Loading/i)).toBeNull();
  });

  it("does not show another note's history while the current note's is loading", async () => {
    // Loaded history belongs to a previous note (historyPath != activePath):
    // the pane must show the spinner, never the stale revisions.
    await cairnStore.getState().openNote("index.md");
    cairnStore.setState({
      history: [
        {
          id: "old1",
          message: "stale",
          timestamp_secs: 1,
          author: "t",
          summary: null,
          name: null,
        },
      ],
      historyPath: "some-other-note.md",
      historyLoading: false,
    });
    renderPane();
    expect(screen.queryByText("stale")).toBeNull();
    expect(screen.getByText(/Loading/i)).toBeInTheDocument();
  });
});
