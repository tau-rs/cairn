# Note history / version restore — UI design

Date: 2026-06-14 · Track 01 (`.context/handoff/01-note-history.md`) · Branch: `feat/note-history`

## Goal

Surface the engine's git-backed per-note history in the UI: a revision
**timeline**, **view a note at an old revision** (read-only), and **restore** an
old version. The engine shipped all three ops; the contract is vendored and the
UI wires none of them today.

## Prior art (why this shape)

Researched version-history UX across comparable tools:

- **VS Code Timeline** (closest analog — git-backed, developer-flavored): a
  side-panel of timeline entries; clicking one opens a diff in the editor area.
- **Box Notes / HighLevel**: right-sidebar history, current at top, one-click
  restore. Side panels beat modals for persistent visibility.
- **Notion / Obsidian-Git**: both show a **diff** (not just raw old content) and
  one-click restore; modal/full-page is the consumer-doc pattern, heavier and
  less fitting for a panel-based tool.
- **Restore is non-destructive** everywhere: restoring adds a new version on top
  rather than discarding later ones. cairn's `restore_note` matches this exactly
  (overwrite working copy → user commits → new commit on top of history).

**Chosen shape:** the VS Code Timeline shape — a **history tab in the right
aside** + a revision view in the editor region — **phased**: Phase 1 ships a
**read-only** revision view; Phase 2 swaps in a **diff** as a render-only upgrade.
Approved via visual mockups.

## Contract (verified, ready — do not hand-edit the vendored contract)

- Query `{ type: "note_history", path }` → `QueryResponse { type: "history", revisions: Revision[] }` (newest first).
- Query `{ type: "note_at", path, revision }` → `QueryResponse { type: "note", contents }` (`revision` is a git revspec).
- Command `{ type: "restore_note", path, revision }` → `CommandResponse { type: "done" }` (overwrites the working copy).
- `Revision = { id: string /* short hash */, message: string, timestamp_secs: bigint, author: string }`.

All three are plain `Query`/`Command`s over the existing `CairnClient.runQuery` /
`sendCommand` — **no new client-interface method** (unlike `noteTags`, which is a
non-contract capability).

## Architecture

### Conflict-rule compliance

`store.ts` is the shared monolith. Per the handoff rule, all state/actions live in
a **new** `web/src/store/historySlice.ts`, wired into `createCairnStore` with
**one import + one spread line**:

```ts
import { createHistorySlice } from "./historySlice";
// …inside the returned object:
...createHistorySlice({ set, get, client, pushError, setBuffer }),
```

`pushError` and `setBuffer` are existing internal closures in `createCairnStore`;
passing them in lets the slice reuse the shared error-toast channel and the
buffer/mirror update (needed for restore's live editor refresh) without
duplicating logic. The `HistorySlice` interface is intersected into `CairnState`.

### Store slice — `historySlice.ts`

State (kept inside the slice; **not** added to `store.ts`'s `loading` map or
`UiState`, to keep `store.ts` edits to the single spread line):

- `history: Revision[] | null` — revisions for `historyPath` (null = not loaded).
- `historyPath: string | null` — note the loaded history belongs to (stale-guard).
- `historyLoading: boolean` — distinct from empty result.
- `viewingRevision: { path: string; revision: string; contents: string } | null`
  — when set, the editor renders that revision read-only.
- `rightTab: "backlinks" | "history"` — active right-aside tab (default `"backlinks"`).

Actions:

- `setRightTab(tab)` — switch the aside tab.
- `showHistory()` — `setRightTab("history")`, open the drawer on small screens via
  the existing `setUi({ backlinksOpen: true })`, then `loadHistory()`. This is the
  ⌘K / toolbar entry point.
- `loadHistory()` — no-op if no `activePath`; else run `note_history`, set
  `history` + `historyPath`. Race-guarded with a monotonic token (mirrors the
  existing `seq` pattern) so a superseded request can't clobber a newer one.
- `viewRevision(revision)` — run `note_at`, set `viewingRevision` (carrying the
  current `activePath`).
- `exitRevisionView()` — clear `viewingRevision`.
- `restoreRevision(revision)` — confirm happens in the UI; this action runs
  `restore_note`, then **reloads the active buffer** (re-`get_note` + `setBuffer`
  with `dirty:false`) so the editor live-refreshes, sets `uncommitted: true`,
  clears `viewingRevision`, and reloads history. Errors go through `pushError`.

All async actions catch `ContractError` and route through `pushError`
(operation-prefixed toast), matching the rest of the store.

### Components — `web/src/components/history/`

Mirrors the Backlinks split (presentational + container):

- `HistoryList.tsx` (presentational) — props `{ revisions, loading, selected,
  onView, onRestore }`. Renders a git-style timeline (dot + connector), each row:
  message · short hash · relative time (with absolute tooltip) · author. Shows a
  "Working copy" pseudo-row at the top when there are uncommitted edits.
- `HistoryPane.tsx` (container) — pulls slice state, calls `loadHistory()` on
  mount / `activePath` change, wires `onView`/`onRestore`.
- `RevisionView.tsx` — rendered in the editor region when `viewingRevision`
  matches `activePath`. Read-only contents + amber "Viewing <hash> — read-only"
  banner + **← Back to current** + **Restore**. Phase 2 adds a `mode:"full"|"diff"`
  prop; the diff branch is the only Phase-2 code.
- `RestoreConfirmDialog.tsx` — Radix dialog (same pattern as `CommitDialog`).
  Copy states it overwrites the working copy and current edits become uncommitted
  changes. **Restore always confirms first.**
- `formatRevision.ts` — `timestamp_secs` (bigint) → `new Date(Number(ts)*1000)`,
  relative label + absolute title. Pure, unit-tested.

### Right-aside tab toggle

`App.tsx` currently passes `backlinks={<BacklinksPane />}` to every shell. Replace
with a small `RightAside.tsx` (one-line change) that renders a **Backlinks /
History** tab header (reading/writing `rightTab`) and the matching pane. Because
all shells receive the same `backlinks` region node, the toggle works on desktop
(always-visible aside) and inside the tablet/mobile drawer unchanged.

### Editor integration — `EditorPane.tsx` (minimal additive edit)

- When `viewingRevision && viewingRevision.path === activePath`, render
  `<RevisionView/>` instead of the normal editor.
- One `useEffect`: on `activePath` change, call `exitRevisionView()` so a lingering
  revision view never leaks across note switches.

This is the only edit outside the owned file set; it is additive and low-conflict
(no other parallel track touches `EditorPane`).

### Command palette + keybinding

- `commands.ts`: add `{ id: "show-history", label: "Show note history",
  defaultBinding: "Mod+Shift+H" }` to `COMMAND_DEFS`.
- `useCommands.ts`: add `case "show-history": st.showHistory(); break;`.
- Editor toolbar: a clock icon button calling `showHistory()`.

### Mock client — `mock.ts` / `fixtures.ts`

`mock.ts` serves none of the three ops today. Add:

- Fixture state: per-path `Revision[]` (newest first) + a
  `revisionContents` map keyed by `path@revision`. Seed ≥1 note with 2–3
  revisions so store/component tests run on the mock.
- `note_history` → revisions for the path (empty array if none).
- `note_at` → historical contents; unknown revision → `not_found` ContractError.
- `restore_note` → set working copy to the revision's contents, emit
  `note_changed`, return `{ type: "done" }`.

Note: restore's `note_changed` is treated as an *external* change by the store's
`onEvent` (it is not a tracked self-write), which refreshes derived views but not
the active buffer — hence `restoreRevision` reloads the buffer explicitly.

## Data flow

1. Open note → `activePath` set.
2. History tab / ⌘K "Show note history" / toolbar clock → `showHistory()` →
   `loadHistory()` → timeline populates.
3. Click a revision → `viewRevision()` → `note_at` → editor shows it read-only +
   banner.
4. Back to current → `exitRevisionView()` → editor returns to the live buffer.
5. Restore → confirm dialog → `restoreRevision()` → `restore_note` → buffer
   reloads (editor live-refreshes) → `uncommitted:true` → history reloads.

## Error handling

Every query/command is wrapped; `ContractError` → `pushError(operation, err,
ctx)` → operation-prefixed, auto-dismissing toast (existing mechanism). Stale
async results are token-guarded and dropped.

## Testing (TDD — tests are part of done)

- **mock**: `note_history` newest-first; `note_at` returns historical contents;
  `note_at` unknown revision → `not_found`; `restore_note` overwrites + emits
  `note_changed`.
- **slice**: `loadHistory` populates + stale-path race guard; `viewRevision` sets
  state; `exitRevisionView` clears; `restoreRevision` round-trip (buffer updated,
  `uncommitted` set, `viewingRevision` cleared, history reloaded); error path
  pushes a toast; `showHistory` sets tab + opens drawer + loads.
- **formatRevision**: bigint → relative/absolute formatting incl. edge cases.
- **HistoryList**: renders rows, formats fields, `onView`/`onRestore` fire,
  loading vs empty states.
- **RevisionView**: banner + read-only + back/restore callbacks.
- **RestoreConfirmDialog**: confirm/cancel.
- **RightAside**: tab toggle swaps panes; `rightTab` honored.
- **EditorPane**: shows `RevisionView` when `viewingRevision` matches `activePath`;
  clears on note switch.

Run the **full local gate** (incl. `prettier --check` / format) before claiming
green — eslint won't catch format drift.

## Scope / phasing

- **Phase 1 (this PR):** everything above with a **read-only** revision view.
- **Phase 2 (fast-follow):** diff-vs-current in `RevisionView` (`mode:"diff"`).
  Needs a line-diff (small dep or a ~40-line util — repo has neither today).
  Out of scope for the Phase-1 PR; the `RevisionView` `mode` prop is the seam.

## Done

Timeline renders real revisions, view-at-revision works, restore round-trips and
the editor live-refreshes; tests + full local gate green.
