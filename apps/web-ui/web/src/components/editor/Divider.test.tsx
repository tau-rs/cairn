import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { Divider } from "./Divider";

describe("Divider", () => {
  it("nudges ratio with arrow keys", () => {
    const onRatio = vi.fn();
    render(<Divider ratio={0.5} onRatio={onRatio} />);
    const sep = screen.getByRole("separator");
    fireEvent.keyDown(sep, { key: "ArrowRight" });
    expect(onRatio).toHaveBeenCalledWith(expect.closeTo(0.52, 5));
    fireEvent.keyDown(sep, { key: "ArrowLeft" });
    expect(onRatio).toHaveBeenCalledWith(expect.closeTo(0.48, 5));
  });
});
