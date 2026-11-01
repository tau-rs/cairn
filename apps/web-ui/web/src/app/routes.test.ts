import { describe, it, expect } from "vitest";
import {
  noteUrl,
  notePathFromLocation,
  tagUrl,
  tagFromLocation,
  isGraph,
  toggleViewTarget,
} from "./routes";

const loc = (pathname: string) => ({ pathname });

describe("noteUrl / notePathFromLocation", () => {
  it("round-trips simple, nested, and special-char paths", () => {
    for (const p of [
      "ideas.md",
      "projects/demo.md",
      "a b/c#d%e.md",
      "notes/é→ñ.md",
    ]) {
      expect(notePathFromLocation(loc(noteUrl(p)))).toBe(p);
    }
  });

  it("encodes each segment but keeps the slash separators", () => {
    expect(noteUrl("a b/c.md")).toBe("/note/a%20b/c.md");
  });

  it("returns null when the location is not a note route", () => {
    expect(notePathFromLocation(loc("/graph"))).toBeNull();
    expect(notePathFromLocation(loc("/"))).toBeNull();
    expect(notePathFromLocation(loc("/tags/x"))).toBeNull();
  });
});

describe("tagUrl / tagFromLocation", () => {
  it("round-trips and encodes", () => {
    expect(tagUrl("a/b c")).toBe("/tags/a%2Fb%20c");
    expect(tagFromLocation(loc("/tags/a%2Fb%20c"))).toBe("a/b c");
  });
  it("returns null off a tag route", () => {
    expect(tagFromLocation(loc("/note/x.md"))).toBeNull();
  });
});

describe("isGraph", () => {
  it("is true only for the graph route", () => {
    expect(isGraph(loc("/graph"))).toBe(true);
    expect(isGraph(loc("/"))).toBe(false);
    expect(isGraph(loc("/note/x.md"))).toBe(false);
  });
});

describe("toggleViewTarget", () => {
  it("from the graph, targets the active note", () => {
    expect(toggleViewTarget({ pathname: "/graph" }, "a.md")).toBe("/note/a.md");
  });
  it("from the graph with no active note, targets root", () => {
    expect(toggleViewTarget({ pathname: "/graph" }, null)).toBe("/");
  });
  it("from a note, targets the graph", () => {
    expect(toggleViewTarget({ pathname: "/note/a.md" }, "a.md")).toBe("/graph");
  });
});
