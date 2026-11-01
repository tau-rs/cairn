# CE‑B Rich Table Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make GFM tables editable in place — click a rendered table to enter an editable grid (contenteditable cells + inline add/remove row/column controls), click away to commit the change back to the markdown source in one undo step.

**Architecture:** A pure `TableModel` (parse/serialize/ops, alignment-free, pipe-escaping) plus a read-only ⇄ editable widget pair gated by the existing reveal-on-cursor logic: caret-not-in-table → read-only `TableWidget`; caret-in-table → interactive `EditableTableWidget`. The editable widget edits in LOCAL DOM during the session (no CodeMirror dispatch, so it's never rebuilt mid-edit) and commits once on `focusout` of the whole table.

**Tech Stack:** React 18 + TypeScript, CodeMirror 6 (`@codemirror/view`/`state`), `@uiw/react-codemirror`, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-02-ce-b-rich-table-editor-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` on the final task (and e2e where a task says so). Run `pnpm format` + re-stage if format fails.
- e2e uses port **5273** (already configured; 5173 is tau-web-ui). The dev server convention is `pnpm dev --port 5273 --strictPort`.
- Current state: 122 unit tests, 5 e2e, all green.
- **Relevant existing code:**
  - `tableParse.ts` exports `parseTable(md): { header: string[]; rows: string[][] }` and a `cells()` helper (splits a line on `|`, trims). The delimiter row (line 2) is dropped.
  - `widgets/tableWidget.ts`: read-only `TableWidget(md)` → builds `<table class="cm-lp-table">`; `eq` compares `md`; `ignoreEvent()` → true.
  - `livePreview.ts` Table branch (inside `tree.iterate`):
    ```ts
    } else if (name === "Table") {
      const start = state.doc.lineAt(from).from;
      const end = state.doc.lineAt(to).to;
      if (!selectionTouches(state, start, end)) {
        const md = state.doc.sliceString(start, end);
        decos.push(Decoration.replace({ widget: new TableWidget(md), block: true }).range(start, end));
      }
    }
    ```
    `LivePreviewOptions` currently has `resolve`, `onOpenNote`, `onToggleCheckbox`, `resolveImage`, `onEditImage`. The builder returns `{ decorations, atomic }`; only widget-bearing decorations are atomic (so the Table widgets are atomic — keep them so).
  - `Editor.tsx`: the `extensions` memo calls `livePreview({...})` with the options above; each callback reads the stable `viewRef` (NOT in the memo deps). `EditorSelection` is imported from `@codemirror/state`. `viewRef` captured via `onCreateEditor`.
- CodeMirror widget DOM + interaction don't render under jsdom — assert pure-model logic in Vitest; assert click/edit interaction in Playwright e2e. Never assert widget DOM in Vitest.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/editor/tableParse.ts` | Pure model: `parseTable` (unescape `\|`), `serializeTable`, `addRow`/`removeRow`/`addColumn`/`removeColumn`, `TableModel` type. |
| `web/src/components/editor/widgets/tableWidget.ts` | Read-only render + mousedown→`onEnterTableEdit(from)`. |
| `web/src/components/editor/widgets/editableTableWidget.ts` | Interactive grid: editable cells, inline controls, local-DOM editing, commit on focus-out. |
| `web/src/components/editor/livePreview.ts` | Table branch: untouched → read-only, touched → editable; `LivePreviewOptions` gains `onEnterTableEdit`, `onCommitTable`. |
| `web/src/components/Editor.tsx` | Supplies `onEnterTableEdit` (caret dispatch) + `onCommitTable` (changes dispatch). |
| `web/src/components/editor/livePreview.css` | Editable-table + edge-control styles. |
| `web/e2e/skeleton.spec.ts` | Edit-a-table interaction assertions. |

---

## Task 1: TableModel — serialize + ops + pipe-escaping

**Files:**
- Modify: `web/src/components/editor/tableParse.ts`
- Modify: `web/src/components/editor/tableParse.test.ts`

- [ ] **Step 1: Write the failing tests**

Append to `web/src/components/editor/tableParse.test.ts`:

```ts
import {
  serializeTable,
  addRow,
  removeRow,
  addColumn,
  removeColumn,
} from "./tableParse";

describe("serializeTable", () => {
  it("emits GFM with a left-aligned delimiter row", () => {
    expect(serializeTable({ header: ["A", "B"], rows: [["1", "2"]] })).toBe(
      "| A | B |\n| --- | --- |\n| 1 | 2 |",
    );
  });
  it("round-trips through parseTable", () => {
    const model = { header: ["A", "B"], rows: [["1", "2"], ["3", "4"]] };
    expect(parseTable(serializeTable(model))).toEqual(model);
  });
  it("escapes pipes in cell text and parse unescapes them", () => {
    const model = { header: ["A"], rows: [["x|y"]] };
    const md = serializeTable(model);
    expect(md).toContain("x\\|y");
    expect(parseTable(md)).toEqual(model);
  });
});

describe("table model ops", () => {
  const m = { header: ["A", "B"], rows: [["1", "2"]] };
  it("addRow appends a blank row", () => {
    expect(addRow(m)).toEqual({ header: ["A", "B"], rows: [["1", "2"], ["", ""]] });
  });
  it("removeRow deletes a row but keeps at least one", () => {
    expect(removeRow(m, 0)).toEqual(m); // only one row → no-op
    const m2 = { header: ["A"], rows: [["1"], ["2"]] };
    expect(removeRow(m2, 0)).toEqual({ header: ["A"], rows: [["2"]] });
  });
  it("addColumn appends a blank column to header and every row", () => {
    expect(addColumn(m)).toEqual({ header: ["A", "B", ""], rows: [["1", "2", ""]] });
  });
  it("removeColumn deletes a column but keeps at least one", () => {
    expect(removeColumn(m, 1)).toEqual({ header: ["A"], rows: [["1"]] });
    const oneCol = { header: ["A"], rows: [["1"]] };
    expect(removeColumn(oneCol, 0)).toEqual(oneCol); // no-op at minimum
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- tableParse`
Expected: FAIL — `serializeTable`/ops not exported; pipe-escape not handled in `parseTable`.

- [ ] **Step 3: Implement in `tableParse.ts`**

Replace the file contents with:

```ts
export interface TableModel {
  header: string[];
  rows: string[][];
}

// Split a table row on UNescaped pipes, drop outer pipes, unescape \| → |.
const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, "|"));

/** Parse a GFM pipe table's source into a header + body rows (line 2 = delimiter, dropped). */
export function parseTable(md: string): TableModel {
  const lines = md
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return { header: [], rows: [] };
  const header = cells(lines[0]);
  const rows = lines.slice(2).map(cells);
  return { header, rows };
}

const escapeCell = (s: string): string => s.trim().replace(/\|/g, "\\|");
const fmtRow = (cs: string[]): string => `| ${cs.map(escapeCell).join(" | ")} |`;

/** Serialize a model to GFM markdown (alignment normalized to left). */
export function serializeTable(m: TableModel): string {
  const delim = `| ${m.header.map(() => "---").join(" | ")} |`;
  return [fmtRow(m.header), delim, ...m.rows.map(fmtRow)].join("\n");
}

/** Append a blank row (column count = header length). */
export function addRow(m: TableModel): TableModel {
  return { header: m.header, rows: [...m.rows, m.header.map(() => "")] };
}

/** Remove a body row; keeps at least one body row. */
export function removeRow(m: TableModel, index: number): TableModel {
  if (m.rows.length <= 1) return m;
  return { header: m.header, rows: m.rows.filter((_, i) => i !== index) };
}

/** Append a blank column to the header and every row. */
export function addColumn(m: TableModel): TableModel {
  return {
    header: [...m.header, ""],
    rows: m.rows.map((r) => [...r, ""]),
  };
}

/** Remove a column; keeps at least one column. */
export function removeColumn(m: TableModel, index: number): TableModel {
  if (m.header.length <= 1) return m;
  return {
    header: m.header.filter((_, i) => i !== index),
    rows: m.rows.map((r) => r.filter((_, i) => i !== index)),
  };
}
```

(Note: `addRow`/`addColumn` append at the end — the inline `+` controls add at the end per the spec. `(?<!\\)` lookbehind is supported in the target runtimes.)

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- tableParse`
Expected: PASS. Also confirm the existing `parseTable` tests still pass (the `cells` change is backward-compatible for unescaped input).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (122 existing + new tableParse tests). Fix format if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts
git commit -m "feat(editor): table model serialize + row/column ops + pipe-escaping"
```

---

## Task 2: Read-only table → enter edit on click

**Files:**
- Modify: `web/src/components/editor/widgets/tableWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/Editor.tsx`

This makes clicking a read-only table place the caret in its range (entering "touched" state). The editable widget itself comes in Task 3; after this task, clicking a table still reveals raw pipes (intermediate, acceptable).

- [ ] **Step 1: Update `TableWidget` to carry `from` + an enter-edit handler**

Replace `web/src/components/editor/widgets/tableWidget.ts` with:

```ts
import { WidgetType } from "@codemirror/view";
import { parseTable } from "../tableParse";

export class TableWidget extends WidgetType {
  constructor(
    readonly md: string,
    readonly from: number,
    readonly onEnterEdit: (from: number) => void,
  ) {
    super();
  }
  eq(other: TableWidget): boolean {
    return other.md === this.md && other.from === this.from;
  }
  toDOM(): HTMLElement {
    const { header, rows } = parseTable(this.md);
    const table = document.createElement("table");
    table.className = "cm-lp-table";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    for (const h of header) {
      const th = document.createElement("th");
      th.textContent = h;
      hr.appendChild(th);
    }
    const tbody = table.createTBody();
    for (const r of rows) {
      const tr = tbody.insertRow();
      for (const c of r) {
        tr.insertCell().textContent = c;
      }
    }
    table.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEnterEdit(this.from);
    });
    return table;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
```

- [ ] **Step 2: Add `onEnterTableEdit` to options and pass it in the Table branch**

In `web/src/components/editor/livePreview.ts`:

Add to `LivePreviewOptions`:

```ts
  onEnterTableEdit: (from: number) => void;
```

Update the Table branch's read-only construction:

```ts
      } else if (name === "Table") {
        const start = state.doc.lineAt(from).from;
        const end = state.doc.lineAt(to).to;
        if (!selectionTouches(state, start, end)) {
          const md = state.doc.sliceString(start, end);
          decos.push(
            Decoration.replace({
              widget: new TableWidget(md, start, opts.onEnterTableEdit),
              block: true,
            }).range(start, end),
          );
        }
      }
```

- [ ] **Step 3: Supply `onEnterTableEdit` in `Editor.tsx`**

In the `livePreview({...})` options object in `web/src/components/Editor.tsx`, add:

```tsx
      onEnterTableEdit: (pos: number) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ selection: EditorSelection.cursor(pos) });
      },
```

- [ ] **Step 4: Update any test `opts` that construct `LivePreviewOptions`**

In `web/src/components/editor/livePreview.test.ts`, add to the shared `opts`:

```ts
  onEnterTableEdit: vi.fn(),
```

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS. (Typecheck enforces the new required option is provided everywhere.)

- [ ] **Step 6: Commit**

```bash
git add web/src/components/editor/widgets/tableWidget.ts web/src/components/editor/livePreview.ts web/src/components/Editor.tsx web/src/components/editor/livePreview.test.ts
git commit -m "feat(editor): click a read-only table to enter edit (caret into range)"
```

---

## Task 3: EditableTableWidget — editable cells + commit on focus-out

**Files:**
- Create: `web/src/components/editor/widgets/editableTableWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/Editor.tsx`
- Modify: `web/src/components/editor/livePreview.css`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Create the editable widget (cells only; controls come in Task 4)**

Create `web/src/components/editor/widgets/editableTableWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";
import { parseTable, serializeTable, type TableModel } from "../tableParse";

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

  toDOM(): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "cm-lp-table-edit";
    const table = document.createElement("table");
    table.className = "cm-lp-table editing";
    wrap.appendChild(table);
    this.render(table, parseTable(this.md));

    // Commit once when focus leaves the whole table (click away / Esc).
    wrap.addEventListener("focusout", (e) => {
      const next = e.relatedTarget as Node | null;
      if (next && wrap.contains(next)) return; // moving between cells
      const md = serializeTable(this.readModel(table));
      if (md !== this.md) this.onCommit(this.from, this.to, md);
    });

    // Focus the first cell on enter.
    requestAnimationFrame(() => {
      wrap.querySelector<HTMLElement>("th, td")?.focus();
    });
    return wrap;
  }

  /** Read the current model back out of the live DOM. Reads ONLY each cell's
   *  direct text nodes, so control buttons added inside cells (Task 4) are
   *  excluded from the committed text. */
  protected readModel(table: HTMLElement): TableModel {
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
    return { header, rows };
  }

  /** Build the table DOM from a model. Re-callable to re-render in place. */
  protected render(table: HTMLElement, model: TableModel): void {
    table.textContent = "";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    for (const h of model.header) {
      const th = document.createElement("th");
      th.contentEditable = "plaintext-only";
      th.textContent = h;
      hr.appendChild(th);
    }
    const tbody = table.createTBody();
    for (const row of model.rows) {
      const tr = tbody.insertRow();
      for (const c of row) {
        const td = tr.insertCell();
        td.contentEditable = "plaintext-only";
        td.textContent = c;
      }
    }
  }
}
```

- [ ] **Step 2: Render the editable widget when the table is touched**

In `web/src/components/editor/livePreview.ts`:

Add the import:

```ts
import { EditableTableWidget } from "./widgets/editableTableWidget";
```

Add to `LivePreviewOptions`:

```ts
  onCommitTable: (from: number, to: number, md: string) => void;
```

Change the Table branch so the touched case renders the editable widget (instead of revealing raw pipes):

```ts
      } else if (name === "Table") {
        const start = state.doc.lineAt(from).from;
        const end = state.doc.lineAt(to).to;
        const md = state.doc.sliceString(start, end);
        if (selectionTouches(state, start, end)) {
          decos.push(
            Decoration.replace({
              widget: new EditableTableWidget(
                md,
                start,
                end,
                opts.onCommitTable,
              ),
              block: true,
            }).range(start, end),
          );
        } else {
          decos.push(
            Decoration.replace({
              widget: new TableWidget(md, start, opts.onEnterTableEdit),
              block: true,
            }).range(start, end),
          );
        }
      }
```

- [ ] **Step 3: Supply `onCommitTable` in `Editor.tsx`**

In the `livePreview({...})` options in `web/src/components/Editor.tsx`, add:

```tsx
      onCommitTable: (from: number, to: number, md: string) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ changes: { from, to, insert: md } });
      },
```

- [ ] **Step 4: Update the test `opts`**

In `web/src/components/editor/livePreview.test.ts`, add to the shared `opts`:

```ts
  onCommitTable: vi.fn(),
```

- [ ] **Step 5: Style the editable table**

Append to `web/src/components/editor/livePreview.css`:

```css
.cm-lp-table-edit {
  position: relative;
  display: inline-block;
  padding: 2px 18px 18px 2px;
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
  color: #f1f1f4;
}
```

- [ ] **Step 6: Add the cell-edit e2e**

Append to `web/e2e/skeleton.spec.ts`:

```ts
test("table editor: click to edit a cell and commit on click-away", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "kitchensink.md" }).click();
  const content = page.locator(".cm-content");

  // Click the rendered table → it becomes editable.
  await page.locator(".cm-lp-table").first().click();
  const firstCell = page.locator(".cm-lp-table.editing th, .cm-lp-table.editing td").first();
  await expect(firstCell).toBeVisible();

  // Edit a body cell, then click away to commit.
  const cell = page.locator(".cm-lp-table.editing td").first();
  await cell.click();
  await page.keyboard.press("Control+A");
  await page.keyboard.type("X1");
  await page.getByText("Kitchen sink").click(); // click away
  // The committed value appears in the document source / re-rendered table.
  await expect(content).toContainText("X1");
});
```

(`contenteditable` cells don't support Playwright's `fill`; the click + `Control+A` + `keyboard.type` above replaces the cell text. If select-all behaves oddly in the cell, adjust the typing mechanics — but do NOT weaken the final `toContainText("X1")` commit assertion.)

- [ ] **Step 7: Run unit gate + e2e**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Then: `pnpm e2e`
Expected: unit green; e2e 6/6 (5 existing + new). If the contenteditable typing needs adjusting, fix the test mechanics (not the assertion). If clicking the table doesn't enter edit mode or the commit doesn't land, STOP and report (a real bug).

- [ ] **Step 8: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/src/components/editor/livePreview.ts web/src/components/Editor.tsx web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css web/e2e/skeleton.spec.ts
git commit -m "feat(editor): editable table cells with commit-on-blur"
```

---

## Task 4: Inline add/remove row & column controls

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`
- Modify: `web/src/components/editor/livePreview.css`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Add the controls + structural ops to the widget**

In `web/src/components/editor/widgets/editableTableWidget.ts`:

Add imports:

```ts
import {
  parseTable,
  serializeTable,
  addRow,
  removeRow,
  addColumn,
  removeColumn,
  type TableModel,
} from "../tableParse";
```

Extend `render(table, model)` so it also adds the controls. After building thead/tbody, add per-column delete buttons in each `th`, a per-row delete button in each row's first `td`, and append edge `+` controls to the wrapping `.cm-lp-table-edit` element. Replace the `render` method with:

```ts
  protected render(table: HTMLElement, model: TableModel): void {
    table.textContent = "";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    model.header.forEach((h, ci) => {
      const th = document.createElement("th");
      th.contentEditable = "plaintext-only";
      th.textContent = h;
      th.appendChild(
        this.ctl("cm-lp-col-del", "×", () =>
          this.apply(table, removeColumn(this.readModel(table), ci)),
        ),
      );
      hr.appendChild(th);
    });
    const tbody = table.createTBody();
    model.rows.forEach((row, ri) => {
      const tr = tbody.insertRow();
      row.forEach((c, ci) => {
        const td = tr.insertCell();
        td.contentEditable = "plaintext-only";
        td.textContent = c;
        if (ci === 0) {
          td.appendChild(
            this.ctl("cm-lp-row-del", "×", () =>
              this.apply(table, removeRow(this.readModel(table), ri)),
            ),
          );
        }
      });
    });
    // edge "+" controls live on the wrapper, positioned via CSS
    const wrap = table.parentElement!;
    wrap.querySelectorAll(".cm-lp-add-col, .cm-lp-add-row").forEach((n) => n.remove());
    wrap.appendChild(
      this.ctl("cm-lp-add-col", "+", () =>
        this.apply(table, addColumn(this.readModel(table))),
      ),
    );
    wrap.appendChild(
      this.ctl("cm-lp-add-row", "+", () =>
        this.apply(table, addRow(this.readModel(table))),
      ),
    );
  }

  /** A non-editable control button that doesn't steal the contenteditable caret. */
  private ctl(cls: string, label: string, onClick: () => void): HTMLElement {
    const b = document.createElement("button");
    b.className = cls;
    b.type = "button";
    b.textContent = label;
    b.contentEditable = "false";
    b.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus inside the table (no commit)
      onClick();
    });
    return b;
  }

  /** Apply a structural op: re-render in place, keep editing, refocus a cell. */
  private apply(table: HTMLElement, model: TableModel): void {
    this.render(table, model);
    requestAnimationFrame(() => {
      table.querySelector<HTMLElement>("th, td")?.focus();
    });
  }
```

Note: `apply` does NOT dispatch to CodeMirror — it mutates local DOM only. The single commit still happens on `focusout`. Because the control buttons `preventDefault` on `mousedown`, focus stays within `wrap`, so `focusout` does NOT fire during a structural op (no commit, no CM rebuild).

- [ ] **Step 2: Style the controls**

Append to `web/src/components/editor/livePreview.css`:

```css
.cm-lp-table-edit .cm-lp-add-col,
.cm-lp-table-edit .cm-lp-add-row,
.cm-lp-col-del,
.cm-lp-row-del {
  position: absolute;
  border: 1px dashed #6366f155;
  background: #6366f120;
  color: #6366f1;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  opacity: 0;
  transition: opacity 0.12s;
}
.cm-lp-table-edit:hover .cm-lp-add-col,
.cm-lp-table-edit:hover .cm-lp-add-row {
  opacity: 1;
}
.cm-lp-table-edit .cm-lp-add-col {
  top: 2px;
  right: 0;
  bottom: 18px;
  width: 14px;
}
.cm-lp-table-edit .cm-lp-add-row {
  left: 2px;
  right: 18px;
  bottom: 0;
  height: 14px;
}
.cm-lp-col-del {
  top: 2px;
  right: 2px;
  border: none;
  background: transparent;
  color: #f4a3a3;
}
.cm-lp-table.editing th { position: relative; }
.cm-lp-table.editing td:first-child { position: relative; }
.cm-lp-row-del {
  left: -16px;
  top: 4px;
  border: none;
  background: transparent;
  color: #f4a3a3;
}
.cm-lp-table.editing th:hover .cm-lp-col-del,
.cm-lp-table.editing tr:hover .cm-lp-row-del {
  opacity: 1;
}
```

(Positioning is approximate; refine during the visual check in Task 5. The load-bearing behavior is the ops + commit, which e2e covers.)

- [ ] **Step 3: Add structural-op e2e**

Append to `web/e2e/skeleton.spec.ts`:

```ts
test("table editor: add a row and a column, commit on click-away", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "kitchensink.md" }).click();

  await page.locator(".cm-lp-table").first().click();
  const rowsBefore = await page.locator(".cm-lp-table.editing tbody tr").count();

  await page.locator(".cm-lp-add-row").click();
  await expect(page.locator(".cm-lp-table.editing tbody tr")).toHaveCount(rowsBefore + 1);

  const colsBefore = await page.locator(".cm-lp-table.editing thead th").count();
  await page.locator(".cm-lp-add-col").click();
  await expect(page.locator(".cm-lp-table.editing thead th")).toHaveCount(colsBefore + 1);

  // Click away → commit; re-render read-only, then re-open to confirm persisted.
  await page.getByText("Kitchen sink").click();
  await page.locator(".cm-lp-table").first().click();
  await expect(page.locator(".cm-lp-table.editing tbody tr")).toHaveCount(rowsBefore + 1);
  await expect(page.locator(".cm-lp-table.editing thead th")).toHaveCount(colsBefore + 1);
});
```

(If `.cm-lp-add-row`/`.cm-lp-add-col` need a hover to be clickable, use `.click({ force: true })` or hover first — but keep the count assertions intact. If a structural op doesn't persist after commit, STOP and report.)

- [ ] **Step 4: Run unit gate + e2e**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Then: `pnpm e2e`
Expected: unit green; e2e 7/7.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/src/components/editor/livePreview.css web/e2e/skeleton.spec.ts
git commit -m "feat(editor): inline add/remove row & column controls in table editor"
```

---

## Task 5: Keyboard nav + final gate + visual check

**Files:**
- Modify: `web/src/components/editor/widgets/editableTableWidget.ts`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Add keyboard navigation to cells**

In `editableTableWidget.ts`, give each editable cell a keydown handler. In `render`, after creating each `th`/`td` (both are cells), attach navigation. Add a helper and call it for every cell:

```ts
  /** Tab / Shift-Tab between cells; Enter → cell below; Esc → leave (commit). */
  private cellKeys(table: HTMLElement, cell: HTMLElement): void {
    cell.addEventListener("keydown", (e) => {
      const cells = [...table.querySelectorAll<HTMLElement>("th, td")];
      const i = cells.indexOf(cell);
      const cols = table.querySelectorAll("thead th").length || 1;
      if (e.key === "Tab") {
        e.preventDefault();
        const next = cells[i + (e.shiftKey ? -1 : 1)];
        next?.focus();
      } else if (e.key === "Enter") {
        e.preventDefault();
        cells[i + cols]?.focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cell.blur(); // focus leaves the table → focusout commit
      }
    });
  }
```

Call `this.cellKeys(table, th)` for each header cell and `this.cellKeys(table, td)` for each body cell inside `render` (right after setting `contentEditable`).

- [ ] **Step 2: Add a keyboard e2e**

Append to `web/e2e/skeleton.spec.ts`:

```ts
test("table editor: Tab moves between cells", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "kitchensink.md" }).click();
  await page.locator(".cm-lp-table").first().click();
  const cells = page.locator(".cm-lp-table.editing th, .cm-lp-table.editing td");
  await cells.first().click();
  await page.keyboard.press("Tab");
  // focus advanced to the second cell
  await expect(cells.nth(1)).toBeFocused();
});
```

- [ ] **Step 3: Final full gate + build + e2e**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Then: `pnpm e2e`
Expected: all PASS; e2e 8/8.

- [ ] **Step 4: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop it. Report the app loads. (The human visually confirms the controls' positioning + edit flow, and may file refinements.)

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/widgets/editableTableWidget.ts web/e2e/skeleton.spec.ts
git commit -m "feat(editor): keyboard nav in table editor (Tab/Enter/Esc)"
```

---

## Notes for the executor

- **Why no dispatch during the edit session:** every cell edit and structural op stays in the widget's local DOM; the only CodeMirror `changes` dispatch is the single commit on `focusout`. This is what prevents CM from rebuilding the widget mid-edit and destroying `contenteditable` focus. Keep it that way — do NOT dispatch on cell input or per structural op.
- **Control buttons `preventDefault` on `mousedown`** so clicking them doesn't blur the table (which would commit + rebuild). They mutate local DOM and refocus a cell.
- **The Table widgets stay atomic** (they're widget-bearing decorations, included in the CE-A `atomic` set automatically) — no change needed there.
- **`focusout` commit guard:** only commit when `e.relatedTarget` is outside the wrapper (focus truly left the table), not when moving between cells/controls.
- **Alignment is dropped** on commit (normalized to left) — per spec scope; don't try to preserve `:--:`.
- **Known v1 limitations** (spec §6): first-cell focus on enter (not the clicked cell); single-line plaintext cells; no alignment/reorder/cell-merge. Don't implement these.
- **e2e contenteditable typing:** Playwright's `fill` may not work on `contenteditable`; use click + `Control+A` + `keyboard.type`. Keep the commit/persistence assertions strong.
