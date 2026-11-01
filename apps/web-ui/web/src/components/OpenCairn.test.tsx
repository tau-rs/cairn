import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OpenCairn } from "./OpenCairn";

describe("OpenCairn", () => {
  it("calls onOpen when the button is clicked", async () => {
    const onOpen = vi.fn();
    render(<OpenCairn onOpen={onOpen} />);
    await userEvent.click(
      screen.getByRole("button", { name: /open a cairn/i }),
    );
    expect(onOpen).toHaveBeenCalled();
  });
});
