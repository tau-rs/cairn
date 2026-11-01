import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TabStrip } from "./TabStrip";

const tabs = [
  { path: "a.md", preview: false, dirty: false },
  { path: "ideas.md", preview: true, dirty: true },
];

function setup(over = {}) {
  const props = {
    tabs,
    activePath: "a.md",
    onSelect: vi.fn(),
    onPin: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  render(<TabStrip {...props} />);
  return props;
}

describe("TabStrip", () => {
  it("renders a tab per open note with the stem label", () => {
    setup();
    expect(screen.getByRole("tab", { name: /a$/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /ideas/ })).toBeInTheDocument();
  });
  it("marks the active tab as selected", () => {
    setup();
    expect(screen.getByRole("tab", { name: /a$/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
  it("calls onSelect when a tab is clicked", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("tab", { name: /ideas/ }));
    expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
  });
  it("calls onPin on double-click", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("tab", { name: /ideas/ }));
    expect(props.onPin).toHaveBeenCalledWith("ideas.md");
  });
  it("calls onClose (not onSelect) when the × is clicked", () => {
    const props = setup();
    fireEvent.click(screen.getByLabelText("close ideas"));
    expect(props.onClose).toHaveBeenCalledWith("ideas.md");
    expect(props.onSelect).not.toHaveBeenCalled();
  });
  it("makes the active tab focusable and others not (roving tabindex)", () => {
    setup();
    expect(screen.getByRole("tab", { name: /a$/ })).toHaveAttribute(
      "tabindex",
      "0",
    );
    expect(screen.getByRole("tab", { name: /ideas/ })).toHaveAttribute(
      "tabindex",
      "-1",
    );
  });
  it("activates a tab on Enter", () => {
    const props = setup();
    fireEvent.keyDown(screen.getByRole("tab", { name: /ideas/ }), {
      key: "Enter",
    });
    expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
  });
  it("activates a tab on Space", () => {
    const props = setup();
    fireEvent.keyDown(screen.getByRole("tab", { name: /ideas/ }), { key: " " });
    expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
  });
  it("moves focus (not selection) to the next tab on ArrowRight (wrapping)", () => {
    const props = setup(); // active = a.md (index 0)
    fireEvent.keyDown(screen.getByRole("tab", { name: /a$/ }), {
      key: "ArrowRight",
    });
    expect(screen.getByRole("tab", { name: /ideas/ })).toHaveFocus();
    expect(props.onSelect).not.toHaveBeenCalled(); // manual activation
  });
  it("moves focus to the previous tab on ArrowLeft (wrapping to last)", () => {
    const props = setup(); // active = a.md (index 0) -> wraps to ideas.md
    fireEvent.keyDown(screen.getByRole("tab", { name: /a$/ }), {
      key: "ArrowLeft",
    });
    expect(screen.getByRole("tab", { name: /ideas/ })).toHaveFocus();
    expect(props.onSelect).not.toHaveBeenCalled();
  });
  it("renders nothing when there are no tabs", () => {
    const { container } = render(
      <TabStrip
        tabs={[]}
        activePath={null}
        onSelect={vi.fn()}
        onPin={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(container.querySelector('[role="tablist"]')).toBeNull();
  });

  it("renders a Split right button and fires onSplit", () => {
    const onSplit = vi.fn();
    render(
      <TabStrip
        tabs={[{ path: "a.md", preview: false, dirty: false }]}
        activePath="a.md"
        onSelect={() => {}}
        onPin={() => {}}
        onClose={() => {}}
        onSplit={onSplit}
      />,
    );
    fireEvent.click(screen.getByLabelText("Split editor right"));
    expect(onSplit).toHaveBeenCalled();
  });

  it("renders a Close pane button and fires onClosePane", () => {
    const onClosePane = vi.fn();
    render(
      <TabStrip
        tabs={[{ path: "a.md", preview: false, dirty: false }]}
        activePath="a.md"
        onSelect={() => {}}
        onPin={() => {}}
        onClose={() => {}}
        onClosePane={onClosePane}
      />,
    );
    fireEvent.click(screen.getByLabelText("Close pane"));
    expect(onClosePane).toHaveBeenCalled();
  });
});
