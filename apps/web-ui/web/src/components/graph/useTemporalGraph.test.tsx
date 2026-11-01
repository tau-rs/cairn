import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTemporalGraph } from "./useTemporalGraph";
import { cairnStore } from "../../app/cairnStore";
import type { Revision } from "../../contract";

const TL: Revision[] = [
  {
    id: "r2",
    message: "add b",
    timestamp_secs: 20,
    author: "x",
    summary: null,
    name: null,
  },
  {
    id: "r1",
    message: "init",
    timestamp_secs: 10,
    author: "x",
    summary: null,
    name: null,
  },
];

describe("useTemporalGraph", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(cairnStore.getState(), "loadVaultTimeline").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadSnapshot").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadDiff").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(
      () => {},
    );
    cairnStore.setState({
      temporal: { timeline: TL, snapshot: null, diff: null },
    });
  });
  afterEach(() => vi.useRealTimers());

  it("loads the whole-vault timeline on mount and starts live", () => {
    const { result } = renderHook(() => useTemporalGraph());
    expect(cairnStore.getState().loadVaultTimeline).toHaveBeenCalled();
    expect(result.current.mode).toBe("live");
    expect(result.current.source).toBeNull();
  });

  it("debounces loadSnapshot until the delay elapses on a snapshot selection", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 })); // r2
    expect(cairnStore.getState().loadSnapshot).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(150));
    expect(cairnStore.getState().loadSnapshot).toHaveBeenCalledWith("r2");
    expect(result.current.mode).toBe("snapshot");
  });

  it("clears temporal immediately (no debounce) when returning to live", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 }));
    act(() => vi.advanceTimersByTime(150));
    act(() => result.current.setSelection({ kind: "live" }));
    expect(cairnStore.getState().clearTemporal).toHaveBeenCalled();
  });

  it("cancels a pending snapshot load when returning to live before the debounce fires", () => {
    const { result } = renderHook(() => useTemporalGraph());
    // Spies are shared across this file's tests (beforeEach re-spies but doesn't
    // clear); scope the assertion to this interaction only.
    const { loadSnapshot, loadDiff, clearTemporal } = cairnStore.getState();
    vi.mocked(loadSnapshot).mockClear();
    vi.mocked(loadDiff).mockClear();
    vi.mocked(clearTemporal).mockClear();

    act(() => result.current.setSelection({ kind: "snapshot", at: 0 }));
    act(() => vi.advanceTimersByTime(100)); // < 150ms: debounce still pending
    act(() => result.current.setSelection({ kind: "live" }));
    act(() => vi.advanceTimersByTime(100)); // cumulative > 150ms, but it was cancelled
    expect(loadSnapshot).not.toHaveBeenCalled();
    expect(loadDiff).not.toHaveBeenCalled();
    expect(clearTemporal).toHaveBeenCalled();
  });

  it("dispatches loadDiff on a compare selection after the delay", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "compare", from: 1, to: 0 }));
    act(() => vi.advanceTimersByTime(150));
    expect(cairnStore.getState().loadDiff).toHaveBeenCalledWith("r1", "r2");
    expect(result.current.mode).toBe("compare");
  });

  it("starts non-structural and loads the full vault timeline", () => {
    const { result } = renderHook(() => useTemporalGraph());
    expect(result.current.structural).toBe(false);
    expect(cairnStore.getState().loadVaultTimeline).toHaveBeenCalledWith(false);
  });

  it("toggling structural reloads the structural source and resets to live", () => {
    const { result } = renderHook(() => useTemporalGraph());
    // Move off live first so the reset is observable.
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 }));
    vi.mocked(cairnStore.getState().loadVaultTimeline).mockClear();
    vi.mocked(cairnStore.getState().clearTemporal).mockClear();
    act(() => result.current.setStructural(true));
    expect(result.current.structural).toBe(true);
    expect(result.current.mode).toBe("live");
    expect(cairnStore.getState().loadVaultTimeline).toHaveBeenCalledWith(true);
    // A snapshot was active; the source swap must drop it explicitly so the
    // graph returns to Live rather than leaving a stale historical snapshot.
    expect(cairnStore.getState().clearTemporal).toHaveBeenCalled();
  });
});
