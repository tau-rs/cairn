import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskBar } from "./AskBar";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onPromote: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskBar", () => {
  it("submits the typed question on Enter and clears the input", () => {
    const onSubmit = vi.fn();
    render(<AskBar {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask about your notes…");
    fireEvent.change(input, { target: { value: "how?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("how?");
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("renders the latest assistant turn", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q", citations: [], tools: [] },
      { role: "assistant", text: "the answer", citations: [], tools: [] },
    ];
    render(<AskBar {...base} turns={turns} />);
    expect(screen.getByText("the answer")).toBeInTheDocument();
  });

  it("promotes to the panel", () => {
    const onPromote = vi.fn();
    render(<AskBar {...base} onPromote={onPromote} />);
    fireEvent.click(screen.getByRole("button", { name: /continue in panel/i }));
    expect(onPromote).toHaveBeenCalled();
  });

  it("shows the error state", () => {
    render(<AskBar {...base} error="boom" />);
    expect(screen.getByTestId("ask-error")).toHaveTextContent("boom");
  });
});
