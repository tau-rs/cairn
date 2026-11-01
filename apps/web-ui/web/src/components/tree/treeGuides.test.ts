import { describe, it, expect } from "vitest";
import {
  onActivePath,
  childGuides,
  rowGuides,
  indentPad,
  type Guide,
} from "./treeGuides";

describe("onActivePath", () => {
  it("matches the active note exactly", () => {
    expect(onActivePath("a/b.md", "a/b.md")).toBe(true);
  });
  it("matches an ancestor folder of the active note", () => {
    expect(onActivePath("a", "a/b.md")).toBe(true);
    expect(onActivePath("a/sub", "a/sub/c.md")).toBe(true);
  });
  it("does not match an unrelated path or a prefix substring", () => {
    expect(onActivePath("a", "x.md")).toBe(false);
    expect(onActivePath("ab", "a/b.md")).toBe(false); // not a path-segment prefix
  });
  it("is false when nothing is active", () => {
    expect(onActivePath("a", null)).toBe(false);
  });
});

describe("childGuides", () => {
  it("root nodes contribute no column", () => {
    expect(childGuides([], 0, false, false)).toEqual([]);
  });
  it("appends a continuing column when the node has a younger sibling", () => {
    expect(childGuides([], 1, false, true)).toEqual([
      { show: true, accent: true },
    ]);
  });
  it("appends a non-showing column when the node is the last sibling", () => {
    expect(childGuides([], 1, true, false)).toEqual([
      { show: false, accent: false },
    ]);
  });
});

describe("rowGuides", () => {
  it("draws nothing at the root", () => {
    expect(rowGuides([], 0, false, false)).toEqual([]);
  });

  it("draws a connector line + tick for a depth-1 row", () => {
    const marks = rowGuides([], 1, false, false);
    expect(marks).toEqual([
      { x: 16, variant: "line", accent: false, toCenter: false },
      { x: 16, variant: "tick", accent: false },
    ]);
  });

  it("ends the connector at center (└) for the last child", () => {
    const marks = rowGuides([], 1, true, false);
    expect(marks[0]).toMatchObject({ variant: "line", toCenter: true });
  });

  it("draws an ancestor pass-through column plus its own connector", () => {
    const ancestors: Guide[] = [{ show: true, accent: false }];
    const marks = rowGuides(ancestors, 2, false, true);
    expect(marks).toEqual([
      { x: 16, variant: "line", accent: false }, // pass-through (level 0)
      { x: 32, variant: "line", accent: true, toCenter: false }, // connector
      { x: 32, variant: "tick", accent: true },
    ]);
  });

  it("omits a hidden ancestor column", () => {
    const ancestors: Guide[] = [{ show: false, accent: false }];
    const marks = rowGuides(ancestors, 2, true, false);
    // only the connector line + tick, no pass-through
    expect(marks.every((m) => m.x === 32)).toBe(true);
  });
});

describe("indentPad", () => {
  it("indents 16px per level from an 8px base", () => {
    expect(indentPad(0)).toBe(8);
    expect(indentPad(1)).toBe(24);
    expect(indentPad(2)).toBe(40);
  });
});
