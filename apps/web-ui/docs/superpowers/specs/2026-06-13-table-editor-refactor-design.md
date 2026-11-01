# Table Editor Refactor — Design

**Date:** 2026-06-13
**Status:** Approved (design); pending implementation plan
**Area:** `web/src/components/editor` (live-preview table editing)

## 1. Problem

The click-to-edit Markdown table editor works but is unpleasant to use. Concretely
(confirmed pain points, in priority order):

1. **Append-only structure** — you can only tack a row/column onto the end (via thin
   14px hover strips). No insert mid-table, no reorder.
2. **Fiddly, hidden controls** — delete is a tiny hover-only `×`; add-strips are 14px
   slivers. Nothing is discoverable; everything depends on hover.
3. **No spreadsheet paste** — can't paste a block of cells from Excel/Sheets/Notion.
4. **Unpolished look/feel.**

A latent correctness bug also rides along: `serializeTable` normalizes every column to
left alignment (`---`), silently destroying `:--`/`:-:`/`--:` markers on any edit. We
fix that regardless of whether we expose an alignment *control* (we are not, this round).

## 2. Goals / Non-goals

**Goals**

- Insert / delete / **reorder** rows and columns at any position.
- Large, discoverable hit targets on desktop, touch, and keyboard.
- Paste a TSV block (spreadsheet clipboard) that spills across cells and auto-grows.
- Polished dark-theme refresh of the table chrome and controls.
- **Lossless** Markdown round-trip: preserve alignment markers and escaped pipes; emit
  prettified (column-padded) pipe tables.

**Non-goals (this round)**

- An alignment-control UI (left/center/right picker). We *preserve* alignment; we don't
  add a control. Deferred.
- Cell merging / spanning, multi-column sort, formulas, CSV import dialogs.
- Whole-table deletion via the editor chrome — keep the "≥1 row, ≥1 column" guard; remove
  a whole table by selecting its source block and deleting it.

## 3. Chosen interaction model

Validated against the mockups (`.superpowers/brainstorm/.../interaction-model*.html`,
`context-menu.html`, `mobile.html`) and the UX research brief. **Approach A
(Notion-style grips) as the spine, layered with a context menu and a mobile sheet** — the
"layered surfaces" pattern the research endorses.

### Desktop / fine pointer

- **Grips**: a six-dot handle on each row's left edge and each column's top edge,
  revealed on table/row/column hover.
- **Click a grip** → action menu (popover) anchored to it:
  - Row: Insert row above · Insert row below · Move (up/down) · **Delete row**
  - Column: Insert column left · Insert column right · Move (left/right) · **Delete column**
- **Drag a grip** → reorder that row/column, with a drop-position indicator line.
- **Edge `+`** buttons (larger than today's slivers) append a row/column.
- **Right-click any cell** → context menu mirroring the grip actions, grouped Row /
  Column, **plus Cut/Copy/Paste** (our menu suppresses the native one, so it must carry
  clipboard ops). Paste here does the TSV spill.

### Mobile / coarse pointer

- Grips are **always visible** (subtle), ~40px tap targets (no hover to reveal them).
- **Tap a grip** → **bottom sheet** (reuses the app's `Drawer`-style language) listing the
  same actions with large targets. Reorder is **Move up/down/left/right buttons** (drag is
  unreliable on touch).
- **Long-press a cell** → same sheet (the right-click equivalent).
- Wide tables **scroll horizontally**; a **keyboard accessory bar** (Prev / Next / Done)
  drives cell-to-cell movement while the on-screen keyboard is up.

### Keyboard (all platforms)

- In-cell: **Tab** / **Shift-Tab** move next/prev cell; **Enter** moves to the cell below;
  **Tab at the last cell** appends a row; **Esc** leaves the table (commit).
- Scoped insert/delete bindings while a cell is focused (e.g. Mod-Enter = insert row
  below), defined in the CodeMirror keymap — see §4.4.

## 4. Architecture

Four layers, dependencies pointing inward. The pure core holds all logic and is the test
surface; rendering is a thin imperative adapter consistent with the other live-preview
widgets (`bulletWidget`, `imageWidget`, etc.).

### 4.1 Pure core — `tableParse.ts` (extended, framework-free, fully unit-tested)

This is the domain. No DOM, no CodeMirror.

```
type Align = "none" | "left" | "center" | "right";
interface TableModel { header: string[]; rows: string[][]; align: Align[]; }
```

- `parseTable(md)` — **read the delimiter row** to populate `align` per column
  (`:--`→left, `:-:`→center, `--:`→right, `---`→none). Currently it drops this row.
- `serializeTable(m)` — **write alignment markers back**, and **prettify**: pad every
  column to its max display width so the raw Markdown stays readable (Advanced
  Tables / Prettier behavior). Never silently normalize alignment.
- Positional ops (all keep column-count consistent and honor the ≥1 guard):
  `insertRow(m, index)`, `insertColumn(m, index, align?)`,
  `moveRow(m, from, to)`, `moveColumn(m, from, to)`,
  `removeRow(m, index)`, `removeColumn(m, index)`.
  (Existing `addRow`/`addColumn` become `insertRow(m, rows.length)` etc.)
- Clipboard: `parseTSV(text): string[][]` (tab = column, newline = row) and
  `pasteBlock(m, atRow, atCol, block): TableModel` — spill across cells, auto-growing
  rows/columns as needed.
- Pipe handling: escape `\|` on serialize, unescape on parse (already present; keep and
  test against the new ops).

### 4.2 Source-transform spine — one code path for every structural op

A single pure-ish function maps an **op + target** to a document edit:

```
applyTableOp(view, op, target)
  // 1. locate the enclosing Table block range at target (cursor cell, or grip's row/col)
  // 2. parse → run the §4.1 op → serialize
  // 3. dispatch ONE replace over the block range  (= one undo step)
  // 4. set selection inside the table so it re-renders editable; mark the cell to refocus
```

Both the menus/sheet **and** any keyboard/⌘K command route through this. The grip/menu
path passes an explicit `{row}` / `{col}` target; the keyboard path derives the target
from the cursor's cell. Each op is its own undo step — better than today's
commit-on-blur-only model.

In-progress (uncommitted) typing is captured before the op: read the live cell text from
the DOM into the model, apply the op to *that*, then dispatch. Plain typing still stays
local until focus-out (unchanged); only structural ops dispatch immediately.

### 4.3 View adapter — `editableTableWidget.ts` (rewritten) + helpers

- Holds the live `TableModel`; renders `contenteditable` cells **plus** row/column grips.
- Delegates menu/sheet rendering to a shared `tableMenu.ts`: takes an **action list** and
  renders a **popover** on `pointer: fine`, a **bottom sheet** on `pointer: coarse`. One
  action list, two presentations — keeps desktop/mobile in sync.
- `tableDnd.ts`: grip drag-to-reorder with a drop indicator; on drop calls
  `moveRow`/`moveColumn` via the §4.2 spine.
- **Focus restoration**: because structural ops now dispatch and the widget re-mounts, the
  widget must, on mount during an active table edit, focus the intended cell. Tracked via
  a CodeMirror `StateEffect`/field carrying "editing table at X, focus cell (r,c)". This is
  the main implementation risk (see §8).
- `TableWidget.ts` (read-only): polish only.
- `livePreview.css`: refreshed chrome (rounded container, header tint, row hover, clear
  focus ring), grips, popover/menu, sheet, drag indicator, larger `+` affordances.

### 4.4 Keyboard / commands

- A **scoped CodeMirror keymap** active when the cursor is inside a table block handles
  Tab/Shift-Tab/Enter/Esc navigation and insert/delete bindings. This is the correct home
  for contextual ops — the existing `commands.ts` registry is global/app-wide and has no
  per-editor context.
- **⌘K palette entries are a stretch goal**, not core: they would require the global
  registry to gain editor-context awareness. Deferred unless cheap.

## 5. Accessibility

- The editable table uses the **ARIA Data Grid** pattern: `role="grid"`, `gridcell`,
  roving `tabindex` (exactly one cell tabbable), arrow-key + Home/End navigation within
  the grid, Tab/Shift-Tab to move in/out.
- Menus get `role="menu"`/`menuitem` with arrow-key navigation and Esc to dismiss; the
  sheet is a labeled dialog.
- **Focus lands sensibly after every mutation**: on the new row/column after insert; on the
  neighbor after delete; on the moved row/column after reorder.
- Grips are keyboard-reachable; their menu is operable without a pointer.

## 6. Markdown / correctness rules

- Round-trip alignment markers losslessly (the §1 bug fix).
- Prettify output: pad columns to equal width; delimiter row stays valid (≥3 dashes,
  alignment colons preserved).
- Escape literal `|` as `\|` on serialize; unescape on parse.
- Column counts stay consistent across every op.

## 7. Guards / edge cases

- Refuse to delete the **last** remaining row or column (existing guard, kept).
- Paste larger than the table → auto-grow. Paste smaller → fill from the anchor cell.
- Reorder to the same position → no-op (no spurious commit / undo entry).
- Move/insert/delete must not corrupt an in-progress cell edit (capture-then-apply, §4.2).

## 8. Risks

1. **Focus restoration across re-mount (highest).** Switching structural ops to
   dispatch-per-op (for proper undo) means the widget rebuilds and loses the
   contenteditable caret. Mitigation: a `StateEffect` carrying the table position + target
   cell, consumed on `toDOM`. If this proves too fragile, fall back to the current
   "local-DOM session, single commit on focus-out" model for structural ops (no per-op
   undo) — a known-good fallback.
2. **Drag-reorder inside a CodeMirror block widget** — pointer events vs. CM's own
   handling. Mitigation: `ignoreEvent()` already returns true for the editable widget;
   scope DnD to the grips only.
3. **Touch detection** — `pointer: coarse` may misfire on hybrid devices. Mitigation:
   the bottom sheet works on fine pointers too; degrade gracefully.

## 9. Testing

- **Pure core (`tableParse.test.ts`)**: alignment round-trip (all four markers), prettify
  padding, each positional op (incl. guards), `parseTSV`, `pasteBlock` spill/auto-grow,
  pipe escaping under ops. This is the bulk of the coverage and is straightforward.
- **`applyTableOp`**: block-location + single-dispatch + post-op selection, via the
  existing live-preview test harness.
- **View**: grip menu builds the right action list; popover-vs-sheet selection by pointer
  type; focus lands correctly after insert/delete/move (jsdom focus assertions).
- Manual pass on desktop + a real touch device for drag, sheet, and paste.

## 10. Out of scope / future

Alignment-control UI; ⌘K contextual commands; cell merge/span; sort; CSV import. Each can
build on the §4.1 core without rework.
