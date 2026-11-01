# Cairn Web UI — Command Palette Design Spec

**Date:** 2026-06-09
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 5 (Shell polish) — the command palette. First of the
shell-polish features (panes/tabs, themes, shortcuts are later, separate cycles).
**Builds on:** the UI‑1 design system + primitives, the existing store actions,
and the Radix dialog pattern (`Modal`/`SettingsDialog`).

---

## 1. Purpose

Add a **⌘K / Ctrl‑K command palette**: one overlay with a search input and a
unified, fuzzy-filtered list of **commands** and **notes** — run an action or
jump to any note from anywhere, keyboard-first.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Open | Global **⌘K (Mac) / Ctrl‑K**; Esc closes; clicking the backdrop closes. |
| Layout | A top-anchored overlay (Radix `Dialog`), ~520px: search input + grouped result list + footer hints. |
| Results | **Unified** list (no separate "files"/"commands" modes): commands + notes, fuzzy-filtered together. Empty query → commands + the first ~6 notes; typing fuzzy-filters both. |
| Commands (v1) | New note · Commit changes… · Toggle Graph/Editor · Open Settings · Toggle Source/Live‑preview. |
| Notes | Quick-open from `notePaths` → `openNote(path)` + switch to the editor view. |
| Keyboard | ↑/↓ move the highlight, ↵ runs/opens the highlighted item, Esc closes. |
| Fuzzy | Lightweight case-insensitive **subsequence** match + score (contiguous/early matches rank higher). |
| Dialog reuse | **Lift** the New-note + Commit dialogs to App so both the toolbar buttons and the palette open the same instances. |
| Out of scope | A full global-shortcut system (the inline ⌘‑hints in the mockup are illustrative); command history; plugin-contributed commands; multi-step palettes. |

### Non-goals (deferred)

- Per-command global shortcuts beyond ⌘K (separate "keyboard shortcuts" cycle).
- Recent-notes ordering (no recency is tracked; empty state shows the first N notes).
- Fuzzy highlight ranges in the result text (match is functional, not highlighted) — optional polish, not required.
- Command grouping/customization, plugin commands (Phase 6).

---

## 2. Architecture

Contained to the shell. **No store/host/contract changes** — the palette composes
existing actions. One small refactor: lifting two dialogs to App.

```
web/src/components/command-palette/fuzzy.ts            NEW (pure) — fuzzyScore + filterItems.
web/src/components/command-palette/fuzzy.test.ts       NEW.
web/src/components/command-palette/CommandPalette.tsx   NEW — Radix overlay + input + list + keyboard nav.
web/src/components/command-palette/CommandPalette.test.tsx  NEW.
web/src/app/App.tsx                                    MODIFY — paletteOpen + global ⌘K listener + command list + render palette; own New-note/Commit dialog state.
web/src/components/NoteList.tsx (+ test)               MODIFY — "+ New note" calls props.onRequestNewNote (dialog lifted to App).
web/src/components/CommitBar.tsx (+ test)              MODIFY — "Commit" calls props.onRequestCommit (dialog lifted to App).
web/e2e/skeleton.spec.ts                               MODIFY — ⌘K → quick-open a note; ⌘K → "comm" → commit dialog.
```

### `fuzzy.ts` (pure)

```ts
// Case-insensitive subsequence match. null = no match. Higher score = better
// (rewards contiguous runs and early/word-start matches).
fuzzyScore(query: string, text: string): number | null

// Score each item by its searchable text, drop non-matches, sort by score desc
// then label asc. Empty query → all items (caller decides any cap).
filterItems<T>(items: T[], query: string, text: (item: T) => string): T[]
```

- Empty query → `fuzzyScore` returns a neutral score (so `filterItems` returns all
  items in their text order / stable). All matching is on `query.toLowerCase()` vs
  `text.toLowerCase()`.

### `CommandPalette.tsx`

`props: { open: boolean; onClose: () => void; commands: PaletteCommand[]; notes: string[]; onRunCommand: (id: string) => void; onOpenNote: (path: string) => void }`
where `PaletteCommand = { id: string; label: string; icon?: ReactNode }`.

- Radix `Dialog.Root open onOpenChange→onClose` → `Dialog.Portal` → `Dialog.Overlay`
  (dimmed) + a top-anchored `Dialog.Content` (~520px, graphite). An autofocused
  text `<input>` (the query) + a scrollable result list + a footer (↑↓ / ↵ / esc).
- Builds a unified item list: each command → `{ kind:"command", id, label }`; each
  note → `{ kind:"note", id:path, label: stem(path), path }`. `filterItems` over
  the label (notes also match on path). Empty query → commands + the first 6 notes;
  non-empty → all matches (commands + notes) ranked by score.
- Renders results grouped under "Commands" / "Notes" headers (a group is omitted
  when it has no results). One row per item; the highlighted row is styled.
- Keyboard: `↑`/`↓` move the highlight (clamped, wraps optional), `Enter` runs the
  highlighted item (`onRunCommand(id)` or `onOpenNote(path)`) then `onClose`, `Esc`
  closes (Radix default). Typing resets the highlight to the first result. Mouse
  click on a row runs it too.
- On open, the input autofocuses and the query resets to empty.

### `App.tsx` wiring

- `const [paletteOpen, setPaletteOpen] = useState(false)`.
- **Global keydown:** a `useEffect` adds a `window` keydown listener: `(e.metaKey || e.ctrlKey) && e.key === "k"` → `e.preventDefault(); setPaletteOpen(o => !o)`. Cleaned up on unmount.
- **Lift dialogs:** App owns `newNoteOpen`/`commitOpen` (plus the existing `settingsOpen`). Render `<NewNoteDialog open={newNoteOpen} … onCreate={actions.createNote} />` and `<CommitDialog open={commitOpen} … committing={committing} onCommit={actions.commitManual} />` at App level (next to `SettingsDialog`), where `committing` is the store's `committing` flag (`useCairn((s) => s.committing)`). Pass `onRequestNewNote={() => setNewNoteOpen(true)}` to `NoteList` and `onRequestCommit={() => setCommitOpen(true)}` to `CommitBar`; their buttons call those instead of owning the dialog.
- **Command list** (built in App, where the actions/state live):
  - `new-note` → `setNewNoteOpen(true)`
  - `commit` → `setCommitOpen(true)`
  - `toggle-view` → `setView(v => v === "graph" ? "editor" : "graph")` (load graph if switching to graph, mirroring the navbar toggle)
  - `open-settings` → `setSettingsOpen(true)`
  - `toggle-editor-mode` → `actions.setSettings({ editorMode: editorMode === "livepreview" ? "source" : "livepreview" })`
- Render `<CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} commands={COMMANDS} notes={notePaths} onRunCommand={runCommand} onOpenNote={(p) => { void actions.openNote(p); setView("editor"); setPaletteOpen(false); }} />`. `runCommand(id)` dispatches the map above then closes the palette.

---

## 3. Testing

- **Unit (Vitest):**
  - `fuzzy`: `fuzzyScore` matches subsequences ("comm"→"Commit changes", "ide"→"ideas"),
    returns `null` on no match, ranks a contiguous/prefix match above a scattered one;
    `filterItems` drops non-matches and sorts by score, empty query → all items.
  - `CommandPalette` (Testing Library): open → the commands render; typing "comm"
    filters to the Commit command (and away non-matches); `Enter` on the highlight
    calls `onRunCommand("commit")`; typing a note stem + `Enter` calls `onOpenNote`;
    `↓` then `Enter` runs the second item; Esc/`onClose` fires.
- **e2e (Playwright):** press **Meta+k** (or Control+k) → the palette opens; type a
  note name (`ideas`), press Enter → the note opens (assert the editor shows it);
  re-open, type `comm`, Enter → the Commit dialog appears. Keep existing tests green.
- **Refactor regression:** `NoteList`/`CommitBar` tests update to the new
  `onRequestNewNote`/`onRequestCommit` callbacks (the dialogs now live in App), and
  App-level behavior (toolbar "+ New note" / "Commit" still open the dialogs) stays
  working — covered by the existing dialog e2e flows (new-note + commit via modals).
- All existing unit + e2e stay green; Tauri unaffected.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/command-palette/fuzzy.ts` (+ test) | **New.** Pure fuzzy match/filter. |
| `web/src/components/command-palette/CommandPalette.tsx` (+ test) | **New.** Overlay + input + list + keyboard nav. |
| `web/src/app/App.tsx` | **Modify.** Palette state + ⌘K + command list + lifted dialogs. |
| `web/src/components/NoteList.tsx` (+ test) | **Modify.** "+ New note" → `onRequestNewNote`. |
| `web/src/components/CommitBar.tsx` (+ test) | **Modify.** "Commit" → `onRequestCommit`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** ⌘K palette flows. |

Uses the existing `@radix-ui/react-dialog`. No new npm dependencies. No store/host/contract changes.

---

## 5. Risks

- **Global ⌘K capture:** the listener must `preventDefault` so the browser/OS
  doesn't hijack ⌘K, and must NOT fire while typing inside the editor in a way that
  blocks normal `k` (only when `meta/ctrl` is held). Clean up the listener on
  unmount. Esc/backdrop close come from Radix.
- **Dialog-lift refactor:** moving `NewNoteDialog`/`CommitDialog` from
  `NoteList`/`CommitBar` to App changes those components' props (add
  `onRequestNewNote`/`onRequestCommit`, remove their internal dialog state). Update
  their unit tests; keep the toolbar-button → dialog flow working (the existing
  dialog e2e covers it). This is the main regression surface — do it carefully.
- **Focus management:** the palette input autofocuses on open; Radix returns focus
  to the trigger on close. Opening via ⌘K (no trigger element) should still focus
  the input and restore focus sensibly on close.
- **Editor key interception:** CodeMirror has its own keymap; ⌘K must still open the
  palette when focus is in the editor (the `window`-level listener with
  `preventDefault` handles it; verify CodeMirror doesn't swallow ⌘K first — if it
  does, the listener on `window` in the capture phase resolves it).
- **Empty-state note cap:** showing the first N notes (not "recent", since recency
  isn't tracked) — acceptable; a recency model is a later enhancement.
- **Canvas/jsdom:** the palette is plain DOM (Radix) — fully unit-testable, unlike
  the canvas graph; no special handling needed.
