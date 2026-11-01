import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useBreakpoint } from "./useBreakpoint";

/** Build a matchMedia stub driven by a single viewport width. */
function installMatchMedia(width: number) {
  const listeners = new Set<() => void>();
  const mql = (query: string) => {
    const min = Number(/min-width:\s*(\d+)px/.exec(query)?.[1] ?? "0");
    return {
      matches: width >= min,
      media: query,
      addEventListener: (_: string, cb: () => void) => listeners.add(cb),
      removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
    } as unknown as MediaQueryList;
  };
  window.matchMedia = mql as unknown as typeof window.matchMedia;
  return {
    resize(next: number) {
      width = next;
      act(() => listeners.forEach((cb) => cb()));
    },
  };
}

describe("useBreakpoint", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("maps width to tier at the boundaries", () => {
    installMatchMedia(500);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("mobile");
    installMatchMedia(767);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("mobile");
    installMatchMedia(768);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("tablet");
    installMatchMedia(1023);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("tablet");
    installMatchMedia(1024);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("desktop");
  });

  it("updates when the viewport crosses a breakpoint", () => {
    const mm = installMatchMedia(500);
    const { result } = renderHook(() => useBreakpoint());
    expect(result.current).toBe("mobile");
    mm.resize(1200);
    expect(result.current).toBe("desktop");
  });
});
