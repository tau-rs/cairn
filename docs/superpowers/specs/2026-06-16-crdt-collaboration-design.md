# CRDT Collaboration — Design Spec

**Date:** 2026-06-16
**Status:** Approved (design); ready for implementation planning
**Scope of this document:** the convergence *model* for multi-writer real-time
collaboration in cairn, its hexagonal seam (`CollabSession` port + a domain
`BlockDoc` type), and the first build slice (an in-memory, property-tested
convergence proof). Transport (relay / daemon `/events` reuse), persistence
sidecar, presence/cursors, UI, and server-side multi-tenant hosting are
**out of scope here** and are later additive slices behind the same seam.

This is sub-project ⑤ (CRDT collaboration) from the engine design
(`2026-06-01-cairn-engine-design.md` §9, §12). It exists today only as a
"proven seam": `CollabSession` with an `is_active()`-only `NoCollab` adapter.

---

## 1. Problem

Two writers can edit the same note at the same instant and lose each other's
work. `LocalFsStore::write` overwrites the whole file, so the last writer wins
and the other's edit vanishes — and git never knows there were two edits,
because git only sees the file *after* it is committed, below the live-edit
layer.

The two writers that matter **now**:

1. **You + a tau agent** on the same note. The `AgentRuntime` seam is live
   (`cairn ask` streams answers over seconds); an agent that rewrites/appends
   to a note while you type is a real, present race.
2. **You across your own surfaces** — desktop app, CLI, browser tab — all
   bound to one cairn.

Multiple *distinct humans* co-editing over a network (Google-Docs style) is a
**non-goal for this slice** — it adds a relay/presence tier and nothing in the
convergence core changes when it arrives.

### Why not just a lock

A lock prevents loss by preventing concurrency. cairn agents *stream*: an
agent writing a 300-word block into a note holds the work for seconds. A lock
either freezes the human for that whole time or starves the agent — it turns
"real-time collaboration" into "take turns," and degrades further across
machines (central lock server, stuck locks on crash). We reject a lock as the
*editing* mechanism. A short lock is retained only around the atomic
materialize-and-commit flush (§5), which is a ~millisecond file-write lock, not
a lock on editing.

### Why not git alone

Git is canonical **at rest** and reconciles *committed, divergent histories
across sessions* via 3-way merge. It operates at the file/commit boundary and
has no visibility into two writers racing over an unsaved file in the same
second. Git is the durable floor, not the live-merge mechanism.

---

## 2. The unifying idea: one convergence core, pluggable transport

The "local edition problem" (agent ⇄ you, one machine) and the "collaboration
layer" (people over a network) are the **same problem at different distances**.
The only difference is how an edit travels between writers.

```
            ┌───────────────────────────────────────────────┐
            │         CONVERGENCE CORE  (the model)          │
            │  merge N divergent edits of one note,          │
            │  deterministically, never lose work            │
            └───────────────────────────────────────────────┘
                     ▲                            ▲
              edits  │                            │  edits
         ┌───────────┴──────────┐      ┌──────────┴────────────┐
         │  LOCAL transport     │      │  NETWORK transport     │
         │  in-process channel  │      │  relay / WS / server   │
         │  agent ⇄ you, tabs   │      │  machine ⇄ machine     │
         └──────────────────────┘      └────────────────────────┘
            slice 1 (this spec)            later slice (scenario C)
```

One CRDT core. Transport is the pluggable axis. The local-vs-collaboration
split collapses to "which transport is plugged in." We build the core once and
prove it with a local/in-memory transport; the network transport reuses the
identical core.

---

## 3. Convergence model: a structured (block-level) CRDT

The convergence unit is a **block**, not a character. A character-level (text)
CRDT guarantees convergence and loses no characters, but on a true same-span
overlap it can *interleave into nonsense* (`"hello world"` + `"hello there"` →
`"hello worldthere"`). That garble is rare and self-correcting for two humans
typing, but **likely and harmful for an agent rewriting a region** — cairn's
primary case. Block granularity makes different-block edits conflict-free (the
common case) and confines true collisions to a single block, where an explicit
policy resolves them without garble.

### 3.1 What is a block

A block is a **top-level CommonMark structural element**, identified by its
**source byte span** in the markdown:

| Construct | Block? |
|---|---|
| Heading (`## Notes`) | one block |
| Paragraph (maximal run of consecutive non-blank lines) | one block |
| **Each** list item (`- call Bob`) | one block per item |
| Fenced code block | one **atomic** block (never split inside) |
| Blockquote / table / thematic break (`---`) | one block each |
| YAML frontmatter | one block (held as note metadata) |

**Boundary rule for prose:** a block is a maximal run of consecutive non-blank
lines; a **blank line** ends it. A lone newline does *not* start a new block
(CommonMark renders wrapped lines as one paragraph).

```
no blank line between → ONE block         blank line between → TWO blocks
  The review went well.                     The review went well.
  We hit our targets.
                                            We hit our targets.
```

Writers opt into finer granularity simply by separating thoughts with blank
lines — each becomes its own block. The file stays plain markdown.

**Granularity is leaf-level** — the finest *meaningful* markdown unit (each
list item is a block, so an agent appending items never collides with you
editing another item), but never finer (never split a code fence or a
paragraph mid-sentence).

### 3.2 Two operations, one of them coarse on purpose

```
DOCUMENT = sequence CRDT over blocks      ── insert / delete / move blocks
BLOCK    = { id, kind, text }             ── text is an OPAQUE register (LWW)
```

- **Across blocks (different IDs):** a sequence CRDT handles insert/delete/move.
  Always conflict-free. You edit the intro while the agent appends a list item →
  both land, every time.
- **Within a block (same ID):** the block's text is an **opaque register**.
  Concurrent edits do **not** interleave; they follow a policy:
  - **agent ⇄ human → human-wins LWW**, and the agent's losing block version is
    **stashed** (recoverable / surfaced), never silently dropped. An agent can
    never overwrite a paragraph you are editing.
  - **human ⇄ human → surface-both**, the human picks.

A block merges as a *whole*: you get your paragraph or theirs, never a blend.
No `"worldthere"`, ever.

**Explicitly no inner text CRDT in this slice.** Character-level merging of two
humans typing in the *same* paragraph is scenario C (a non-goal). It can be
added later as a nested text CRDT *inside* a block without changing the block
model — a deferred refinement, not a one-way door.

**Accepted cost:** because the block is the unit, two edits to *different
sentences of the same paragraph* still collide (the loser's whole-block version
is stashed, not merged). This is rare in practice — agents operate at block
granularity (append/replace whole blocks), and writers can tune granularity
with blank lines.

### 3.3 Block identity vs. plain-markdown purity

A block CRDT needs a **stable ID per block** so concurrent edits to "the same
block" are recognized. Plain markdown has no IDs, and embedding them
(`<!-- id:a3f -->`) would break cairn's core promise (diffable, portable,
CLI-readable plain markdown). Resolution:

```
   LIVE CRDT doc (memory / .cairn sidecar)   ← block IDs live HERE only
     b2 heading   "# Standup"
     b3 paragraph "Notes from today."
          │
          │ materialize  (strip IDs → pure markdown)
          ▼
   meeting.md   ── plain markdown, NO ids, pristine, diffable
          │  commit
          ▼
   git
```

- Block IDs exist **only in live/ephemeral CRDT state**. Concurrent editors
  share one doc, so they share IDs → same-block collisions are detected.
- On **materialize**, IDs are stripped → the `.md` file is pure plain markdown.
  Git never sees an ID.
- On **re-open**, markdown is re-parsed into blocks with **fresh** IDs. Stable
  identity is only required *within one live session*; across-session identity
  is git's job (§4), not the CRDT's.

### 3.4 Round-trip fidelity (a hard parser constraint)

Materialize must round-trip **byte-for-byte**:
`materialize(from_markdown(x)) == x` (modulo a defined normalization). We split
markdown by **source byte ranges** (a CommonMark parser such as
`pulldown-cmark` exposes source offsets) and slice the original text into block
spans. We do **not** re-render markdown from an AST — re-rendering normalizes
whitespace and reflows lists, producing noisy git diffs. **Slice, don't
re-render.**

---

## 4. Relationship to git: git commit = the snapshot boundary

```
  CRDT layer   ─ real-time merge. agent & you edit concurrently, no lock,
   (live)        no loss, deterministic.            ← the live tier
       │ materialize (on idle / debounce / save)
       ▼
  meeting.md   ─ plain markdown, canonical bytes on disk
       │ commit
       ▼
  git          ─ durable history + cross-session 3-way merge   ← the durable tier
```

- **The materialized markdown commit is the snapshot boundary.** The CRDT
  op-log lives only *between* snapshots. It is **never canonical**.
- **Persistence (optional, later):** CRDT state may be cached as a `.cairn/`
  sidecar (gitignored, like the on-disk index in ADR-0006) for fast session
  resume. It is an accelerator, never the source of truth.
- **Cross-session / offline reconciliation falls back to git 3-way merge** on
  the materialized files (the `MergePolicy` port), *not* to the CRDT. A CRDT
  op-log is not carried across a materialize boundary. This keeps files pure
  and makes the live tier a true optional accelerator.

**Accepted tradeoff:** an editor offline across a materialize boundary (e.g.
reconnecting after a week) does not get character-perfect CRDT merge; it gets
git 3-way merge on the materialized files. This is the right tradeoff: it keeps
the canonical file pristine and prevents a second source of truth.

---

## 5. Architecture (hexagonal)

```
cairn-domain   BlockDoc  ── convergence type: blocks, ops, merge, materialize
               (pure, no I/O, property-tested)      ← the in-memory proof
                  ▲
cairn-ports    CollabSession  ── expands from is_active() to the live-session
               seam; transport-blind (no WS, no relay)
                  ▲
cairn-infra    NoCollab (default, unchanged)  +  LocalCrdt (in-mem, wraps BlockDoc)
```

The model is a **pure domain type** (like `Note`, `Graph`); the **port**
abstracts a live session over a note; the **slice-1 adapter** is in-memory
only. Daemon `/events` WS reuse, relay, and the persistence sidecar are later
slices behind the same port.

### 5.1 Indicative domain shapes (refined during planning)

```rust
// cairn-domain — pure; the convergence proof lives here
struct BlockDoc { /* replica id, lamport clock, ordered blocks (RGA) */ }

struct BlockId(/* (ReplicaId, counter) — globally unique, live-only */);

enum BlockOp {
    Insert { id: BlockId, after: Option<BlockId>, kind: BlockKind, text: String, ts: Lamport },
    Delete { id: BlockId, ts: Lamport },
    SetContent { id: BlockId, text: String, ts: Lamport, author: Author },
    Move { id: BlockId, after: Option<BlockId>, ts: Lamport },
}

enum Author { Human, Agent }

impl BlockDoc {
    fn from_markdown(replica: ReplicaId, src: &str) -> Self; // parse → blocks, fresh IDs
    fn apply_local(&mut self, edit: Edit) -> Vec<BlockOp>;   // returns ops to share
    fn merge(&mut self, op: BlockOp);                        // commutative, idempotent
    fn materialize(&self) -> String;                         // → pure markdown, IDs stripped
}
```

- **Block ordering:** an RGA / fractional-index sequence CRDT keyed by
  `BlockId`. Insert/delete/move are commutative and idempotent.
- **Block content:** a Lamport-timestamped LWW register with a deterministic
  **total** order `(author_rank, lamport)`, the block **text** breaking a true
  `(author_rank, lamport)` tie (greater text wins). `author_rank` makes `Human`
  beat `Agent`; among equal rank the higher Lamport wins; the text tiebreak
  guarantees convergence even when two concurrent edits collide on author and
  Lamport. The loser's text is retained in a stash list on the block, never
  dropped. (An earlier draft tiebroke on `ReplicaId`, but the op carried only
  the block's *birth* replica — constant across a block's content edits — so it
  was inert and same-author/same-Lamport edits could diverge; the text tiebreak
  replaces it.)
- **Convergence guarantee:** for any set of ops applied to any number of
  replicas in any order, with arbitrary duplication, all replicas
  `materialize()` to identical markdown.

### 5.2 Port shape (transport-blind)

The current port is `fn is_active(&self) -> bool`. It expands to express
opening a live session over a note, applying a local edit, merging a remote op,
and materializing — **without** naming any transport. The exact trait is
finalized in the plan; `NoCollab` keeps returning "inactive" and remains the
default adapter so the engine runs unchanged when collaboration is off.

### 5.3 Library decision

The model is small (a block sequence CRDT + LWW registers), far less than what
the large libraries target (rich collaborative *text*).

- **Slice 1: hand-roll the minimal `BlockDoc` in `cairn-domain`.** It *is* the
  deliverable ("the model + a single in-memory convergence proof"); it adds no
  heavy dependency before a transport exists; its convergence laws are ours to
  property-test directly; and it sits behind the port so a library adapter can
  replace it later.
- **Presumptive future library: `automerge` (automerge-rs).** Rust-native,
  document-as-op-log, built-in binary sync protocol, local-first lineage that
  matches the git-canonical model. When the **network** transport (scenario C)
  lands, an automerge-backed adapter can replace `LocalCrdt` behind
  `CollabSession`. We name it now so the port does not paint us into a corner;
  we **do not take the dependency** in slice 1.
- `yrs` (Yjs in Rust) is rejected as the presumptive library: editor-centric,
  heaviest, weakest fit for "git is canonical / CRDT is ephemeral," awkward
  block-move support.

---

## 6. Build slices

| # | Slice | Scope |
|---|---|---|
| **1** | **Convergence proof (this spec's build target)** | domain `BlockDoc` (block sequence CRDT + LWW registers + materialize), markdown block parser with byte-span round-trip, `CollabSession` port shape, `LocalCrdt` adapter, property tests. **No transport, no UI, no relay, no persistence.** |
| 2 | Local wiring | route agent (`apply_local` with `Author::Agent`) and engine writes through `CollabSession`; materialize-and-commit flush with the atomic file-write lock; surface stashed loser-versions. |
| 3 | Persistence sidecar | optional `.cairn/` CRDT-state cache for fast resume (ADR-0006-style). |
| 4 | Network transport | op relay over the daemon `/events` WS; scenario C; presumptive `automerge` adapter. |
| 5 | Inner text CRDT (if needed) | nested character-level merge *within* a block for human↔human same-paragraph co-typing. |

Each later slice gets its own spec → plan → implementation cycle behind the
same `CollabSession` seam.

### Slice 1 = one PR

domain `BlockDoc` + property-tested in-memory convergence + markdown block
round-trip + the `CollabSession` port shape + `LocalCrdt` adapter.

---

## 7. Testing (part of done)

- **Convergence property tests (the proof):** `proptest` — for any sequence of
  `BlockOp`s applied to N replicas in any order, with duplication, all replicas
  `materialize()` to identical markdown. This is the CRDT law set:
  commutativity, associativity, idempotence of `merge`.
- **Round-trip test:** `materialize(from_markdown(x)) == x` for arbitrary
  markdown inputs (byte fidelity → clean git diffs), modulo the defined
  normalization.
- **Policy tests:** agent ⇄ human same block → human wins, agent version
  stashed and recoverable; human ⇄ human → both retained for selection.
- **Block-boundary tests:** wrapped paragraph → one block; blank-line-separated
  lines → separate blocks; code fence stays atomic; each list item is a block.

---

## 8. Non-goals (this slice)

- UI / editor integration.
- Presence / cursors / awareness.
- Server-side multi-tenant hosting.
- A network relay or any transport (op exchange is in-memory only).
- An inner character-level text CRDT.
- Carrying a CRDT op-log across a materialize boundary (offline → git 3-way).

---

## 9. Open questions deferred to later specs/ADRs

- Exact `CollabSession` trait signature and `LocalCrdt` session lifecycle.
- Materialize cadence (on idle / debounce / on save) — slice 2.
- The precise markdown normalization allowed in the round-trip equality.
- Move-reorder semantics under concurrency (interleave vs. last-move-wins).
- `.cairn/` sidecar serialization format — slice 3.
- automerge adapter mapping (blocks ↔ automerge list/map) — slice 4.
- Whether stashed loser-versions surface as note sidecars (git-versioned) or
  ephemeral UI state.
