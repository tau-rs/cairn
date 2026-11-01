import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { TreeContextMenu } from "./TreeContextMenu";

function setup(overrides = {}) {
  const handlers = {
    kind: "note" as "folder" | "note",
    x: 10,
    y: 10,
    onSetIcon: vi.fn(),
    onOpen: vi.fn(),
    onOpenToSide: vi.fn(),
    onNewNote: vi.fn(),
    onRename: vi.fn(),
    onDelete: vi.fn(),
    onRecover: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<TreeContextMenu {...handlers} />);
  return handlers;
}

describe("TreeContextMenu", () => {
  it("renders a note's items", () => {
    setup();
    expect(screen.getByText("Open")).toBeInTheDocument();
    expect(screen.getByText("Open to the side")).toBeInTheDocument();
    expect(screen.getByText("Set icon…")).toBeInTheDocument();
    expect(screen.getByText("Rename")).toBeInTheDocument();
    expect(screen.getByText("Delete")).toBeInTheDocument();
  });

  it("renders a folder's items (New note here, no Open/Delete)", () => {
    setup({ kind: "folder" });
    expect(screen.getByText("New note here")).toBeInTheDocument();
    expect(screen.getByText("Set icon…")).toBeInTheDocument();
    expect(screen.getByText("Rename")).toBeInTheDocument();
    expect(screen.queryByText("Open")).not.toBeInTheDocument();
    expect(screen.queryByText("Delete")).not.toBeInTheDocument();
  });

  it("fires Open to the side then closes", () => {
    const h = setup();
    fireEvent.click(screen.getByText("Open to the side"));
    expect(h.onOpenToSide).toHaveBeenCalled();
    expect(h.onClose).toHaveBeenCalled();
  });

  it("fires Set icon then closes", () => {
    const h = setup();
    fireEvent.click(screen.getByText("Set icon…"));
    expect(h.onSetIcon).toHaveBeenCalled();
    expect(h.onClose).toHaveBeenCalled();
  });

  it("closes on Escape", () => {
    const h = setup();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(h.onClose).toHaveBeenCalled();
  });

  it("fires Recover lost work then closes", () => {
    const h = setup();
    fireEvent.click(screen.getByText("Recover lost work…"));
    expect(h.onRecover).toHaveBeenCalled();
    expect(h.onClose).toHaveBeenCalled();
  });

  it("does not offer Recover lost work for a folder", () => {
    setup({ kind: "folder" });
    expect(screen.queryByText("Recover lost work…")).not.toBeInTheDocument();
  });
});
