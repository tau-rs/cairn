import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TemporalScrubber } from "./TemporalScrubber";
import type { Revision } from "../../contract";

const tl: Revision[] = [
  {
    id: "c3",
    message: "third",
    timestamp_secs: 30,
    author: "a",
    summary: null,
    name: null,
  },
  {
    id: "c2",
    message: "second",
    timestamp_secs: 20,
    author: "a",
    summary: null,
    name: null,
  },
  {
    id: "c1",
    message: "first",
    timestamp_secs: 10,
    author: "a",
    summary: null,
    name: null,
  },
];

function renderScrubber(overrides = {}) {
  const props = {
    timeline: tl,
    selection: { kind: "live" } as const,
    onSelect: vi.fn(),
    counts: { notes: 3, links: 2 },
    delta: null,
    structural: false,
    onToggleStructural: vi.fn(),
    ...overrides,
  };
  render(<TemporalScrubber {...props} />);
  return props;
}

describe("TemporalScrubber", () => {
  it("renders Live, Browse and Compare controls", () => {
    renderScrubber();
    expect(screen.getByRole("button", { name: /live/i })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^browse$/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^compare$/i }),
    ).toBeInTheDocument();
  });

  it("Browse: moving to the leftmost position selects the oldest snapshot", () => {
    const { onSelect } = renderScrubber();
    // leftmost display index 0 = oldest (c1) → timeline index 2
    fireEvent.change(screen.getByLabelText(/timeline position/i), {
      target: { value: "0" },
    });
    expect(onSelect).toHaveBeenCalledWith({ kind: "snapshot", at: 2 });
  });

  it("Compare: setting from=oldest and to=newest emits a compare selection", async () => {
    const { onSelect } = renderScrubber();
    await userEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    fireEvent.change(screen.getByLabelText(/compare from/i), {
      target: { value: "0" }, // oldest → tl index 2
    });
    fireEvent.change(screen.getByLabelText(/compare to/i), {
      target: { value: "2" }, // newest → tl index 0
    });
    expect(onSelect).toHaveBeenLastCalledWith({
      kind: "compare",
      from: 2,
      to: 0,
    });
  });

  it("banner shows the snapshot date + message for a snapshot selection", () => {
    renderScrubber({ selection: { kind: "snapshot", at: 2 } }); // c1 "first"
    expect(screen.getByText(/viewing vault as of/i)).toBeInTheDocument();
    expect(screen.getByText(/first/)).toBeInTheDocument();
  });

  it("Compare mode syncs the range inputs from an incoming compare selection", () => {
    // partial range (not the full span, so it diverges from the old buggy
    // {from:0, to:n-1} default): from=1 (tl idx, c2) / to=0 (tl idx, c3)
    // → display from = n-1-1 = 1, display to = n-1-0 = 2
    renderScrubber({ selection: { kind: "compare", from: 1, to: 0 } });
    expect(
      (screen.getByLabelText(/compare from/i) as HTMLInputElement).value,
    ).toBe("1");
    expect(
      (screen.getByLabelText(/compare to/i) as HTMLInputElement).value,
    ).toBe("2");
  });

  it("entering Browse from an active compare selection reconciles the selection", async () => {
    // Bug T3a: the Browse tab only flipped local `mode`, leaving `selection` a
    // compare — so the banner (which reads `selection`) kept saying "Comparing…"
    // under the single Browse slider until the user dragged. Entering Browse must
    // emit a browse-consistent selection (snapshot of the newest revision).
    const { onSelect } = renderScrubber({
      selection: { kind: "compare", from: 1, to: 0 },
    });
    await userEvent.click(screen.getByRole("button", { name: /^browse$/i }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "snapshot", at: 0 });
  });

  it("collapsing a compare range to one point switches to Browse mode", () => {
    // Bug T3b: emitCompare emitted a {kind:"snapshot"} on from===to but left
    // local `mode` on "compare", so the "Viewing vault as of…" banner rendered
    // under the two-slider compare UI. The collapse must reconcile mode too.
    const { onSelect } = renderScrubber();
    fireEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    // set `from` (display idx 2, newest) equal to `to` (defaults to idx 2)
    fireEvent.change(screen.getByLabelText(/compare from/i), {
      target: { value: "2" },
    });
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "snapshot", at: 0 });
    // UI follows the snapshot banner: a single Browse slider, no compare pair.
    expect(screen.getByLabelText(/timeline position/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/compare from/i)).toBeNull();
  });

  it("renders a long timeline without a per-revision DOM blowup", () => {
    const long: Revision[] = Array.from({ length: 120 }, (_, i) => ({
      id: `r${i}`,
      message: `m${i}`,
      timestamp_secs: i,
      author: "a",
      summary: null,
      name: null,
    }));
    renderScrubber({ timeline: long });
    // histogram is fixed-bucket; no 120 buttons
    expect(screen.getAllByRole("button").length).toBeLessThan(20);
  });

  it("renders a Structural-only toggle reflecting the structural prop", () => {
    renderScrubber({ structural: true });
    const toggle = screen.getByRole("button", { name: /structural only/i });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });

  it("clicking the Structural-only toggle flips the current value", () => {
    const { onToggleStructural } = renderScrubber({ structural: false });
    fireEvent.click(screen.getByRole("button", { name: /structural only/i }));
    expect(onToggleStructural).toHaveBeenCalledWith(true);
  });

  it("clamps stale compare endpoints when the timeline shrinks (source swap)", () => {
    // Toggling "Structural only" swaps the timeline to a shorter source at
    // runtime. The compare endpoints were stored against the OLD length, so a
    // subsequent Compare click emitted out-of-range indices that silently
    // degraded to Live. The endpoints must re-clamp to the new range.
    const onSelect = vi.fn();
    const long: Revision[] = Array.from({ length: 12 }, (_, i) => ({
      id: `r${i}`,
      message: `m${i}`,
      timestamp_secs: i,
      author: "a",
      summary: null,
      name: null,
    }));
    const short = long.slice(0, 3); // 3 revisions → display idx 0..2
    const { rerender } = render(
      <TemporalScrubber
        timeline={long}
        selection={{ kind: "live" }}
        onSelect={onSelect}
        counts={null}
        delta={null}
        structural={false}
        onToggleStructural={vi.fn()}
      />,
    );
    // Enter compare while long (cmp defaults to the full 0..11 span).
    fireEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    // Timeline shrinks (structural source swapped in).
    rerender(
      <TemporalScrubber
        timeline={short}
        selection={{ kind: "live" }}
        onSelect={onSelect}
        counts={null}
        delta={null}
        structural={true}
        onToggleStructural={vi.fn()}
      />,
    );
    onSelect.mockClear();
    fireEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    // Emitted endpoints are within the new 3-revision timeline (tl indices 0..2).
    const calls = onSelect.mock.calls;
    const arg = calls[calls.length - 1]?.[0];
    expect(arg.kind).toBe("compare");
    expect(arg.from).toBeLessThanOrEqual(2);
    expect(arg.from).toBeGreaterThanOrEqual(0);
    expect(arg.to).toBeLessThanOrEqual(2);
    expect(arg.to).toBeGreaterThanOrEqual(0);
  });
});
