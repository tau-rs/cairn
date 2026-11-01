import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { cairnStore } from "./cairnStore";
import { useSealHints } from "./useSealHints";

describe("useSealHints", () => {
  beforeEach(() => {
    cairnStore.setState({ activePath: "a.md" });
  });

  it("seals on note switch", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    cairnStore.setState({ activePath: "b.md" });
    expect(seal).toHaveBeenCalledTimes(1);
  });

  it("seals on window blur", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    window.dispatchEvent(new Event("blur"));
    expect(seal).toHaveBeenCalledTimes(1);
  });

  it("does not seal when the path did not change", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    cairnStore.setState({ query: "unrelated" });
    expect(seal).not.toHaveBeenCalled();
  });
});
