import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { PluginWidget } from "../../contract/PluginWidget";
import { WidgetView } from "./WidgetView";

const invokePlugin = vi.fn();
vi.mock("../../app/cairnStore", () => ({
  useActions: () => ({ invokePlugin }),
}));

beforeEach(() => invokePlugin.mockClear());

describe("WidgetView", () => {
  it("renders text widget", () => {
    const widget: PluginWidget = { kind: "text", text: "hello", muted: null };
    render(<WidgetView plugin="p" widget={widget} />);
    expect(screen.getByText("hello")).toBeInTheDocument();
  });

  it("renders a <script> text value as inert text, not executed", () => {
    const payload = "<script>window.__pwned = true</script>";
    const widget: PluginWidget = { kind: "text", text: payload, muted: null };
    render(<WidgetView plugin="p" widget={widget} />);
    // React auto-escapes: the literal string shows up as text.
    expect(screen.getByText(payload)).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
  });

  it("action click invokes plugin with args passthrough", () => {
    const widget: PluginWidget = {
      kind: "action",
      label: "Go",
      icon: null,
      command: "go",
      args: { a: 1 },
    };
    render(<WidgetView plugin="p" widget={widget} />);
    fireEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(invokePlugin).toHaveBeenCalledTimes(1);
    expect(invokePlugin).toHaveBeenCalledWith("p", "go", { a: 1 });
  });

  it("list item click invokes plugin with null args", () => {
    const widget: PluginWidget = {
      kind: "list",
      items: [{ id: "1", label: "Row", icon: null, command: "c", args: null }],
    };
    render(<WidgetView plugin="p" widget={widget} />);
    fireEvent.click(screen.getByText("Row"));
    expect(invokePlugin).toHaveBeenCalledTimes(1);
    expect(invokePlugin).toHaveBeenCalledWith("p", "c", null);
  });

  it("list item with null command is inert", () => {
    const widget: PluginWidget = {
      kind: "list",
      items: [
        { id: "1", label: "Inert", icon: null, command: null, args: null },
      ],
    };
    render(<WidgetView plugin="p" widget={widget} />);
    fireEvent.click(screen.getByText("Inert"));
    expect(invokePlugin).not.toHaveBeenCalled();
  });

  it("renders nothing for an unknown kind", () => {
    const { container } = render(
      <WidgetView plugin="p" widget={{ kind: "x" } as never} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
