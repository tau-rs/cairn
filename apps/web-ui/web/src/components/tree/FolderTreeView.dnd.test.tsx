import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FolderTree } from "./FolderTreeView";

beforeEach(() => localStorage.clear());

function setup(over = {}) {
  const props = {
    paths: ["archive/keep.md", "notes/ideas.md", "notes/todo.md"],
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

describe("FolderTree rename/move", () => {
  it("double-click a note → input → Enter renames within its folder", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "ideas" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "renamed" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "notes/renamed.md" },
    ]);
  });
  it("double-click a folder → input → Enter bulk-renames its notes", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "notes" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "work" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "work/ideas.md" },
      { from: "notes/todo.md", to: "work/todo.md" },
    ]);
  });
  it("renaming a folder remaps its styles by path (covers note-less folders)", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "notes" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "work" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onRemapFolderStyles).toHaveBeenCalledWith("notes", "work");
  });
  it("Escape cancels an inline rename", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "ideas" }));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(props.onApplyRenames).not.toHaveBeenCalled();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
  it("dragging a note onto a folder moves it", () => {
    const props = setup();
    fireEvent.dragStart(screen.getByRole("button", { name: "ideas" }));
    fireEvent.drop(screen.getByRole("button", { name: "archive" }));
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "archive/ideas.md" },
    ]);
  });
  it("dropping a folder onto itself does nothing", () => {
    const props = setup();
    fireEvent.dragStart(screen.getByRole("button", { name: "notes" }));
    fireEvent.drop(screen.getByRole("button", { name: "notes" }));
    expect(props.onApplyRenames).not.toHaveBeenCalled();
  });
});
