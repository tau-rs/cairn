import { describe, it, expect } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { AskSheetHost } from "./AskSheetHost";
import { cairnStore } from "../../app/cairnStore";

describe("AskSheetHost", () => {
  it("shows the sheet only in panel mode and submits follow-ups", () => {
    render(
      <MemoryRouter>
        <AskSheetHost side="bottom" />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId("ask-sheet")).toBeNull();

    act(() => {
      cairnStore.getState().askOpen();
      cairnStore.getState().askPromote();
    });
    expect(screen.getByTestId("ask-sheet")).toBeInTheDocument();

    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "hi" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(cairnStore.getState().ask.turns.length).toBeGreaterThan(0);

    act(() => {
      cairnStore.getState().askClose();
    });
  });
});
