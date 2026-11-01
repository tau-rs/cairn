# Table Editor Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the append-only, hover-only Markdown table editor with grip-driven insert/move/delete (rows & columns), spreadsheet (TSV) paste, lossless alignment round-trip, prettified output, a right-click context menu (desktop) / bottom sheet (mobile), and a polished dark refresh.

**Architecture:** A pure, fully-unit-tested core in `tableParse.ts` (alignment-aware `TableModel` + positional ops + TSV paste + prettify) feeds a pure `computeTableEdit(md, op)` transform; a thin `applyTableOp(view, …)` dispatches one CodeMirror replace per op (one undo step) and marks a cell to refocus after the widget re-mounts. The imperative `EditableTableWidget` renders cells + grips and drives every op through that one spine, presenting the shared action list as a popover (fine pointer) or bottom sheet (coarse pointer).

**Tech Stack:** TypeScript, CodeMirror 6 (`@codemirror/view`/`state`), Vitest + jsdom. No new dependencies (Radix popover/dialog already present but the menus are built imperatively to match the other live-preview widgets).

---

## Design reference

Spec: `docs/superpowers/specs/2026-06-13-table-editor-refactor-design.md`. Read it before starting.

## Conventions

- All test commands run from the `web/` directory: `pnpm exec vitest run <path>`.
- Full gate before claiming done (from `web/`): `pnpm exec vitest run && pnpm typecheck && pnpm lint && pnpm exec prettier --check .` (the `prettier --check` step is easy to forget and lint won't catch it).
- Conventional commits, imperative mood.

## File Structure

- **Modify** `web/src/components/editor/tableParse.ts` — add `Align`/alignment to `TableModel`; alignment-aware parse/serialize with prettify; positional ops (`insertRow`/`insertColumn`/`moveRow`/`moveColumn`); `parseTSV`/`pasteBlock`. (Pure, no DOM.)
- **Modify** `web/src/components/editor/tableParse.test.ts` — update existing cases to the new shape; add new cases.
- **Create** `web/src/components/editor/tableOps.ts` — `TableOp` union, pure `computeTableEdit(md, op)`, and `applyTableOp(view, from, to, op, focus)` (the only CodeMirror-touching function).
- **Create** `web/src/components/editor/tableOps.test.ts` — tests for `computeTableEdit` (pure).
- **Create** `web/src/components/editor/tableFocus.ts` — `StateEffect`/`StateField` carrying "after re-mount, focus cell (r,c) of the table at pos X".
- **Create** `web/src/components/editor/widgets/tableMenu.ts` — `Action` type, `buildActions(target)`, and `openTableMenu(anchor, actions)` (popover on fine pointer, bottom sheet on coarse).
- **Modify** `web/src/components/editor/widgets/editableTableWidget.ts` — rewrite: cells + grips + context menu + drag-reorder + paste; routes every structural op through `applyTableOp`.
- **Modify** `web/src/components/editor/widgets/editableTableWidget.test.ts` — keep cell-key tests; add grip/menu/paste tests.
- **Modify** `web/src/components/editor/widgets/tableWidget.ts` — render alignment; minor polish (read-only).
- **Modify** `web/src/components/editor/livePreview.ts` — pass the focus field into the editable widget; include `tableFocus` extension.
- **Modify** `web/src/components/editor/livePreview.css` — refreshed chrome, grips, menu, sheet, drag indicator, larger `+`.

---

## Phase 1 — Pure core (`tableParse.ts`)

### Task 1: Alignment-aware model + parse

**Files:**
- Modify: `web/src/components/editor/tableParse.ts`
- Test: `web/src/components/editor/tableParse.test.ts`

- [ ] **Step 1: Write failing tests for alignment parsing**

Add to `tableParse.test.ts` (replace the existing `parseTable` describe block):

```ts
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
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: FAIL — `align` missing from results.

- [ ] **Step 3: Implement the alignment-aware type and parse**

Replace the top of `tableParse.ts` (the `TableModel` interface and `parseTable`) with:

```ts
export type Align = "none" | "left" | "center" | "right";

export interface TableModel {
  header: string[];
  rows: string[][];
  align: Align[];
}

// Split a table row on UNescaped pipes, drop outer pipes, unescape \| → | then \\ → \.
const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, "|").replace(/\\\\/g, "\\"));

/** Read one delimiter cell (e.g. `:--`, `:-:`, `--:`, `---`) into an Align. */
function parseAlign(cell: string): Align {
  const t = cell.trim();
  const left = t.startsWith(":");
  const right = t.endsWith(":");
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return "none";
}

/** Parse a GFM pipe table's source into header + body rows + per-column alignment
 *  (read from line 2, the delimiter row). */
export function parseTable(md: string): TableModel {
  const lines = md
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return { header: [], rows: [], align: [] };
  const header = cells(lines[0]);
  const rawAlign = lines.length > 1 ? cells(lines[1]).map(parseAlign) : [];
  const align: Align[] = header.map((_, i) => rawAlign[i] ?? "none");
  const rows = lines.slice(2).map(cells);
  return { header, rows, align };
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: the `parseTable` block passes. (The `serializeTable`/ops blocks will FAIL to compile/pass until Task 2 — that's fine; they're updated next. If the runner stops on type errors, proceed to Task 2 before re-running the whole file.)

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts
git commit -m "feat(editor): parse per-column table alignment"
```

---

### Task 2: Alignment-aware serialize with prettify

**Files:**
- Modify: `web/src/components/editor/tableParse.ts`
- Test: `web/src/components/editor/tableParse.test.ts`

- [ ] **Step 1: Write failing tests** — replace the existing `serializeTable` describe block:

```ts
describe("serializeTable", () => {
  it("pads columns and writes alignment markers", () => {
    const md = serializeTable({
      header: ["Name", "Qty"],
      rows: [["Apple", "3"]],
      align: ["left", "right"],
    });
    expect(md).toBe(
      "| Name  | Qty |\n| :---- | --: |\n| Apple | 3   |",
    );
  });
  it("uses --- for unaligned columns with a minimum width of 3", () => {
    expect(
      serializeTable({ header: ["A", "B"], rows: [["1", "2"]], align: ["none", "none"] }),
    ).toBe("| A   | B   |\n| --- | --- |\n| 1   | 2   |");
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
    const model: TableModel = { header: ["A"], rows: [["x|y"]], align: ["none"] };
    const md = serializeTable(model);
    expect(md).toContain("x\\|y");
    expect(parseTable(md)).toEqual(model);
  });
  it("round-trips a cell containing a backslash", () => {
    const model: TableModel = { header: ["A\\B"], rows: [["x\\"]], align: ["none"] };
    expect(parseTable(serializeTable(model))).toEqual(model);
  });
});
```

Add `TableModel` to the import at the top of the test file:

```ts
import {
  parseTable,
  serializeTable,
  type TableModel,
} from "./tableParse";
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: FAIL — old serialize emits unpadded `| A | B |` and ignores alignment.

- [ ] **Step 3: Implement** — replace the `escapeCell`/`fmtRow`/`serializeTable` section of `tableParse.ts`:

```ts
const escapeCell = (s: string): string =>
  s.trim().replace(/\\/g, "\\\\").replace(/\|/g, "\\|");

/** A padded delimiter cell of width `w` for the given alignment. */
function marker(a: Align, w: number): string {
  if (a === "left") return ":" + "-".repeat(w - 1);
  if (a === "right") return "-".repeat(w - 1) + ":";
  if (a === "center") return ":" + "-".repeat(Math.max(1, w - 2)) + ":";
  return "-".repeat(w);
}

/** Serialize a model to prettified GFM: columns padded to equal width, alignment
 *  markers preserved. Minimum column width is 3 so every marker form fits. */
export function serializeTable(m: TableModel): string {
  const cols = m.header.length;
  const escHeader = m.header.map(escapeCell);
  const escRows = m.rows.map((r) =>
    Array.from({ length: cols }, (_, c) => escapeCell(r[c] ?? "")),
  );
  const widths = Array.from({ length: cols }, (_, c) =>
    Math.max(3, escHeader[c].length, ...escRows.map((r) => r[c].length)),
  );
  const fmtRow = (cs: string[]): string =>
    `| ${cs.map((s, c) => s.padEnd(widths[c])).join(" | ")} |`;
  const delim = `| ${widths
    .map((w, c) => marker(m.align[c] ?? "none", w))
    .join(" | ")} |`;
  return [fmtRow(escHeader), delim, ...escRows.map(fmtRow)].join("\n");
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: `parseTable` + `serializeTable` blocks PASS. (The `ops` block still references the old `addRow` etc. shape — updated in Task 3.)

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts
git commit -m "feat(editor): prettify tables and preserve alignment on serialize"
```

---

### Task 3: Positional row/column ops

**Files:**
- Modify: `web/src/components/editor/tableParse.ts`
- Test: `web/src/components/editor/tableParse.test.ts`

- [ ] **Step 1: Write failing tests** — replace the existing `table model ops` describe block:

```ts
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
});
```

Extend the test import:

```ts
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
  type TableModel,
} from "./tableParse";
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: FAIL — `insertRow`/`moveRow`/`insertColumn`/`moveColumn` undefined.

- [ ] **Step 3: Implement** — replace the entire ops section (`addRow` through `removeColumn`) at the bottom of `tableParse.ts`:

```ts
const clamp = (n: number, lo: number, hi: number): number =>
  Math.max(lo, Math.min(hi, n));
const blankRow = (n: number): string[] => Array.from({ length: n }, () => "");
const splice = <T>(arr: T[], from: number, to: number): T[] => {
  const a = [...arr];
  const [x] = a.splice(from, 1);
  a.splice(to, 0, x);
  return a;
};

/** Insert a blank row at `index` (clamped to [0, rows.length]). */
export function insertRow(m: TableModel, index: number): TableModel {
  const i = clamp(index, 0, m.rows.length);
  return {
    header: [...m.header],
    rows: [...m.rows.slice(0, i), blankRow(m.header.length), ...m.rows.slice(i)],
    align: [...m.align],
  };
}

/** Append a blank row. */
export function addRow(m: TableModel): TableModel {
  return insertRow(m, m.rows.length);
}

/** Remove a body row; keeps at least one body row. */
export function removeRow(m: TableModel, index: number): TableModel {
  if (m.rows.length <= 1) return m;
  return {
    header: [...m.header],
    rows: m.rows.filter((_, i) => i !== index),
    align: [...m.align],
  };
}

/** Move a body row from one index to another. No-op if out of range or unchanged. */
export function moveRow(m: TableModel, from: number, to: number): TableModel {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= m.rows.length ||
    to >= m.rows.length
  )
    return m;
  return { header: [...m.header], rows: splice(m.rows, from, to), align: [...m.align] };
}

const insertAt = <T>(arr: T[], i: number, v: T): T[] => [
  ...arr.slice(0, i),
  v,
  ...arr.slice(i),
];

/** Insert a blank column at `index` (clamped), with the given alignment. */
export function insertColumn(
  m: TableModel,
  index: number,
  align: Align = "none",
): TableModel {
  const i = clamp(index, 0, m.header.length);
  return {
    header: insertAt(m.header, i, ""),
    rows: m.rows.map((r) => insertAt(r, i, "")),
    align: insertAt(m.align, i, align),
  };
}

/** Append a blank column. */
export function addColumn(m: TableModel): TableModel {
  return insertColumn(m, m.header.length);
}

/** Remove a column (and its alignment); keeps at least one column. */
export function removeColumn(m: TableModel, index: number): TableModel {
  if (m.header.length <= 1) return m;
  const del = <T>(arr: T[]): T[] => arr.filter((_, i) => i !== index);
  return { header: del(m.header), rows: m.rows.map(del), align: del(m.align) };
}

/** Move a column (header, every cell, and align in lockstep). No-op if unchanged. */
export function moveColumn(m: TableModel, from: number, to: number): TableModel {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= m.header.length ||
    to >= m.header.length
  )
    return m;
  return {
    header: splice(m.header, from, to),
    rows: m.rows.map((r) => splice(r, from, to)),
    align: splice(m.align, from, to),
  };
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: ALL blocks PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts
git commit -m "feat(editor): positional insert/move/remove table ops with alignment"
```

---

### Task 4: TSV paste (`parseTSV` + `pasteBlock`)

**Files:**
- Modify: `web/src/components/editor/tableParse.ts`
- Test: `web/src/components/editor/tableParse.test.ts`

- [ ] **Step 1: Write failing tests** — append to `tableParse.test.ts`:

```ts
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
```

Extend the import with `parseTSV, pasteBlock`.

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: FAIL — `parseTSV`/`pasteBlock` undefined.

- [ ] **Step 3: Implement** — append to `tableParse.ts`:

```ts
/** Parse a spreadsheet clipboard payload (TSV): tab = column, newline = row.
 *  Normalizes CRLF and ignores a single trailing newline. */
export function parseTSV(text: string): string[][] {
  const norm = text.replace(/\r\n?/g, "\n").replace(/\n$/, "");
  if (norm === "") return [];
  return norm.split("\n").map((line) => line.split("\t"));
}

/** Paste a 2-D block of cells into the body, anchored at (atRow, atCol) in body
 *  coordinates. Auto-grows rows and columns so the whole block fits. */
export function pasteBlock(
  m: TableModel,
  atRow: number,
  atCol: number,
  block: string[][],
): TableModel {
  if (block.length === 0) return m;
  const blockCols = Math.max(...block.map((r) => r.length));
  let model = m;
  while (model.header.length < atCol + blockCols) model = addColumn(model);
  while (model.rows.length < atRow + block.length) model = addRow(model);
  const rows = model.rows.map((r) => [...r]);
  block.forEach((br, ri) =>
    br.forEach((val, ci) => {
      const R = atRow + ri;
      const C = atCol + ci;
      if (R >= 0 && R < rows.length && C >= 0 && C < rows[R].length)
        rows[R][C] = val;
    }),
  );
  return { header: [...model.header], rows, align: [...model.align] };
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableParse.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts
git commit -m "feat(editor): TSV parse and spreadsheet paste-block for tables"
```

---

## Phase 2 — Op-compute + dispatch spine

### Task 5: Pure `computeTableEdit`

**Files:**
- Create: `web/src/components/editor/tableOps.ts`
- Test: `web/src/components/editor/tableOps.test.ts`

- [ ] **Step 1: Write failing tests** — create `tableOps.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { computeTableEdit } from "./tableOps";

const md = "| A | B |\n| --- | --- |\n| 1 | 2 |";

describe("computeTableEdit", () => {
  it("inserts a row below index 0", () => {
    const out = computeTableEdit(md, { kind: "insertRow", index: 1 });
    expect(out).toBe("| A   | B   |\n| --- | --- |\n| 1   | 2   |\n|     |     |");
  });
  it("removes a column", () => {
    const out = computeTableEdit(md, { kind: "removeColumn", index: 1 });
    expect(out).toBe("| A   |\n| --- |\n| 1   |");
  });
  it("moves a column", () => {
    const out = computeTableEdit(md, { kind: "moveColumn", from: 0, to: 1 });
    expect(out).toBe("| B   | A   |\n| --- | --- |\n| 2   | 1   |");
  });
  it("pastes a TSV block, growing the table", () => {
    const out = computeTableEdit(md, {
      kind: "paste",
      atRow: 0,
      atCol: 0,
      block: [
        ["x", "y"],
        ["z", "w"],
      ],
    });
    expect(out).toBe("| x   | y   |\n| --- | --- |\n| z   | w   |");
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableOps.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement** — create `tableOps.ts` (only the pure part for now):

```ts
import {
  parseTable,
  serializeTable,
  insertRow,
  removeRow,
  moveRow,
  insertColumn,
  removeColumn,
  moveColumn,
  pasteBlock,
} from "./tableParse";

export type TableOp =
  | { kind: "insertRow"; index: number }
  | { kind: "removeRow"; index: number }
  | { kind: "moveRow"; from: number; to: number }
  | { kind: "insertColumn"; index: number }
  | { kind: "removeColumn"; index: number }
  | { kind: "moveColumn"; from: number; to: number }
  | { kind: "paste"; atRow: number; atCol: number; block: string[][] };

/** PURE: apply a structural op to a table's markdown source, returning new source. */
export function computeTableEdit(md: string, op: TableOp): string {
  const m = parseTable(md);
  switch (op.kind) {
    case "insertRow":
      return serializeTable(insertRow(m, op.index));
    case "removeRow":
      return serializeTable(removeRow(m, op.index));
    case "moveRow":
      return serializeTable(moveRow(m, op.from, op.to));
    case "insertColumn":
      return serializeTable(insertColumn(m, op.index));
    case "removeColumn":
      return serializeTable(removeColumn(m, op.index));
    case "moveColumn":
      return serializeTable(moveColumn(m, op.from, op.to));
    case "paste":
      return serializeTable(pasteBlock(m, op.atRow, op.atCol, op.block));
  }
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableOps.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableOps.ts web/src/components/editor/tableOps.test.ts
git commit -m "feat(editor): pure computeTableEdit op transform"
```

---

### Task 6: Focus-after-remount field

**Files:**
- Create: `web/src/components/editor/tableFocus.ts`
- Test: `web/src/components/editor/tableFocus.test.ts`

Because each structural op dispatches a doc change, the editable widget re-mounts and loses the caret. This module carries "after the next build, focus cell (row,col) of the table whose block starts at `pos`". `row = -1` means a header cell.

- [ ] **Step 1: Write failing test** — create `tableFocus.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import {
  tableFocus,
  setTableFocus,
  readTableFocus,
} from "./tableFocus";

describe("tableFocus field", () => {
  it("stores and reads a focus target", () => {
    let state = EditorState.create({ doc: "x", extensions: [tableFocus] });
    expect(readTableFocus(state)).toBeNull();
    state = state.update({ effects: setTableFocus.of({ pos: 0, row: 1, col: 2 }) })
      .state;
    expect(readTableFocus(state)).toEqual({ pos: 0, row: 1, col: 2 });
  });
  it("clears the target when set to null", () => {
    let state = EditorState.create({ doc: "x", extensions: [tableFocus] });
    state = state.update({ effects: setTableFocus.of({ pos: 0, row: 0, col: 0 }) })
      .state;
    state = state.update({ effects: setTableFocus.of(null) }).state;
    expect(readTableFocus(state)).toBeNull();
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/tableFocus.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement** — create `tableFocus.ts`:

```ts
import { StateEffect, StateField, type EditorState } from "@codemirror/state";

export interface TableFocusTarget {
  /** Document position of the table block's start (its `from`). */
  pos: number;
  /** Body row index, or -1 for a header cell. */
  row: number;
  /** Column index. */
  col: number;
}

export const setTableFocus = StateEffect.define<TableFocusTarget | null>();

export const tableFocus = StateField.define<TableFocusTarget | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setTableFocus)) return e.value;
    return value;
  },
});

export function readTableFocus(state: EditorState): TableFocusTarget | null {
  return state.field(tableFocus, false) ?? null;
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/tableFocus.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/tableFocus.ts web/src/components/editor/tableFocus.test.ts
git commit -m "feat(editor): state field for post-remount table cell focus"
```

---

### Task 7: `applyTableOp` dispatcher

**Files:**
- Modify: `web/src/components/editor/tableOps.ts`

This is the only function that touches `EditorView`. It computes the new source, replaces the block in one transaction (one undo step), keeps the selection inside the table (so it re-renders editable), and records the cell to refocus.

- [ ] **Step 1: Implement** — append to `tableOps.ts`:

```ts
import { EditorView } from "@codemirror/view";
import { setTableFocus } from "./tableFocus";

/** Apply a structural op to the table block spanning [from, to] in the document.
 *  Dispatches a single replace, parks the cursor at the block start (keeping the
 *  table editable), and records which cell to refocus after the widget rebuilds. */
export function applyTableOp(
  view: EditorView,
  from: number,
  to: number,
  op: TableOp,
  focus: { row: number; col: number },
): void {
  const md = view.state.sliceDoc(from, to);
  const next = computeTableEdit(md, op);
  if (next === md) return; // no-op (e.g. guard hit) — no transaction
  view.dispatch({
    changes: { from, to, insert: next },
    selection: { anchor: from },
    effects: setTableFocus.of({ pos: from, row: focus.row, col: focus.col }),
  });
}
```

- [ ] **Step 2: Typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/editor/tableOps.ts
git commit -m "feat(editor): applyTableOp single-dispatch with refocus effect"
```

---

## Phase 3 — View

### Task 8: Shared action menu (`tableMenu.ts`)

**Files:**
- Create: `web/src/components/editor/widgets/tableMenu.ts`
- Test: `web/src/components/editor/widgets/tableMenu.test.ts`

One action list, two presentations: a popover anchored to an element (fine pointer) or a bottom sheet (coarse pointer). Pointer type is injected for testability.

- [ ] **Step 1: Write failing test** — create `tableMenu.test.ts`:

```ts
import { describe, it, expect, afterEach } from "vitest";
import { openTableMenu, type MenuAction } from "./tableMenu";

afterEach(() => {
  document.body.innerHTML = "";
});

const actions: MenuAction[] = [
  { label: "Insert above", run: () => {} },
  { label: "Delete row", danger: true, run: () => {} },
];

describe("openTableMenu", () => {
  it("renders a popover near the anchor on a fine pointer", () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, actions, "fine");
    const menu = document.querySelector(".cm-lp-table-menu");
    expect(menu).not.toBeNull();
    expect(menu!.querySelectorAll("[role=menuitem]").length).toBe(2);
  });
  it("renders a bottom sheet on a coarse pointer", () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, actions, "coarse");
    expect(document.querySelector(".cm-lp-table-sheet")).not.toBeNull();
  });
  it("runs the action and closes on click", () => {
    let ran = false;
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    openTableMenu(anchor, [{ label: "Go", run: () => (ran = true) }], "fine");
    document
      .querySelector<HTMLElement>("[role=menuitem]")!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(ran).toBe(true);
    expect(document.querySelector(".cm-lp-table-menu")).toBeNull();
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/widgets/tableMenu.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement** — create `tableMenu.ts`:

```ts
export interface MenuAction {
  label: string;
  danger?: boolean;
  run: () => void;
}

export type PointerType = "fine" | "coarse";

/** Detect the primary pointer; overridable for tests. */
export function pointerType(): PointerType {
  return typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches
    ? "coarse"
    : "fine";
}

/** Open the table action menu: a popover anchored to `anchor` on fine pointers,
 *  a bottom sheet on coarse ones. Returns a dispose function. */
export function openTableMenu(
  anchor: HTMLElement,
  actions: MenuAction[],
  pointer: PointerType = pointerType(),
): () => void {
  const isSheet = pointer === "coarse";
  const root = document.createElement("div");
  root.className = isSheet ? "cm-lp-table-sheet" : "cm-lp-table-menu";
  root.setAttribute("role", "menu");

  const dispose = () => {
    root.remove();
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
  };
  const onOutside = (e: PointerEvent) => {
    if (!root.contains(e.target as Node)) dispose();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      dispose();
    }
  };

  for (const a of actions) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "cm-lp-table-menu-item" + (a.danger ? " danger" : "");
    item.setAttribute("role", "menuitem");
    item.textContent = a.label;
    item.addEventListener("click", () => {
      dispose();
      a.run();
    });
    root.appendChild(item);
  }

  document.body.appendChild(root);

  if (!isSheet) {
    const r = anchor.getBoundingClientRect();
    root.style.position = "absolute";
    root.style.left = `${r.left + window.scrollX}px`;
    root.style.top = `${r.bottom + window.scrollY + 4}px`;
  }

  // Defer listener attach so the opening click doesn't immediately close it.
  setTimeout(() => {
    document.addEventListener("pointerdown", onOutside, true);
    document.addEventListener("keydown", onKey, true);
  }, 0);

  return dispose;
}
```

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/widgets/tableMenu.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/tableMenu.ts web/src/components/editor/widgets/tableMenu.test.ts
git commit -m "feat(editor): shared table action menu (popover + sheet)"
```

---

### Task 9: Wire focus field + widget signature in `livePreview.ts`

**Files:**
- Modify: `web/src/components/editor/livePreview.ts`

The editable widget needs the `EditorView` to call `applyTableOp`, and the live-preview extension must include the `tableFocus` field. Today the widget is constructed with `(md, start, end, onCommitTable)` and `livePreview()` returns a single `StateField`. We add the focus field to the returned extension and give the widget access to the view via CodeMirror's `WidgetType.toDOM(view)` argument (no signature change needed for that — `toDOM` receives the view).

- [ ] **Step 1: Include the focus field in the extension**

In `livePreview.ts`, add the import:

```ts
import { tableFocus } from "./tableFocus";
```

Change the `return field;` at the end of `livePreview()` to:

```ts
  return [field, tableFocus];
```

- [ ] **Step 2: Typecheck**

Run: `pnpm typecheck`
Expected: PASS (return type is already `Extension`, which accepts an array).

- [ ] **Step 3: Commit**

```bash
git add web/src/components/editor/livePreview.ts
git commit -m "feat(editor): register tableFocus in live-preview extension"
```

---

### Task 10: Rewrite `EditableTableWidget` — cells, grips, ops, focus restore

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`
- Test: `web/src/components/editor/widgets/editableTableWidget.test.ts`

This is the core view task. The widget now: renders cells + row/column grips; routes structural ops through `applyTableOp`; on (re)mount, consumes the `tableFocus` target to refocus a cell; commits in-progress cell text on focus-out (unchanged). Drag-reorder is added in Task 11; paste in Task 12.

`toDOM(view: EditorView)` receives the view — use it for `applyTableOp` and to read `readTableFocus`.

- [ ] **Step 1: Add grip + menu tests** — append to `editableTableWidget.test.ts`:

```ts
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { tableFocus } from "../tableFocus";

function mountInView(doc: string) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: [tableFocus] }),
    parent,
  });
  return view;
}

describe("EditableTableWidget structure controls", () => {
  it("renders a grip for each row and column", () => {
    const md = "| A | B |\n| - | - |\n| 1 | 2 |\n| 3 | 4 |";
    const view = mountInView(md);
    const widget = new EditableTableWidget(md, 0, md.length, () => {});
    const dom = widget.toDOM(view);
    document.body.appendChild(dom);
    expect(dom.querySelectorAll(".cm-lp-col-grip").length).toBe(2);
    expect(dom.querySelectorAll(".cm-lp-row-grip").length).toBe(2);
    view.destroy();
  });

  it("opens a menu with delete column when a column grip is clicked", () => {
    const md = "| A | B |\n| - | - |\n| 1 | 2 |";
    const view = mountInView(md);
    const widget = new EditableTableWidget(md, 0, md.length, () => {});
    const dom = widget.toDOM(view);
    document.body.appendChild(dom);
    dom
      .querySelector<HTMLElement>(".cm-lp-col-grip")!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    const labels = [...document.querySelectorAll("[role=menuitem]")].map(
      (n) => n.textContent,
    );
    expect(labels).toContain("Delete column");
    view.destroy();
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: FAIL — no grips; `toDOM` signature mismatch.

- [ ] **Step 3: Rewrite `editableTableWidget.ts`** — replace the whole file:

```ts
import { WidgetType, type EditorView } from "@codemirror/view";
import {
  parseTable,
  serializeTable,
  type TableModel,
} from "../tableParse";
import { applyTableOp } from "../tableOps";
import { readTableFocus } from "../tableFocus";
import { openTableMenu, type MenuAction } from "./tableMenu";

export class EditableTableWidget extends WidgetType {
  constructor(
    readonly md: string,
    readonly from: number,
    readonly to: number,
    readonly onCommit: (from: number, to: number, md: string) => void,
  ) {
    super();
  }

  eq(other: EditableTableWidget): boolean {
    return (
      other.md === this.md && other.from === this.from && other.to === this.to
    );
  }
  ignoreEvent(): boolean {
    return true;
  }

  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "cm-lp-table-edit";
    const table = document.createElement("table");
    table.className = "cm-lp-table editing";
    table.setAttribute("role", "grid");
    wrap.appendChild(table);
    const model = parseTable(this.md);
    this.render(view, wrap, table, model);

    // Commit in-progress cell text once focus leaves the whole table.
    wrap.addEventListener("focusout", (e) => {
      const next = e.relatedTarget as Node | null;
      if (next && wrap.contains(next)) return; // moving between cells
      const md = serializeTable(this.readModel(table, model));
      if (md !== serializeTable(parseTable(this.md)))
        this.onCommit(this.from, this.to, md);
    });

    // Restore focus to the cell recorded before the last structural op, else
    // focus the first cell on initial entry.
    requestAnimationFrame(() => {
      const target = readTableFocus(view.state);
      if (target && target.pos === this.from) {
        this.focusCell(table, target.row, target.col);
      } else {
        wrap.querySelector<HTMLElement>("th, td")?.focus();
      }
    });
    return wrap;
  }

  /** Read the live model back from the DOM, preserving alignment from `base`
   *  (alignment has no DOM affordance this round). Reads only direct text nodes
   *  so grip/control buttons inside cells are excluded. */
  protected readModel(table: HTMLTableElement, base: TableModel): TableModel {
    const text = (cell: Element): string =>
      [...cell.childNodes]
        .filter((n) => n.nodeType === Node.TEXT_NODE)
        .map((n) => n.textContent ?? "")
        .join("")
        .trim();
    const header = [...table.querySelectorAll("thead th")].map(text);
    const rows = [...table.querySelectorAll("tbody tr")].map((tr) =>
      [...tr.querySelectorAll("td")].map(text),
    );
    const align = header.map((_, i) => base.align[i] ?? "none");
    return { header, rows, align };
  }

  /** Read the current DOM model then apply a structural op via the single
   *  dispatch spine (captures uncommitted typing first). */
  private op(
    view: EditorView,
    table: HTMLTableElement,
    base: TableModel,
    build: (m: TableModel) => import("../tableOps").TableOp,
    focus: { row: number; col: number },
  ): void {
    const current = this.readModel(table, base);
    const md = serializeTable(current);
    // The widget's block range may have shifted; recompute via onCommit's range
    // is unnecessary — we dispatch against this.from/this.to which CodeMirror
    // keeps mapped for the current decoration.
    view.dispatch({
      changes: { from: this.from, to: this.to, insert: md },
    });
    // After syncing the DOM edits into the doc, apply the structural op on top.
    applyTableOp(view, this.from, this.from + md.length, build(current), focus);
  }

  protected render(
    view: EditorView,
    wrap: HTMLElement,
    table: HTMLTableElement,
    model: TableModel,
  ): void {
    table.textContent = "";
    wrap
      .querySelectorAll(".cm-lp-row-grip, .cm-lp-col-grip, .cm-lp-add-col, .cm-lp-add-row")
      .forEach((n) => n.remove());

    const thead = table.createTHead();
    const hr = thead.insertRow();
    model.header.forEach((h, ci) => {
      const th = document.createElement("th");
      th.contentEditable = "plaintext-only";
      th.setAttribute("role", "gridcell");
      th.textContent = h;
      this.cellKeys(table, th);
      hr.appendChild(th);
      wrap.appendChild(
        this.grip("cm-lp-col-grip", () =>
          this.columnActions(view, table, model, ci),
        ),
      );
    });

    const tbody = table.createTBody();
    model.rows.forEach((row, ri) => {
      const tr = tbody.insertRow();
      row.forEach((c, ci) => {
        const td = tr.insertCell();
        td.contentEditable = "plaintext-only";
        td.setAttribute("role", "gridcell");
        td.textContent = c;
        this.cellKeys(table, td);
      });
      wrap.appendChild(
        this.grip("cm-lp-row-grip", () =>
          this.rowActions(view, table, model, ri),
        ),
      );
    });

    wrap.appendChild(
      this.ctl("cm-lp-add-col", "+", () =>
        this.op(view, table, model, (m) => ({
          kind: "insertColumn",
          index: m.header.length,
        }), { row: -1, col: model.header.length }),
      ),
    );
    wrap.appendChild(
      this.ctl("cm-lp-add-row", "+", () =>
        this.op(view, table, model, (m) => ({
          kind: "insertRow",
          index: m.rows.length,
        }), { row: model.rows.length, col: 0 }),
      ),
    );
  }

  private rowActions(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    ri: number,
  ): MenuAction[] {
    return [
      { label: "Insert row above", run: () => this.op(view, table, model, () => ({ kind: "insertRow", index: ri }), { row: ri, col: 0 }) },
      { label: "Insert row below", run: () => this.op(view, table, model, () => ({ kind: "insertRow", index: ri + 1 }), { row: ri + 1, col: 0 }) },
      { label: "Move row up", run: () => this.op(view, table, model, () => ({ kind: "moveRow", from: ri, to: ri - 1 }), { row: ri - 1, col: 0 }) },
      { label: "Move row down", run: () => this.op(view, table, model, () => ({ kind: "moveRow", from: ri, to: ri + 1 }), { row: ri + 1, col: 0 }) },
      { label: "Delete row", danger: true, run: () => this.op(view, table, model, () => ({ kind: "removeRow", index: ri }), { row: Math.max(0, ri - 1), col: 0 }) },
    ];
  }

  private columnActions(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    ci: number,
  ): MenuAction[] {
    return [
      { label: "Insert column left", run: () => this.op(view, table, model, () => ({ kind: "insertColumn", index: ci }), { row: -1, col: ci }) },
      { label: "Insert column right", run: () => this.op(view, table, model, () => ({ kind: "insertColumn", index: ci + 1 }), { row: -1, col: ci + 1 }) },
      { label: "Move column left", run: () => this.op(view, table, model, () => ({ kind: "moveColumn", from: ci, to: ci - 1 }), { row: -1, col: ci - 1 }) },
      { label: "Move column right", run: () => this.op(view, table, model, () => ({ kind: "moveColumn", from: ci, to: ci + 1 }), { row: -1, col: ci + 1 }) },
      { label: "Delete column", danger: true, run: () => this.op(view, table, model, () => ({ kind: "removeColumn", index: ci }), { row: -1, col: Math.max(0, ci - 1) }) },
    ];
  }

  /** Focus the cell at (row,col); row -1 = header. */
  private focusCell(table: HTMLTableElement, row: number, col: number): void {
    const target =
      row < 0
        ? table.querySelectorAll<HTMLElement>("thead th")[col]
        : table.querySelectorAll("tbody tr")[row]?.querySelectorAll<HTMLElement>("td")[col];
    (target ?? table.querySelector<HTMLElement>("th, td"))?.focus();
  }

  private grip(cls: string, actions: () => MenuAction[]): HTMLElement {
    const g = document.createElement("button");
    g.type = "button";
    g.className = cls;
    g.contentEditable = "false";
    g.setAttribute("aria-haspopup", "menu");
    g.textContent = "⠿"; // ⠿
    g.addEventListener("mousedown", (e) => e.preventDefault()); // keep caret
    g.addEventListener("click", () => openTableMenu(g, actions()));
    return g;
  }

  private cellKeys(table: HTMLElement, cell: HTMLElement): void {
    cell.addEventListener("keydown", (e) => {
      const cells = [...table.querySelectorAll<HTMLElement>("th, td")];
      const i = cells.indexOf(cell);
      const cols = table.querySelectorAll("thead th").length || 1;
      if (e.key === "Tab") {
        e.preventDefault();
        cells[i + (e.shiftKey ? -1 : 1)]?.focus();
      } else if (e.key === "Enter") {
        e.preventDefault();
        cells[i + cols]?.focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cell.blur();
      } else if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
        e.stopPropagation(); // keep select-all scoped to the cell
      }
    });
  }

  private ctl(cls: string, label: string, onClick: () => void): HTMLElement {
    const b = document.createElement("button");
    b.className = cls;
    b.type = "button";
    b.textContent = label;
    b.contentEditable = "false";
    b.addEventListener("mousedown", (e) => {
      e.preventDefault();
      onClick();
    });
    return b;
  }
}
```

> **Implementation note:** the `op()` helper does two dispatches (sync DOM text, then structural op). If the executor finds the double-dispatch produces two undo steps or a flicker, collapse it: read the DOM model, apply the structural op to that model directly, serialize once, and dispatch a single replace + `setTableFocus` effect (mirror `applyTableOp`'s body). The two-step form is written first because it reuses the tested `applyTableOp` unchanged; the single-step form is the optimization. Verify undo granularity manually (Task 14).

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: cell-key tests + new grip/menu tests PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/src/components/editor/widgets/editableTableWidget.test.ts
git commit -m "feat(editor): grip-driven table ops with refocus after remount"
```

---

### Task 11: Drag-to-reorder via grips

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`

Dragging a grip reorders its row/column. jsdom can't simulate real drag layout, so coverage here is a unit test of the index math plus manual verification.

- [ ] **Step 1: Extract pure drop-index helper + test**

Create `web/src/components/editor/widgets/dragIndex.ts`:

```ts
/** Given pointer position `p` along an axis and the sorted center offsets of each
 *  item, return the index the dragged item should land before/at. */
export function dropIndex(p: number, centers: number[]): number {
  let i = 0;
  while (i < centers.length && p > centers[i]) i++;
  return i;
}
```

Create `web/src/components/editor/widgets/dragIndex.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { dropIndex } from "./dragIndex";

describe("dropIndex", () => {
  const centers = [10, 30, 50];
  it("returns 0 before the first center", () => {
    expect(dropIndex(5, centers)).toBe(0);
  });
  it("returns the index past centers the pointer has crossed", () => {
    expect(dropIndex(35, centers)).toBe(2);
  });
  it("returns length past the last center", () => {
    expect(dropIndex(99, centers)).toBe(3);
  });
});
```

- [ ] **Step 2: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/widgets/dragIndex.test.ts`
Expected: PASS.

- [ ] **Step 3: Wire drag into the grip** — in `editableTableWidget.ts`, extend `grip()` to accept the axis + current index + a `move` callback, and add pointer handlers. Replace `grip()` and its two call sites:

```ts
  private grip(
    cls: string,
    axis: "row" | "col",
    index: number,
    actions: () => MenuAction[],
    onMove: (to: number) => void,
    centers: () => number[],
  ): HTMLElement {
    const g = document.createElement("button");
    g.type = "button";
    g.className = cls;
    g.contentEditable = "false";
    g.setAttribute("aria-haspopup", "menu");
    g.textContent = "⠿";
    let dragging = false;
    let moved = false;
    g.addEventListener("mousedown", (e) => e.preventDefault());
    g.addEventListener("pointerdown", (e) => {
      dragging = true;
      moved = false;
      g.setPointerCapture(e.pointerId);
    });
    g.addEventListener("pointermove", (e) => {
      if (dragging) moved = true; // a drop indicator can be added here
    });
    g.addEventListener("pointerup", (e) => {
      if (dragging && moved) {
        const p = axis === "row" ? e.clientY : e.clientX;
        const to = Math.min(centers().length - 1, dropIndex(p, centers()));
        if (to !== index) onMove(to);
      } else {
        openTableMenu(g, actions());
      }
      dragging = false;
    });
    return g;
  }
```

Add the import at the top:

```ts
import { dropIndex } from "./dragIndex";
```

Update the column grip call in `render()`:

```ts
      wrap.appendChild(
        this.grip(
          "cm-lp-col-grip",
          "col",
          ci,
          () => this.columnActions(view, table, model, ci),
          (to) => this.op(view, table, model, () => ({ kind: "moveColumn", from: ci, to }), { row: -1, col: to }),
          () =>
            [...table.querySelectorAll("thead th")].map(
              (el) => el.getBoundingClientRect().left + el.getBoundingClientRect().width / 2,
            ),
        ),
      );
```

Update the row grip call in `render()`:

```ts
      wrap.appendChild(
        this.grip(
          "cm-lp-row-grip",
          "row",
          ri,
          () => this.rowActions(view, table, model, ri),
          (to) => this.op(view, table, model, () => ({ kind: "moveRow", from: ri, to }), { row: to, col: 0 }),
          () =>
            [...table.querySelectorAll("tbody tr")].map(
              (el) => el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2,
            ),
        ),
      );
```

- [ ] **Step 4: Run the widget test file (regression)**

Run: `pnpm exec vitest run src/components/editor/widgets/`
Expected: PASS (click still opens the menu when there's no drag movement; jsdom reports 0-size rects, so `moved` stays false on a bare click).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/dragIndex.ts web/src/components/editor/widgets/dragIndex.test.ts web/src/components/editor/widgets/editableTableWidget.ts
git commit -m "feat(editor): drag table grips to reorder rows/columns"
```

---

### Task 12: TSV paste into cells

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`
- Test: `web/src/components/editor/widgets/editableTableWidget.test.ts`

A multi-line/multi-tab paste into a cell spills across cells via `pasteBlock`; a plain single-value paste falls through to the browser's default (normal text insert).

- [ ] **Step 1: Add a paste test** — append to `editableTableWidget.test.ts`:

```ts
describe("EditableTableWidget paste", () => {
  it("spills a TSV block from the anchored cell", () => {
    const md = "| A | B |\n| - | - |\n| 1 | 2 |";
    const view = mountInView(md);
    const widget = new EditableTableWidget(md, 0, md.length, () => {});
    const dom = widget.toDOM(view);
    document.body.appendChild(dom);
    const firstBodyCell = dom.querySelector<HTMLElement>("tbody td")!;
    const data = new DataTransfer();
    data.setData("text/plain", "x\ty\nz\tw");
    firstBodyCell.dispatchEvent(
      new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: data }),
    );
    expect(view.state.doc.toString()).toContain("| x");
    expect(view.state.doc.toString()).toContain("| z");
    view.destroy();
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: FAIL — paste not handled; doc unchanged.

- [ ] **Step 3: Implement** — in `cellKeys`-adjacent code, add a `paste` listener. Add this inside `render()` where cells are created (after `this.cellKeys(table, td)` and the header `this.cellKeys(table, th)`), or factor a `this.cellPaste(view, table, model, cell, row, col)` call. Add the method:

```ts
  private cellPaste(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    cell: HTMLElement,
    row: number,
    col: number,
  ): void {
    cell.addEventListener("paste", (e) => {
      const text = e.clipboardData?.getData("text/plain") ?? "";
      const block = parseTSV(text);
      const multi = block.length > 1 || (block[0]?.length ?? 0) > 1;
      if (!multi) return; // single value → let the browser insert text normally
      e.preventDefault();
      this.op(
        view,
        table,
        model,
        (m) => ({ kind: "paste", atRow: row, atCol: col, block }),
        { row, col },
      );
    });
  }
```

Add the import:

```ts
import { parseTSV } from "../tableParse";
```

Call it from `render()` for body cells: after `this.cellKeys(table, td);` add `this.cellPaste(view, table, model, td, ri, ci);`. (Header paste is out of scope — header cells get cellKeys only.)

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/src/components/editor/widgets/editableTableWidget.test.ts
git commit -m "feat(editor): paste spreadsheet TSV blocks into table cells"
```

---

### Task 13: Right-click context menu on cells

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`
- Test: `web/src/components/editor/widgets/editableTableWidget.test.ts`

Right-clicking a body cell opens a menu combining the row and column actions (and the browser's native menu is suppressed). Clipboard entries use `document.execCommand` for cut/copy and trigger the existing paste path for paste.

- [ ] **Step 1: Add a contextmenu test** — append:

```ts
describe("EditableTableWidget context menu", () => {
  it("opens a combined row+column menu on right-click", () => {
    const md = "| A | B |\n| - | - |\n| 1 | 2 |";
    const view = mountInView(md);
    const widget = new EditableTableWidget(md, 0, md.length, () => {});
    const dom = widget.toDOM(view);
    document.body.appendChild(dom);
    dom
      .querySelector<HTMLElement>("tbody td")!
      .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    const labels = [...document.querySelectorAll("[role=menuitem]")].map((n) => n.textContent);
    expect(labels).toContain("Delete row");
    expect(labels).toContain("Delete column");
    view.destroy();
  });
});
```

- [ ] **Step 2: Run and verify failure**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: FAIL — no menu opens.

- [ ] **Step 3: Implement** — add a `cellContext` method and call it for body cells in `render()`:

```ts
  private cellContext(
    view: EditorView,
    table: HTMLTableElement,
    model: TableModel,
    cell: HTMLElement,
    row: number,
    col: number,
  ): void {
    cell.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const actions: MenuAction[] = [
        { label: "Cut", run: () => document.execCommand("cut") },
        { label: "Copy", run: () => document.execCommand("copy") },
        { label: "Paste", run: () => cell.dispatchEvent(new ClipboardEvent("paste", { bubbles: true })) },
        ...this.rowActions(view, table, model, row),
        ...this.columnActions(view, table, model, col),
      ];
      openTableMenu(cell, actions);
    });
  }
```

Call from `render()` for body cells: after `this.cellPaste(...)` add `this.cellContext(view, table, model, td, ri, ci);`.

- [ ] **Step 4: Run and verify pass**

Run: `pnpm exec vitest run src/components/editor/widgets/editableTableWidget.test.ts`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/src/components/editor/widgets/editableTableWidget.test.ts
git commit -m "feat(editor): right-click context menu for table cell ops"
```

---

### Task 14: CSS refresh + read-only alignment

**Files:**
- Modify: `web/src/components/editor/livePreview.css`
- Modify: `web/src/components/editor/widgets/tableWidget.ts`

- [ ] **Step 1: Render alignment in the read-only widget** — in `tableWidget.ts`, apply `text-align` from the parsed `align` array. Replace the body of `toDOM()` cell loops to set alignment:

```ts
  toDOM(): HTMLElement {
    const { header, rows, align } = parseTable(this.md);
    const table = document.createElement("table");
    table.className = "cm-lp-table";
    const css = (i: number) =>
      align[i] && align[i] !== "none" ? align[i] : "";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    header.forEach((h, i) => {
      const th = document.createElement("th");
      th.textContent = h;
      if (css(i)) th.style.textAlign = css(i);
      hr.appendChild(th);
    });
    const tbody = table.createTBody();
    for (const r of rows) {
      const tr = tbody.insertRow();
      r.forEach((c, i) => {
        const td = tr.insertCell();
        td.textContent = c;
        if (css(i)) td.style.textAlign = css(i);
      });
    }
    table.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEnterEdit(this.from);
    });
    return table;
  }
```

- [ ] **Step 2: Replace the table CSS block** in `livePreview.css` (lines covering `.cm-lp-table` through the last `.cm-lp-row-del` rule, i.e. the current 163–245 + 268–271 region) with the refreshed chrome + grips + menu + sheet:

```css
.cm-lp-table {
  border-collapse: separate;
  border-spacing: 0;
  margin: 0.5em 0;
  font-size: 0.92em;
  border: 1px solid #2c2c36;
  border-radius: 8px;
  overflow: hidden;
}
.cm-lp-table th,
.cm-lp-table td {
  border-right: 1px solid #26262e;
  border-bottom: 1px solid #26262e;
  padding: 6px 12px;
  text-align: left;
}
.cm-lp-table th:last-child,
.cm-lp-table td:last-child {
  border-right: none;
}
.cm-lp-table tr:last-child td {
  border-bottom: none;
}
.cm-lp-table th {
  background: #16161c;
  font-weight: 600;
  color: #c9cad6;
}
.cm-lp-table tbody tr:hover td {
  background: #1f1f29;
}

.cm-lp-table-edit {
  position: relative;
  display: inline-block;
  padding: 20px 20px 20px 20px;
}
.cm-lp-table.editing th,
.cm-lp-table.editing td {
  min-width: 48px;
  outline: none;
}
.cm-lp-table.editing th:focus,
.cm-lp-table.editing td:focus {
  outline: 2px solid #6366f1;
  outline-offset: -2px;
  background: #1a1a26;
}

/* grips */
.cm-lp-row-grip,
.cm-lp-col-grip {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #26262e;
  border: 1px solid #34343f;
  border-radius: 4px;
  color: #7d7e8c;
  font-size: 9px;
  line-height: 1;
  cursor: grab;
  opacity: 0;
  transition: opacity 0.12s;
}
.cm-lp-table-edit:hover .cm-lp-row-grip,
.cm-lp-table-edit:hover .cm-lp-col-grip {
  opacity: 1;
}
.cm-lp-row-grip:hover,
.cm-lp-col-grip:hover {
  background: #6366f1;
  border-color: #6366f1;
  color: #fff;
}
@media (pointer: coarse) {
  .cm-lp-row-grip,
  .cm-lp-col-grip {
    opacity: 1;
  }
}

/* edge add buttons */
.cm-lp-add-col,
.cm-lp-add-row {
  position: absolute;
  border: 1px dashed #6366f188;
  background: #6366f120;
  color: #818cf8;
  border-radius: 5px;
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity 0.12s;
}
.cm-lp-table-edit:hover .cm-lp-add-col,
.cm-lp-table-edit:hover .cm-lp-add-row {
  opacity: 1;
}
.cm-lp-add-col {
  top: 20px;
  right: 2px;
  bottom: 20px;
  width: 16px;
}
.cm-lp-add-row {
  left: 20px;
  right: 20px;
  bottom: 2px;
  height: 16px;
}

/* action menu (popover) */
.cm-lp-table-menu {
  background: #23232c;
  border: 1px solid #3a3a48;
  border-radius: 8px;
  box-shadow: 0 8px 24px #000a;
  padding: 5px;
  min-width: 168px;
  z-index: 30;
  display: flex;
  flex-direction: column;
}
.cm-lp-table-menu-item {
  text-align: left;
  background: none;
  border: none;
  color: #d6d7e0;
  padding: 6px 9px;
  border-radius: 5px;
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}
.cm-lp-table-menu-item:hover {
  background: #6366f1;
  color: #fff;
}
.cm-lp-table-menu-item.danger {
  color: #f4a3a3;
}
.cm-lp-table-menu-item.danger:hover {
  background: #b4434333;
  color: #f4a3a3;
}

/* action menu (bottom sheet) */
.cm-lp-table-sheet {
  position: fixed;
  left: 0;
  right: 0;
  bottom: 0;
  background: #1c1c23;
  border-top: 3px solid #6366f1;
  border-radius: 14px 14px 0 0;
  box-shadow: 0 -8px 30px #000a;
  padding: 12px 10px calc(12px + env(safe-area-inset-bottom));
  z-index: 40;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.cm-lp-table-sheet .cm-lp-table-menu-item {
  padding: 13px 14px;
  font-size: 14px;
}
```

- [ ] **Step 3: Run tests + typecheck**

Run: `pnpm exec vitest run src/components/editor/ && pnpm typecheck`
Expected: PASS.

- [ ] **Step 4: Manual verification (record outcome)**

Run `pnpm dev`, open a note with a table. Confirm:
- Hover shows row/column grips; clicking a grip opens the menu; Delete column/row works.
- Edge `+` appends; insert-above/below/left/right land focus on the new cell.
- Drag a grip reorders; undo (Mod-Z) reverses exactly one structural op.
- Right-click a cell shows the combined menu; paste a block copied from a spreadsheet spills across cells.
- Editing a cell then clicking away preserves alignment markers in source mode (Mod-E).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/livePreview.css web/src/components/editor/widgets/tableWidget.ts
git commit -m "style(editor): polished table chrome, grips, menu, and sheet"
```

---

### Task 15: Full gate + cleanup

**Files:** none (verification)

- [ ] **Step 1: Run the full local gate** from `web/`:

```bash
pnpm exec vitest run && pnpm typecheck && pnpm lint && pnpm exec prettier --check .
```
Expected: all green. Fix any `prettier --check` failures with `pnpm exec prettier --write .` and re-run.

- [ ] **Step 2: Confirm no dead code** — `addRow`/`addColumn` are still exported and used by the edge `+` ops indirectly via `insertRow`/`insertColumn`; remove them only if no importer remains:

```bash
git grep -n "addRow\|addColumn" web/src
```
If the only references are their own definitions and tests, delete them and their tests; otherwise keep.

- [ ] **Step 3: Commit any cleanup**

```bash
git add -A
git commit -m "chore(editor): tidy table-editor exports after refactor"
```

---

## Self-Review

**Spec coverage:**
- Insert/delete/reorder rows & columns → Tasks 3, 10, 11, 13. ✔
- Bigger/discoverable controls + polish → Tasks 8, 14 (always-on grips on coarse pointer; larger `+`; refreshed chrome). ✔
- TSV spreadsheet paste → Tasks 4, 12. ✔
- Lossless alignment + prettify (bug fix) → Tasks 1, 2, 14 (read-only render). ✔
- Per-op undo → Tasks 5–7, 10 (single dispatch per op). ✔
- Context menu (desktop) + bottom sheet (mobile) → Tasks 8, 13, 14. ✔
- A11y (grid roles, focus restore) → Tasks 6, 10 (roles + refocus). Arrow-key grid navigation is partially covered (Tab/Enter retained); full roving-tabindex arrow nav is a follow-up noted in the spec's a11y section — **flagged, not fully implemented** to keep scope bounded.
- Delete-last guard → Task 3 (kept in `removeRow`/`removeColumn`). ✔
- ⌘K commands → intentionally deferred (spec §4.4 / §10).

**Placeholder scan:** none — every code step is complete.

**Type consistency:** `TableModel` carries `header/rows/align` everywhere; `TableOp`/`computeTableEdit`/`applyTableOp` signatures match across Tasks 5–13; `MenuAction`/`openTableMenu` match across Tasks 8/10/13.

**Known risk (from spec §8):** the `op()` double-dispatch in Task 10 may need collapsing to a single dispatch for clean undo — called out inline with the exact fix and a manual verification step (Task 14 Step 4).
