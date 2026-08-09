# Collab restore path — design

**Date:** 2026-08-09
**Status:** approved
**Follows:** [collab recovery surface](2026-08-09-collab-recovery-surface-design.md) (#161)
**Domain decision:** [0014-crdt-collaboration-model](../../decisions/0014-crdt-collaboration-model.md)

## Problem

The view-only recovery surface (#161) exposes `BlockDoc::recoverable_blocks()
-> Vec<RecoverableBlock { id, tombstoned, versions: Vec<String> }>` over `/collab`
as `Recover`/`Recoverable`. It only *shows* retained content; there is no way to
bring it back. This adds the restore half: a client action that promotes a chosen
recoverable version into the live document, fanned out to peers, materialized on
the next flush.

The restore op must stay commutative + idempotent, like every other `BlockOp`.

## Decisive constraint

The recovery surface is **version-indexed**. `versions` covers two retained kinds:

- **live block** → its stashed LWW *losers* (the winner is already visible)
- **tombstoned block** → *all* its content (winner + losers)

Any restore mechanism must cover **both** kinds, addressable by `version_index`.

## Chosen approach — promote a recoverable version to a NEW block (no new `BlockOp`)

Restore re-inserts the chosen version's text as a **new live block**, anchored
right after the (retained) source block. It reuses the existing, already-convergent
`BlockOp::Insert` via the existing `Edit::InsertAfter` intent. Delete stays
delete-wins; the tombstone and the source block are never mutated.

```rust
// cairn-domain/src/crdt.rs
impl BlockDoc {
    /// Re-insert a recoverable version of `id` as a NEW live block, anchored right
    /// after the (retained) source block so it lands in its original slot. Restore
    /// is additive: it never mutates the tombstone or the source register — delete
    /// stays delete-wins. Returns the op(s) to fan out; empty when `id` is unknown
    /// or `version_index` is out of range.
    pub fn restore(&mut self, id: BlockId, version_index: usize) -> Vec<BlockOp> {
        let versions = self.recoverable(id);       // same ordering the client saw
        let Some(text) = versions.get(version_index).cloned() else { return Vec::new() };
        let Some(kind) = self.entries.get(&id).map(|e| e.kind) else { return Vec::new() };
        self.apply_local(Edit::InsertAfter { after: Some(id), kind, text, author: Author::Human })
    }
}
```

### Why this is convergent by construction

The produced op is a plain `BlockOp::Insert` with a fresh `BlockId`. Insert's
commutativity + idempotency are already proven and tested. The anchor
`after: Some(id)` references a block that always exists (Delete tombstones, never
removes the entry), so RGA causal delivery holds unchanged.

### In-place restore for free

The tombstone keeps its `after` pointer in the RGA tree; `materialize()` walks the
tree and skips tombstones. Inserting `after` the tombstone id, the new block sorts
ahead of the deleted block's former successor (siblings order by newest
`ins_lamport` first), so materialized order `A, R, N` reproduces the deleted
block's exact original slot `A, [D], N`. In-place restore falls out of the RGA
anchor — no un-delete, no touching delete convergence.

`restore` becomes the first reader of `Entry.kind` (drops its `#[allow(dead_code)]`).

## Rejected alternative — un-delete / tombstone-lamport (handoff Option A)

Add a monotonic delete generation (tombstone-lamport LWW) and a `Revive` op.
Rejected because:

- It only addresses the **tombstoned** subset — it un-deletes to the block's
  current winner. It has no answer for restoring a **live block's stashed loser**
  (no tombstone to flip), so it does not cover the surface it is meant to complete.
- It cannot target `version_index` without an extra `SetContent`.
- It changes `Entry`, `Delete` merge, and `state_as_ops` (which today fabricates
  the delete lamport — a tombstone register would need the *real* delete lamport
  re-emitted or snapshots diverge). More surface, new register law to prove.

Option A's only edge — "restore in place" — is delivered by the chosen approach
anyway via the RGA anchor.

## Wire + daemon layering (mirrors Recover, but mutating)

One new client variant; **no new `CollabServerMsg`** — restore emits a normal `Op`.

```rust
// cairn-contract/src/lib.rs
pub enum CollabClientMsg {
    // ...
    /// Promote a recoverable version of a block back into the live document
    /// (mutating; fanned out to peers, unlike view-only `Recover`).
    Restore { note: String, id: WireBlockId, version_index: usize },
}
```

```rust
// cairn-daemon/src/collab.rs — new arm, mirrors the Op arm's fanout + dirty.
// Authored as the daemon (origin: DAEMON_REPLICA), exactly like fold_foreign, so
// the resulting Insert reaches EVERY client including the requester (which does
// not yet have the block; no client's replica equals DAEMON_REPLICA = u64::MAX).
CollabClientMsg::Restore { note, id, version_index } => {
    // NotePath::new(&note) or Error; then under the collab lock:
    //   let ops = sess.doc.restore(block_id_from_wire(id), version_index);
    //   for op in ops { sess.peers.send(Fanout { origin: DAEMON_REPLICA,
    //       msg: CollabServerMsg::Op { note, op: block_op_to_wire(op) } }); }
    //   if !ops.is_empty() { sess.dirty = true; sess.last_op = Instant::now(); }
}
```

- Session docs are seeded with `DAEMON_REPLICA` (u64::MAX), so daemon-minted
  restore inserts share the session doc's single counter — unique, disjoint from
  client-minted ids.
- No service-layer mapping: the daemon consumes the contract `CollabClientMsg`
  directly. `block_id_from_wire` / `block_op_to_wire` already exist.
- No new wire struct — `version_index` is inline; the web-ui recovery UI just gets
  a new variant on the exported TS union.

## Deliberate non-goals (YAGNI)

- **No dedup** of recovery-view vs live blocks: restoring twice yields two copies
  (honest + convergent; symmetric with the "never lose work" floor).
- **No client-chosen anchor**: derived as `after: Some(source_id)`. Add a
  configurable anchor only when a real need appears.
- The tombstone stays listed as recoverable after a restore (restore is
  repeatable) — deduping it against live content would require non-convergent
  guesswork.

## Test plan (TDD the convergence law)

Domain (`crdt.rs`):
1. `restore_reinserts_deleted_content_in_original_slot` — delete a middle block,
   restore v0 → `materialize()` back to `A, R, N`.
2. `restore_and_concurrent_delete_converge_either_order` — apply
   `[Delete(id), Insert(after=id)]` vs the reverse on two replicas → identical
   `materialize()`; the restored block survives (fresh id, unaffected by the
   tombstone).
3. `restore_of_live_stash_loser_inserts_adjacent_and_converges`.
4. `restore_is_repeatable_two_replicas_converge` — both restore → two blocks,
   identical across replicas.
5. `restore_invalid_id_or_index_is_noop`.

Daemon (`collab.rs`):
6. `restore_fans_out_and_marks_dirty` — the arm produces an Insert fanned out with
   `origin: DAEMON_REPLICA`, session left dirty; unknown/invalid restore is inert.

Contract (`lib.rs`): `Restore` round-trips through serde (tagged `restore`).

## Definition of done

- `cargo build/test --workspace --locked`; `clippy --all-targets --locked -D
  warnings`; `fmt --check`.
- Convergence law tested in both application orders.
- Conventional commits → rebase on origin/main → PR `--base main` →
  `gh pr merge --auto` (merge queue owns strategy; never manually update a queued
  branch).
