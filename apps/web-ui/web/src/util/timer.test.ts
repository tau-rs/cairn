import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { debounce } from "./timer";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("debounce", () => {
  it("invokes once after the delay, coalescing rapid calls", () => {
    const fn = vi.fn();
    const d = debounce(fn, 1000);
    d();
    d();
    d();
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("cancel() prevents a pending invocation", () => {
    const fn = vi.fn();
    const d = debounce(fn, 1000);
    d();
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
  });

  it("re-reads a thunk delay on every trigger", () => {
    // Exercises the `typeof ms === "function"` branch: a changed delay must
    // take effect without rebuilding the debounce.
    let delay = 100;
    const fn = vi.fn();
    const d = debounce(fn, () => delay);

    d();
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1);

    delay = 500;
    d();
    vi.advanceTimersByTime(100); // old delay would have fired here
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(400);
    expect(fn).toHaveBeenCalledTimes(2);
  });
});
