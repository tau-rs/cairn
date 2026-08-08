# CRDT Live Collaboration Transport — Design Spec

**Date:** 2026-07-19
**Status:** Approved (design); ready for implementation planning
**Builds on:** ADR-0011 (`docs/decisions/0011-crdt-collaboration-model.md`) and the
slice-1 convergence proof (`2026-06-16-crdt-collaboration-design.md`, landed as
#85: `BlockDoc` in `cairn-domain`, the `CollabSession` port, `LocalCrdt` +
`NoCollab` adapters, property tests).

**Scope of this document:** the first *real* multi-writer transport — a daemon
op-relay, a transport-backed `CollabSession` adapter, the session lifecycle, and
the truth-ownership model that governs the live document, the on-disk markdown,
the file-watcher, and autosave→git-commit while a session is open. This is the
**full live vertical** (option A below): it folds the previously-unbuilt "local
wiring" concern (spec §6 slice 2) into the same spec because the relay is
undesignable without deciding who owns truth.

**Out of scope (later slices behind the same seam):** presence / cursors /
awareness (v2), an inner character-level text CRDT (scenario C), the `.cairn/`
persistence sidecar (slice 3), an `automerge`-backed adapter, and server-side
multi-tenant hosting.

---

## 1. Starting state (verified)

- **Domain (`cairn-domain/src/crdt.rs`):** `BlockDoc` (RGA of blocks + per-block
  author-priority LWW register), `BlockOp{Insert,Delete,SetContent}`,
  `Edit{InsertAfter,UpdateText,Remove}`, `BlockId{replica,counter}`,
  `Author{Human,Agent}`. **Serde-free by design.**
- **Port (`cairn-ports/src/lib.rs`):** `trait CollabSession` — `is_active`,
  `open`, `edit(&mut, &NotePath, Edit) -> Vec<BlockOp>`, `merge_remote`,
  `materialize(&NotePath) -> Option<String>`. Synchronous, single-threaded.
- **Adapters (`cairn-infra`):** `LocalCrdt` (in-memory, one `BlockDoc` per note;
  ops returned to caller, no transport) and `NoCollab` (inert default).
- **Daemon (`cairn-daemon/src/lib.rs`):** an axum HTTP + WS transport.
  `/command`,`/query`,`/ask` are bearer-gated; `/mcp` accepts the token as
  `?token=`; `/events` is a **one-way** `WireEvent` broadcast fan-out
  (server→client), Origin-gated only, whose forward loop deliberately ignores
  inbound frame content. A **file-watcher + auto-commit loop**
  (`run_watch_loop_timeout` → `commit_external_blocking`) already commits
  external edits after a quiet period.
- **Contract (`cairn-contract`):** owns every wire type; all derive
  `Serialize, Deserialize, TS`, with mapping fns (e.g. `agent_event_to_wire`).

**Gaps this spec closes:** `CollabSession` is **not wired into the `Engine`** at
all; the existing `/events` socket is not an op-relay; `BlockOp` has no wire
form; the watcher/auto-commit loop has no knowledge of a live session.

---

## 2. The unifying architecture — peer-replica, dumb relay, daemon commit-agent

Decision **P**: the authoritative live document is **replicated per surface**;
the daemon is a **dumb fan-out relay** that *also* holds one `BlockDoc` replica
per open note, whose sole extra job is to bridge the live CRDT tier to git.

```
 desktop replica ─┐            ┌─ browser replica        each surface: a full BlockDoc,
   (owner task)   │            │    (owner task)         actor-owned, WS handle
                  ▼            ▼
              daemon   /collab  (bidirectional, note-multiplexed, Origin + ?token=)
                  │  · relays every BlockOp to the *other* peers (dumb fan-out)
                  │  · holds ONE BlockDoc replica per open note   ── the git bridge
                  ▼
           debounce → materialize (strip IDs) → commit          ← daemon = sole disk writer
                  ▼
                 git
```

**Why P over a daemon-authoritative model (S):** S makes the daemon the single
`BlockDoc` and serializes all edits through it — reintroducing the central
lock/round-trip that ADR-0011 §2 explicitly rejects, and breaking local echo /
offline responsiveness. P is the literal realization of ADR-0011's "one
convergence core, pluggable transport": the CRDT already guarantees convergence
under any arrival order, so the daemon holding a replica adds **no** ordering
constraint. The daemon is a relay that happens to also be a participant.

**Data-ownership rule (the crux).** While a session is open on note `N`:

- the **live CRDT is truth for content** (every replica is authoritative for
  display; edits never block on a round-trip),
- the **daemon's replica is truth for disk** (single materialize-and-commit
  point — no two writers race on `N.md`),
- **git is truth at rest** — the materialized commit is the snapshot boundary
  (ADR-0011 §4). The CRDT op-log lives only between snapshots and is never
  canonical.

These are exactly ADR-0011's three tiers, now with a live network transport
plugged in under them.

---

## 3. Truth-ownership while a session is open (the hard part)

Decision **①**: **the session owns `N.md`.** The daemon is the sole writer of
`N.md` while a session is open, and external edits are folded back into the live
document rather than lost or racing.

### 3.1 Self-writes vs foreign writes

The daemon now writes `N.md` itself (on materialize) *and* a watcher is running.
Without care this feedback-loops (daemon write → watcher fires → auto-commit →
possible re-read). Resolution: the daemon **tags its own materialize writes**
(by path + content-hash, or a short echo-suppression window) so the watcher can
distinguish:

- **Self-write** (the daemon's own materialize of `N.md`): the watcher **ignores
  it** for sessioned notes. Auto-commit for `N` is deferred to the session's own
  materialize-and-commit flush (§3.3), not the generic external-edit
  auto-commit.
- **Foreign write** (someone edits `N.md` directly — e.g. vim — while a session
  is open): folded back in (§3.2).

### 3.2 External editor as a filesystem-transport peer

A foreign change to `N.md` mid-session is detected by the watcher; the daemon
re-parses the file, **block-diffs** it against its current replica, and emits the
delta as `BlockOp`s (`SetContent` for changed blocks, `Insert`/`Delete` for
added/removed blocks) merged into the live doc and fanned out to peers. The
external editor thus becomes just another peer whose transport is the
filesystem — a direct unification of the two problems ADR-0011 calls "the same
problem at different distances." **Never lose work.**

The block-diff is bounded: a re-parse (byte-span block split, per ADR-0011 §3.4)
plus an alignment of the new block sequence against the current one. Alignment
heuristic and its edge cases (block reordered vs deleted+inserted) are pinned in
the plan; correctness floor is "no silent loss," not "minimal op-set."

### 3.3 Materialize-and-commit flush

The daemon's replica materializes `N.md` on a **debounce** (quiescence after the
last op), strips block IDs (ADR-0011 §4 — the file stays pure, ID-free,
diffable markdown), writes byte-for-byte by slicing source spans (not
AST re-render), and commits. Materialize round-trips
`materialize(from_markdown(x)) == x` modulo the defined normalization. The
commit is the snapshot boundary; a short file-write lock guards only the atomic
write+commit, never editing (ADR-0011 §2).

---

## 4. Session lifecycle & catch-up

### 4.1 Lifecycle

```
open(N):   first Join on N → daemon creates a Session, seeds its replica from
           git HEAD's N.md (BlockDoc::from_markdown), subscribes the peer.
while open: live doc = truth. Peers echo local edits, ship BlockOps to the relay,
            merge remote ops. Daemon debounce-materializes + commits N.md.
close(N):  last Leave / disconnect on N → final materialize+commit → drop the
           Session (its replica, its broadcast channel, its participant set).
```

### 4.2 Catch-up — snapshot as an op-set

Decision **①**. Block IDs are **session-scoped and shared** across peers so
same-block edits are recognized (ADR-0011 §3.3); a joiner therefore **cannot**
independently parse `N.md` (it would mint fresh IDs and never converge). It must
receive the live `BlockDoc` state from a peer that already holds it — the
daemon's ever-present replica.

The snapshot is represented as an **op-set**, not serialized internal state: the
daemon exports the minimal `Vec<BlockOp>` that reconstructs its replica's current
state, and the joiner `merge`s them (merge is idempotent + commutative — the CRDT
guarantee). Consequently **`BlockOp` is the only CRDT type that ever crosses the
wire**: steady-state ops and catch-up snapshots are the same type.

New domain capability: `BlockDoc::state_as_ops(&self) -> Vec<BlockOp>`. (This is
also what the future `.cairn/` resume sidecar wants, so it is not throwaway.)
The op-set carries the real (shared) block IDs and enough Lamport information for
a joiner to initialize its clock above what it observes.

---

## 5. Wire protocol

### 5.1 Endpoint & auth

Decision: a **new `/collab` route** (bidirectional WS), **note-multiplexed** (one
WS per surface carries many open notes, keyed by a `note` field), gated by the
**Origin allowlist + `?token=`** (as strong as `/command`, but WS-compatible like
`/mcp` — browsers cannot set an `Authorization` header on a WS handshake).

Rejected: extending `/events`. It is a one-way `WireEvent` firehose; overloading
it would spam every engine-event consumer with CRDT ops and force collab peers to
filter engine noise, and its Origin-only auth is too weak for a **mutating**
(file-writing, commit-creating) protocol. Separate socket, separate auth, smaller
blast radius.

### 5.2 Wire types (mirror in `cairn-contract`)

Decision **①**: `cairn-domain` stays serde-free; `BlockOp` is mirrored as a wire
type in `cairn-contract` with `From`/`Into` mappings (the established
`agent_event_to_wire` pattern) and `TS` bindings for the desktop/browser
surfaces.

```rust
// cairn-contract — the only CRDT type on the wire + the collab envelope
#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WireBlockOp {
    Insert { id: WireBlockId, after: Option<WireBlockId>, lamport: u64,
             kind: BlockKind, text: String },
    Delete { id: WireBlockId, lamport: u64 },
    SetContent { id: WireBlockId, text: String, lamport: u64, author: Author },
}
#[derive(Serialize, Deserialize, TS)]
pub struct WireBlockId { pub replica: u64, pub counter: u64 }

pub fn block_op_to_wire(op: BlockOp) -> WireBlockOp;   // domain → wire
pub fn block_op_from_wire(w: WireBlockOp) -> BlockOp;  // wire → domain

#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CollabClientMsg {
    Join  { note: String, replica: u64 },   // seed me + subscribe
    Op    { note: String, op: WireBlockOp }, // one local edit
    Leave { note: String },                  // unsubscribe
}
#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CollabServerMsg {
    Joined   { note: String },                        // ack
    Snapshot { note: String, ops: Vec<WireBlockOp> },  // catch-up (state-as-ops)
    Op       { note: String, op: WireBlockOp },        // a peer's edit, fanned out
    Error    { note: String, message: String },        // e.g. replica-id collision
}
```

`BlockKind` and `Author` are already `cairn-contract`-friendly enums (or are
mirrored alongside). `WireBlockOp` gets a round-trip property test:
`block_op_from_wire(block_op_to_wire(op)) == op`.

### 5.3 On-the-wire session flow

```
desktop → /collab:  Join{ "standup.md", replica:1 }
        ← Joined{ "standup.md" }
        ← Snapshot{ "standup.md", ops:[…] }        # daemon seeds (from git HEAD if first Join)
desktop → Op{ "standup.md", Insert{…} }            # daemon merges into its replica, fans out
browser ← Op{ "standup.md", Insert{…} }
        …                                          # daemon debounce-materializes + commits
desktop → Leave{ "standup.md" }                    # last peer gone → final commit, drop Session
```

---

## 6. Daemon-side design

`AppState` gains a collab **session registry**:

```rust
Mutex<HashMap<NotePath, Session>>
struct Session {
    doc: BlockDoc,                        // the daemon's replica (the git bridge)
    peers: broadcast::Sender<CollabServerMsg>, // fan-out to participants
    participants: HashSet<u64>,           // replica ids; last-out triggers teardown
    // + debounce/dirty state for materialize-and-commit
}
```

- **Join:** validate Origin + token; reject a colliding `replica` id with
  `Error`; create the `Session` on first join (seed from git HEAD via the engine
  store); reply `Joined` then `Snapshot{ doc.state_as_ops() }`; subscribe the
  peer to `peers`.
- **Op:** `block_op_from_wire` → `doc.merge` → fan out to the *other* peers →
  mark dirty (schedule debounced flush).
- **Leave / disconnect:** remove the participant; if none remain, final flush
  and drop the `Session`.
- **Watcher integration:** self-write suppression + foreign-edit fold-back per
  §3. The generic auto-commit loop skips notes with an open session (their
  commits come from the session flush).

Concurrency: engine work already runs under a mutex via `spawn_blocking`; the
collab registry is an independent `Mutex` so a collab bug cannot stall
`/command`. Materialize+commit runs through the engine store under its existing
locking.

---

## 7. Client-side adapter — actor / single-owner

Decision **③**: a **single owner task holds each `BlockDoc`**; the
`CollabSession` impl is a thin, cloneable **handle**. No shared mutex — all
convergence logic runs single-threaded in the owner, data-race-free by
construction.

```
     engine (sync)                    OWNER TASK (async, sole holder of BlockDoc)      WS
 edit(path, Edit) ─cmd+oneshot─►  ┌────────────────────────────────────────┐
                  ◄──Vec<BlockOp>─ │ loop select {                          │
                                   │   cmd  = rx.recv()   → apply_local      │─► Op{…} out
                                   │                        reply ops        │
                                   │   frame= ws.next()   → merge_remote     │◄─ Op/Snapshot in
                                   │   tick = debounce    → materialize      │
                                   │ }                                       │
                                   └────────────────────────────────────────┘
```

- `CollabSession` methods send a command to the owner over an mpsc; `edit()`
  does `tx.blocking_send(cmd)` then `reply_rx.blocking_recv()` — a **synchronous
  blocking bridge**, so the trait stays sync and the engine's call sites do not
  churn.
- **Trait impact (minimal):** `&mut self → &self` (the handle is cheaply
  cloneable; the owner task holds the real state). Methods stay synchronous.
- The owner task also owns the WS connection: outbound `Op`s from local edits,
  inbound `Op`/`Snapshot` merged into the doc. Clients do **not** commit —
  materialize/commit is the daemon's job.
- `NoCollab` remains the default; the engine is unchanged when collaboration is
  off.

The daemon side mirrors this ownership shape per note (the `Session` is the
owner; §6).

---

## 8. Testing (part of done)

1. **Wire convergence (the headline proof):** two in-process clients connected
   to a real `/collab` WS, editing one note, converge to identical
   `materialize()` output.
2. **External-edit fold-back:** a foreign write to `N.md` mid-session appears in
   both replicas (external editor as filesystem peer); no lost work.
3. **Join catch-up:** a late joiner receives `Snapshot`, `merge`s the op-set, and
   converges to the same state (shared block IDs preserved).
4. **Commit boundary:** a session flush produces exactly one clean, ID-free
   markdown commit; `materialize(from_markdown(x)) == x` round-trip holds.
5. **Self-write suppression:** the daemon's own materialize does not trigger the
   external-edit auto-commit / feedback loop.
6. **Auth/origin:** `/collab` rejects a bad Origin and a missing/invalid token
   before upgrade.
7. **`WireBlockOp` round-trip** property test: `from(to(op)) == op`.
8. Existing `BlockDoc` convergence property tests (commutativity, associativity,
   idempotence) remain green, unchanged.

Anything not testable is called out at plan time (e.g. the block-diff alignment
heuristic is tested by construction, but ambiguous reorder-vs-replace cases are
documented, not exhaustively covered, in this slice).

---

## 9. Slice / PR boundary

This is a large vertical. The natural cut, to be finalized in `writing-plans`:

- **PR-1 — relay + protocol, no disk.** `WireBlockOp` + `CollabClient/ServerMsg`
  in `cairn-contract` (+ mappings, TS, round-trip test); `BlockDoc::state_as_ops`
  in `cairn-domain`; daemon `/collab` route + session registry + fan-out;
  wire-convergence + join-catch-up + auth tests. No file writes yet
  (materialize is a no-op stub on the daemon side).
- **PR-2 — the git bridge + client adapter.** Daemon commit-agent (debounced
  materialize+commit, self-write suppression, foreign-edit fold-back, watcher
  reconciliation); the client actor-based `CollabSession` adapter; the
  external-edit and commit-boundary tests.

Both PRs go through the merge queue (branch → PR → `gh pr merge --auto
--squash`); `cargo fmt --check` + `clippy --locked` + `cargo test --workspace`
green per PR.

---

## 10. Locked decisions (index)

| # | Decision | Choice |
|---|---|---|
| A | Scope | Full live vertical (fold truth-ownership + local wiring in) |
| P | Where the doc lives | Peer-replica + dumb relay; daemon holds one replica as commit-agent |
| ① | Truth while open | Session owns `N.md`; self-writes suppressed; foreign edits folded back as ops |
| ① | Join / catch-up | `Snapshot` = state-as-ops (`Vec<BlockOp>`); shared session-scoped IDs |
| ① | Wire types | `BlockOp` mirrored as `WireBlockOp` in `cairn-contract`; domain stays serde-free |
| — | Endpoint | New `/collab`, note-multiplexed, Origin + `?token=` |
| ③ | Client concurrency | Actor / single-owner task; `CollabSession` is a sync handle (`&self`) |

---

## 11. Open questions deferred to the plan

- ~~Exact self-write tagging mechanism (content-hash vs echo-suppression window)
  and the debounce interval / commit-message format for the session flush.~~
  **Resolved in §12 (PR-2 / A1).**
- Block-diff alignment algorithm for foreign-edit fold-back, and its documented
  edge cases (reorder vs delete+insert). **(A2 — the non-clobber guard in §12
  defers to this; A1 skips-and-warns rather than folding back.)**
- Whether `Author` on a folded-back external edit is `Human` (an external editor
  is a human surface) — presumed yes.
- Precise `state_as_ops` minimality (winning content only, or include stashed
  loser-versions so a joiner sees identical stashes).
- Replica-id assignment/registration and collision policy specifics.
- Reconnection/resume semantics after a dropped WS (re-`Join` → fresh
  `Snapshot`; any client-side op buffering during disconnect).

---

## 12. Commit boundary — PR-2 / A1 (the daemon commit-agent)

**Date:** 2026-07-24. **Status:** Approved. This section resolves the §11
commit-boundary open questions and pins the A1 slice (A2 = foreign-edit
fold-back is a follow-up). Roadmap: Epic A.

**Scope of A1:** the daemon becomes the sole disk writer for a sessioned note —
it materializes its `BlockDoc` replica to `N.md` and git-commits it. A2
(foreign-edit fold-back, spec §3.2) is explicitly *not* here; A1's floor is
"never silently clobber foreign work."

### 12.1 Flush trigger — centralized debounced ticker

The daemon commits **debounced-on-quiescence**, not per-op (per-op = commit
storm). A single `AppState::run_collab_flush_pass(&self, debounce)` performs one
pass; `main.rs` spawns a ticker (`loop { sleep(250ms); run_collab_flush_pass(quiet) }`)
on a blocking thread, symmetric with the existing watcher auto-commit loop.
Exposing the pass as one callable unit makes it deterministically testable (tests
call it with `debounce = 0`, no sleeps).

- **Debounce interval:** reuse `config.sync.quiet_period_ms` (default 2000ms) —
  one coalescing knob shared with the watcher; 250ms tick granularity.
- **Commit message:** `format!("cairn: collab sync {note}")`.
- **Lock order (single global order, no nesting):** phase 1 materializes the due
  sessions *under the collab lock only* (no engine call); the lock is released;
  phase 2 takes the *engine lock* per note to write+commit. The collab lock is
  never held across an engine call, and the engine lock is never held while
  acquiring the collab lock — preserving the engine-then-collab discipline the
  seed path already follows (§6, `run_collab` seeds off the executor outside the
  collab lock).

### 12.2 Self-write suppression — reuse the engine stat-guard

The materialize write is routed through **`Engine::write_note`**, which records
the file's `(mtime, len)` stamp (and content-hash memo) *as it writes*. When the
file-watcher subsequently fires `Changed(N)`, `Engine::apply_change`'s existing
stat-guard / hash-dedup drops the echo — no re-index, no re-emit, no
feedback-loop commit. **No new suppression API is introduced**; this is exactly
the mechanism command-writes already use (`apply_write` vs `apply_change`). The
daemon must never write `N.md` via `std::fs`/the store directly, or the
fingerprint is not pre-seeded and the watcher treats it as a foreign edit.

### 12.3 Non-clobber guard (A1 floor; A2 replaces with fold-back)

Each `Session` records `last_written: String` (initialized to the seed). Before
phase-2 writing, the flush reads the current on-disk `N.md`; if it differs from
`last_written`, a **foreign edit** intervened → the flush **warns and skips the
write** (never overwrites), leaving the session dirty so nothing in-memory is
lost. A2 turns this skip into a block-diff fold-back (§3.2). This guarantees the
spec's "never lose work" floor without implementing the alignment algorithm yet.

### 12.4 Seed source — working tree (A1)

A1 keeps seeding from the working tree (`Engine::read_note`), not git HEAD, so
`last_written == on-disk` at open and the first flush cannot false-positive the
§12.3 guard. HEAD-seed plus reconciliation of pre-existing uncommitted changes is
an A2 concern (the `Engine::note_at(path, "HEAD")` capability already exists).

### 12.5 Session teardown (resolves the ticker's close-flush gap)

`Leave`/disconnect removes the participant. If the session is then empty **and
clean**, it is dropped immediately (PR-1 behavior). If empty **and dirty**, it is
kept so the ticker can finalize it: an empty session is flushed ignoring the
debounce, then reaped. The last edit before everyone leaves therefore still
persists (best-effort — a mid-flush process exit may lose it, as with the
watcher).

### 12.6 Watcher vs session commit

Under `auto_commit = false` (the default) the watcher never commits, so there is
no conflict. Under `auto_commit = true` both the watcher quiet-loop and the
collab ticker may commit the whole working tree; both guard on
`has_uncommitted_changes()` and the collab write is stat-suppressed against
re-ingest, so the worst case is one idempotent extra commit. Full arbitration
(watcher defers per-note to the session flush, spec §3.1) is A2.

### 12.7 A1 locked decisions

| # | Decision | Choice |
|---|---|---|
| a | Flush trigger | Centralized debounced ticker; one testable `run_collab_flush_pass` |
| a | Debounce / msg | Reuse `quiet_period_ms` (2000ms), 250ms tick; `cairn: collab sync {note}` |
| b | Self-write suppression | Route through `Engine::write_note`; reuse the stat-guard (no new API) |
| c | Non-clobber | Compare disk vs `last_written`; diverged → warn + skip (A2 → fold-back) |
| d | Seed source | Working tree (`read_note`); HEAD-seed deferred to A2 |
| e | Teardown | Empty+clean → drop; empty+dirty → ticker finalizes then reaps |
| f | Watcher/session | No conflict when `auto_commit=false`; idempotent-safe otherwise; arbitration = A2 |
