# Cairn Web UI — Folder-Tree Sidebar Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 5 (Shell polish) — replace the flat note list with a
collapsible **folder tree** in the left sidebar.
**Builds on:** the existing `NoteList` slot, the `notePaths` store selector, the
`createNote(path)` action (already accepts nested paths), `stem()` (`wikilink.ts`),
and the localStorage persistence pattern (`tabsPersistence.ts` / `forceSettings.ts`).

---

## 1. Purpose

Group the flat note list into a **collapsible folder tree** keyed on `/` in note
paths, so larger vaults are navigable. Note paths already support subfolders
(`notes/ideas.md`) and the backend creates intermediate folders implicitly on
`write_note`, so this is a **pure-frontend** feature: build a tree from
`notePaths`, render it recursively, persist expand/collapse, and add a per-folder
"new note here" affordance.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Scope | **B** — read-only tree (open/delete/global new) **plus** a per-folder hover **+** that opens the New-note dialog pre-filled with that folder's path. |
| Labels | Leaves show the **filename stem** (`notes/ideas.md` → `ideas`); folders show the segment name; `title=` tooltip shows the full path. |
| Sort | Within each folder: **folders first, then notes**, each alphabetical (case-insensitive). |
| Toggle | Clicking anywhere on a folder row expands/collapses it (▸/▾). |
| Default expand | First-ever load: **all folders expanded**. Thereafter the **collapsed** set persists (`localStorage` `cairn.folderTree`); unknown/new folders default to expanded. |
| Auto-reveal | If the active note is inside collapsed folders, its **ancestors auto-expand** so it's always visible (e.g. a restored tab on load). |
| Open / delete | Click a note → `openNote` (unchanged); hover a note → `✕` → `deleteNote` (unchanged). |
| New note (global) | The existing top **"+ New note"** button → the existing dialog, empty (unchanged). |
| New note in folder | Hover a folder → **+** → the New-note dialog **pre-filled with `<folderPath>/`** (cursor at end). Same `createNote`. |
| Out of scope | Drag-to-move notes between folders, rename/delete folders, manual reorder — **the engine has no move/rename command** (a later cycle if it gains one). No explicit "create empty folder" (folders are implied by note paths). |

### Non-goals (deferred)

- Move/rename/drag (needs an engine command that doesn't exist yet).
- Multi-select, cut/paste, folder context menus.
- Showing note `title` instead of filename (Obsidian-style filename/stem is intended).
- Virtualized rendering for huge trees (YAGNI at current scale).

---

## 2. Architecture

Pure tree-building + a recursive component + a persistence module + thin wiring.
**No store / contract / backend changes.** `NoteList` is replaced by `FolderTree`.

```
web/src/components/tree/folderTree.ts            NEW (pure) — buildTree(paths) + ancestorFolders(path).
web/src/components/tree/folderTree.test.ts       NEW.
web/src/components/tree/treePersistence.ts       NEW (pure) — load/save the collapsed-folder set (localStorage cairn.folderTree).
web/src/components/tree/treePersistence.test.ts  NEW.
web/src/components/tree/FolderTree.tsx           NEW — recursive render; owns expand state; header + "+ New note".
web/src/components/tree/FolderTree.test.tsx      NEW.
web/src/components/NewNoteDialog.tsx             MODIFY — optional initialPath to pre-fill the input on open.
web/src/app/App.tsx                              MODIFY — render <FolderTree> in the list slot; track the new-note initial path.
web/src/components/NoteList.tsx (+ .test)        DELETE — superseded by FolderTree.
web/src/client/fixtures.ts                       MODIFY — add one nested fixture note so the tree is exercised in dev + e2e.
web/e2e/skeleton.spec.ts                         MODIFY — migrate sidebar selectors (stem labels) + a folder-tree test.
```

### 2.1 `folderTree.ts` (pure)

```ts
export type TreeNode =
  | { kind: "folder"; name: string; path: string; children: TreeNode[] }
  | { kind: "note"; name: string; path: string };

// Split each path on "/", merge into a tree, sort each level folders-first then
// alpha (case-insensitive). `name` = last segment (stem for notes, segment for
// folders); folder `path` = the slash-joined prefix (e.g. "a/b").
export function buildTree(paths: string[]): TreeNode[];

// Folder paths enclosing a note path, outermost→innermost:
// "a/b/c.md" → ["a", "a/b"]. Root note "x.md" → []. For auto-reveal.
export function ancestorFolders(path: string): string[];
```

### 2.2 `treePersistence.ts` (pure)

Mirrors `tabsPersistence.ts`: guarded `localStorage` under `cairn.folderTree`.
Persists the **collapsed** folder paths (so any folder not in the set — including
brand-new ones — defaults to expanded).

```ts
export function loadCollapsed(): Set<string>;     // [] / parse error → empty set
export function saveCollapsed(collapsed: Set<string>): void;
```

### 2.3 `FolderTree.tsx`

`props: { paths: string[]; activePath: string | null; onOpen(path); onDelete(path); onRequestNew(); onRequestNewInFolder(folderPath) }`.

- `const tree = useMemo(() => buildTree(paths), [paths])`.
- Owns `collapsed: Set<string>` state, seeded from `loadCollapsed()`; toggling a
  folder updates it and calls `saveCollapsed`.
- A folder is shown collapsed iff it's in `collapsed`.
- **Reveal-on-change:** a `useEffect` on `activePath` removes
  `ancestorFolders(activePath)` from `collapsed` (and persists) so a newly-opened or
  restored-on-load note is revealed. This runs only when `activePath` changes, so the
  user can freely re-collapse the active note's folder afterward (it won't snap back).
- Renders the header (`SectionLabel` "Notes" + the "+ New note" `Button` →
  `onRequestNew`), then the tree recursively. **Folder row:** disclosure ▸/▾ +
  name; whole row toggles; hover shows a `+` → `onRequestNewInFolder(folder.path)`.
  **Note row:** click → `onOpen(path)`; hover `✕` (aria-label `delete <path>`, full
  path) → `onDelete(path)`. Active note highlighted (`bg-surface-2`). Indentation by
  depth. Note rows keep `role="button"`; their accessible name is the **stem**.

### 2.4 `NewNoteDialog.tsx` change

Add `initialPath?: string` (default `""`). Seed the input from it and, via a
`useEffect` on `open`, set the field to `initialPath` whenever the dialog opens
(so the folder `+` pre-fills `<folder>/` and the global button opens empty). Reset
on close stays.

### 2.5 `App.tsx` wiring

- Replace `<NoteList .../>` with `<FolderTree paths={notePaths} activePath={activePath} onOpen={actions.openNote} onDelete={actions.deleteNote} onRequestNew={() => { setNewNoteInitial(""); setNewNoteOpen(true); }} onRequestNewInFolder={(f) => { setNewNoteInitial(f + "/"); setNewNoteOpen(true); }} />`.
- Add `const [newNoteInitial, setNewNoteInitial] = useState("")`; pass
  `initialPath={newNoteInitial}` to `<NewNoteDialog>`.
- Delete `NoteList.tsx` + its test; drop the import.

---

## 3. Testing

- **Unit (Vitest):**
  - `folderTree`: `buildTree` groups a root+nested mix into the right shape; sorts
    folders-first then alpha; deep nesting (`a/b/c.md`); leaf `name` is the stem.
    `ancestorFolders` for root, one-deep, multi-deep.
  - `treePersistence`: round-trips the collapsed set; empty/malformed storage → empty set.
  - `FolderTree` (Testing Library): renders nested notes under their folder;
    clicking a folder row collapses (hides children) and re-expands; clicking a note
    fires `onOpen`; folder hover `+` fires `onRequestNewInFolder(folderPath)`; the
    `✕` fires `onDelete` (full path) and not `onOpen`; setting `activePath` to a note
    inside a collapsed folder expands its ancestors so the note becomes visible
    (reveal-on-change).
- **e2e (Playwright):** with a nested fixture note (e.g. `projects/demo.md`): the
  `projects` folder shows; `demo` renders under it; collapsing `projects` hides
  `demo`; the folder `+` opens the dialog pre-filled with `projects/`. Keep all
  existing e2e green.
- **e2e migration (important):** the sidebar now shows **stems** (`ideas`) where the
  old list showed full paths (`ideas.md`). Existing tests that target the sidebar by
  `"ideas.md"` / `"index.md"` / `"todo.md"` (note-list clicks, the "app loaded"
  text check) must switch to the stem (`"ideas"` etc.). Backlinks and search results
  are separate components that still render full paths — leave those selectors
  alone. After the change, all existing e2e stay green.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/tree/folderTree.ts` (+test) | **New.** Pure tree build + ancestors. |
| `web/src/components/tree/treePersistence.ts` (+test) | **New.** Collapsed-set localStorage. |
| `web/src/components/tree/FolderTree.tsx` (+test) | **New.** Recursive tree component. |
| `web/src/components/NewNoteDialog.tsx` | **Modify.** `initialPath` prop. |
| `web/src/app/App.tsx` | **Modify.** Render FolderTree; new-note initial path. |
| `web/src/components/NoteList.tsx` (+test) | **Delete.** Superseded. |
| `web/src/client/fixtures.ts` | **Modify.** Add one nested note. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Stem-selector migration + folder-tree test. |

No new npm dependencies. No store/contract/backend changes.

---

## 5. Risks

- **e2e selector migration is the main risk.** Replacing `NoteList` (full-path
  labels) with `FolderTree` (stem labels) breaks every existing e2e selector that
  clicks the sidebar by `"<name>.md"`. Mitigation: the App-wiring task explicitly
  migrates those to stems and re-runs the full e2e suite; the `✕` keeps a full-path
  aria-label so delete selectors (if any) are stable; backlinks/search selectors are
  untouched (different components, still full-path).
- **Stem collisions across folders.** Two notes `a/note.md` and `b/note.md` both
  show `note`. Fine in the tree (different folders/rows); only a concern for flat
  `getByText` in tests — scope tree queries by folder/role where ambiguous.
- **Reveal-on-change, not always-expanded.** Reveal must fire only when `activePath`
  *changes* (a `useEffect` dependency on it), expanding the active note's ancestors
  once. If instead the active note's ancestors were forced expanded on every render,
  the user could never collapse the folder holding the active note (it would snap
  back open). Reveal-on-change persists the expansion (matching "reveal active file")
  while leaving the user free to re-collapse.
- **NewNoteDialog initialPath reset.** The field must re-seed to `initialPath` each
  time the dialog opens (a `useEffect` on `open`), or a stale value from a prior open
  leaks. The global button passes `""`; the folder `+` passes `<folder>/`.
- **Fixture addition.** Adding a nested fixture note must not break existing e2e
  (backlinks/search assertions). Use a self-contained note with no wikilinks to
  others (e.g. `projects/demo.md`).
- **Plain DOM, jsdom-safe.** The tree is plain DOM — fully unit-testable.
```
