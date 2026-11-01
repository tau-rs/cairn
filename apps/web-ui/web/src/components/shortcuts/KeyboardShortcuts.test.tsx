import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { KeyboardShortcuts } from "./KeyboardShortcuts";

beforeEach(() => localStorage.clear());

function setup(overrides = {}) {
  const onChange = vi.fn();
  render(<KeyboardShortcuts overrides={overrides} onChange={onChange} />);
  return { onChange };
}

describe("KeyboardShortcuts", () => {
  it("renders each command's effective binding", () => {
    setup();
    // jsdom is non-mac → "Ctrl+N"
    expect(
      screen.getByRole("button", { name: "rebind New note" }),
    ).toHaveTextContent("Ctrl+N");
  });
  it("captures a new modifier-bearing chord", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    const input = screen.getByLabelText("press keys for New note");
    fireEvent.keyDown(input, { key: "j", ctrlKey: true });
    expect(onChange).toHaveBeenCalledWith({ "new-note": "Mod+J" });
  });
  it("ignores a bare key during capture", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    fireEvent.keyDown(screen.getByLabelText("press keys for New note"), {
      key: "j",
    });
    expect(onChange).not.toHaveBeenCalled();
  });
  it("warns on a conflict and Force unbinds the other command", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    fireEvent.keyDown(screen.getByLabelText("press keys for New note"), {
      key: "w",
      ctrlKey: true,
    }); // Mod+W = Close tab's default
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText(/already bound to Close tab/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /force/i }));
    expect(onChange).toHaveBeenCalledWith({
      "close-tab": null,
      "new-note": "Mod+W",
    });
  });
  it("resets an overridden binding", () => {
    const { onChange } = setup({ "new-note": "Mod+J" });
    fireEvent.click(screen.getByRole("button", { name: "reset New note" }));
    expect(onChange).toHaveBeenCalledWith({});
  });
});
