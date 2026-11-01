# Cairn Web UI — Editor Tabs Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 5 (Shell polish) — **tabs** (the "A · Tabs only" target from the
panes/tabs brainstorm). Split-panes is a deliberately deferred follow-on cycle that
will reuse this multi-note model.
**Builds on:** the existing single-note store, the stateless `Editor` component, the
graph-settings localStorage pattern (`forceSettings.ts`), and the ⌘K palette
(`command-palette/`) whose global-keydown + command-list seams this extends.

---

## 1. Purpose

Let the user keep **multiple notes open at once** in a VSCode-style tab strip above
the editor: browse via a single ephemeral **preview** tab, **pin** notes by editing
or double-clicking, switch instantly, and have the pinned working set survive a
reload. Today Cairn holds exactly one open note (`activePath`/`activeContents` — a
single slot); this spec generalizes that to a set of open notes while keeping
"the current note" reading exactly as it does now for the editor, backlinks, and graph.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Target | **Tabs only** over a single editor pane. Split-panes / drag-reorder are out of scope (later cycles). |
| Open model | **VSCode preview tabs.** Opening a note (list, search, palette, wikilink) shows it in the single preview tab (italic); the next open *replaces* it. A tab **pins** when the note is edited or its tab is double-clicked. New notes open pinned. Reopening an already-open note focuses its tab. |
| Active note | The active tab's path is "the current note" — drives the editor, backlinks, and the graph's local-mode/highlight (no change to those consumers). |
| Close & focus | `×` on a tab or **⌘W** closes the active tab and focuses the right neighbour (else left). Closing the last tab returns to the current empty state. Autosave flushes on close — no unsaved prompt. |
| Persistence | **Pinned** tabs + which was active persist to `localStorage` (`cairn.tabs`); restored on load, dropping paths that no longer exist. The ephemeral preview tab is not persisted. |
| Keyboard | **⌘W** close · **Ctrl+Tab** cycle · **⌘1…9** jump to tab N · plus a **"Close tab"** command in the ⌘K palette. |
| Look | Strip above the editor (graphite). Pinned = upright label; preview = *italic*; active = accent top-border + brighter text; dirty = accent dot until autosave flushes; `×` per tab; overflow scrolls horizontally. |
| Graph | Stays the **global** Editor/Graph toggle (it replaces the editor+tabs region; it is not a tab). |

### Non-goals (deferred)

- Split panes / multiple editor columns (next cycle; this multi-note model is its foundation).
- Drag-to-reorder tabs; drag a tab to a new split.
- A tab for the graph view, or per-pane graph.
- "Pin to keep" lock distinct from preview/pinned; middle-click close; tab context menu.
- Unsaved-changes prompts (Cairn autosaves; close just flushes).

---

## 2. Architecture

The hard part is the store (single-slot → multi-note); the UI is thin. Keep the
established pattern: **pure unit-tested modules + a props-based component + a store seam.**

```
web/src/components/tabs/tabsModel.ts          NEW (pure) — the tab-list reducer (open/preview/pin/close/cycle/jump).
web/src/components/tabs/tabsModel.test.ts     NEW.
web/src/components/tabs/tabsPersistence.ts    NEW (pure) — serialize/load pinned tabs ↔ localStorage, with clamping.
web/src/components/tabs/tabsPersistence.test.ts NEW.
web/src/components/tabs/TabStrip.tsx          NEW — the strip (preview italic / dirty dot / active / × close / dbl-click pin).
web/src/components/tabs/TabStrip.test.tsx     NEW.
web/src/store/store.ts                        MODIFY — openNotes + tabs state; per-note autosave; openNote/editBuffer/saveActive/createNote/deleteNote rework; tab actions; persistence load/save.
web/src/app/App.tsx                           MODIFY — render <TabStrip>; extend the global keydown (⌘W/Ctrl+Tab/⌘1-9); add the "Close tab" palette command.
web/e2e/skeleton.spec.ts                      MODIFY — tabs e2e (preview replace → edit pins → ⌘W close → reload restores).
```

### 2.1 `tabsModel.ts` (pure — no React, no store)

Owns ordering + the preview/active transitions. Operates on a plain list; the
per-note *buffer* (contents/dirty/saving) lives in the store, not here.

```ts
export interface Tab { path: string; preview: boolean }
export interface TabsState { tabs: Tab[]; activePath: string | null }

// Open a note. If already open → just focus it (preview tabs that get re-opened
// stay preview). Else if a preview tab exists → replace it (same slot, still
// preview, now this path). Else → append a new preview tab. Returns new state.
export function openOrPreview(state: TabsState, path: string): TabsState

// Pin the tab for `path` (preview → false). No-op if absent/already pinned.
export function pinTab(state: TabsState, path: string): TabsState

// Close `path`; if it was active, focus the right neighbour, else the left,
// else null. Returns new state (tab removed).
export function closeTab(state: TabsState, path: string): TabsState

// Focus the tab `delta` steps from the active one (wraps). For Ctrl+Tab.
export function cycle(state: TabsState, delta: 1 | -1): TabsState

// Focus the Nth tab (1-based); no-op if out of range. For ⌘1..9.
export function jumpTo(state: TabsState, n: number): TabsState
```

All are pure transforms returning a fresh `TabsState`; unit-tested exhaustively
(open-when-empty, open-replaces-preview, open-already-open focuses, pin, close-active
focuses neighbour, close-last → null, cycle wraps, jump out of range).

### 2.2 `tabsPersistence.ts` (pure)

Mirrors `forceSettings.ts`: try/catch-guarded `localStorage` under key
`cairn.tabs`. Persists only **pinned** paths + the active path.

```ts
export interface PersistedTabs { pinned: string[]; activePath: string | null }
export function saveTabs(state: TabsState): void           // writes pinned paths + activePath
export function loadTabs(existingPaths: string[]): PersistedTabs  // reads, drops paths not in existingPaths
```

`loadTabs` clamps to currently-existing notes (so a deleted/renamed note doesn't
resurrect a broken tab) and returns `{pinned: [], activePath: null}` on any
parse/storage error.

### 2.3 Store refactor (`store.ts`)

**New state:**
```ts
openNotes: Record<string, { contents: string; dirty: boolean; saving: boolean }>  // per-open-note buffer
tabs: { path: string; preview: boolean }[]                                          // order + preview flag
// activePath stays. activeContents/dirty/saving stay as the ACTIVE TAB'S MIRROR:
// they always equal openNotes[activePath] (or "", false, false when none). This
// keeps every existing consumer (App, CommitBar) reading the current note unchanged.
```
`uncommitted`/`lastCommit`/`committing` stay **global** (repo-level — correct as-is).

A single internal helper keeps the mirror honest: when the active note's buffer
changes or the active tab switches, the store updates `openNotes[path]` **and** the
top-level `activeContents`/`dirty`/`saving` together, so they can never diverge.

**Autosave becomes per-note:** replace the single module-scoped `autosave: Debounced`
with `autosaves: Map<string, Debounced>` — `editBuffer(path)` (re)schedules that
path's own timer; `saveActive` becomes `saveNote(path)` operating on
`openNotes[path]`. Idle/interval auto-commit stay global (they commit the repo).

**Reworked actions:**
- `openNote(path)` → fetch contents (if not already in `openNotes`), set
  `openNotes[path]`, apply `openOrPreview`, set `activePath`, refresh backlinks,
  persist. Re-opening an in-`openNotes` path skips the fetch and just focuses.
- `editBuffer(contents)` → write the active note's buffer (`dirty: true`), **pin**
  the active tab (`pinTab(activePath)`), (re)arm that note's autosave + the idle
  commit, persist.
- `saveNote(path)` / `saveActive()` → save `openNotes[path]`; the dirty-after-write
  reconciliation matches today's logic but keyed on that path's buffer.
- `createNote(path)` → write empty, open it **pinned** (not preview), persist.
- `deleteNote(path)` → `closeTab(path)`, drop `openNotes[path]`, focus neighbour
  (fetch its buffer if needed — it'll already be in `openNotes`), persist.
- **New actions:** `selectTab(path)`, `closeTab(path)`/`closeActiveTab()`,
  `cycleTab(delta)`, `jumpToTab(n)`, `pinTab(path)` — thin wrappers over `tabsModel`
  that also swap the active mirror to the newly-focused note's buffer and persist.
  Double-clicking a tab calls `pinTab(path)` (and focuses it); `editBuffer` pins the
  active path via the same action.
- `openCairn()` → reset `openNotes={}`, `tabs=[]`, `activePath=null`.
- **Restore on startup:** after `refreshNotePaths()` in `init()` (when a cairn is
  open), call `loadTabs(notePaths)` and `openNote` each restored pinned path in
  order (skipping fetch failures), then focus the persisted active path. Eager fetch
  — the pinned working set is small.

### 2.4 `TabStrip.tsx`

Props-only, no store coupling:
`{ tabs: {path; preview; dirty}[]; activePath: string|null; onSelect(path); onPin(path); onClose(path) }`.
Renders a horizontal, scroll-on-overflow strip; each tab shows `stem(path)` (italic
when `preview`), a dirty dot when `dirty`, and an `×`. Click → `onSelect`;
double-click → `onPin`; `×` (stopPropagation) → `onClose`. Active tab styled with the
accent top-border. Testable with Testing Library.

### 2.5 `App.tsx` wiring

- Select `tabs` + per-tab dirty (`openNotes[path].dirty`) and render `<TabStrip>`
  above the editor region (only in `view === "editor"`), wired to `selectTab` /
  `pinTab` (`onPin` → focus + pin) / `closeTab`. The editor still gets the active
  note's path/contents (unchanged — they're the mirror).
- **Extend the existing global keydown** (the ⌘K listener): add `⌘W`→`closeActiveTab`
  (preventDefault so the browser doesn't close the tab), `Ctrl+Tab`→`cycleTab(±1)`,
  `⌘1…9`→`jumpToTab(n)`.
- Add a **"Close tab"** command (`close-tab` → `closeActiveTab`) to the palette's
  `COMMANDS`/`runCommand`.

---

## 3. Testing

- **Unit (Vitest):**
  - `tabsModel`: every transform — open into empty / open replaces the preview tab /
    open an already-open path focuses (no dup) / pin / close-active focuses the right
    then left neighbour / close-last → null / cycle wraps both directions / jumpTo
    in and out of range.
  - `tabsPersistence`: round-trips pinned + active; `loadTabs` drops paths absent from
    `existingPaths`; returns empty on malformed/missing storage. (jsdom localStorage
    works via the existing `vitest.setup.ts`.)
  - `TabStrip` (Testing Library): preview renders italic; dirty shows the dot; click
    calls `onSelect`; double-click calls `onPin`; `×` calls `onClose` (and not
    `onSelect`); active tab marked.
  - **Store** (extend existing store tests): opening two notes keeps both buffers
    (switch A→B→A preserves A's edits and dirty); editing pins the preview tab;
    per-note autosave saves the right path; `deleteNote` closes its tab and focuses a
    neighbour; restore-on-init reopens persisted pinned tabs and skips missing ones.
- **e2e (Playwright):** open `index` then `ideas` (preview replaces, one preview tab) →
  type into `ideas` (its tab pins, dirty dot, then Saved) → open `todo` (new preview)
  → **⌘W** closes the active tab and focuses a neighbour → reload → the pinned tabs are
  restored. Keep all existing e2e green.
- All existing unit + e2e stay green; Tauri unaffected (pure web-shell change; the
  client contract is untouched).

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/tabs/tabsModel.ts` (+test) | **New.** Pure tab-list reducer. |
| `web/src/components/tabs/tabsPersistence.ts` (+test) | **New.** localStorage round-trip + clamping. |
| `web/src/components/tabs/TabStrip.tsx` (+test) | **New.** The strip component. |
| `web/src/store/store.ts` | **Modify.** Multi-note buffers + tabs + per-note autosave + tab actions + restore. |
| `web/src/app/App.tsx` | **Modify.** Render TabStrip; extend keyboard; add the "Close tab" command. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Tabs e2e. |

No new npm dependencies. No client/host/contract changes. Reuses `stem` (`wikilink.ts`),
`debounce` (`util/timer.ts`), and the palette's global-keydown seam.

---

## 5. Risks

- **Store refactor is the crux.** Single-slot → multi-note touches autosave, dirty
  reconciliation, and every "current note" reader. Mitigation: keep
  `activeContents`/`dirty`/`saving` as a **derived mirror of the active tab** so App
  and CommitBar are unchanged; route all buffer mutations through one helper so the
  mirror can't drift; do the store refactor as its own task with the existing
  single-note store tests kept green before any tab UI lands.
- **Per-note autosave map.** Each note needs its own debouncer; a switched-away note
  must still flush. Mitigation: `Map<path, Debounced>`; `saveNote(path)` reads that
  path's buffer (not "active"); close flushes the pending timer first.
- **⌘W / browser capture.** ⌘W closes the browser tab by default — the in-app handler
  must `preventDefault`. In Tauri it's the app's own accelerator. Ctrl+Tab likewise.
- **Restore correctness.** A persisted pinned path may have been deleted/renamed
  out-of-band. Mitigation: `loadTabs` clamps to current `notePaths`, and restore skips
  any path whose fetch fails — never resurrect a broken tab.
- **Preview-tab churn vs autosave.** Single-clicking through many notes must not
  spam writes (preview tabs aren't dirty, so no autosave fires until an edit). Edit →
  pin + dirty + autosave, as today. Verified by the "browse doesn't save" store test.
- **Empty state.** Closing the last tab must cleanly return to the existing
  no-note-open view (`activePath = null`, empty editor) without errors in backlinks/graph.
- **Plain DOM, jsdom-safe.** TabStrip is plain DOM (no canvas), fully unit-testable.
