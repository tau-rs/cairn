import { describe, it, expect } from "vitest";
import { TableWidget } from "./tableWidget";

describe("TableWidget read-only alignment", () => {
  it("applies per-column text-align from the delimiter row", () => {
    const md = "| A | B | C | D |\n|---|:--|:-:|--:|\n| 1 | 2 | 3 | 4 |";
    const table = new TableWidget(md, 0, () => {}).toDOM();
    const headers = [...table.querySelectorAll<HTMLElement>("thead th")];
    expect(headers.map((h) => h.style.textAlign)).toEqual([
      "", // none → no inline style
      "left",
      "center",
      "right",
    ]);
    const cells = [...table.querySelectorAll<HTMLElement>("tbody td")];
    expect(cells.map((c) => c.style.textAlign)).toEqual([
      "",
      "left",
      "center",
      "right",
    ]);
  });
});
