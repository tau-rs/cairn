import { describe, it, expect } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { AskPanelHost } from "./AskPanelHost";
import { cairnStore } from "../../app/cairnStore";

describe("AskPanelHost", () => {
  it("shows the panel only when mode is 'panel' and submits follow-ups", () => {
    render(
      <MemoryRouter>
        <AskPanelHost />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId("ask-panel")).toBeNull();

    act(() => {
      cairnStore.getState().askOpen();
      cairnStore.getState().askPromote();
    });
    expect(screen.getByTestId("ask-panel")).toBeInTheDocument();

    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "hi" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(cairnStore.getState().ask.turns.length).toBeGreaterThan(0);

    act(() => {
      cairnStore.getState().askClose();
    });
  });
});
