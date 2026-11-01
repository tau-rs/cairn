import { describe, it, expect } from "vitest";
import { confineToRoot } from "./vaultPath";

describe("confineToRoot", () => {
  it("joins a simple relative path onto the root", () => {
    expect(confineToRoot("/vault", "img/logo.png")).toBe("/vault/img/logo.png");
  });
  it("tolerates a trailing slash on the root", () => {
    expect(confineToRoot("/vault/", "img/logo.png")).toBe(
      "/vault/img/logo.png",
    );
  });
  it("normalizes interior `.` and `..` that stay inside the root", () => {
    expect(confineToRoot("/vault", "a/../img/./logo.png")).toBe(
      "/vault/img/logo.png",
    );
  });
  it("rejects a `..` sequence that escapes the root", () => {
    expect(confineToRoot("/vault", "../../etc/passwd")).toBeNull();
  });
  it("rejects an absolute POSIX path", () => {
    expect(confineToRoot("/vault", "/etc/passwd")).toBeNull();
  });
  it("rejects a Windows drive-absolute path", () => {
    expect(confineToRoot("/vault", "C:\\Windows\\system32")).toBeNull();
  });
  it("rejects a UNC path", () => {
    expect(confineToRoot("/vault", "\\\\server\\share")).toBeNull();
  });
  it("rejects a path that resolves to the root itself", () => {
    expect(confineToRoot("/vault", "a/..")).toBeNull();
  });
  it("treats backslashes as separators when confining", () => {
    expect(confineToRoot("/vault", "img\\..\\..\\secret")).toBeNull();
  });
  it("drops a trailing separator on the relative path (empty last segment)", () => {
    // Exercises the `seg === ""` skip on a trailing-slash-produced empty seg.
    expect(confineToRoot("/vault", "img/")).toBe("/vault/img");
  });
  it("skips a bare interior `.` segment", () => {
    // Exercises the `seg === "."` skip in isolation (no `..` interplay).
    expect(confineToRoot("/vault", "a/./b")).toBe("/vault/a/b");
  });
  it("collapses repeated separators in the relative path", () => {
    expect(confineToRoot("/vault", "a//b")).toBe("/vault/a/b");
  });
  it("strips ALL trailing separators from the root, not just one", () => {
    // Kills the `/[/\\]+$/ → /[/\\]$/` regex mutant on the root.
    expect(confineToRoot("/vault//", "x")).toBe("/vault/x");
  });
  it("rejects a path containing a NUL or control character", () => {
    expect(confineToRoot("/vault", "img/\0/passwd")).toBeNull();
    expect(confineToRoot("/vault", "img/\x07x.png")).toBeNull();
  });
});
