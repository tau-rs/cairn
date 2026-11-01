import { describe, it, expect } from "vitest";
import { computeTableEdit } from "./tableOps";

const md = "| A | B |\n| --- | --- |\n| 1 | 2 |";

describe("computeTableEdit", () => {
  it("inserts a row below index 0", () => {
    const out = computeTableEdit(md, { kind: "insertRow", index: 1 });
    expect(out).toBe(
      "| A   | B   |\n| --- | --- |\n| 1   | 2   |\n|     |     |",
    );
  });
  it("removes a column", () => {
    const out = computeTableEdit(md, { kind: "removeColumn", index: 1 });
    expect(out).toBe("| A   |\n| --- |\n| 1   |");
  });
  it("moves a column", () => {
    const out = computeTableEdit(md, { kind: "moveColumn", from: 0, to: 1 });
    expect(out).toBe("| B   | A   |\n| --- | --- |\n| 2   | 1   |");
  });
  it("pastes a TSV block into the body, growing the table", () => {
    const out = computeTableEdit(md, {
      kind: "paste",
      atRow: 0,
      atCol: 0,
      block: [
        ["x", "y"],
        ["z", "w"],
      ],
    });
    expect(out).toBe(
      "| A   | B   |\n| --- | --- |\n| x   | y   |\n| z   | w   |",
    );
  });
});
