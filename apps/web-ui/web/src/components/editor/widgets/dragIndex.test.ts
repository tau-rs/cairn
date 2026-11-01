import { describe, it, expect } from "vitest";
import { dropIndex, dropTarget } from "./dragIndex";

describe("dropIndex", () => {
  const centers = [10, 30, 50];
  it("returns 0 before the first center", () => {
    expect(dropIndex(5, centers)).toBe(0);
  });
  it("returns the index past centers the pointer has crossed", () => {
    expect(dropIndex(35, centers)).toBe(2);
  });
  it("returns length past the last center", () => {
    expect(dropIndex(99, centers)).toBe(3);
  });
});

describe("dropTarget", () => {
  const centers = [10, 30, 50, 70];
  it("drops a middle item one slot down (no overshoot)", () => {
    // drag item 1 into the gap between items 2 and 3 (p=60) → land at index 2
    expect(dropTarget(60, centers, 1)).toBe(2);
  });
  it("drops an item upward at the crossed index", () => {
    // drag item 3 into the gap between items 0 and 1 (p=20) → land at index 1
    expect(dropTarget(20, centers, 3)).toBe(1);
  });
  it("drags to the very bottom", () => {
    expect(dropTarget(9999, centers, 1)).toBe(3);
  });
  it("returns the same index for a no-op drag", () => {
    // pointer still over the item's own region → same slot
    expect(dropTarget(40, centers, 1)).toBe(1);
  });
});
