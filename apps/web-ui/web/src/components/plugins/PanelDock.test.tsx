import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { cairnStore } from "../../app/cairnStore";
import { PanelDock } from "./PanelDock";
import { WidgetView } from "./WidgetView";
import { DEFAULT_UI } from "../../store/store";

// Stub WidgetView so the dock test doesn't pull in IframeHost/consent. It just
// surfaces the active widget's entry so we can assert which one is rendered.
vi.mock("./WidgetView", () => ({ WidgetView: vi.fn() }));
const mockWidgetView = vi.mocked(WidgetView);

function renderEntry({ widget }: { widget: unknown }) {
  const w = widget as { entry?: string };
  return <div data-testid="widget">{w.entry}</div>;
}

/** A panel.main SlotEntry carrying an iframe widget. */
function entry(plugin: string, id: string, title: string, file: string) {
  return {
    plugin,
    epoch: 1,
    c: {
      id,
      slot: "panel.main",
      title,
      icon: null,
      order: null,
      widget: { kind: "iframe", entry: file, height: null },
    },
  };
}

function seed(entries: ReturnType<typeof entry>[]) {
  cairnStore.setState({
    pluginContributions: { "panel.main": entries },
    ui: { ...DEFAULT_UI, panelDock: { activeId: null, collapsed: false } },
  } as never);
}

beforeEach(() => {
  mockWidgetView.mockImplementation(renderEntry as never);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  cairnStore.setState({ pluginContributions: {} } as never);
});

describe("PanelDock", () => {
  it("renders nothing when no panel.main entries", () => {
    cairnStore.setState({ pluginContributions: {} } as never);
    const { container } = render(<PanelDock />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a tab per entry and switches the active widget on tab click", () => {
    seed([
      entry("a", "one", "One", "a.html"),
      entry("b", "two", "Two", "b.html"),
    ]);
    render(<PanelDock />);

    // Both tabs are present by title; the first entry is active by default.
    expect(screen.getByRole("tab", { name: "One" })).toBeInTheDocument();
    const two = screen.getByRole("tab", { name: "Two" });
    expect(screen.getByTestId("widget")).toHaveTextContent("a.html");

    fireEvent.click(two);
    expect(two).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("widget")).toHaveTextContent("b.html");
  });

  it("collapse toggle hides the widget", () => {
    seed([entry("a", "one", "One", "a.html")]);
    render(<PanelDock />);
    expect(screen.getByTestId("widget")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: /collapse plugin panel/i }),
    );
    expect(screen.queryByTestId("widget")).toBeNull();
    // The tab strip stays; an expand affordance is now offered.
    expect(
      screen.getByRole("button", { name: /expand plugin panel/i }),
    ).toBeInTheDocument();
  });
});
