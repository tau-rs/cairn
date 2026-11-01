import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskPanel } from "./AskPanel";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskPanel", () => {
  it("renders nothing when closed", () => {
    render(<AskPanel {...base} open={false} />);
    expect(screen.queryByTestId("ask-panel")).toBeNull();
  });

  it("renders all turns when open", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q1", citations: [], tools: [] },
      { role: "assistant", text: "a1", citations: [], tools: [] },
    ];
    render(<AskPanel {...base} turns={turns} />);
    expect(screen.getByText("q1")).toBeInTheDocument();
    expect(screen.getByText("a1")).toBeInTheDocument();
  });

  it("submits a follow-up", () => {
    const onSubmit = vi.fn();
    render(<AskPanel {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "more?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("more?");
  });

  it("closes via the close button", () => {
    const onClose = vi.fn();
    render(<AskPanel {...base} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /close ask panel/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<AskPanel {...base} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
