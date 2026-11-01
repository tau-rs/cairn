import { describe, it, expect } from "vitest";
import {
  planRenameNote,
  planRenameNotePath,
  planRenameFolder,
  planMoveNote,
  planMoveFolder,
  canDrop,
} from "./treeMoves";

describe("planRenameNote", () => {
  it("renames within the same folder", () => {
    expect(planRenameNote("projects/ideas.md", "plan")).toEqual([
      { from: "projects/ideas.md", to: "projects/plan.md" },
    ]);
  });
  it("strips a typed .md and works at root", () => {
    expect(planRenameNote("a.md", "b.md")).toEqual([
      { from: "a.md", to: "b.md" },
    ]);
  });
  it("no-ops on an unchanged or invalid name", () => {
    expect(planRenameNote("a.md", "a")).toEqual([]);
    expect(planRenameNote("a.md", "")).toEqual([]);
    expect(planRenameNote("a.md", "x/y")).toEqual([]);
  });
});

describe("planRenameNotePath", () => {
  it("renames within the folder when no slash is given", () => {
    expect(planRenameNotePath("dir/a.md", "b")).toEqual([
      { from: "dir/a.md", to: "dir/b.md" },
    ]);
  });
  it("moves the note when a path with slashes is given", () => {
    expect(planRenameNotePath("a.md", "dir/sub/a")).toEqual([
      { from: "a.md", to: "dir/sub/a.md" },
    ]);
  });
  it("strips a trailing .md and a leading slash", () => {
    expect(planRenameNotePath("a.md", "/dir/a.md")).toEqual([
      { from: "a.md", to: "dir/a.md" },
    ]);
  });
  it("returns [] for an empty input or a no-op", () => {
    expect(planRenameNotePath("a.md", "  ")).toEqual([]);
    expect(planRenameNotePath("dir/a.md", "dir/a")).toEqual([]);
  });
  it("returns [] when the final segment is empty (trailing slash)", () => {
    expect(planRenameNotePath("a.md", "dir/")).toEqual([]);
  });
});

describe("planRenameFolder", () => {
  it("bulk-renames every descendant note, preserving the parent + nesting", () => {
    expect(
      planRenameFolder("projects", "work", [
        "projects/ideas.md",
        "projects/sub/b.md",
        "index.md",
      ]),
    ).toEqual([
      { from: "projects/ideas.md", to: "work/ideas.md" },
      { from: "projects/sub/b.md", to: "work/sub/b.md" },
    ]);
  });
  it("preserves the parent of a nested folder", () => {
    expect(planRenameFolder("a/b", "c", ["a/b/x.md"])).toEqual([
      { from: "a/b/x.md", to: "a/c/x.md" },
    ]);
  });
});

describe("planMoveNote", () => {
  it("moves a note into a folder", () => {
    expect(planMoveNote("ideas.md", "archive")).toEqual([
      { from: "ideas.md", to: "archive/ideas.md" },
    ]);
  });
  it("moves a note to root and no-ops when already there", () => {
    expect(planMoveNote("a/x.md", "")).toEqual([
      { from: "a/x.md", to: "x.md" },
    ]);
    expect(planMoveNote("archive/x.md", "archive")).toEqual([]);
  });
});

describe("planMoveFolder", () => {
  it("moves a subtree under the destination (basename preserved)", () => {
    expect(
      planMoveFolder("projects", "archive", [
        "projects/a.md",
        "projects/sub/b.md",
      ]),
    ).toEqual([
      { from: "projects/a.md", to: "archive/projects/a.md" },
      { from: "projects/sub/b.md", to: "archive/projects/sub/b.md" },
    ]);
  });
  it("no-ops into itself or its own descendant", () => {
    expect(planMoveFolder("a", "a", ["a/x.md"])).toEqual([]);
    expect(planMoveFolder("a", "a/b", ["a/x.md"])).toEqual([]);
  });
});

describe("canDrop", () => {
  it("allows a real note/folder move and blocks no-ops/self/descendant", () => {
    expect(canDrop("ideas.md", false, "archive")).toBe(true);
    expect(canDrop("archive/x.md", false, "archive")).toBe(false);
    expect(canDrop("a", true, "b")).toBe(true);
    expect(canDrop("a", true, "a")).toBe(false);
    expect(canDrop("a", true, "a/b")).toBe(false);
    expect(canDrop("a/b", true, "a")).toBe(false);
  });
});
