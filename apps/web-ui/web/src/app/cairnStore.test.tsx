import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { cairnStore, useActions } from "./cairnStore";

describe("useActions", () => {
  it("reactively selects the store's action methods", () => {
    const { result } = renderHook(() => useActions());
    // Same identity as the store's own action — selected, not re-wrapped.
    expect(result.current.openNote).toBe(cairnStore.getState().openNote);
    expect(typeof result.current.editBuffer).toBe("function");
    expect(typeof result.current.refreshAll).toBe("function");
  });

  it("keeps a stable reference when an unrelated data slice changes", () => {
    const { result } = renderHook(() => useActions());
    const first = result.current;
    act(() => cairnStore.setState({ backlinks: ["x.md"] }));
    // useShallow keeps the action bag referentially stable, so consumers don't
    // re-render on unrelated state changes.
    expect(result.current).toBe(first);
  });
});
