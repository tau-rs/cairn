import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AnswerView } from "./AnswerView";
import type { AskTurn } from "../../store/askReducer";

const turn = (over: Partial<AskTurn> = {}): AskTurn => ({
  role: "assistant",
  text: "",
  citations: [],
  tools: [],
  ...over,
});

describe("AnswerView", () => {
  it("renders text and linkifies inline citations", () => {
    const onOpenNote = vi.fn();
    render(
      <AnswerView
        turn={turn({ text: "see [[store]] now" })}
        streaming={false}
        onOpenNote={onOpenNote}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "store" }));
    expect(onOpenNote).toHaveBeenCalledWith("store");
  });

  it("shows a sources footer for assistant turns with citations", () => {
    render(
      <AnswerView
        turn={turn({ text: "x [[store]]", citations: ["notes/store.md"] })}
        streaming={false}
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByTestId("sources")).toHaveTextContent("store");
  });

  it("renders a source citation labeled by stem but opens the full path", () => {
    const turn = {
      role: "assistant" as const,
      text: "answer",
      citations: ["notes/alpha.md"],
      tools: [],
    };
    const onOpenNote = vi.fn();
    render(
      <AnswerView turn={turn} streaming={false} onOpenNote={onOpenNote} />,
    );
    const btn = screen.getByRole("button", { name: /alpha/ });
    // label should be the stem, not the full path
    expect(btn.textContent).not.toContain("notes/");
    fireEvent.click(btn);
    expect(onOpenNote).toHaveBeenCalledWith("notes/alpha.md");
  });

  it("shows a caret while streaming and a running tool indicator", () => {
    render(
      <AnswerView
        turn={turn({
          text: "partial",
          tools: [{ tool: "search_notes", ok: null }],
        })}
        streaming
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByTestId("caret")).toBeInTheDocument();
    expect(screen.getByTestId("tool")).toHaveTextContent("search_notes");
  });

  it("renders a user turn as plain text with no sources", () => {
    render(
      <AnswerView
        turn={turn({ role: "user", text: "my question" })}
        streaming={false}
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByText("my question")).toBeInTheDocument();
    expect(screen.queryByTestId("sources")).toBeNull();
  });
});
