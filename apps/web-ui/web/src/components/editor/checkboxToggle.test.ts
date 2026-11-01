import { describe, it, expect } from "vitest";
import { toggleCheckboxChange } from "./checkboxToggle";

describe("toggleCheckboxChange", () => {
  it("turns an unchecked box into a checked one", () => {
    const doc = "- [ ] task";
    const open = doc.indexOf("[");
    expect(toggleCheckboxChange(doc, open)).toEqual({
      from: open + 1,
      to: open + 2,
      insert: "x",
    });
  });
  it("turns a checked box into an unchecked one", () => {
    const doc = "- [x] task";
    const open = doc.indexOf("[");
    expect(toggleCheckboxChange(doc, open)).toEqual({
      from: open + 1,
      to: open + 2,
      insert: " ",
    });
  });
});
