import { describe, it, expect } from "vitest";
import { splitSnippet } from "./searchHighlight";

describe("splitSnippet", () => {
  it("no highlights → one plain segment", () => {
    expect(splitSnippet("alpha note", [])).toEqual([
      { text: "alpha note", match: false },
    ]);
  });
  it("empty snippet → no segments", () => {
    expect(splitSnippet("", [])).toEqual([]);
  });
  it("a single leading range splits into match + plain", () => {
    expect(splitSnippet("alpha note", [[0, 5]])).toEqual([
      { text: "alpha", match: true },
      { text: " note", match: false },
    ]);
  });
  it("a mid range yields plain/match/plain", () => {
    expect(splitSnippet("abcdef", [[2, 4]])).toEqual([
      { text: "ab", match: false },
      { text: "cd", match: true },
      { text: "ef", match: false },
    ]);
  });
  it("multiple ranges, in order", () => {
    expect(
      splitSnippet("a b a", [
        [0, 1],
        [4, 5],
      ]),
    ).toEqual([
      { text: "a", match: true },
      { text: " b ", match: false },
      { text: "a", match: true },
    ]);
  });
  it("merges overlapping/adjacent ranges", () => {
    expect(
      splitSnippet("abcdef", [
        [0, 3],
        [2, 5],
      ]),
    ).toEqual([
      { text: "abcde", match: true },
      { text: "f", match: false },
    ]);
  });
  it("clamps out-of-range ends", () => {
    expect(splitSnippet("abc", [[1, 99]])).toEqual([
      { text: "a", match: false },
      { text: "bc", match: true },
    ]);
  });
});
