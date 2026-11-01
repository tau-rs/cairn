import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConflictDialog } from "./ConflictDialog";

describe("ConflictDialog", () => {
  it("renders nothing when closed", () => {
    render(
      <ConflictDialog
        open={false}
        onOpenChange={() => {}}
        onKeepMine={() => {}}
        onSeeTheirs={() => {}}
      />,
    );
    expect(screen.queryByText(/also changed/i)).not.toBeInTheDocument();
  });

  it("keeps my version by default action, no danger styling", async () => {
    const onKeep = vi.fn();
    render(
      <ConflictDialog
        open
        onOpenChange={() => {}}
        onKeepMine={onKeep}
        onSeeTheirs={() => {}}
      />,
    );
    expect(
      screen.getByText("This note also changed on another device"),
    ).toBeInTheDocument();
    const keep = screen.getByRole("button", { name: "Keep my version" });
    expect(keep.className).not.toMatch(/danger/);
    await userEvent.click(keep);
    expect(onKeep).toHaveBeenCalled();
  });

  it("see their version is offered and non-destructive", async () => {
    const onSee = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <ConflictDialog
        open
        onOpenChange={onOpenChange}
        onKeepMine={() => {}}
        onSeeTheirs={onSee}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "See their version" }),
    );
    expect(onSee).toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
