# A2 fold-back: two minor correctness/UX gaps

Follow-up to #152 (A2 foreign-edit fold-back) and #157 (read-error/delete wipe
fix). Tracks issue #158. Both gaps are minor; neither blocks the feature.

## Gap #1 — foreign delete vs. concurrent live edit (silent loss)

### Problem

`BlockDoc::merge` (`crates/cairn-domain/src/crdt.rs`, `BlockOp::Delete` arm)
tombstones a block unconditionally. The same-block `SetContent` vs `SetContent`
path preserves the loser in `Entry.stash` ("never lose work", spec §3.2), but the
delete/edit path surfaces nothing. So a block that was concurrently edited and
deleted, or a folded foreign `SetContent` onto a block a live user deleted, keeps
its content in the entry but exposes it nowhere — silent loss.

### Key finding

The `Delete` arm **never destroys content**. It only sets `tombstone = true`; it
touches neither `text` nor `stash`. And `merge_set_content` runs its LWW register
on a block regardless of tombstone. So the concurrently-edited content is already
retained in the entry — the LWW winner in `Entry.text`, prior losers in
`Entry.stash`. The loss is only that a tombstoned entry's content is (a) not in
`materialize()` (tombstoned) and (b) not returned by `stashed()` (which returns
only `stash`, not `text`).

This reframes the fix from "retain the content" (already done) to "surface it".

### Convergence hazard (rejected approach)

The obvious fix — push `text` into `stash` at delete time — is **order-dependent
and diverges**. Seed `"orig"`, concurrent `Delete` (D) + `SetContent "edited"` (S):

| order    | resulting `stash`      |
|----------|------------------------|
| S then D | `[orig, edited]`       |
| D then S | `[orig, orig]`         |

The replicas hold different stashes forever. Rejected.

### Chosen design: read-side recovery accessor (B1)

Leave the `Delete` arm untouched (zero convergence risk; `materialize()`
convergence is already pinned). Add one derived, order-independent accessor:

```rust
/// Content versions retained in the CRDT but NOT shown by materialize():
///   live block  -> stash            (losers; winner is already visible)
///   tombstoned  -> [text] ++ stash  (nothing is visible → all recoverable)
/// Convergent: a pure function of the (convergent) register + tombstone,
/// so equal inputs give equal output on every replica.
pub fn recoverable(&self, id: BlockId) -> Vec<String> {
    let Some(e) = self.entries.get(&id) else { return Vec::new() };
    let mut out = Vec::new();
    if e.tombstone {
        out.push(e.text.clone());
    }
    out.extend(e.stash.iter().cloned());
    out
}
```

Because `Delete` moves nothing, both op orders converge to the same entry state
(`text = "edited"`, `stash = ["orig"]`, `tombstone = true`), so
`recoverable` returns the same retained set `{edited, orig}` on every replica.
This is a **derived projection** (the same shape as `materialize()`), not
memoization — nothing derived is stored, so there is no divergence surface.

`recoverable` is added alongside `stashed`, not folded into it: `stashed`'s
documented contract is "loser versions", and a tombstoned block's `text` is a
*winner*, so overloading `stashed` would muddy its meaning.

### Documented caveat

`recoverable` surfaces a tombstoned block's content whether or not the delete was
actually concurrent with an edit. Distinguishing a genuinely-concurrent
delete/edit from a plain delete needs causality tracking (version vectors) the
model does not have. Over-preserving is consistent with the "never lose work"
floor, so a plain delete's content also reads as recoverable. This is intended.

### Ordering note

`stash` is a `BTreeSet<(author_rank, lamport, text)>`, so the order of
`recoverable` (and of `stashed`) is a pure function of content — byte-for-byte
identical across replicas in the multi-loser case, not merely set-equal. Each
loser is keyed by its own `(author_rank, lamport, text)`, so the same text lands
under the same key whichever side lost; re-applying a losing op is idempotent
(the set dedups). Tests assert `Vec` equality across merge orders, not just
sorted-set equality.

(Historical: the stash was originally a push-ordered `Vec<String>` that converged
only as a set; the ordering was hardened to the current keyed set.)

### Tests

- `deleted_block_keeps_concurrent_edit_recoverable_either_order`: apply S and D
  in both orders; assert `recoverable(id)` is equal (sorted) across orders and
  contains the edited text.
- `foreign_set_content_onto_deleted_block_is_recoverable`: live delete + foreign
  `SetContent`; assert the foreign text is recoverable, both orders.
- `recoverable_on_live_block_returns_only_losers`: a live (non-tombstoned) block
  returns its stash and never its visible `text`.

## Gap #2 — whitespace-only foreign edit triggers a needless rewrite + commit

### Problem

`collab::fold_foreign` (`crates/cairn-daemon/src/collab.rs`) always advances
`last_written` to the consumed `foreign` bytes and sets `dirty = true`, even when
the block-diff produced zero ops (the foreign edit differed only in whitespace
that `parse_blocks` normalizes away). The next flush pass then rewrites the file
to canonical form and commits `cairn: collab sync` with no semantic change.

### Fix

When the fold produces no ops, still reconcile `last_written = foreign` (so the
divergence is not re-detected every pass — the flush pass and `note_foreign_edit`
both compare disk against `last_written`), but do **not** set `dirty` and do not
bump `last_op`. An already-dirty session stays dirty (we only ever *set*
`dirty`, never clear it here), so a real pending edit still flushes.

```rust
let ops = sess.doc.fold_foreign(&base, foreign);
let produced = !ops.is_empty();
for op in ops { /* fan out */ }
// Always reconcile so a re-fold does not re-detect the same divergence.
sess.last_written = foreign.to_string();
if produced {
    sess.dirty = true;
    sess.last_op = Instant::now();
}
```

The daemon tolerates the foreign whitespace on disk (matches `last_written`)
rather than fighting it with a normalizing commit — harmless and convergent.

### Tests

- `fold_foreign_whitespace_only_edit_does_not_mark_dirty`: seed a session, fold a
  foreign edit that normalizes to the same blocks; assert no ops fanned out,
  `dirty` unchanged (false), and `last_written` advanced to the foreign bytes.
- Regression guard that a semantic foreign edit still sets `dirty` (existing
  `fold_foreign_merges_disk_edit_fans_out_and_advances_baseline` covers this).

## Definition of done

- `cargo build/test --workspace --locked`; `clippy --all-targets --locked
  -D warnings`; `fmt --check`.
- Conventional commits → PR against `main` → `gh pr merge --auto` (merge queue
  owns strategy; never manually update a queued branch).
