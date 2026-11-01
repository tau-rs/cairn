# Recover lost work — UI design (web-ui consumer)

Date: 2026-08-10
Branch: `recover-lost-work-ui`
Status: design approved (brainstorm), pending implementation plan.

## Goal

Give a user a way to see and reclaim content the collab engine has retained for a
note — deleted (tombstoned) blocks and the losing side of concurrent overwrites —
and bring a chosen version back into the live document with one click.

## Engine reality (verified 2026-08-10)

The engine side is **fully merged** — this slice is purely the consumer.

- `recover` (view-only surface): `CollabClientMsg::Recover { note }` →
  `CollabServerMsg::Recoverable { note, blocks: WireRecoverableBlock[] }`. Unicast
  reply, read-only. PR #161.
- `restore` (mutation): `CollabClientMsg::Restore { note, id, version_index }`.
  Promotes the chosen version to a **new** block anchored right after the source
  block (reappears in its original slot). Emits a normal `Insert` `Op` — **no
  `Restored` ack** — fanned out to **every peer including the requester**, and
  marks the note dirty (persisted to disk). PR #163, engine HEAD `ed037d99`.
- `WireRecoverableBlock { id: WireBlockId, tombstoned: boolean, versions: string[] }`.
  For a tombstoned block, `versions` is `[winner, ..losers]` (former content);
  for a live block, `versions` is its stashed LWW losers. `version_index` addresses
  both kinds uniformly.

Contract vendored at `ed037d99` via `scripts/sync-contract.sh` (this branch).

## Scope

**Scope 2 (real end-to-end), against a live daemon.** Not a mock demo: recover
lists real retained content and Restore actually brings a block back. A thin,
recovery-focused `/collab` client — it does **not** implement a local CRDT/block
doc model. Tauri/Mock clients get honest stubs (collab is a daemon feature).

Out of scope: full collab editing/presence; Insert-at-cursor (dropped — Restore
supersedes it); un-delete semantics (engine deliberately restores as a new block).

## Surface

A **docked side panel**, scoped to the active note, mirroring the Ask/History
surface pattern:

- Desktop: `RecoveryPanel` mounted via `RecoveryPanelHost` as a new named shell
  region (`web/src/components/shells/regions.ts`), wired in `App.tsx`.
- Tablet/mobile: `RecoverySheet` in a `Drawer` (`side="right"` / `side="bottom"`)
  via `RecoverySheetHost`, mounted in `TabletShell`/`MobileShell`.
- Visibility is driven off store state (a recovery `mode`/open flag), not the
  breakpoint — the shell selection routes panel-vs-sheet, exactly like Ask.

## Triggers

1. Command palette: a `recover-lost-work` entry in `COMMAND_DEFS`
   (`commands.ts`), dispatched in `useCommands.ts` `runCommand` against
   `st.activePath` (the analog of the existing `show-history` case).
2. Tree context menu: a "Recover lost work…" `item(...)` in `TreeContextMenu.tsx`,
   backed by a new `onRecover(path)` prop threaded up through `FolderTreeView`
   (closing over `menu.path`, like `onDelete`).

Both call the same store action to open recovery for a given note path.

## Rendering

Per block, grouped by kind with count headers:

- **Deleted** (tombstoned) — `versions` = former content.
- **Overwritten** (live) — `versions` = dropped LWW-loser edits.

Each block renders each surviving version as a git-style **diff against the current
note text**, reusing `history/lineDiff` (`DiffRow`) and the exact `RevisionView`
row markup. Layout is **side-by-side by default, collapsing to unified single-
column** when the panel is too narrow (driven off panel width, consistent with
`useBreakpoint`). A short `#replica·counter` id labels each block (the only
"locator" — no live-doc block correlation is possible).

**Noise rules:**
- Drop empty-string versions.
- Hide a tombstoned block whose only version is empty (nothing to recover).
- Every tombstoned block surfaces (engine cannot distinguish concurrent-delete
  from plain delete); grouping + counts manage the volume.

## Actions (per version)

- **Copy** — recovered text → clipboard. Always available; universal.
- **Restore** — send `Restore { note, id, version_index }` over the `/collab`
  client. One-click; brings the block back in its original slot. Real against a
  live daemon; disabled with an explanatory tooltip when no daemon/collab session
  is available (e.g. Tauri/Mock).

Insert-at-cursor is intentionally **not** built (Restore replaces it; it would
duplicate content and needs risky focused-editor plumbing).

## Client seam

`CairnClient` gains a small recovery capability (JSDoc'd like `noteTags`/`ask`):

- open a `/collab` session for a note, request `recover`, surface the blocks;
- send `restore(note, id, versionIndex)`;
- close/leave on panel close.

Shape (finalized in the plan): a session handle is cleaner than two independent
request/response calls, because `recover` and `restore` must share one joined
`/collab` socket (restore's fanout reaches us only as a session member).

- `DaemonClient`: real `/collab` WebSocket (join → recover → restore → leave),
  reusing the `subscribe()` ws construction + backoff patterns.
- `TauriClient`: honest stub (collab is daemon-only) — surfaced as "unavailable".
- `MockClient`: fixtures for `recover` (derive plausible blocks); `restore`
  applies a deterministic local effect so component/slice tests are real.

## Reflecting a restore

Engine restore has no ack and web-ui holds no doc model. Mechanism: after sending
`restore`, the `/collab` client awaits the incoming `Op` (Insert authored by the
daemon replica) for that note as the "landed" signal, then the store **reloads the
note buffer** from the engine (render/read) so the restored block appears in the
editor. (Fallback if op-await proves unreliable: reload on a short settle.)
This avoids reimplementing the CRDT. **Risk flagged for the plan:** confirm the
reload path updates the open editor buffer cleanly (interaction with autosave and
the watcher).

## State

`createRecoverySlice` (`web/src/store/recoverySlice.ts`), modeled on `askSlice`
(closure-held session handle + token; `set((s) => …)` actions), spread into
`CairnState`. State keyed by note path: `{ mode, note, status, blocks[], error }`.
Actions: `openRecovery(note)`, `restoreVersion(id, versionIndex)`,
`closeRecovery()`.

## Components (`web/src/components/recovery/`)

- `RecoveryPanel` / `RecoveryPanelHost` — desktop docked panel.
- `RecoverySheet` / `RecoverySheetHost` — tablet/mobile `Drawer`.
- `RecoveryBlock` — one block: kind badge, id, versions as diffs, Copy + Restore
  (+ disabled Restore state with tooltip).
- Pure helpers (unit-tested): version filtering (empty-drop), block grouping,
  the side-by-side↔unified layout decision.

## Testing (part of done)

- `recoverySlice.test.ts` — open/populate/restore/close against `MockClient`
  (askSlice.test pattern).
- Client tests — `DaemonClient` `/collab` recover/restore via a `FakeWebSocket`
  (daemon.test pattern: join frame, recover→recoverable, restore→op→reload).
- Component tests — `RecoveryBlock` rendering, noise rules, Copy (clipboard),
  Restore (disabled vs enabled), grouping; panel/sheet host visibility.
- Helper unit tests — filtering/grouping/layout.
- Full gate green: `typecheck`, `lint`, `format:check`, `test`, `build`.

## Landing

web-ui merge queue + STRICT classic protection: stale PRs MUST update-branch; the
`web-deny` audit gate flaps on a stale lockfile. See `[[cairn-web-ui-merge-mechanics]]`.
