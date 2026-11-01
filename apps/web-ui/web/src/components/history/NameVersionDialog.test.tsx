import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NameVersionDialog } from "./NameVersionDialog";

describe("NameVersionDialog", () => {
  it("names the version and closes", async () => {
    const onName = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <NameVersionDialog open onOpenChange={onOpenChange} onName={onName} />,
    );
    await userEvent.type(screen.getByPlaceholderText(/e\.g\./i), "Draft 1");
    await userEvent.click(screen.getByRole("button", { name: "Name version" }));
    expect(onName).toHaveBeenCalledWith("Draft 1");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("disables submit when empty", () => {
    render(
      <NameVersionDialog open onOpenChange={() => {}} onName={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "Name version" })).toBeDisabled();
  });
});
