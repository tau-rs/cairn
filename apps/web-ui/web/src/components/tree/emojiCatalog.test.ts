import { describe, it, expect } from "vitest";
import { EMOJI_CATALOG, searchEmoji } from "./emojiCatalog";

describe("emojiCatalog", () => {
  it("has a non-empty catalog with chars + groups", () => {
    expect(EMOJI_CATALOG.length).toBeGreaterThan(20);
    expect(EMOJI_CATALOG.every((e) => e.char && e.name && e.group)).toBe(true);
  });

  it("searches by name and keyword (case-insensitive)", () => {
    expect(searchEmoji("book").some((e) => e.char === "📚")).toBe(true);
    expect(searchEmoji("idea").some((e) => e.char === "💡")).toBe(true);
  });

  it("returns the full catalog for an empty query", () => {
    expect(searchEmoji("").length).toBe(EMOJI_CATALOG.length);
  });
});
