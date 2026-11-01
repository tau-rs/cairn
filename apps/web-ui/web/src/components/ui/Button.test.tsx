import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button } from "./Button";

describe("Button", () => {
  it("renders children and fires onClick", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Go</Button>);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(onClick).toHaveBeenCalled();
  });
  it("applies the primary variant accent background", () => {
    render(<Button variant="primary">P</Button>);
    expect(screen.getByRole("button", { name: "P" }).className).toContain(
      "bg-accent",
    );
  });
  it("ghost variant has no accent background", () => {
    render(<Button variant="ghost">G</Button>);
    expect(screen.getByRole("button", { name: "G" }).className).not.toContain(
      "bg-accent",
    );
  });
});
