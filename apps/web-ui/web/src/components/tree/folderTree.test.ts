import { describe, it, expect } from "vitest";
import { buildTree, ancestorFolders } from "./folderTree";

describe("buildTree", () => {
  it("groups root notes and nested folders, folders-first then alpha", () => {
    const tree = buildTree([
      "index.md",
      "ideas.md",
      "notes/todo.md",
      "notes/sub/deep.md",
      "projects/cairn.md",
    ]);
    expect(tree.map((n) => [n.kind, n.name])).toEqual([
      ["folder", "notes"],
      ["folder", "projects"],
      ["note", "ideas"],
      ["note", "index"],
    ]);
  });

  it("nests children with full folder paths and stem leaf names", () => {
    const tree = buildTree(["notes/sub/deep.md", "notes/todo.md"]);
    const notes = tree[0];
    expect(notes).toMatchObject({
      kind: "folder",
      name: "notes",
      path: "notes",
    });
    if (notes.kind !== "folder") throw new Error("expected folder");
    expect(notes.children.map((n) => [n.kind, n.name, n.path])).toEqual([
      ["folder", "sub", "notes/sub"],
      ["note", "todo", "notes/todo.md"],
    ]);
    const sub = notes.children[0];
    if (sub.kind !== "folder") throw new Error("expected folder");
    expect(sub.children).toEqual([
      { kind: "note", name: "deep", path: "notes/sub/deep.md" },
    ]);
  });
});

describe("ancestorFolders", () => {
  it("returns enclosing folders outermost-first", () => {
    expect(ancestorFolders("a/b/c.md")).toEqual(["a", "a/b"]);
  });
  it("returns empty for a root note", () => {
    expect(ancestorFolders("index.md")).toEqual([]);
  });
});
