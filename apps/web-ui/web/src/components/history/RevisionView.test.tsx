import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RevisionView } from "./RevisionView";

describe("RevisionView", () => {
  it("defaults to a diff vs the current copy, marking add/del/ctx rows", () => {
    render(
      <RevisionView
        revision="r1"
        contents={"a\nb\nc"}
        current={"a\nB\nc"}
        onBack={vi.fn()}
        onRestore={vi.fn()}
      />,
    );
    expect(screen.getByText(/r1/)).toBeInTheDocument();
    // The changed line shows as a deletion (old "b") and addition (new "B").
    expect(screen.getByText("b").closest("[data-diff-row]")).toHaveAttribute(
      "data-diff-row",
      "del",
    );
    expect(screen.getByText("B").closest("[data-diff-row]")).toHaveAttribute(
      "data-diff-row",
      "add",
    );
    // Unchanged lines are context.
    expect(screen.getByText("a").closest("[data-diff-row]")).toHaveAttribute(
      "data-diff-row",
      "ctx",
    );
  });

  it("shows old/new gutter line numbers on diff rows", () => {
    render(
      <RevisionView
        revision="r1"
        contents={"a\nb"}
        current={"a\nb"}
        onBack={vi.fn()}
        onRestore={vi.fn()}
      />,
    );
    // Context row "b" is line 2 on both sides.
    const rowB = screen.getByText("b").closest("[data-diff-row]");
    expect(rowB?.textContent).toContain("2");
  });

  it("toggles to Full mode showing only the revision contents", () => {
    render(
      <RevisionView
        revision="r1"
        contents={"old body"}
        current={"new body"}
        onBack={vi.fn()}
        onRestore={vi.fn()}
      />,
    );
    // Diff mode renders both sides as rows.
    expect(
      screen.getByText("new body").closest("[data-diff-row]"),
    ).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^full$/i }));
    // Full mode renders the revision verbatim — no diff rows, no current copy.
    expect(document.querySelector("[data-diff-row]")).toBeNull();
    expect(screen.getByText("old body")).toBeInTheDocument();
    expect(screen.queryByText("new body")).toBeNull();
  });

  it("fires onBack and onRestore from the diff view", () => {
    const onBack = vi.fn();
    const onRestore = vi.fn();
    render(
      <RevisionView
        revision="r1"
        contents={"old"}
        current={"new"}
        onBack={onBack}
        onRestore={onRestore}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /back to current/i }));
    fireEvent.click(screen.getByRole("button", { name: /restore/i }));
    expect(onBack).toHaveBeenCalled();
    expect(onRestore).toHaveBeenCalled();
  });
});
