import { describe, it, expect } from "vitest";
import { wikilinkCompletionState, wikilinkInsert } from "./wikilinkComplete";

const stems = ["ideas", "inbox", "journal"];

describe("wikilinkCompletionState — fire rule", () => {
  it("fires on an open [[ with a partial, from at the partial start", () => {
    const r = wikilinkCompletionState("see [[ide", stems);
    expect(r).not.toBeNull();
    expect(r!.from).toBe("see [[".length);
    expect(r!.stems).toEqual(["ideas"]);
  });

  it("suggests all stems for an empty partial (just typed [[)", () => {
    const r = wikilinkCompletionState("see [[", stems);
    expect(r).not.toBeNull();
    expect(r!.from).toBe("see [[".length);
    expect(r!.stems).toEqual(["ideas", "inbox", "journal"]);
  });

  it("does not fire when there is no [[ before the cursor", () => {
    expect(wikilinkCompletionState("just text", stems)).toBeNull();
  });

  it("does not fire in the alias part (after a |)", () => {
    expect(wikilinkCompletionState("see [[ideas|al", stems)).toBeNull();
  });

  it("does not fire once the link is closed (]] before the cursor)", () => {
    expect(wikilinkCompletionState("see [[ideas]] ", stems)).toBeNull();
  });

  it("filters case-insensitively by substring", () => {
    expect(wikilinkCompletionState("[[IN", stems)!.stems).toEqual(["inbox"]);
    expect(wikilinkCompletionState("[[na", stems)!.stems).toEqual(["journal"]);
  });

  it("dedupes stems that repeat across folders", () => {
    const r = wikilinkCompletionState("[[", ["ideas", "ideas", "inbox"]);
    expect(r!.stems).toEqual(["ideas", "inbox"]);
  });
});

describe("wikilinkInsert — closing bracket handling", () => {
  it("appends ]] when the following text is not already ]]", () => {
    expect(wikilinkInsert("ideas", "")).toBe("ideas]]");
    expect(wikilinkInsert("ideas", " rest")).toBe("ideas]]");
  });

  it("reuses an existing ]] (no doubling)", () => {
    expect(wikilinkInsert("ideas", "]]")).toBe("ideas");
    expect(wikilinkInsert("ideas", "]] rest")).toBe("ideas");
  });
});
