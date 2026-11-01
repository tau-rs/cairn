import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GraphForcesPanel } from "./GraphForcesPanel";
import { DEFAULT_FORCE_SETTINGS } from "./forceSettings";

describe("GraphForcesPanel", () => {
  it("renders a slider per force with the current value", () => {
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={vi.fn()}
        onReset={vi.fn()}
      />,
    );
    // Read the DOM string value directly (range inputs + jest-dom's numeric
    // coercion make toHaveValue ambiguous).
    const val = (label: string) =>
      (screen.getByLabelText(label) as HTMLInputElement).value;
    expect(val("Center force")).toBe("0.05");
    expect(val("Repel force")).toBe("-150");
    expect(val("Link force")).toBe("0.7");
    expect(val("Link distance")).toBe("80");
  });
  it("fires onChange with the updated field when a slider moves", () => {
    const onChange = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={onChange}
        onReset={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Link distance"), {
      target: { value: "120" },
    });
    expect(onChange).toHaveBeenCalledWith({
      ...DEFAULT_FORCE_SETTINGS,
      linkDistance: 120,
    });
  });
  it("toggles frozen via the freeze control", () => {
    const onChange = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={onChange}
        onReset={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByLabelText("Freeze layout"));
    expect(onChange).toHaveBeenCalledWith({
      ...DEFAULT_FORCE_SETTINGS,
      frozen: true,
    });
  });
  it("fires onReset when Reset is clicked", () => {
    const onReset = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={vi.fn()}
        onReset={onReset}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /reset/i }));
    expect(onReset).toHaveBeenCalled();
  });
});
