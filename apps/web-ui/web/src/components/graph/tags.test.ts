import { describe, it, expect } from "vitest";
import { extractTags } from "./tags";

describe("extractTags", () => {
  it("reads a frontmatter inline list", () => {
    expect(extractTags("---\ntags: [Alpha, beta]\n---\nbody")).toEqual([
      "alpha",
      "beta",
    ]);
  });
  it("reads a frontmatter comma list", () => {
    expect(extractTags("---\ntags: a, b\n---\n")).toEqual(["a", "b"]);
  });
  it("reads a frontmatter block list", () => {
    expect(
      extractTags("---\ntags:\n  - one\n  - two\ntitle: x\n---\nbody"),
    ).toEqual(["one", "two"]);
  });
  it("ignores inline #tags in the body (engine reads frontmatter only)", () => {
    expect(extractTags("see #Idea and #graph-view here")).toEqual([]);
  });
  it("reads only frontmatter tags; returns [] when none", () => {
    expect(extractTags("---\ntags: [x]\n---\nbody #y")).toEqual(["x"]);
    expect(extractTags("plain note, no tags")).toEqual([]);
  });
});
