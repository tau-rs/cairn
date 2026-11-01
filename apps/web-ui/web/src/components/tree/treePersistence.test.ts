import { describe, it, expect, beforeEach } from "vitest";
import { loadCollapsed, saveCollapsed } from "./treePersistence";

beforeEach(() => localStorage.clear());

describe("treePersistence", () => {
  it("round-trips the collapsed folder set", () => {
    saveCollapsed(new Set(["notes", "notes/sub"]));
    expect([...loadCollapsed()].sort()).toEqual(["notes", "notes/sub"]);
  });
  it("returns an empty set when nothing is stored", () => {
    expect(loadCollapsed().size).toBe(0);
  });
  it("returns an empty set on malformed storage", () => {
    localStorage.setItem("cairn.folderTree", "{not json");
    expect(loadCollapsed().size).toBe(0);
  });
});
