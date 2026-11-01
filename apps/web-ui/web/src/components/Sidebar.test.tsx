import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { cairnStore } from "../app/cairnStore";

describe("Sidebar plugin slot mount", () => {
  it("renders sidebar.section plugin contributions", async () => {
    await cairnStore.getState().init();
    render(
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Insert stamp")).toBeInTheDocument();
  });
});
