# ADR-0011: CRDT collaboration — block-level convergence over a git-canonical store

**Status:** Accepted
**Date:** 2026-06-16

## Context

Two writers can edit the same note concurrently and lose each other's work:
`LocalFsStore::write` overwrites the whole file, so the last writer wins and git
never knows there were two edits (git only sees a file at commit time, below the
live-edit layer). The concrete, present case is **you + a tau agent** on one
note (the `AgentRuntime` seam is live and streams over seconds) and **you across
your own surfaces** (desktop / CLI / browser). Distinct humans co-editing over a
network is a non-goal for this first slice.

Collaboration is sub-project ⑤ from the engine design
(`2026-06-01-cairn-engine-design.md` §9, §12) and exists today only as a proven
seam: `CollabSession` with an `is_active()`-only `NoCollab` adapter. This ADR
records the two foundational, hard-to-reverse decisions — **the CRDT choice and
the git-reconciliation model**. The full design is in
`docs/superpowers/specs/2026-06-16-crdt-collaboration-design.md`.

This is an architectural decision because it fixes (a) the convergence
granularity that all later transport/persistence/UI slices build on and (b)
where CRDT state stands relative to git, which is cairn's canonical store.

## Decision

### 1. One convergence core, pluggable transport

The local race (agent ⇄ you) and network collaboration are the same problem at
different distances; only the transport differs. We build **one** CRDT core and
plug transports under it (in-memory now, daemon `/events` WS / relay later).
The local-vs-collaboration distinction collapses to "which transport is
plugged in."

### 2. Reject a lock as the editing mechanism

A lock prevents loss by preventing concurrency. cairn agents *stream*, so a lock
either freezes the human for seconds or starves the agent, and degrades across
machines. A lock is retained **only** around the atomic materialize-and-commit
flush (a ~millisecond file-write lock), never around editing.

### 3. Block-level (structured) CRDT, not character-level

The convergence unit is a **block**, not a character. A character CRDT can
interleave a true same-span overlap into nonsense (`"hello world"` +
`"hello there"` → `"hello worldthere"`) — rare and self-correcting for two
humans, but likely and harmful for an agent rewriting a region (cairn's primary
case). Block granularity makes different-block edits conflict-free and confines
collisions to one block.

- A **block** is a top-level CommonMark element (heading, paragraph, each list
  item, atomic code fence, blockquote, table, thematic break, frontmatter),
  identified by its **source byte span**. Prose boundary: a maximal run of
  non-blank lines; a blank line ends a block.
- **Document** = a sequence CRDT over blocks (insert/delete/move,
  conflict-free). **Block content** = an opaque LWW register. Same-block
  concurrency follows a **policy**, never interleave: **agent ⇄ human →
  human-wins, agent version stashed**; human ⇄ human → surface-both. No
  `"worldthere"`, ever.
- **No inner text CRDT** in this slice (character-level human↔human
  same-paragraph merge is scenario C, deferred; it nests inside a block later
  without changing the model).

### 4. Git commit = the snapshot boundary; CRDT state is never canonical

- Block IDs live **only in ephemeral CRDT state**; they are **stripped on
  materialize** so the `.md` file stays pure, ID-free, diffable markdown. Git
  never sees an ID. Re-opening a note re-parses markdown with fresh IDs — stable
  identity is required only within one live session.
- The **materialized markdown commit is the snapshot boundary**. The CRDT
  op-log lives only between snapshots and is never the source of truth.
- **Cross-session / offline reconciliation falls back to git 3-way merge** (the
  `MergePolicy` port) on materialized files — *not* the CRDT. CRDT state may
  later be cached as a gitignored `.cairn/` sidecar (ADR-0006 style) purely as a
  resume accelerator.
- Materialize round-trips **byte-for-byte** by slicing source spans, not
  re-rendering an AST (re-rendering reflows whitespace → noisy diffs).

### 5. Hand-roll the minimal CRDT for slice 1; `automerge` is the presumptive future library

The model (block sequence CRDT + LWW registers) is small. Slice 1 hand-rolls a
pure `BlockDoc` in `cairn-domain` — it *is* the deliverable (the in-memory
convergence proof), takes no heavy dependency before a transport exists, and
sits behind `CollabSession` so a library adapter can replace it. `automerge`
(Rust-native, op-log + binary sync, local-first lineage) is named as the
presumptive **network**-tier adapter; `yrs` is rejected (editor-centric,
weakest fit for "git canonical / CRDT ephemeral"). No CRDT dependency is taken
in slice 1.

## Consequences

- A pure, property-tested `BlockDoc` in `cairn-domain` proves convergence
  (commutativity / associativity / idempotence) in memory; `LocalCrdt` wraps it
  behind the existing `CollabSession` port; `NoCollab` stays the default so the
  engine is unchanged when collaboration is off.
- The user's notes repo stays pure plain markdown — no IDs, no CRDT artifacts;
  clean git diffs preserved.
- **Accepted:** two edits to different sentences of the *same* block collide
  (loser's whole-block version stashed, not merged) — rare, since agents act at
  block granularity and writers tune granularity with blank lines.
- **Accepted:** an editor offline across a materialize boundary gets git 3-way
  merge, not character-perfect CRDT merge — the price of keeping files canonical
  and avoiding a second source of truth.
- Transport, persistence sidecar, network relay, presence, UI, and an inner text
  CRDT are later additive slices behind the same seam.
