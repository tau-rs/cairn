import { describe, it, expect } from "vitest";
import { lineDiff } from "./lineDiff";

describe("lineDiff", () => {
  it("returns all context lines when both sides are identical", () => {
    const rows = lineDiff("a\nb\nc", "a\nb\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "ctx", text: "b", oldLine: 2, newLine: 2 },
      { type: "ctx", text: "c", oldLine: 3, newLine: 3 },
    ]);
  });

  it("marks a purely added line", () => {
    const rows = lineDiff("a\nc", "a\nb\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "add", text: "b", oldLine: null, newLine: 2 },
      { type: "ctx", text: "c", oldLine: 2, newLine: 3 },
    ]);
  });

  it("marks a purely deleted line", () => {
    const rows = lineDiff("a\nb\nc", "a\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "del", text: "b", oldLine: 2, newLine: null },
      { type: "ctx", text: "c", oldLine: 3, newLine: 2 },
    ]);
  });

  it("emits a deletion then an addition for a changed line", () => {
    const rows = lineDiff("a\nb\nc", "a\nB\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "del", text: "b", oldLine: 2, newLine: null },
      { type: "add", text: "B", oldLine: null, newLine: 2 },
      { type: "ctx", text: "c", oldLine: 3, newLine: 3 },
    ]);
  });

  it("preserves blank lines and whitespace", () => {
    const rows = lineDiff("a\n\n  b", "a\n\n  b");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "ctx", text: "", oldLine: 2, newLine: 2 },
      { type: "ctx", text: "  b", oldLine: 3, newLine: 3 },
    ]);
  });

  it("treats an empty old side as all additions", () => {
    const rows = lineDiff("", "x\ny");
    expect(rows).toEqual([
      { type: "add", text: "x", oldLine: null, newLine: 1 },
      { type: "add", text: "y", oldLine: null, newLine: 2 },
    ]);
  });

  it("treats two empty strings as zero rows", () => {
    expect(lineDiff("", "")).toEqual([]);
  });
});
