import { describe, it, expect } from "vitest";
import {
  openOrPreview,
  pinTab,
  closeTab,
  cycle,
  jumpTo,
  type TabsState,
} from "./tabsModel";

const empty: TabsState = { tabs: [], activePath: null };

describe("openOrPreview", () => {
  it("appends a preview tab when opening into an empty set", () => {
    expect(openOrPreview(empty, "a.md")).toEqual({
      tabs: [{ path: "a.md", preview: true }],
      activePath: "a.md",
    });
  });
  it("replaces the existing preview tab in place", () => {
    const s = openOrPreview(empty, "a.md");
    expect(openOrPreview(s, "b.md")).toEqual({
      tabs: [{ path: "b.md", preview: true }],
      activePath: "b.md",
    });
  });
  it("appends a new preview tab when the current one is pinned", () => {
    const s = pinTab(openOrPreview(empty, "a.md"), "a.md");
    const r = openOrPreview(s, "b.md");
    expect(r.tabs).toEqual([
      { path: "a.md", preview: false },
      { path: "b.md", preview: true },
    ]);
    expect(r.activePath).toBe("b.md");
  });
  it("focuses an already-open tab without duplicating", () => {
    const s = pinTab(openOrPreview(empty, "a.md"), "a.md");
    const withB = openOrPreview(s, "b.md");
    const r = openOrPreview(withB, "a.md");
    expect(r.tabs.map((t) => t.path)).toEqual(["a.md", "b.md"]);
    expect(r.activePath).toBe("a.md");
  });
});

describe("pinTab", () => {
  it("clears the preview flag for the given path", () => {
    const s = openOrPreview(empty, "a.md");
    expect(pinTab(s, "a.md").tabs[0]).toEqual({ path: "a.md", preview: false });
  });
});

describe("closeTab", () => {
  const three: TabsState = {
    tabs: [
      { path: "a.md", preview: false },
      { path: "b.md", preview: false },
      { path: "c.md", preview: false },
    ],
    activePath: "b.md",
  };
  it("focuses the right neighbour when closing the active tab", () => {
    const r = closeTab(three, "b.md");
    expect(r.tabs.map((t) => t.path)).toEqual(["a.md", "c.md"]);
    expect(r.activePath).toBe("c.md");
  });
  it("focuses the left neighbour when closing the active last tab", () => {
    const r = closeTab({ ...three, activePath: "c.md" }, "c.md");
    expect(r.activePath).toBe("b.md");
  });
  it("keeps the active path when closing a non-active tab", () => {
    const r = closeTab(three, "a.md");
    expect(r.activePath).toBe("b.md");
  });
  it("returns null active when closing the last remaining tab", () => {
    const one: TabsState = {
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    };
    expect(closeTab(one, "a.md")).toEqual({ tabs: [], activePath: null });
  });
});

describe("cycle / jumpTo", () => {
  const s: TabsState = {
    tabs: [
      { path: "a.md", preview: false },
      { path: "b.md", preview: false },
      { path: "c.md", preview: false },
    ],
    activePath: "a.md",
  };
  it("cycles forward and wraps", () => {
    expect(cycle(s, 1).activePath).toBe("b.md");
    expect(cycle({ ...s, activePath: "c.md" }, 1).activePath).toBe("a.md");
  });
  it("cycles backward and wraps", () => {
    expect(cycle(s, -1).activePath).toBe("c.md");
  });
  it("jumps to the Nth tab (1-based) and ignores out of range", () => {
    expect(jumpTo(s, 2).activePath).toBe("b.md");
    expect(jumpTo(s, 9)).toEqual(s);
  });
});
