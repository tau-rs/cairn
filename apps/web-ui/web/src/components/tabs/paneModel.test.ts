import { describe, it, expect } from "vitest";
import { splitPane, closePane, focusPane, type PanesState } from "./paneModel";

const single: PanesState = {
  panes: [
    {
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    },
  ],
  activePane: 0,
};

describe("splitPane", () => {
  it("appends a second pane seeded (pinned) with seedPath and focuses it", () => {
    const r = splitPane(single, "a.md");
    expect(r.panes).toHaveLength(2);
    expect(r.panes[1]).toEqual({
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    });
    expect(r.activePane).toBe(1);
  });
  it("is a no-op when already split", () => {
    const split = splitPane(single, "a.md");
    expect(splitPane(split, "b.md")).toBe(split);
  });
  it("is a no-op when seedPath is null", () => {
    expect(splitPane(single, null)).toBe(single);
  });
});

describe("closePane", () => {
  it("removes the pane and clamps activePane to the survivor", () => {
    const split = splitPane(single, "a.md"); // activePane 1
    const r = closePane(split, 1);
    expect(r.panes).toHaveLength(1);
    expect(r.activePane).toBe(0);
  });
  it("keeps activePane valid when closing pane 0 while focused on it", () => {
    const split = { ...splitPane(single, "a.md"), activePane: 0 };
    const r = closePane(split, 0);
    expect(r.panes).toHaveLength(1);
    expect(r.activePane).toBe(0);
  });
  it("refuses to remove the last pane", () => {
    expect(closePane(single, 0)).toBe(single);
  });
});

describe("focusPane", () => {
  it("sets activePane within range", () => {
    const split = splitPane(single, "a.md");
    expect(focusPane(split, 0).activePane).toBe(0);
  });
  it("ignores out-of-range indices", () => {
    expect(focusPane(single, 5)).toBe(single);
    expect(focusPane(single, -1)).toBe(single);
  });
});
