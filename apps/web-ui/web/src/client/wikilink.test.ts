import { describe, it, expect } from "vitest";
import { extractLinks, stem } from "./wikilink";

describe("extractLinks", () => {
  it("extracts plain and aliased links in order with duplicates", () => {
    expect(
      extractLinks("see [[Alpha]] and [[Beta|the second]] then [[Alpha]]"),
    ).toEqual(["Alpha", "Beta", "Alpha"]);
  });

  it("ignores unclosed and whitespace-only links", () => {
    expect(extractLinks("[[ ]] and [[unclosed")).toEqual([]);
  });

  it("does not treat a bare `]]` (no opening `[[`) as a link", () => {
    // Kills the `if (body[i]==="[" && body[i+1]==="[")` → always-true mutant:
    // without the opening-bracket guard, "word]]" would yield ["word"].
    expect(extractLinks("word]]")).toEqual([]);
  });

  it("requires BOTH opening brackets — a single `[` is not a link start", () => {
    // Kills mutating the second half (`body[i+1] === "["`) to true, which
    // would accept a lone `[` and extract "XY" from "[XY]]".
    expect(extractLinks("[XY]]")).toEqual([]);
  });

  it("extracts back-to-back links with no separator", () => {
    // Kills the `i = close + 2` resume-offset mutant: a wrong offset rescans
    // into the previous `]]` and drops or duplicates the adjacent link.
    expect(extractLinks("[[A]][[B]]")).toEqual(["A", "B"]);
  });
});

describe("stem", () => {
  it("strips directory and .md extension", () => {
    expect(stem("dir/sub/note.md")).toBe("note");
    expect(stem("b.md")).toBe("b");
    expect(stem("noext")).toBe("noext");
  });
});
