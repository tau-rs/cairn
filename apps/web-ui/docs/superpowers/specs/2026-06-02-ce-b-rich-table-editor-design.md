# Cairn Web UI — CE‑B: Rich Table Editor Design Spec

**Date:** 2026-06-02
**Status:** approved, ready for implementation planning
**Sub-project:** CE‑B of the click‑to‑edit initiative (follows CE‑A, which made
blockquotes/code/images click-to-edit). Final piece of the editor rework.
**Builds on:** the UI‑3 live-preview pipeline (`livePreview.ts` StateField +
widgets), the existing read-only `TableWidget`/`parseTable`, and the graphite
design system.

---

## 1. Purpose

Make GFM tables editable in place. Today a rendered table is read-only (an atomic
block widget); to change it you drop the caret in and edit raw pipe text. CE‑B
replaces that with an **interactive grid**: click a table to enter edit mode, edit
cells directly, add/remove rows and columns with inline controls, and click away
to commit the changes back to the markdown source.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Edit model | **Read-only until clicked → edit mode** (model B). Click a table → editable grid; click away → commit + re-render read-only. Tables never show raw pipes inline (raw is only via the global Source toggle). |
| Operations | **Edit cell text; add/remove rows; add/remove columns.** |
| Controls | **Inline edge controls (Notion-style):** right-edge `+` add column, bottom-edge `+` add row, per-column `×` (header), per-row `×` (row hover). |
| Alignment | **Out of scope** — editing normalizes a table to left-aligned (any `:--:` markers are dropped). Known limitation (§6). |
| Commit timing | **Commit once on focus-out** of the whole table (one transaction = one undo step). No per-keystroke dispatch. |
| Implementation | Interactive CodeMirror block widget owning local DOM/state during the session (Approach 1). |

### Non-goals (deferred)

- Column alignment editing/preservation.
- Row/column drag-reorder; cell merging; multi-cell selection.
- Rich text inside cells (cells are single-line plain text).
- Markdown rendering *inside* cells (e.g. `**bold**` in a cell renders raw).

---

## 2. Architecture

Contained to the editor. **No store/host/contract changes.** The read-only table
becomes a read-only ⇄ editable pair, gated by the existing reveal-on-cursor logic.

```
components/editor/tableParse.ts                  EXTEND — keep parseTable; add serializeTable(model)
                                                 + pure model ops addRow/removeRow/addColumn/removeColumn;
                                                 pipe-escape (cell "a|b" ⇄ "a\|b"); single-line cells.
components/editor/widgets/tableWidget.ts         MODIFY — read-only render + a mousedown handler that
                                                 enters edit mode (calls opts.onEnterTableEdit(pos)).
components/editor/widgets/editableTableWidget.ts NEW — the interactive grid: contenteditable cells,
                                                 inline edge controls, local-DOM editing, commit on
                                                 focus-out via opts.onCommitTable. ignoreEvent()→true.
components/editor/livePreview.ts                 MODIFY — Table branch: touched → EditableTableWidget,
                                                 untouched → read-only TableWidget. LivePreviewOptions
                                                 gains onEnterTableEdit(pos) and onCommitTable(from,to,md).
components/Editor.tsx                            MODIFY — supply onEnterTableEdit (dispatch caret) and
                                                 onCommitTable (dispatch changes) via viewRef.
components/editor/livePreview.css                MODIFY — editable-table + edge-control styles.
e2e/skeleton.spec.ts                             MODIFY — edit-a-table interaction assertions.
```

### Data model (pure, fully unit-testable)

```
interface TableModel { header: string[]; rows: string[][] }

parseTable(md): TableModel            // existing; extend to unescape \| → |
serializeTable(model): string         // GFM: | h | h | / | --- | --- | / | c | c | ; escapes | → \|
addRow(model, atIndex?): TableModel       // append a blank row (cols = header length)
removeRow(model, index): TableModel       // guard: keep ≥1 body row
addColumn(model, atIndex?): TableModel    // append a blank column to header + every row
removeColumn(model, index): TableModel    // guard: keep ≥1 column
```

- Alignment is normalized to left: the delimiter row is always `---`.
- Cell text is single-line; pipes are escaped on serialize and unescaped on parse.
- All ops are pure (no DOM) and form the bulk of the tested logic.

### Lifecycle (reveal-on-cursor, swapping raw-pipes for the edit grid)

- **Resting** (caret not in the table range): read-only `TableWidget` (today's render).
- **Enter:** the read-only widget's `mousedown` → `opts.onEnterTableEdit(pos)` dispatches a caret into the table range. `selectionTouches(tableRange)` flips true, so the Table branch renders the `EditableTableWidget` (NOT raw pipes). The first cell is focused on enter (focusing the exact clicked cell is deferred — see §7).
- **Edit:** all editing is local to the widget's DOM/model — typing in `contenteditable` cells, and add/remove row/column via the inline controls mutate the widget's own DOM/model and re-render its subtree. **No CodeMirror dispatch occurs during the session**, so the widget is never rebuilt mid-edit and focus is preserved.
- **Commit:** on `focusout` of the whole table (focus moves outside the table DOM), the widget serializes its model → markdown and calls `opts.onCommitTable(from, to, md)`, which dispatches one `changes` transaction. The caret has left the range → the gating re-renders the read-only widget with the committed content. One transaction = one undo step.

`onCommitTable` must run synchronously on `focusout` (before the gating rebuild)
so the read-only render shows committed content without a stale flicker.

---

## 3. Editable widget & controls

`EditableTableWidget` renders `<table class="cm-lp-table editing">`:

- **Cells** (`th`/`td`): `contenteditable="plaintext-only"` — single-line, no rich
  formatting/newlines; paste is sanitized (strip newlines). The widget reads cell
  text from the DOM at commit (or maintains the model on input).
- **Inline edge controls:**
  - **Right edge** — hover-revealed vertical `+` → `addColumn` (at end).
  - **Bottom edge** — hover-revealed horizontal `+` → `addRow` (at end).
  - **Per-column delete** — header cell shows `×` on hover → `removeColumn(i)`.
  - **Per-row delete** — row hover reveals a `×` handle at its left → `removeRow(i)`.
  - Guards: a table keeps ≥1 column and ≥1 body row (delete is a no-op/hidden at the minimum).
  - Structural ops mutate the local model and re-render the widget's own DOM subtree (no CM rebuild).
- **Keyboard:** `Tab`/`Shift-Tab` move between cells (wrap across rows); `Enter`
  moves to the cell directly below (literal newline prevented); `Esc` blurs the
  table → commit + exit to read-only.
- `ignoreEvent()` returns `true` so CodeMirror lets the widget handle its own
  events; the widget never lets a stray event move the CM selection out of range
  except via an explicit blur/click-away.

---

## 4. Wiring changes

- **`livePreview.ts`** Table branch:
  - `if (!touched)` → read-only `TableWidget(md, from, opts.onEnterTableEdit)`.
  - `else` → `EditableTableWidget(md, from, to, opts.onCommitTable)` (focuses its first cell on enter).
  - `LivePreviewOptions` gains `onEnterTableEdit: (pos: number) => void` and
    `onCommitTable: (from: number, to: number, md: string) => void`.
  - The Table widgets remain in the **atomic** set (widget decorations), unchanged
    from CE‑A — both read-only and editable are atomic block widgets.
- **`Editor.tsx`** supplies the callbacks via the existing `viewRef`:
  - `onEnterTableEdit: (pos) => view.dispatch({ selection: EditorSelection.cursor(pos) })`
  - `onCommitTable: (from, to, md) => view.dispatch({ changes: { from, to, insert: md } })`
  - Both read only the stable `viewRef` (not in the `extensions` memo deps), matching the existing `onToggleCheckbox`/`onEditImage` pattern.

---

## 5. Testing

- **Unit (Vitest, pure model):**
  - `serializeTable` emits valid GFM (header + `---` delimiter + body; consistent
    column counts); `parse∘serialize` round-trips a normalized table; pipe-escape
    round-trips (`a|b` ⇄ `a\|b`).
  - `addRow`/`removeRow`/`addColumn`/`removeColumn` produce the expected model and
    honor the ≥1-column / ≥1-row guards.
- **e2e (Playwright — interaction layer; `contenteditable`/focus/controls don't
  render under jsdom):** on `kitchensink.md`:
  - click the table → edit mode (a cell is `contenteditable`); edit a cell, click
    away → the committed text shows in the re-rendered read-only table;
  - bottom `+` adds a row (persists after commit); right `+` adds a column;
  - per-row `×` / per-column `×` delete; assert the read-only render + source reflect each change.
- All existing unit tests + e2e stay green; Tauri/desktop unaffected.

---

## 6. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/editor/tableParse.ts` (+ test) | **Extend.** `serializeTable` + model ops + pipe-escape. |
| `web/src/components/editor/widgets/tableWidget.ts` | **Modify.** Read-only + enter-edit mousedown. |
| `web/src/components/editor/widgets/editableTableWidget.ts` | **New.** Interactive grid + controls + commit. |
| `web/src/components/editor/livePreview.ts` (+ test) | **Modify.** Table branch swap + new options. |
| `web/src/components/Editor.tsx` | **Modify.** Supply `onEnterTableEdit` / `onCommitTable`. |
| `web/src/components/editor/livePreview.css` | **Modify.** Editable table + edge-control styles. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Edit-a-table interaction assertions. |

No new dependencies. No store/host/contract changes.

---

## 7. Risks

- **Commit/click-away ordering:** commit synchronously on `focusout` before the
  gating rebuild, so the read-only render shows committed content (e2e verifies;
  watch for a stale-render flicker).
- **`contenteditable` quirks:** `plaintext-only` support, paste/IME sanitization;
  keep cells strictly single-line plain text.
- **Mid-session rebuild = lost local edits:** won't happen during a focused session
  (no dispatch occurs); an external doc change would discard uncommitted edits —
  acceptable for single-user, noted.
- **Alignment stripping:** editing a table normalizes alignment to left — a known
  limitation per the chosen scope.
- **Stale source range:** safe because no dispatch occurs mid-session; the widget
  is rebuilt with fresh positions on any real doc change.
- **Empty/degenerate tables:** guard removal at ≥1 column / ≥1 body row; adding a
  column/row to a minimal table must produce valid GFM.
- **Focus on enter:** entering edit mode focuses the **first cell**. Focusing the
  exact clicked cell is deferred — it would require threading the clicked cell's
  coordinates through the caret dispatch to the freshly-built widget; not worth the
  complexity for v1.
