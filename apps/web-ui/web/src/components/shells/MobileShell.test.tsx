import { describe, it, expect, beforeEach } from "vitest";
import { act } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { cairnStore } from "../../app/cairnStore";
import { MobileShell } from "./MobileShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

function renderShell() {
  return render(
    <MemoryRouter>
      <MobileShell {...regions} />
    </MemoryRouter>,
  );
}

describe("MobileShell", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ mobileTab: "editor", backlinksOpen: false });
    act(() => {
      cairnStore.setState({ searchResults: null, searchSnippets: null });
    });
  });

  it("shows the editor by default and the bottom nav", () => {
    renderShell();
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(screen.getByRole("navigation")).toBeInTheDocument();
  });

  it("switches to the Files view when the Files tab is tapped", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(screen.getByText("LIST")).toBeInTheDocument();
    expect(screen.queryByText("EDITOR")).not.toBeInTheDocument();
  });

  it("switches to the More view when the More tab is tapped", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "More" }));
    expect(
      screen.getByRole("button", { name: /settings/i }),
    ).toBeInTheDocument();
  });

  it("opens the backlinks bottom sheet from the header", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: /backlinks/i }));
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
  });

  it("opens the dedicated search view and highlights the Search tab when tapped", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
      screen.getByRole("searchbox", { name: /search notes/i }),
    ).toBeInTheDocument();
    expect(screen.queryByText("EDITOR")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("stays on the search view when results arrive instead of bouncing to the editor", async () => {
    cairnStore.getState().setUi({ mobileTab: "search" });
    renderShell();
    act(() => {
      cairnStore.setState({ searchResults: ["a.md"], searchSnippets: null });
    });
    expect(
      screen.getByRole("searchbox", { name: /search notes/i }),
    ).toBeInTheDocument();
    expect(screen.queryByText("EDITOR")).not.toBeInTheDocument();
  });

  it("leaves the search view for the editor when a result is opened", async () => {
    cairnStore.getState().setUi({ mobileTab: "search" });
    cairnStore.setState({ searchResults: ["a.md"], searchSnippets: null });
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "a.md" }));
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(
      screen.queryByRole("searchbox", { name: /search notes/i }),
    ).not.toBeInTheDocument();
  });

  it("can return to the Files tree while a note is open", async () => {
    render(
      <MemoryRouter initialEntries={["/note/a.md"]}>
        <MobileShell {...regions} />
      </MemoryRouter>,
    );
    // A note route renders the editor first…
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    // …and tapping Files must not bounce straight back to it.
    await userEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(screen.getByText("LIST")).toBeInTheDocument();
    expect(screen.queryByText("EDITOR")).not.toBeInTheDocument();
  });

  it("leaves the Files tab and shows the editor when a note route is active", () => {
    cairnStore.getState().setUi({ mobileTab: "files" });
    render(
      <MemoryRouter initialEntries={["/note/a.md"]}>
        <MobileShell {...regions} />
      </MemoryRouter>,
    );
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(screen.queryByText("LIST")).not.toBeInTheDocument();
  });
});
