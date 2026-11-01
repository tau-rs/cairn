import { describe, it, expect } from "vitest";
import { errMsg } from "./errMsg";

describe("errMsg", () => {
  it("formats a ContractError not_found with its `what`", () => {
    expect(errMsg({ type: "not_found", what: "note.md" })).toBe(
      "Not found: note.md",
    );
  });

  it("uses `message` for a non-not_found ContractError", () => {
    expect(errMsg({ type: "internal", message: "engine exploded" })).toBe(
      "engine exploded",
    );
  });

  it("uses Error.message for a thrown Error", () => {
    expect(errMsg(new Error("boom"))).toBe("boom");
  });

  it("stringifies a plain string / number / boolean", () => {
    expect(errMsg("plain")).toBe("plain");
    expect(errMsg(42)).toBe("42");
    expect(errMsg(true)).toBe("true");
  });

  it("stringifies null and undefined (not treated as a ContractError)", () => {
    // Guards the `err && … && "type" in err` shape: null/undefined must NOT
    // enter the tagged-object branch (property access would throw).
    expect(errMsg(null)).toBe("null");
    expect(errMsg(undefined)).toBe("undefined");
  });

  it("stringifies a plain object that has no `type` tag", () => {
    // A bare object lacking `type` is not a ContractError — must fall through
    // to String(err), not be read as one.
    expect(errMsg({ foo: 1 })).toBe("[object Object]");
  });
});
