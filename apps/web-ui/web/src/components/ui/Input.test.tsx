import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Input } from "./Input";

describe("Input", () => {
  it("forwards placeholder and fires onChange", async () => {
    const onChange = vi.fn();
    render(<Input placeholder="Search…" onChange={onChange} />);
    await userEvent.type(screen.getByPlaceholderText("Search…"), "x");
    expect(onChange).toHaveBeenCalled();
  });
});
