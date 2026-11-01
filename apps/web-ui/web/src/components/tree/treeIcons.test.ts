import { describe, it, expect, beforeEach } from "vitest";
import {
  loadStyles,
  saveStyles,
  remapStyles,
  remapStylesByPrefix,
  type TreeStyleMap,
} from "./treeIcons";

beforeEach(() => localStorage.clear());

describe("treeIcons persistence", () => {
  it("returns {} when nothing stored", () => {
    expect(loadStyles()).toEqual({});
  });

  it("round-trips a saved map", () => {
    const map: TreeStyleMap = {
      "notes/a.md": { icon: { kind: "emoji", value: "📚" } },
      notes: { folderColor: "#46b3e6" },
    };
    saveStyles(map);
    expect(loadStyles()).toEqual(map);
  });

  it("returns {} on malformed JSON", () => {
    localStorage.setItem("cairn.treeIcons", "{not json");
    expect(loadStyles()).toEqual({});
  });

  it("returns {} when stored value is not an object", () => {
    localStorage.setItem("cairn.treeIcons", "[1,2,3]");
    expect(loadStyles()).toEqual({});
  });
});

describe("remapStyles", () => {
  it("returns the same map for no ops", () => {
    const map = { "a.md": { icon: { kind: "emoji" as const, value: "🧠" } } };
    expect(remapStyles([], map)).toEqual(map);
  });

  it("remaps a renamed note key directly", () => {
    const map = { "a.md": { icon: { kind: "emoji" as const, value: "🧠" } } };
    const out = remapStyles([{ from: "a.md", to: "b.md" }], map);
    expect(out).toEqual({ "b.md": { icon: { kind: "emoji", value: "🧠" } } });
  });

  it("remaps a folder key and its descendants when a folder is renamed", () => {
    const map = {
      notes: { folderColor: "#46b3e6" },
      "notes/a.md": {
        icon: { kind: "lucide" as const, name: "star", color: "#fff" },
      },
      "notes/sub": { folderColor: "#e5484d" },
      "other.md": { icon: { kind: "emoji" as const, value: "📌" } },
    };
    const ops = [
      { from: "notes/a.md", to: "docs/a.md" },
      { from: "notes/sub/b.md", to: "docs/sub/b.md" },
    ];
    const out = remapStyles(ops, map);
    expect(out).toEqual({
      docs: { folderColor: "#46b3e6" },
      "docs/a.md": { icon: { kind: "lucide", name: "star", color: "#fff" } },
      "docs/sub": { folderColor: "#e5484d" },
      "other.md": { icon: { kind: "emoji", value: "📌" } },
    });
  });

  it("remaps a moved folder (prefix change at the top level)", () => {
    const map = { "a/b": { folderColor: "#30a46c" } };
    const out = remapStyles([{ from: "a/b/x.md", to: "c/b/x.md" }], map);
    expect(out).toEqual({ "c/b": { folderColor: "#30a46c" } });
  });
});

describe("remapStylesByPrefix", () => {
  it("remaps a note-less (empty) folder's own style key", () => {
    // The bug case: a folder carrying a color but no descendant notes -> no ops.
    const map = { "empty/folder": { folderColor: "#30a46c" } };
    const out = remapStylesByPrefix("empty/folder", "moved/folder", map);
    expect(out).toEqual({ "moved/folder": { folderColor: "#30a46c" } });
  });

  it("remaps the folder key and every descendant (notes + subfolders)", () => {
    const map = {
      notes: { folderColor: "#46b3e6" },
      "notes/sub": { folderColor: "#e5484d" },
      "notes/a.md": {
        icon: { kind: "lucide" as const, name: "star", color: "#fff" },
      },
      "other.md": { icon: { kind: "emoji" as const, value: "📌" } },
    };
    const out = remapStylesByPrefix("notes", "docs", map);
    expect(out).toEqual({
      docs: { folderColor: "#46b3e6" },
      "docs/sub": { folderColor: "#e5484d" },
      "docs/a.md": { icon: { kind: "lucide", name: "star", color: "#fff" } },
      "other.md": { icon: { kind: "emoji", value: "📌" } },
    });
  });

  it("does not remap a sibling whose name is a prefix substring", () => {
    const map = { notesX: { folderColor: "#fff" } };
    expect(remapStylesByPrefix("notes", "docs", map)).toEqual(map);
  });

  it("is a no-op when from === to", () => {
    const map = { notes: { folderColor: "#fff" } };
    expect(remapStylesByPrefix("notes", "notes", map)).toBe(map);
  });
});
