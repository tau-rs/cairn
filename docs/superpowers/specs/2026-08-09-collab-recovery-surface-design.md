# Collab recovery surface (view-only)

Give `BlockDoc::recoverable(id)` (shipped in #159) its first consumer: let a
collab client see content the CRDT retains but does not materialize — deleted
blocks and LWW losers — so a human can recover work that is otherwise invisible.

## Motivating constraint

`recoverable()` content is **ephemeral and daemon-in-memory only**. Block IDs are
live-only and never reach disk; there is no persisted stash (the `.cairn/` resume
sidecar `state_as_ops` hints at is unbuilt). So `stash` and tombstoned content
exist **only while a collab session is open** and vanish once it flushes and
reaps. Therefore:

- A CLI reading files or git can recover nothing — there is no persisted stash.
  Any surface must route through the running daemon's live session.
- The natural requester is the **app**: it is already a connected collab client
  holding the session open, and (via `Snapshot`) shares the daemon's block IDs,
  so it can correlate recovery data to blocks it already knows.

## Scope

Protocol + daemon handler + domain accessor, in this repo. **Out of scope:**
rendering recovered content to a human — that is the WS client's job and lives in
the web-ui repo. **Out of scope:** *restoring* a version (un-delete / promote a
stashed loser to winner) — that needs new CRDT ops and their own convergence
design; this is view-only. Copy-paste is the user's restore path for now.

## Design

### 1. Domain — `cairn-domain/src/crdt.rs`

```rust
/// One block's recoverable content: versions retained by the CRDT but not shown
/// by materialize(). `tombstoned` tells the client the block is currently deleted
/// (versions = its former content) vs. live (versions = stashed LWW losers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableBlock {
    pub id: BlockId,
    pub tombstoned: bool,
    pub versions: Vec<String>,
}

/// Every block with non-empty `recoverable(id)`, for a recovery view. Reuses
/// `recoverable`. Deterministic order: materialized live blocks first (in
/// document order), then tombstoned blocks by id — so the output converges and
/// tests can assert a stable sequence.
pub fn recoverable_blocks(&self) -> Vec<RecoverableBlock>
```

Only blocks whose `recoverable(id)` is non-empty are included. Live blocks
contribute their stashed losers; tombstoned blocks contribute all their content.

### 2. Contract — `cairn-contract/src/lib.rs`

```rust
// CollabClientMsg
Recover { note: String }                       // on-demand request

// CollabServerMsg
Recoverable { note: String, blocks: Vec<WireRecoverableBlock> }

pub struct WireRecoverableBlock {
    pub id: WireBlockId,       // shared live id; correlates with the client's replica
    pub tombstoned: bool,
    pub versions: Vec<String>,
}
```

`WireBlockId { replica, counter }` already exists. Domain stays serde-free.

### 3. Service — `cairn-service`

`recoverable_block_to_wire(RecoverableBlock) -> WireRecoverableBlock`, symmetric
with the existing `block_op_to_wire` mapping (contract stays domain-independent).

### 4. Daemon — `cairn-daemon/src/collab.rs` + `run_collab`

Handle `CollabClientMsg::Recover { note }`:

- Resolve `NotePath`; on failure send `CollabServerMsg::Error { note, message }`.
- Under the collab lock, look up the session. If present, call
  `sess.doc.recoverable_blocks()`, map each to wire, and send
  `CollabServerMsg::Recoverable { note, blocks }` **to the requesting socket
  only** (via `out_tx`), never fanned out to peers.
- If there is no session for the note, send `Recoverable { note, blocks: [] }`
  (an open-but-empty answer is clearer than an error for "nothing to recover").

The handler is **read-only**: no `merge`, no `dirty`, no `last_op`, no fan-out.

## Behavior summary

- On-demand: the client sends `Recover` when it wants to show a recovery view.
- Requester-scoped: the response goes only to the asking socket.
- Read-only: does not perturb session state or convergence.

## Over-preserve note (carried from #158)

`recoverable_blocks()` lists *every* tombstoned block, not only those deleted
concurrently with an edit — distinguishing them needs causality (version vectors)
the model does not track. The set is bounded by one session's activity and is
honest ("everything retained but hidden"). The client decides how to present it.

## Testing

- Domain: `recoverable_blocks_enumerates_losers_and_tombstoned` — a doc with one
  live block carrying a stashed loser and one tombstoned-with-content block;
  assert both appear with correct `tombstoned` flags and versions, in the
  documented order; a doc with nothing recoverable returns empty.
- Contract/service: `recoverable_block_wire_round_trips` — domain → wire → assert
  fields preserved.
- Daemon: `recover_returns_blocks_for_an_open_session` — seed a session, produce
  recoverable content, send `Recover`, assert a `Recoverable` with the blocks
  arrives on the requester and is NOT fanned out to a subscribed peer;
  `recover_on_absent_session_returns_empty` — `Recover` for an unopened note
  yields `Recoverable { blocks: [] }`.

## Definition of done

- `cargo build/test --workspace --locked`; `clippy --all-targets --locked
  -D warnings`; `fmt --check`.
- Conventional commits → PR against `main` → `gh pr merge --auto` (merge queue
  owns strategy; never manually update a queued branch).
