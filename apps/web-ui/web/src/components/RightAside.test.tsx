import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { RightAside } from "./RightAside";
import { cairnStore } from "../app/cairnStore";

beforeEach(async () => {
  localStorage.clear();
  cairnStore.getState().setRightTab("backlinks");
  await cairnStore.getState().init();
});

function renderAside() {
  return render(
    <MemoryRouter>
      <RightAside />
    </MemoryRouter>,
  );
}

describe("RightAside", () => {
  it("shows Backlinks by default and switches to Versions on tab click", async () => {
    await cairnStore.getState().openNote("index.md");
    renderAside();
    expect(screen.getByRole("tab", { name: /backlinks/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /versions/i }));
    expect(await screen.findByText(/No versions yet/i)).toBeInTheDocument();
  });
});
