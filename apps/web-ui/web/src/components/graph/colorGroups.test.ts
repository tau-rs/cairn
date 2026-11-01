import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  type ColorGroup,
  loadColorGroups,
  saveColorGroups,
  matchGroupColor,
} from "./colorGroups";

beforeEach(() => localStorage.clear());

describe("loadColorGroups / saveColorGroups", () => {
  it("returns [] when empty or corrupt", () => {
    expect(loadColorGroups()).toEqual([]);
    localStorage.setItem("cairn.graph.groups", "{not json");
    expect(loadColorGroups()).toEqual([]);
  });
  it("drops malformed entries, keeps valid ones", () => {
    localStorage.setItem(
      "cairn.graph.groups",
      JSON.stringify([
        { kind: "path", query: "projects", color: "#6366f1" },
        { kind: "bogus", query: "x", color: "#fff" },
        { query: "no-kind" },
      ]),
    );
    expect(loadColorGroups()).toEqual([
      { kind: "path", query: "projects", color: "#6366f1" },
    ]);
  });
  it("round-trips", () => {
    const g: ColorGroup[] = [{ kind: "tag", query: "idea", color: "#f59e0b" }];
    saveColorGroups(g);
    expect(loadColorGroups()).toEqual(g);
  });
  it("swallows storage errors", () => {
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveColorGroups([])).not.toThrow();
    spy.mockRestore();
  });
});

describe("matchGroupColor", () => {
  const groups: ColorGroup[] = [
    { kind: "path", query: "Projects", color: "#6366f1" },
    { kind: "tag", query: "idea", color: "#f59e0b" },
  ];
  it("matches a path query case-insensitively (substring)", () => {
    expect(matchGroupColor("projects/app.md", [], groups)).toBe("#6366f1");
  });
  it("matches a tag query (exact, case-insensitive)", () => {
    expect(matchGroupColor("notes/x.md", ["idea"], groups)).toBe("#f59e0b");
  });
  it("returns the first matching group's color", () => {
    // path 'Projects' matches first even though the tag would also match
    expect(matchGroupColor("projects/x.md", ["idea"], groups)).toBe("#6366f1");
  });
  it("returns null when nothing matches and ignores empty queries", () => {
    expect(matchGroupColor("notes/x.md", ["other"], groups)).toBeNull();
    expect(
      matchGroupColor(
        "anything",
        [],
        [{ kind: "path", query: "", color: "#fff" }],
      ),
    ).toBeNull();
  });
});
