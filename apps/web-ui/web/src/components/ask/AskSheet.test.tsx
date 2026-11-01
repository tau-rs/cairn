import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskSheet } from "./AskSheet";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  side: "bottom" as const,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskSheet", () => {
  it("renders nothing when closed", () => {
    render(<AskSheet {...base} open={false} />);
    expect(screen.queryByTestId("ask-sheet")).toBeNull();
  });

  it("renders all turns when open", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q1", citations: [], tools: [] },
      { role: "assistant", text: "a1", citations: [], tools: [] },
    ];
    render(<AskSheet {...base} turns={turns} />);
    expect(screen.getByText("q1")).toBeInTheDocument();
    expect(screen.getByText("a1")).toBeInTheDocument();
  });

  it("submits a follow-up", () => {
    const onSubmit = vi.fn();
    render(<AskSheet {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "more?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("more?");
  });

  it("closes via the ✕ button", () => {
    const onClose = vi.fn();
    render(<AskSheet {...base} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /close ask/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it("renders as a right side sheet too", () => {
    render(<AskSheet {...base} side="right" />);
    expect(screen.getByTestId("ask-sheet")).toBeInTheDocument();
  });
});
