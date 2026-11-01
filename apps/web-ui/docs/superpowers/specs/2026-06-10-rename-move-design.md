# Cairn Web UI — Rename / Move Notes & Folders Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Surface the engine's `RenameNote` to rename and move notes — and folders
(bulk) — directly in the folder tree (inline edit + drag-and-drop).
**Builds on:** the engine's `RenameNote { from, to }` (synced, link-rewriting), the
`FolderTree` sidebar, the multi-note store (`openNotes`/`tabs`/`activePath`), and the
`deleteNote`/`createNote` precedent.

---

## 1. Purpose

The engine's `RenameNote { from, to }` moves a note's file **and rewrites `[[wikilinks]]`**
that reference it, but the UI never exposes it. This adds **inline rename** (double-click a
name) and **drag-to-move** (drag onto a folder) in the tree, for **both notes and folders**.
Folders are implicit (path prefixes) and the engine has no folder command, so a folder
operation is a **bulk** sequence of per-note `RenameNote`s.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Rename trigger | **Inline** — double-click a note/folder name → an `<input>`; Enter commits, Esc cancels, blur commits; no-op on empty/unchanged. (No modal dialog.) |
| Move trigger | **Drag-and-drop** (native HTML5) — drag a note/folder row onto a folder row (or the "Notes" header = root) to move it. Drop target highlights. |
| Folders | First-class: rename a folder (bulk-rename all descendant notes) and move a folder (bulk-move the subtree). |
| Engine call | Every op is one or more `RenameNote { from, to }`. A note op = 1; a folder op = one per descendant note, issued **sequentially**. |
| Guards | Drop is a no-op (and not highlighted) when into the current parent, onto itself, or a folder into its own descendant. |
| Errors | Sequential; on the first failing `RenameNote` (e.g. target exists), **stop and surface the error** — earlier renames in the batch already applied (no transaction). |
| Open-note follow | The path-keyed `openNotes`/`tabs`/`activePath` are remapped from→to as each rename applies, so an open tab follows its note to the new path. |
| Out of scope | Undo, multi-select, atomic folder rename (needs an engine command), a modal RenameDialog, drag-to-reorder within a folder. |

---

## 2. Architecture

A pure path-planner (the logic), a store action that executes a rename list, and the tree
interaction (inline-edit + DnD). **No engine/contract change.**

```
web/src/components/tree/treeMoves.ts (+test)        NEW (pure) — plan rename/move → Rename[]; drop guards.
web/src/client/mock.ts (+test)                      MODIFY — `rename_note`: move + rewrite [[wikilinks]] + emit events.
web/src/store/store.ts (+test)                      MODIFY — `applyRenames(ops)`: sequential RenameNote + path-keyed remap.
web/src/components/tree/FolderTreeView.tsx (+test)  MODIFY — inline rename (dbl-click) + HTML5 drag/drop → onApplyRenames.
web/src/app/App.tsx                                 MODIFY — wire onApplyRenames.
web/e2e/skeleton.spec.ts                            MODIFY — rename + move e2e.
```

### 2.1 `treeMoves.ts` (pure)

```ts
export interface Rename { from: string; to: string }

// Same folder, new filename stem (no slash / .md added). [] if unchanged.
export function planRenameNote(notePath: string, newName: string): Rename[];
// Replace the folder's last segment; one Rename per descendant note. [] if unchanged.
export function planRenameFolder(folderPath: string, newName: string, allPaths: string[]): Rename[];
// Move a note into destFolder ("" = root) → one Rename. [] if already there.
export function planMoveNote(notePath: string, destFolder: string): Rename[];
// Move a folder's subtree under destFolder → one Rename per descendant note. [] if no-op.
export function planMoveFolder(folderPath: string, destFolder: string, allPaths: string[]): Rename[];
// Whether a drop is allowed (not into current parent / itself / a folder's own descendant).
export function canDrop(draggedPath: string, isFolder: boolean, destFolder: string): boolean;
```
Helpers (local): `dirOf(path)`, `baseName(path)`. Folder plans iterate `allPaths` for notes
under `folderPath + "/"`. All planners drop `from === to` entries.

### 2.2 Store `applyRenames(ops: Rename[])`

```
for (const {from, to} of ops) {            // sequential
  await client.sendCommand({ type: "rename_note", from, to });   // may throw → set error, stop
  // remap path-keyed state from→to:
  //  openNotes:  move the buffer key from→to
  //  tabs:       any tab.path === from → to
  //  activePath: === from → to
}
persist(); if (active) refreshBacklinks();
```
`renameNote(from, to)` is just `applyRenames([{from, to}])` (for the palette/keyboard later,
not in this cycle). The engine also emits `note_deleted(from)`+`note_changed(to)` which refresh
notePaths/backlinks/graph/tags via the existing subscribe handler — that does NOT touch
tabs/openNotes, so the explicit remap above is the mechanism that keeps an open tab attached.

### 2.3 Mock `rename_note`

In `MockClient.sendCommand`, add a `rename_note` case: error `not_found` if `from` absent,
`invalid_request` if `to` exists; else move the entry (`notes.delete(from)` → `notes.set(to, body)`);
if `stem(from) !== stem(to)`, rewrite `[[stem(from)]]` → `[[stem(to)]]` in every other note
(reuse `extractLinks`/`stem`); emit `note_deleted(from)`, `note_changed(to)` (+ `note_changed`
per rewritten note) + `reindexed`. Return `{type:"done"}`.

### 2.4 `FolderTreeView`

- **Inline rename:** `editingPath` state. Double-click a note/folder name → render an `<input>`
  (autofocus, selected) instead of the label. Enter / blur → compute ops
  (`planRenameNote` for a note, `planRenameFolder(…, props.paths)` for a folder) and
  `props.onApplyRenames(ops)`; Esc → cancel. The input's keydown stops propagation so global
  shortcuts don't fire.
- **Drag-to-move:** rows are `draggable`; `onDragStart` records `{path, isFolder}` (a ref).
  Folder rows + the "Notes" header are drop targets: `onDragOver` `preventDefault()` +
  highlight only when `canDrop(dragged, isFolder, destFolder)`; `onDrop` computes ops
  (`planMoveNote`/`planMoveFolder`, dest = folder path or `""` for the header) → `onApplyRenames`.
- New prop: `onApplyRenames(ops: Rename[]) => void`. (Keeps existing `onOpen`/`onDelete`/etc.)

### 2.5 App

Pass `onApplyRenames={actions.applyRenames}` to `<FolderTree>`.

---

## 3. Testing

- **Unit (Vitest):**
  - `treeMoves`: `planRenameNote` (new path, no-op when same); `planRenameFolder` (bulk over
    nested descendants, parent preserved); `planMoveNote` (into folder / to root / no-op when
    already there); `planMoveFolder` (subtree bulk, basename preserved); `canDrop` (blocks
    current-parent, self, descendant; allows a real move).
  - Mock `rename_note`: moves the note; rewrites `[[old]]`→`[[new]]` when the stem changed (not
    when only the folder changed); errors on missing source / existing target; emits the events.
  - Store `applyRenames`: single rename remaps an open tab's path + `activePath` + `openNotes`;
    a bulk (folder) op remaps several; a mid-batch failure sets `error` and stops (no further commands).
  - `FolderTreeView` (Testing Library): double-click a note name → input appears → Enter calls
    `onApplyRenames` with `planRenameNote`'s ops; double-click a folder → bulk ops; a simulated
    drop of a note onto a folder calls `onApplyRenames` with the move ops; a drop into the
    dragged folder's own subtree does nothing. (jsdom HTML5-DnD is limited — drive the
    `onDrop`/`onDragStart` handlers directly with synthetic events.)
- **e2e (Playwright):** double-click a note in the tree, type a new name, Enter → the note
  appears under its new name (and an open tab, if any, follows). A drag-move e2e if HTML5 DnD
  is reliable in Playwright; otherwise cover move via the unit/component tests and keep the e2e
  to inline rename. Keep all existing e2e green.
- All existing unit + e2e stay green.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/tree/treeMoves.ts` (+test) | **New.** Pure rename/move planners + `canDrop`. |
| `web/src/client/mock.ts` (+test) | **Modify.** `rename_note` (move + link rewrite + events). |
| `web/src/store/store.ts` (+test) | **Modify.** `applyRenames` action + path-keyed remap. |
| `web/src/components/tree/FolderTreeView.tsx` (+test) | **Modify.** Inline rename + drag/drop. |
| `web/src/app/App.tsx` | **Modify.** Wire `onApplyRenames`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Rename (+ move) e2e. |

No new npm dependencies (native HTML5 DnD). No engine/contract change.

---

## 5. Risks

- **Path-keyed state remap (the crux).** A rename changes a note's path key; `applyRenames`
  must move `openNotes[from]→[to]`, repoint `tabs`, and update `activePath` for each op, or an
  open tab dangles. The subscribe event handler refreshes derived data only (notePaths/backlinks/
  graph) and never touches tabs/openNotes, so the explicit remap is required. Covered by store tests.
- **Folder ops are non-atomic.** A folder rename/move is N sequential `RenameNote`s; a mid-batch
  failure (e.g. a target collision) leaves a partial move. Mitigation: stop on first error and
  surface it; the engine has no folder command, so this is inherent — documented.
- **Drag-and-drop in jsdom/Playwright.** Native HTML5 DnD is poorly simulated in jsdom and flaky
  in Playwright. Mitigation: unit-test the pure planners + drive the component's drop handlers
  with synthetic events; keep the e2e centered on inline rename (DnD verified manually). The
  drop logic's correctness lives in `treeMoves` (pure) + the handler wiring, both unit-tested.
- **Inline-edit vs click/toggle.** Double-clicking a name also fires the single-click
  (open note / toggle folder) first — harmless (it opens/toggles, then edits). The edit input
  must `stopPropagation` on keydown so typing/Esc doesn't trigger global shortcuts or toggle.
- **Wikilink rewrite scope.** The engine rewrites links only when the **stem** changes; moving a
  note between folders keeps the stem, so `[[ideas]]` stays valid without rewrite. The mock must
  mirror this (rewrite only on stem change) to stay faithful.
- **`canDrop` guards.** Must block dropping a folder into itself or a descendant (else an
  infinite/garbage path), and treat current-parent / same-location as no-ops (no command sent).
- **Plain logic is jsdom-safe.** `treeMoves` is pure; the tree is plain DOM.
