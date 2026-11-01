import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BottomNav } from "./BottomNav";

describe("BottomNav", () => {
  it("renders five tabs and marks the active one", () => {
    render(<BottomNav active="graph" onSelect={() => {}} />);
    for (const label of ["Files", "Editor", "Search", "Graph", "More"]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Graph" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("calls onSelect with the tab id", async () => {
    const onSelect = vi.fn();
    render(<BottomNav active="editor" onSelect={onSelect} />);
    await userEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(onSelect).toHaveBeenCalledWith("files");
  });
});
