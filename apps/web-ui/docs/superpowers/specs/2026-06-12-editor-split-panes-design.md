# Editor split-panes — design

**Date:** 2026-06-12
**Branch:** `editor-split-panes`
**Status:** approved design → implementation plan next

## Goal

Add side-by-side editor split: a second pane with its own independent tab
state, so two notes (or two views of one note) sit next to each other. This is
the deferred Phase-5 follow-on to tabs/preview-pin. It generalises the store's
single tab group into an array of panes and renders them side by side, while
keeping routing, persistence, and every existing tab behaviour intact.

## Scope

**In:**

- Horizontal, **two-pane maximum** layout (left | right).
- Each pane has independent tab state (its own tabs + active note), reusing the
  existing `tabsModel` per pane.
- Shared note buffers: editing a note open in both panes edits one buffer
  (already how `openNotes` works).
- Creation paths: tree right-click "Open to the side", the strip's split icon
  (duplicate current note), and ⌘K commands. Plain tree click opens in the
  focused pane.
- A tree **context menu** (new): Open · Open to the side · Rename · Delete.
- Resizable, persisted divider; focus-follows-click with an accent ring;
  URL/backlinks/search bind to the focused pane.

**Out (deliberate, additive later):**

- Vertical (top/bottom) splits.
- More than two panes in the UI (the store model is an array, so N-way is a
  later UI-only change).
- Drag-to-split / drag a tab between panes (HTML5 DnD layer).
- Encoding the second pane in the URL (split is local view-state only).

## Locked decisions (brainstorm outcomes)

1. **Routing:** URL tracks only the **focused pane's** active note. The split
   layout and the non-focused pane live in the store and persist to
   `localStorage`. RouteSync keeps binding to a single `activePath` mirror — no
   routing rework.
2. **Two horizontal panes** for v1; store modelled as a pane **array** so
   vertical/N-way/DnD are additive.
3. **Choosing the note** for the other pane is explicit, from the tree
   ("Open to the side" / `⌘↵`). The split icon only **duplicates** the current
   note. A plain tree click targets the **focused** pane.
4. Context menu folds in **Rename** and **Delete** (wired to existing handlers)
   alongside Open / Open to the side.
5. Resizable + persisted divider ratio.

## Architecture

### Pure model — `components/tabs/paneModel.ts` (new)

`tabsModel.ts` already operates on a standalone `TabsState` ({ tabs, activePath
}) — it is already pane-shaped and stays **untouched**. A new pure module sits
above it for layout-level operations:

```ts
export interface PaneState extends TabsState {} // { tabs: Tab[]; activePath: string | null }

export interface PanesState {
  panes: PaneState[]; // length 1 (single) or 2 (split); designed for N
  activePane: number; // index into panes
}

splitPane(s, seedPath): PanesState     // append a 2nd pane seeded with seedPath, focus it; no-op if already 2
closePane(s, index): PanesState        // remove pane; clamp activePane; never < 1 pane
focusPane(s, index): PanesState        // set activePane (guarded)
// per-pane tab ops delegate to tabsModel on panes[activePane] (or a given index)
```

All branching/edge logic (clamping `activePane`, refusing to drop the last
pane, seeding) lives here and is unit-tested in isolation, mirroring how
`tabsModel` is tested today.

### Store — `store/store.ts`

Replace the top-level `tabs: Tab[]` with `panes: PaneState[]` + `activePane:
number` + `splitRatio: number`. **Keep** the derived focused-pane mirror
(`activePath`, `activeContents`, `dirty`, `saving`) exactly as today — this is
what RouteSync, the Editor body, autosave, backlinks, commit messages, and many
internal `get().activePath` reads consume, so they stay unchanged.

- `applyTabs(next, paneIndex = activePane)` writes a `TabsState` into a specific
  pane and refreshes the mirror **only when** `paneIndex === activePane`.
- Existing tab actions (`selectTab`, `closeTab`, `cycleTab`, `jumpToTab`,
  `pinTab`, `editBuffer`) operate on the **focused** pane.
- `openNote(path, opts?: { pane?: number })` — opens into the focused pane by
  default; `opts.pane` lets "open to the side" target the other pane.
- New actions:
  - `splitPane()` — `splitPane(state, focusedActivePath)`; the new pane is
    seeded with the focused note (duplicate). No-op if already split or no
    active note.
  - `openToSide(path)` — ensure a second pane exists (split if single), then
    `openNote(path, { pane: otherIndex })` and focus it.
  - `closePane(index?)` — flush+close semantics reuse existing per-note logic;
    drop the pane, focus survivor, refresh mirror + backlinks.
  - `focusPane(index)` — set `activePane`, refresh mirror, refresh backlinks
    (URL follows via the mirror → Lane B).
  - `setSplitRatio(r)` — clamp [0.2, 0.8], persist.
- `closeTab` on a pane that empties it: if it's the **second** pane, collapse to
  single (close the pane); the first/last pane stays even when empty (matches
  today's single-pane empty state).

### Persistence — `components/tabs/tabsPersistence.ts`

Extend the persisted shape, tolerant of the **old** single-group format:

```ts
interface PersistedPanes {
  panes: { pinned: string[]; activePath: string | null }[];
  activePane: number;
  ratio: number;
}
```

`loadTabs` reads the new shape; if it finds the legacy `{ pinned, activePath }`
it lifts it into `panes: [that]`, `activePane: 0`. Restore in `loadCairn`
iterates panes, reopening pinned notes (skipping any that no longer load),
collapsing to a single pane if the second restores empty.

### Routing — unchanged

`RouteSync`, `routeReconcile`, `routes` are **not modified**. They bind the URL
to the `activePath` mirror, which now reflects the focused pane. Focusing the
other pane changes the mirror → Lane B navigates (replace) → Lane A sees the URL
already matches → converges in one step. Deep links open into the focused pane.

### Components

- **`SplitContainer`** (new, in `components/editor/` or alongside EditorPane):
  reads `panes`, `activePane`, `splitRatio`; lays out 1 or 2 `EditorPaneView`s
  with a `Divider` between. Single-pane renders exactly as today.
- **`EditorPane` → per-pane view:** the current `EditorPane` body (TabStrip +
  Editor + search overlay + note spinner) becomes a view rendered **per pane**,
  parameterised by pane index. It reads that pane's `tabs`/`activePath` and the
  shared `openNotes[activePath]` buffer (not the top-level mirror, so each pane
  shows its own note). The graph view stays a single, full-width foreground
  (split applies to the editor only).
- **Focus:** a pane sets itself focused on mousedown / editor focus
  (`focusPane(i)`); focused pane gets the inset accent ring. `editBuffer`
  targets the focused pane's active note.
- **`TabStrip` actions slot:** add a trailing actions area (outside the
  scroll region): **⫷ split** (single-pane only) and **⊟ close pane** (when
  split). Icons inline SVG (no icon dep found in repo — confirm during impl).
- **`Divider`** (new): pointer-drag updates `splitRatio` (store, persisted);
  `col-resize` cursor; keyboard-accessible (←/→ nudge) with an ARIA separator
  role.
- **`TreeContextMenu`** (new, `components/tree/`): positioned menu with Open ·
  Open to the side · Rename · Delete. Opened via `onContextMenu` on a tree
  note row. Rename/Delete call the **existing** tree handlers
  (`setEditingPath`, `onDelete`); Open → `onOpen`; Open to the side →
  `onOpenToSide`. Keyboard a11y (Esc to close, arrow nav, focus trap); closes on
  outside click / scroll. `FolderTreeView` gains `onOpenToSide` prop;
  `Sidebar` wires it to `actions.openToSide`.

### Commands — `components/shortcuts/commands.ts` + `useCommands.ts`

Add to `COMMAND_DEFS` (default chords picked during impl, user-rebindable as all
are): `split-right` → `splitPane()` (duplicate focused note); `close-pane` →
`closePane()`. The ⌘K palette and global key dispatch pick these up
automatically via the existing registry.

**Open to the side is a tree-only gesture** (right-click menu item, plus `⌘↵`
when a tree node is focused) — not a palette command, since the palette has no
note argument / persistent tree selection to target. This keeps "choose the
note" explicit and unambiguous; the palette path for layout is `split-right`
(duplicate).

## Data-flow walkthroughs

- **Open to the side:** right-click `spec` → menu → "Open to the side" →
  `openToSide('spec')` → split if single, `openNote('spec', { pane: 1 })`,
  `focusPane(1)` → mirror = `spec` → Lane B navigates to `/note/spec`.
- **Focus switch:** click left pane → `focusPane(0)` → mirror = pane0's note →
  backlinks refresh, URL reconciles.
- **Edit in a pane:** type in pane → editor focus set `focusPane(i)` →
  `editBuffer` writes `openNotes[pane_i.activePath]`; if that note is also open
  in the other pane, both render the same buffer.
- **Restore:** `loadCairn` reads `PersistedPanes`, reopens each pane's pinned
  notes, restores `activePane` + `ratio`; `ready` flips after, so a deep link
  still wins for the focused pane.

## Error handling / edge cases

- Closing the focused pane focuses the survivor; never fewer than one pane.
- A note open in both panes, closed in one: buffer stays (other pane still
  references it); existing `closeTab` flush-before-drop applies per pane.
- Rename/delete of a note open in either pane: existing `applyRenames` /
  `deleteNote` path-remap extended to map across **all** panes' tabs.
- Same note active in both panes: one shared buffer; both dirty dots reflect it.
- Restore where the second pane's notes all vanished: collapse to single pane.

## Testing (TDD; tests are part of done)

- **`paneModel.test.ts`** — pure: split/close/focus, activePane clamping,
  refuse-last-pane, seed behaviour.
- **`tabsPersistence.test.ts`** — new shape round-trip + **legacy-format
  migration** + drop-missing-paths + collapse-empty-second-pane.
- **store tests** — `splitPane`/`openToSide`/`closePane`/`focusPane`/
  `setSplitRatio`; mirror stays correct as focus moves; rename/delete remap
  across both panes; closeTab collapsing the second pane.
- **Component tests** — `TreeContextMenu` (items + keyboard + outside-click),
  TabStrip split/close actions, Divider drag→ratio, per-pane render shows the
  right note, focus ring follows click.
- **Routing** — existing `RouteSync`/`routeReconcile` tests must stay green
  (proves the mirror approach didn't disturb URL binding); add a focus-switch →
  URL-follows case.

## Files touched

New: `components/tabs/paneModel.ts` (+test), `components/editor/SplitContainer`
or split of `EditorPane`, `components/editor/Divider`,
`components/tree/TreeContextMenu`.

Modified: `store/store.ts`, `components/tabs/tabsPersistence.ts`,
`components/EditorPane.tsx`, `components/tree/FolderTreeView.tsx`,
`components/Sidebar.tsx`, `components/tabs/TabStrip.tsx`,
`components/shortcuts/commands.ts`, `app/useCommands.ts`. Store + coordinator
are the hot shared files — edits kept surgical, mirror preserved.

## Integration / merge

Full `just` gate. PR `--base main`. Merge via the merge queue after task **B**,
before task **D**, so D rebases onto these shell changes.
