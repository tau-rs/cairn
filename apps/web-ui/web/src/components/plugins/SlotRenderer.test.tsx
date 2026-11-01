import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { cairnStore } from "../../app/cairnStore";
import { SlotRenderer } from "./SlotRenderer";
import { WidgetView } from "./WidgetView";

// Mock WidgetView so individual tests can make it throw to exercise the
// per-widget error boundary. The default impl renders the widget's primary
// label so the happy-path assertions still see real text.
vi.mock("./WidgetView", () => ({ WidgetView: vi.fn() }));

const mockWidgetView = vi.mocked(WidgetView);

/** Minimal label-surfacing stand-in for the happy path. */
function renderLabel({ widget }: { widget: unknown }) {
  const w = widget as {
    kind: string;
    items?: { id: string; label: string }[];
    label?: string;
  };
  if (w.kind === "list") {
    return (
      <ul>
        {(w.items ?? []).map((it) => (
          <li key={it.id}>{it.label}</li>
        ))}
      </ul>
    );
  }
  return <span>{w.label}</span>;
}

beforeEach(() => {
  mockWidgetView.mockImplementation(renderLabel as never);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("SlotRenderer", () => {
  // The empty-slot case runs FIRST, before any init(): the shared store's
  // `pluginContributions` starts `{}` and init() is guarded by a one-shot flag,
  // so this is the only clean window to assert the empty path.
  it("renders nothing for an empty slot", () => {
    const { container } = render(<SlotRenderer slot="topbar.action" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the demo contributions for a populated slot", async () => {
    await cairnStore.getState().init();
    render(<SlotRenderer slot="sidebar.section" />);
    expect(await screen.findByText("Insert stamp")).toBeInTheDocument();
  });

  it("isolates a throwing widget behind the local fallback, not the app card", async () => {
    mockWidgetView.mockImplementation(() => {
      throw new Error("boom");
    });
    // Silence the expected React error-boundary console noise.
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    await cairnStore.getState().init();
    render(<SlotRenderer slot="sidebar.section" />);

    // Local fallback shows (one per throwing widget — use getAllBy since
    // sidebar.section now has multiple contributions including wordcount).
    expect(screen.getAllByText(/widget unavailable/i)[0]).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: /retry/i })[0],
    ).toBeInTheDocument();

    // App-level reload card must NOT appear.
    expect(screen.queryByText(/Something went wrong/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Reload/i })).toBeNull();

    errSpy.mockRestore();
  });

  it("the retry button is clickable (wired to reset)", async () => {
    mockWidgetView.mockImplementation(() => {
      throw new Error("boom");
    });
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    await cairnStore.getState().init();
    render(<SlotRenderer slot="sidebar.section" />);

    // Use getAllByRole since sidebar.section now has multiple contributions
    // (wordcount + demo) and all throw when WidgetView is mocked to throw.
    const retries = screen.getAllByRole("button", { name: /retry/i });
    expect(() => fireEvent.click(retries[0])).not.toThrow();

    errSpy.mockRestore();
  });
});
