# Live-collab presence — design

**Date:** 2026-08-21
**Branch:** `feat/live-collab-presence`
**Phase:** 8 (Live-collab + recovery UI) — the *live-collaboration* follow-on to
the recovery/restore slice shipped in PR #146.

## Summary

A **one-way live-awareness layer** over the daemon `/collab` WebSocket. When a
note is open, the app joins that note's collab session, receives peer edits, and
surfaces them non-destructively:

- an ambient **presence pill** (binary "🟢 Live edits") while another party is
  editing the open note, and
- a **non-destructive content refresh**: a clean buffer silently reloads from
  `get_note`; a dirty buffer shows a "N live changes — Reload" nudge and is never
  clobbered.

There is **no** client-side CRDT and **no** outgoing ops. Our own local edits
still save through the existing `save_note`/autosave path; the daemon folds those
disk writes back into the session and fans them to peers. We only *receive*.

## Why this scope (the two hard constraints)

1. **No awareness/presence frame in the contract.** `CollabServerMsg` is only
   `joined{note}` / `snapshot{note,ops}` / `op{note,op}` / `error` / `recoverable`.
   `joined` carries no replica identity and is *not fanned out* to peers — when a
   peer joins, other clients get nothing. The only signal another party is active
   is an incoming `op` frame (whose block id carries a `replica`). A true roster /
   avatars / live cursors would need an engine contract change, which is out of
   scope (we consume the pinned DTOs, we do not re-sync).

2. **The web editor is whole-note text buffers, not a block CRDT.** The engine
   session is a `BlockDoc` (Lamport-ordered block ops). The web app has no CRDT
   model. Faithfully applying block ops would require a client `BlockDoc` mirroring
   the engine's exact markdown→block splitting; any drift silently diverges the
   buffer from disk. That is a multi-PR epic, not this slice.

Given both, this slice treats incoming ops as **opaque "something changed, by
replica R" signals** and always sources content from `get_note` (byte-exact). It
delivers live presence + fast, non-destructive refresh without a client CRDT.

### The `/collab` live-session flow (what a client can observe)

```
client ──join{note,replica}──▶ daemon
client ◀──joined{note}──────── daemon
client ◀──snapshot{note,ops[]} daemon      # full current doc as insert-ops (we ignore content)
        … then, live …
client ◀──op{note,op}────────── daemon      # a PEER's edit, or the daemon's fold-back
                                            #   of a foreign disk save, or a restore
client ──leave{note}──────────▶ daemon
```

We never send `op`. `snapshot` only confirms the join (content is authoritative
from disk via `get_note`).

## Dependency & ordering

This builds on **PR #146** (recovery/restore), which is queued to merge and:

- introduced `/collab` client plumbing (`DaemonClient` recover/restore session,
  Tauri honest stub, Mock fixtures), and
- added `reloadNoteBuffer(path)` to the store — a clean-only external reload that
  re-fetches `get_note` and updates the buffer *only when it is clean* (never
  clobbering unsaved edits).

**First plan step: rebase `feat/live-collab-presence` onto post-#146 `main`.** We
reuse `reloadNoteBuffer` rather than re-implementing it, and coexist with #146's
`/collab` usage on the same endpoint.

## Architecture (hexagonal — dependencies point inward)

```
DaemonClient.openCollab(note, handlers)   [transport / adapter]
        │  join on open, leave on close; routes snapshot/op/error
        ▼
   collabSlice (zustand)                   [domain state — owns the session]
        │  live / pendingCount, token-guarded; clean→reload, dirty→nudge
        ▼
   CollabPresencePill + reload nudge       [thin per-corner subscription]
```

WS handling never leaks into components. Components read `collabSlice` state only.

### 1. Transport — `CairnClient.openCollab`

New long-lived session method on the transport seam, mirroring the existing
`/events` WS plumbing (injectable `WebSocket`, so it is testable):

```ts
export interface CollabHandlers {
  onSnapshot?(note: string): void;              // join confirmed
  onForeignOp?(note: string, op: WireBlockOp): void;
  onError?(note: string, message: string): void;
}
export interface CollabSession {
  close(): void;                                // sends leave{note}, closes socket
}
// on CairnClient:
openCollab(note: string, handlers: CollabHandlers): CollabSession;
```

- **`DaemonClient.openCollab`** — opens `ws://…/collab`, sends
  `join{note, replica}` on open, forwards `snapshot`/`op`/`error` frames (parsed
  via `assertCollabServerMsg`), sends `leave{note}` and closes on `close()`.
  Distinct from #146's short-lived `openRecovery` socket: same endpoint, different
  replica id, coexists (the engine tracks participants as a set).
  - `replica`: a random small positive integer chosen per app session. Because we
    never send ops, it is used only for the daemon's participant set / echo-skip.
  - Reconnect: presence is non-critical, so **no aggressive backoff**. If the
    socket drops, presence goes dark and re-attaches on the next active-note
    change/focus. (Contrast `/events`, which needs the B′ backoff.)
- **`TauriClient.openCollab`** — honest-reject stub ("live collab needs the
  daemon"), exactly like #146's `openRecovery`. Desktop parity is a later
  follow-on (mirrors `/ask` #72 → #79).
- **`MockClient.openCollab`** — deterministic: after join, emits one scripted
  foreign `op` (and no-ops on `close`) so the slice + UI are demoable/testable
  without a daemon.

### 2. Domain state — `collabSlice`

Mirrors `askSlice`/`recoverySlice`: closure-owns the `CollabSession` and a
monotonic token so a fast note-switch cannot race a stale session's callbacks
into state.

```ts
export interface CollabPresence {
  note: string | null;      // the note currently followed
  live: boolean;            // a foreign op arrived recently (decays on quiet)
  pendingCount: number;     // foreign changes not yet reflected (dirty buffer only)
}
export interface CollabState {
  collab: CollabPresence;
  collabFollow(path: string): void;   // open session for the active note
  collabStop(): void;                 // leave + reset
  collabReloadNow(): void;            // user clicked the nudge → reloadNoteBuffer + clear pending
}
```

Behavior on a foreign `op` for the followed note (token still current):

- set `live = true`; arm/refresh a quiet-decay timer that flips `live` back to
  `false` after N seconds of no foreign ops.
- **buffer clean** (`openNotes[path].dirty === false`) → debounce briefly, then
  `reloadNoteBuffer(path)` (silent, from #146). `pendingCount` stays 0.
- **buffer dirty** → `pendingCount++`. Do **not** reload (never clobber unsaved
  edits). The nudge renders from `pendingCount`.

`onSnapshot` → no content change (join confirmation only). `onError` → log/drop;
never disrupts editing, never a hard banner.

Lifecycle: the app calls `collabFollow(activePath)` when the focused pane's active
note changes, and `collabStop()` / re-follow on switch or close. A passive join
that never sends ops leaves the daemon session clean → no disk writes, reaped on
leave.

### 3. UI — corner presence pill + reload nudge

A `CollabPresencePill` in the bottom-right status corner, near
`LiveUpdatesBanner` (stacked so it does not collide with the connection banner):

- `collab.live && !buffer.dirty` → `🟢 Live edits` (binary; hidden when quiet).
- `collab.live && buffer.dirty && pendingCount > 0` → `N live changes — Reload`
  toast; clicking calls `collabReloadNow()`.

Binary label by design: a peer who joined but has not typed is invisible to us
(joins are not broadcast), so a precise count would imply a roster we cannot back
up. "Live edits" is the truthful minimum.

### 4. Wire-parse — `assertCollabServerMsg`

A thin guard added to `contractGuards` validating an incoming frame is a
`CollabServerMsg` before the transport hands it to the slice. Guards stay thin
(S5) — validate at the seam, no guard refactor.

## Error handling

Presence is non-critical awareness. WS attach failure, a socket drop, or an
`error` frame → presence simply stays/goes dark (optionally logged). It never
surfaces a hard banner and never interferes with editing or saving. The
connection banner remains owned by the `/events` stream, unchanged.

## Testing (part of done)

- **`collabSlice` reducers**: foreign op → `live=true`; clean-buffer branch calls
  `reloadNoteBuffer`; dirty-buffer branch increments `pendingCount` and does not
  reload; stale-token guard drops a superseded session's callbacks after a
  note-switch; quiet-decay flips `live` off; `collabReloadNow` reloads + clears.
- **`assertCollabServerMsg`**: accepts each valid variant, rejects malformed
  frames.
- **`DaemonClient.openCollab`** with an injectable `FakeWebSocket`: `join` sent on
  open, `leave` sent on `close`, `snapshot`/`op`/`error` routed to the right
  handler, malformed frame handling.
- **`MockClient.openCollab`**: scripted foreign op reaches `onForeignOp`.

## Out of scope (explicit)

- Outgoing ops / two-way CRDT co-editing (option C).
- A client `BlockDoc` / block-level patch-apply into the editor (option B2).
- Live cursors, a participant roster, avatars.
- Desktop (Tauri) `/collab` transport — honest stub only this slice.

## Definition of done

- The live-half DTOs are consumed: `join`/`leave`/`snapshot`/`op` via
  `openCollab` + a `CollabServerMsg` guard (recovery #146 consumed
  `recover`/`restore`/`recoverable`).
- Presence pill + non-destructive reload behavior shipped and tested; `just
  web-ci` green locally and in CI; PR merged via the queue.
- `docs/roadmap.md`: advance Phase 8 as this lands.
