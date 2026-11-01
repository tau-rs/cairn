import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Drawer } from "./Drawer";

describe("Drawer", () => {
  it("renders children when open and is labelled", () => {
    render(
      <Drawer open onClose={() => {}} side="right" label="Backlinks">
        <div>panel body</div>
      </Drawer>,
    );
    expect(screen.getByText("panel body")).toBeInTheDocument();
    expect(
      screen.getByRole("dialog", { name: "Backlinks" }),
    ).toBeInTheDocument();
  });

  it("does not render children when closed", () => {
    render(
      <Drawer open={false} onClose={() => {}} side="right" label="Backlinks">
        <div>panel body</div>
      </Drawer>,
    );
    expect(screen.queryByText("panel body")).not.toBeInTheDocument();
  });

  it("calls onClose on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Drawer open onClose={onClose} side="bottom" label="Sheet">
        <div>body</div>
      </Drawer>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
