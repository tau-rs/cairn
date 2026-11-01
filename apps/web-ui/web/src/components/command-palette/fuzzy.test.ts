import { describe, it, expect } from "vitest";
import { fuzzyScore, filterItems } from "./fuzzy";

describe("fuzzyScore", () => {
  it("matches a subsequence (case-insensitive)", () => {
    expect(fuzzyScore("comm", "Commit changes")).not.toBeNull();
    expect(fuzzyScore("ide", "ideas")).not.toBeNull();
  });
  it("returns null when not a subsequence", () => {
    expect(fuzzyScore("xyz", "ideas")).toBeNull();
    expect(fuzzyScore("idx", "ideas")).toBeNull();
  });
  it("ranks a contiguous prefix match above a scattered one", () => {
    const contiguous = fuzzyScore("co", "Commit changes")!;
    const scattered = fuzzyScore("cc", "Commit changes")!; // c…c
    expect(contiguous).toBeGreaterThan(scattered);
  });
  it("an empty query matches everything with a neutral score", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });
});

describe("filterItems", () => {
  const items = ["Commit changes", "New note", "Open Settings"];
  it("drops non-matches and sorts by score", () => {
    expect(filterItems(items, "note", (s) => s)).toEqual(["New note"]);
  });
  it("returns all items (original order) for an empty query", () => {
    expect(filterItems(items, "", (s) => s)).toEqual(items);
  });
});
