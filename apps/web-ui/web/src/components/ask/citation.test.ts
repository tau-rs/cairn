import { describe, it, expect } from "vitest";
import { resolveStem } from "./citation";

describe("resolveStem", () => {
  it("finds the note path whose stem matches", () => {
    expect(resolveStem(["a/store.md", "timer.md"], "store")).toBe("a/store.md");
  });
  it("accepts a target with an alias or path form", () => {
    expect(resolveStem(["notes/timer.md"], "notes/timer")).toBe(
      "notes/timer.md",
    );
  });
  it("returns null when nothing matches", () => {
    expect(resolveStem(["a.md"], "missing")).toBeNull();
  });
});
