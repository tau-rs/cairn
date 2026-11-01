import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FolderTree } from "./FolderTreeView";

beforeEach(() => localStorage.clear());

function setup(over = {}) {
  const props = {
    paths: ["index.md", "notes/ideas.md", "notes/todo.md"],
    activePath: null as string | null,
    onOpen: vi.fn(),
    onOpenToSide: vi.fn(),
    onDelete: vi.fn(),
    onRecover: vi.fn(),
    onRequestNew: vi.fn(),
    onRequestNewInFolder: vi.fn(),
    onApplyRenames: vi.fn(),
    styles: {},
    onSetStyle: vi.fn(),
    onRemapFolderStyles: vi.fn(),
    ...over,
  };
  render(<FolderTree {...props} />);
  return props;
}

describe("FolderTree", () => {
  it("renders folders with nested notes, and root notes", () => {
    setup();
    expect(screen.getByRole("button", { name: "notes" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "index" })).toBeInTheDocument();
  });
  it("draws indent guides for nested rows but not for root rows", () => {
    setup(); // index.md (root), notes/ideas.md, notes/todo.md (depth 1)
    // nested notes render a connector line + tick; the root note does not.
    expect(
      document.querySelectorAll('[data-guide="line"]').length,
    ).toBeGreaterThan(0);
    expect(document.querySelectorAll('[data-guide="tick"]').length).toBe(2);
  });
  it("lights the active note's guide path in the accent color", () => {
    setup({ activePath: "notes/ideas.md" });
    // the active note's connector should be accent (bg-accent), the inactive
    // sibling's should not.
    const accentGuides = document.querySelectorAll("[data-guide].bg-accent");
    expect(accentGuides.length).toBeGreaterThan(0);
  });
  it("collapses and re-expands a folder", () => {
    setup();
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "notes" }));
    expect(
      screen.queryByRole("button", { name: "ideas" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "notes" }));
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
  });
  it("opens a note on click (full path)", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "ideas" }));
    expect(props.onOpen).toHaveBeenCalledWith("notes/ideas.md");
  });
  it("requests a new note in a folder via the folder +", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "new note in notes" }));
    expect(props.onRequestNewInFolder).toHaveBeenCalledWith("notes");
  });
  it("deletes a note by full path without opening it", () => {
    const props = setup();
    fireEvent.click(
      screen.getByRole("button", { name: "delete notes/ideas.md" }),
    );
    expect(props.onDelete).toHaveBeenCalledWith("notes/ideas.md");
    expect(props.onOpen).not.toHaveBeenCalled();
  });
  it("requests a global new note", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "+ New note" }));
    expect(props.onRequestNew).toHaveBeenCalled();
  });
  it("reveals the active note by expanding its collapsed ancestors", () => {
    localStorage.setItem("cairn.folderTree", JSON.stringify(["notes"]));
    setup({ activePath: "notes/ideas.md" });
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
  });

  it("opens rename on F2 and renames a note within its folder", async () => {
    const props = setup();
    const row = screen.getByRole("button", { name: "index" });
    row.focus();
    fireEvent.keyDown(row, { key: "F2" });
    const input = screen.getByDisplayValue("index");
    await userEvent.clear(input);
    await userEvent.type(input, "home{Enter}");
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "index.md", to: "home.md" },
    ]);
  });

  it("moves a note via F2 when a slashed path is committed (keyboard move)", async () => {
    const props = setup();
    const row = screen.getByRole("button", { name: "index" });
    row.focus();
    fireEvent.keyDown(row, { key: "F2" });
    const input = screen.getByDisplayValue("index");
    await userEvent.clear(input);
    await userEvent.type(input, "notes/index{Enter}");
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "index.md", to: "notes/index.md" },
    ]);
  });

  it("renders a default folder glyph and note glyph", () => {
    setup();
    expect(document.querySelector("svg.lucide-folder")).toBeTruthy();
    expect(document.querySelector("svg.lucide-file-text")).toBeTruthy();
  });

  it("renders a custom emoji from styles", () => {
    setup({ styles: { "index.md": { icon: { kind: "emoji", value: "📚" } } } });
    expect(screen.getByText("📚")).toBeInTheDocument();
  });

  it("opens the icon picker when the icon trigger is clicked", async () => {
    setup();
    const trigger = screen.getByRole("button", {
      name: "set icon for index.md",
    });
    await userEvent.click(trigger);
    expect(screen.getByRole("tab", { name: "Emoji" })).toBeInTheDocument();
  });

  it("draws a folder color bar when folderColor is set", () => {
    setup({ styles: { notes: { folderColor: "#46b3e6" } } });
    expect(document.querySelector('[data-folder-bar="true"]')).toBeTruthy();
  });

  it("right-clicking a note opens a menu that deletes it", async () => {
    const props = setup();
    fireEvent.contextMenu(screen.getByRole("button", { name: "ideas" }), {
      clientX: 10,
      clientY: 10,
    });
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Delete" }),
    );
    expect(props.onDelete).toHaveBeenCalledWith("notes/ideas.md");
  });

  it("right-clicking a folder offers New note here (and no Delete)", async () => {
    const props = setup();
    fireEvent.contextMenu(screen.getByRole("button", { name: "notes" }), {
      clientX: 10,
      clientY: 10,
    });
    expect(
      screen.queryByRole("menuitem", { name: "Delete" }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "New note here" }),
    );
    expect(props.onRequestNewInFolder).toHaveBeenCalledWith("notes");
  });

  it("right-click → Set icon opens the picker", async () => {
    setup();
    fireEvent.contextMenu(screen.getByRole("button", { name: "index" }), {
      clientX: 10,
      clientY: 10,
    });
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Set icon…" }),
    );
    expect(
      await screen.findByRole("tab", { name: "Emoji" }),
    ).toBeInTheDocument();
  });
});
