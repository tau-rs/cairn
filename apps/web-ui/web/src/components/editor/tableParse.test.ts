import { describe, it, expect } from "vitest";
import {
  parseTable,
  serializeTable,
  insertRow,
  addRow,
  removeRow,
  moveRow,
  insertColumn,
  addColumn,
  removeColumn,
  moveColumn,
  parseTSV,
  pasteBlock,
  type TableModel,
} from "./tableParse";

describe("parseTable", () => {
  it("parses header, body, and per-column alignment", () => {
    const md = "| A | B | C | D |\n|---|:--|:-:|--:|\n| 1 | 2 | 3 | 4 |";
    expect(parseTable(md)).toEqual({
      header: ["A", "B", "C", "D"],
      rows: [["1", "2", "3", "4"]],
      align: ["none", "left", "center", "right"],
    });
  });
  it("tolerates missing outer pipes and defaults alignment to none", () => {
    const md = "A | B\n--- | ---\n1 | 2";
    expect(parseTable(md)).toEqual({
      header: ["A", "B"],
      rows: [["1", "2"]],
      align: ["none", "none"],
    });
  });
  it("pads a short alignment row to header length", () => {
    const md = "| A | B |\n| :-: |\n| 1 | 2 |";
    expect(parseTable(md).align).toEqual(["center", "none"]);
  });
  it("returns empty model for empty input", () => {
    expect(parseTable("")).toEqual({ header: [], rows: [], align: [] });
  });
});

describe("serializeTable", () => {
  it("pads columns and writes alignment markers", () => {
    const md = serializeTable({
      header: ["Name", "Qty"],
      rows: [["Apple", "3"]],
      align: ["left", "right"],
    });
    expect(md).toBe("| Name  | Qty |\n| :---- | --: |\n| Apple | 3   |");
  });
  it("uses --- for unaligned columns with a minimum width of 3", () => {
    expect(
      serializeTable({
        header: ["A", "B"],
        rows: [["1", "2"]],
        align: ["none", "none"],
      }),
    ).toBe("| A   | B   |\n| --- | --- |\n| 1   | 2   |");
  });
  it("serializes a header-only table (no body rows)", () => {
    expect(
      serializeTable({ header: ["A", "B"], rows: [], align: ["none", "none"] }),
    ).toBe("| A   | B   |\n| --- | --- |");
  });
  it("round-trips alignment through parseTable", () => {
    const model: TableModel = {
      header: ["A", "B", "C", "D"],
      rows: [["1", "2", "3", "4"]],
      align: ["none", "left", "center", "right"],
    };
    expect(parseTable(serializeTable(model))).toEqual(model);
  });
  it("escapes pipes in cell text and parse unescapes them", () => {
    const model: TableModel = {
      header: ["A"],
      rows: [["x|y"]],
      align: ["none"],
    };
    const md = serializeTable(model);
    expect(md).toContain("x\\|y");
    expect(parseTable(md)).toEqual(model);
  });
  it("round-trips a cell containing a backslash", () => {
    const model: TableModel = {
      header: ["A\\B"],
      rows: [["x\\"]],
      align: ["none"],
    };
    expect(parseTable(serializeTable(model))).toEqual(model);
  });
});

describe("table model ops", () => {
  const m: TableModel = {
    header: ["A", "B"],
    rows: [
      ["1", "2"],
      ["3", "4"],
    ],
    align: ["left", "right"],
  };

  it("insertRow inserts a blank row at the index", () => {
    expect(insertRow(m, 1).rows).toEqual([
      ["1", "2"],
      ["", ""],
      ["3", "4"],
    ]);
  });
  it("addRow appends a blank row", () => {
    expect(addRow(m).rows).toEqual([
      ["1", "2"],
      ["3", "4"],
      ["", ""],
    ]);
  });
  it("removeRow deletes a row but keeps at least one", () => {
    const one: TableModel = { header: ["A"], rows: [["1"]], align: ["none"] };
    expect(removeRow(one, 0)).toEqual(one); // no-op at minimum
    expect(removeRow(m, 0).rows).toEqual([["3", "4"]]);
  });
  it("moveRow reorders rows", () => {
    expect(moveRow(m, 0, 1).rows).toEqual([
      ["3", "4"],
      ["1", "2"],
    ]);
    expect(moveRow(m, 0, 0)).toEqual(m); // same position → no-op
  });
  it("insertColumn inserts a blank column and align entry", () => {
    const r = insertColumn(m, 1);
    expect(r.header).toEqual(["A", "", "B"]);
    expect(r.rows).toEqual([
      ["1", "", "2"],
      ["3", "", "4"],
    ]);
    expect(r.align).toEqual(["left", "none", "right"]);
  });
  it("addColumn appends a blank column", () => {
    expect(addColumn(m).header).toEqual(["A", "B", ""]);
  });
  it("removeColumn deletes a column (and its align) but keeps at least one", () => {
    const r = removeColumn(m, 1);
    expect(r.header).toEqual(["A"]);
    expect(r.align).toEqual(["left"]);
    const one: TableModel = { header: ["A"], rows: [["1"]], align: ["none"] };
    expect(removeColumn(one, 0)).toEqual(one);
  });
  it("moveColumn reorders header, cells, and align together", () => {
    const r = moveColumn(m, 0, 1);
    expect(r.header).toEqual(["B", "A"]);
    expect(r.align).toEqual(["right", "left"]);
    expect(r.rows).toEqual([
      ["2", "1"],
      ["4", "3"],
    ]);
  });
  it("moveRow is a no-op when out of range", () => {
    expect(moveRow(m, 0, 5)).toEqual(m);
    expect(moveRow(m, -1, 0)).toEqual(m);
  });
  it("moveColumn is a no-op when out of range", () => {
    expect(moveColumn(m, 0, 5)).toEqual(m);
    expect(moveColumn(m, -1, 0)).toEqual(m);
  });
});

describe("parseTSV", () => {
  it("splits tabs into columns and newlines into rows", () => {
    expect(parseTSV("a\tb\nc\td")).toEqual([
      ["a", "b"],
      ["c", "d"],
    ]);
  });
  it("normalizes CRLF and drops a single trailing newline", () => {
    expect(parseTSV("a\tb\r\n")).toEqual([["a", "b"]]);
  });
  it("drops multiple trailing blank lines without phantom rows", () => {
    expect(parseTSV("a\tb\n\n")).toEqual([["a", "b"]]);
  });
});

describe("pasteBlock", () => {
  const m: TableModel = {
    header: ["A", "B"],
    rows: [
      ["1", "2"],
      ["3", "4"],
    ],
    align: ["none", "none"],
  };
  it("fills cells from the anchor without growing when it fits", () => {
    const r = pasteBlock(m, 0, 0, [["x", "y"]]);
    expect(r.rows).toEqual([
      ["x", "y"],
      ["3", "4"],
    ]);
  });
  it("auto-grows rows and columns to fit an oversized block", () => {
    const r = pasteBlock(m, 1, 1, [
      ["x", "y"],
      ["z", "w"],
    ]);
    expect(r.header).toEqual(["A", "B", ""]);
    expect(r.rows).toEqual([
      ["1", "2", ""],
      ["3", "x", "y"],
      ["", "z", "w"],
    ]);
    expect(r.align).toEqual(["none", "none", "none"]);
  });
});
