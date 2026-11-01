# App.tsx Decomposition — Design

**Date:** 2026-06-11
**Finding:** D5 (`audit/design.md`) — MEDIUM, design.

## Problem

`App.tsx` is a 327-line monolith. The root component subscribes to ~20 store
slices individually (`App.tsx:85-109`) and threads them through `Shell` via four
large render-prop slots (`App.tsx:188-342`). It mixes local UI state (dialog-open
flags, keybinding overrides) with global store data and command dispatch
(`runCommand` + the global `keydown` effect both live here).

The harm is **re-render concentration**: because the panes are passed to `Shell`
as freshly-created elements on every App render, *any* of the ~20 slices changing
re-renders the entire shell. High-frequency slices (`activeContents` on every
editor keystroke, `query` on every search keystroke, `saving`/`dirty` on every
autosave) drive a re-render storm across the whole tree. The file is also the
single hardest thing to change safely.

## Goal

A **behavior-preserving** refactor: push store subscriptions down into focused
components that each select only their own slices, and relocate the shared
ephemeral UI state so App subscribes to almost nothing. No feature changes.

## Approach: store-owned UI state (Approach C)

The chosen approach moves the shared ephemeral UI state (dialog-open flags +
keybinding overrides) **into the Zustand store** as a namespaced `ui` slice,
rather than leaving it as App-owned `useState` (Approach A) or wrapping it in a
React context (Approach B).

Rationale: the codebase already has two divergent state systems for the same kind
of thing — `settings` is store-owned, while keybinding `overrides` is App-local
`useState` + hand-rolled `localStorage`. That split is an accident of growth and
is *why* App accreted (it is the only place that can host the second system).
Approach C ends it: one source of truth, one read path (`useCairn`), one write
path (actions). Dialog state stops being "accidentally in App," and any
trigger — a button, a palette command, a keyboard chord, a future plugin — opens
a dialog the same way, without threading setters through the tree.

The classic "don't mix view state into the domain store" objection is answered by
the slice boundary: `ui.*` is namespaced, never serialized to the backend, and the
domain state stays pure.

## 1. Store: a `ui` slice + overrides migration

Add a namespaced `ui` object to `CairnState`, parallel to the existing nested
`settings`:

```ts
interface UiState {
  settingsOpen: boolean;
  newNoteOpen: boolean;
  newNoteInitial: string;
  commitOpen: boolean;
  paletteOpen: boolean;
  keybindingOverrides: Overrides; // migrated out of App's useState
}
```

Actions (top-level, matching the store's flat-action style):

- `setUi(patch: Partial<UiState>)` — generic patch setter, mirrors `setSettings`.
  Gives Radix dialogs their `onOpenChange={(o) => setUi({ settingsOpen: o })}`
  directly, and lets the command layer open dialogs via plain `setUi(...)` calls
  (no bespoke per-dialog actions).
- `setKeybindingOverrides(o: Overrides)` — sets `ui.keybindingOverrides` **and**
  persists via `saveOverrides` (from the existing `keybindingPersistence` module).

`init()` seeds `ui.keybindingOverrides` from `loadOverrides()` — same localStorage
key, same behavior, just relocated from App into the store. This is the
architectural payoff: overrides join the one store instead of being a second,
hand-rolled state system.

Default `ui` state: all flags `false`, `newNoteInitial: ""`,
`keybindingOverrides: {}` (overwritten by `init()`).

## 2. Component decomposition

Each pane calls `useCairn` for **only its own slices**; none receive store data as
props. New components live in `src/components/` following the existing
named-export, inline-props-interface idiom.

| Component | Subscribes to | Renders |
|---|---|---|
| **TopBar** | `query, saving, dirty, uncommitted, lastCommit, committing` + `useLocation` (view) | Logo, label, SearchBar, Graph/Editor toggle, settings button, CommitBar |
| **Sidebar** | `notePaths, activePath, tags, activeTag` | FolderTree + TagsPanel |
| **EditorPane** | `searchResults, searchSnippets, activeTag, graph, noteTags, activePath, tabs, openNotes, notePaths, activeContents, editorMode, loadRemoteImages` + `useLocation` | ErrorBoundary → SearchResults + Graph/(TabStrip+Editor) switch |
| **BacklinksPane** | `backlinks` | Backlinks |
| **DialogHost** | `ui.*, settings, plugins, committing, ui.keybindingOverrides` | SettingsDialog, NewNoteDialog, CommitDialog, CommandPalette |
| **Toasts** | `error, notice` | ErrorToast + NoticeToast |

`Sidebar`, `BacklinksPane`, and `Toasts` are extracted beyond the brief's named
three (TopBar/EditorPane/dialog-host). Reason: since panes are passed as elements
to `Shell`, *any* App subscription re-renders all of them — so to truly kill the
storm, App must subscribe to almost nothing. Extracting these removes App's last
frequent/moderate subscriptions. They are small (1–4 slices each).

`DialogHost` and the panes consume `runCommand`/`commands` (built by `useCommands`
in App) as props where needed (CommandPalette). This is the only remaining prop
flow into a child, and it is two values, not 20 data slices.

## 3. Hooks + the new App

- **`useCommands()`** → `{ commands, runCommand }` (in `src/app/` or
  `src/components/command-palette/`). Builds the palette command list (from
  `COMMAND_DEFS` + `ui.keybindingOverrides` + `plugins`) and the `runCommand`
  dispatcher. Dialog-opening cases become `setUi(...)` store calls. The nav cases
  (`toggle-view`, `nav-back`, `nav-forward`, open-note) stay here because they need
  react-router's `navigate`/`location`. Reads `editorMode`/`activePath` lazily via
  `cairnStore.getState()` inside the handler so it does not subscribe to them.
- **`useGlobalKeys(chordMap, runCommand)`** (in `src/components/shortcuts/`) → the
  `window` `keydown` effect: chord dispatch plus the built-in, non-rebindable
  Ctrl+Tab / Mod+1-9 tab navigation. Carries over the existing `runCommandRef`
  trick so the listener does not rebind on every render. Named `useGlobalKeys`
  (not `KeyboardShortcuts`) because `KeyboardShortcuts.tsx` already exists as the
  Settings rebind UI.

**App** shrinks to a coordinator: the `init()` effect, the
`cairnPath === null → <OpenCairn>` gate (its one real subscription), `useCommands()`,
`useGlobalKeys()`, and the JSX wiring (`RouteSync`, `Shell` with the four panes,
`DialogHost`, `Toasts`). It no longer touches any high-frequency slice. App still
calls `useLocation` (already does today, for routing) and `useCommands` subscribes
to `plugins`/`keybindingOverrides` — all low-frequency, so the re-render storm is
gone.

## 4. Testing

Behavior-preserving refactor, so the existing suites are the baseline; new unit
tests pin the extracted units.

- **Baseline:** existing `e2e/skeleton.spec.ts` (search, palette ⌘K,
  rebind+persist, dialogs, tabs, graph toggle, commit) stays green — proves
  behavior preserved end-to-end. Existing unit tests stay green.
- **New unit tests** (against the singleton `cairnStore` via `setState`/`getState`,
  resetting `ui` in `beforeEach`, in the style of `store/store.test.ts`):
  1. **ui slice + persistence:** `setUi` patches state; `setKeybindingOverrides`
     sets state and persists (round-trips through `loadOverrides`); `init()` seeds
     `ui.keybindingOverrides` from localStorage.
  2. **Subscription isolation** (pins the finding): render `TopBar` with a
     render-counter, change an unrelated slice (`backlinks`) via `setState`, assert
     `TopBar` did **not** re-render.
  3. **Command dispatch routes:** `runCommand("commit")` → `ui.commitOpen === true`;
     `"new-note"` → `ui.newNoteOpen === true`; `"open-palette"` toggles
     `ui.paletteOpen` — proves dispatch survives the move.

## 5. Scope & coordination

- **In scope:** the refactor above only — behavior-identical, no feature changes.
- **Out of scope (follow-ups, per brief):** D6 (capture `actions` once) and U4
  (global-keydown focus-check). Noted, not touched here.
- **Coordination:** heavy overlap with session 63 (dead-plugin) and session 65
  (loading-states/keyboard — edits the same keydown handler). The `useGlobalKeys`
  extraction will conflict with 65; this PR will need rebase coordination.

## Out of scope / non-goals

- No `React.memo` on the panes — App's remaining subscriptions are all
  low-frequency, so memoization is unnecessary; adding it would be speculative.
- No change to the `Shell` render-prop interface — it stays as-is; only the
  *contents* of each slot move into components.
