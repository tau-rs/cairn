# A2 — CRDT Foreign-Edit Fold-Back Implementation Plan

> **Historical record — 2026-08-27.** This plan shipped as written; it is kept
> as the record of what was built, not as a description of current behavior.
> The fold-back, the HEAD seed, and the watcher's defer-to-flush are all still
> in force, but PR #179 moved commit policy into the engine: the flush no longer
> commits, so the "removes the double-commit under `auto_commit=true`" rationale
> below is moot — neither the watcher nor the flush commits, and one seal loop
> commits once per idle session. `auto_commit` now defaults **true**. See
> `docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md` §14
> and ADR-0012 §Update.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace A1's "warn-and-skip on a foreign on-disk edit" with a block-diff fold-back that merges the foreign edit into the live `BlockDoc`, fans it out to peers, and re-materializes — so no work is lost when someone edits `N.md` directly while a session is open.

**Architecture:** A pure LCS block-diff in `cairn-domain` (`BlockDoc::fold_foreign`) turns a foreign markdown revision into `BlockOp`s against the live replica's real block IDs. The daemon calls it from the one A1 hook point (`run_collab_flush_pass`, the `disk != baseline` branch) inside a single collab-lock critical section (`collab::fold_foreign`), advancing the baseline to the consumed disk bytes and leaving the session dirty so the next debounced pass writes the merged result. The file-watcher defers a sessioned `Changed(N)` to that flush; sessions seed from git HEAD and reconcile any pre-existing uncommitted worktree edit through the same fold path.

**Tech Stack:** Rust (workspace, MSRV 1.88), `cairn-domain` (serde-free CRDT), `cairn-daemon` (axum + tokio), `tokio::sync::broadcast`, `git2` via `cairn-infra`, `tracing`.

## Global Constraints

- MSRV **1.88**; every crate is `#![forbid(unsafe_code)]`.
- `cairn-domain` stays **serde-free** and dependency-free of infra (hexagonal: dependencies point inward).
- Errors: `thiserror` at boundaries, `anyhow` internally. Domain returns plain values / `PortError` only where it already does.
- The lock discipline is inviolable: **never hold the collab lock across an engine call, and never acquire the collab lock while holding the engine lock.** Sequential acquire→release→acquire is allowed.
- Every folded op is authored `Author::Human`.
- Green bar per task: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --workspace`.
- Conventional commits, imperative, scoped (`feat(collab):`, `feat(domain):`, `test(collab):`).
- Merge via the queue: `gh pr merge <n> --auto` — the queue owns the squash strategy; **do not pass `--squash`**. Do not commit unless the executor is told to. Check `git branch` before any commit (the working dir is shared).
- Spec of record: `docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md` §13 (and §3.1/§3.2 for intent).

---

### Task 1: Pure LCS block-diff edit script (`cairn-domain`)

A pure sequence diff over block texts, with **no CRDT knowledge** — the building block `fold_foreign` maps onto live IDs. Isolated so it is unit-testable in one file.

**Files:**
- Create: `crates/cairn-domain/src/blockdiff.rs`
- Modify: `crates/cairn-domain/src/lib.rs` (add `mod blockdiff;`)
- Test: inline `#[cfg(test)]` in `crates/cairn-domain/src/blockdiff.rs`

**Interfaces:**
- Produces (crate-internal): `pub(crate) enum DiffStep { Keep { bi: usize, fi: usize }, Delete { bi: usize }, Insert { fi: usize } }` and `pub(crate) fn lcs_edit_script(base: &[String], foreign: &[String]) -> Vec<DiffStep>`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing tests**

Create `crates/cairn-domain/src/blockdiff.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn identical_sequences_are_all_keeps() {
        let s = lcs_edit_script(&v(&["a", "b"]), &v(&["a", "b"]));
        assert_eq!(
            s,
            vec![DiffStep::Keep { bi: 0, fi: 0 }, DiffStep::Keep { bi: 1, fi: 1 }]
        );
    }

    #[test]
    fn pure_insertion_in_the_middle() {
        let s = lcs_edit_script(&v(&["a", "c"]), &v(&["a", "b", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Insert { fi: 1 },
                DiffStep::Keep { bi: 1, fi: 2 },
            ]
        );
    }

    #[test]
    fn pure_deletion() {
        let s = lcs_edit_script(&v(&["a", "b", "c"]), &v(&["a", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Delete { bi: 1 },
                DiffStep::Keep { bi: 1, fi: 1 },
            ]
        );
    }

    #[test]
    fn substitution_is_delete_then_insert() {
        // "b" -> "B": no common block, so delete b then insert B, framed by keeps.
        let s = lcs_edit_script(&v(&["a", "b", "c"]), &v(&["a", "B", "c"]));
        assert_eq!(
            s,
            vec![
                DiffStep::Keep { bi: 0, fi: 0 },
                DiffStep::Delete { bi: 1 },
                DiffStep::Insert { fi: 1 },
                DiffStep::Keep { bi: 1, fi: 2 },
            ]
        );
    }

    #[test]
    fn empty_base_is_all_inserts() {
        let s = lcs_edit_script(&v(&[]), &v(&["x", "y"]));
        assert_eq!(s, vec![DiffStep::Insert { fi: 0 }, DiffStep::Insert { fi: 1 }]);
    }

    #[test]
    fn empty_foreign_is_all_deletes() {
        let s = lcs_edit_script(&v(&["x", "y"]), &v(&[]));
        assert_eq!(s, vec![DiffStep::Delete { bi: 0 }, DiffStep::Delete { bi: 1 }]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-domain blockdiff`
Expected: FAIL — `DiffStep` / `lcs_edit_script` not found (won't compile).

- [ ] **Step 3: Implement the diff**

Prepend to `crates/cairn-domain/src/blockdiff.rs` (above the test module):

```rust
//! Pure LCS sequence diff over block texts: the edit script that
//! `BlockDoc::fold_foreign` maps onto live block IDs. No CRDT knowledge, no I/O.

/// One step aligning a `base` block sequence to a `foreign` one. Indices are
/// into the respective `parse_blocks` outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffStep {
    /// `base[bi]` and `foreign[fi]` are byte-identical — keep the block.
    Keep { bi: usize, fi: usize },
    /// `base[bi]` has no counterpart — removed on disk.
    Delete { bi: usize },
    /// `foreign[fi]` has no counterpart — added on disk.
    Insert { fi: usize },
}

/// Longest-common-subsequence edit script between two block-text sequences.
/// Deletes/Inserts are emitted in source order; a substitution surfaces as a
/// `Delete` immediately followed by an `Insert` (the caller pairs them into a
/// content update). O(n·m) time/space — block counts per note are small.
pub(crate) fn lcs_edit_script(base: &[String], foreign: &[String]) -> Vec<DiffStep> {
    let (n, m) = (base.len(), foreign.len());
    // dp[i][j] = LCS length of base[i..] and foreign[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if base[i] == foreign[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut steps = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if base[i] == foreign[j] {
            steps.push(DiffStep::Keep { bi: i, fi: j });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            steps.push(DiffStep::Delete { bi: i });
            i += 1;
        } else {
            steps.push(DiffStep::Insert { fi: j });
            j += 1;
        }
    }
    while i < n {
        steps.push(DiffStep::Delete { bi: i });
        i += 1;
    }
    while j < m {
        steps.push(DiffStep::Insert { fi: j });
        j += 1;
    }
    steps
}
```

Add to `crates/cairn-domain/src/lib.rs` alongside the other `mod` lines (e.g. next to `mod block;` / `mod crdt;`):

```rust
mod blockdiff;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-domain blockdiff`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/blockdiff.rs crates/cairn-domain/src/lib.rs
git commit -m "feat(domain): pure LCS block-diff edit script for fold-back"
```

---

### Task 2: `BlockDoc::fold_foreign` (`cairn-domain`)

Map the edit script onto the live replica's real block IDs and apply it as ops. Index-align when the doc is exactly the base; content-match fallback otherwise; never drop a foreign block (spec §13.3).

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs` (add `fold_foreign` + private `text_of`)
- Test: inline `#[cfg(test)]` in `crates/cairn-domain/src/crdt.rs`

**Interfaces:**
- Consumes: `crate::blockdiff::{lcs_edit_script, DiffStep}` (Task 1); `crate::block::parse_blocks`; existing `BlockDoc::{materialize, block_ids_in_order, apply_local}`; `Edit`, `BlockOp`, `BlockId`, `Author`.
- Produces: `pub fn fold_foreign(&mut self, base: &str, foreign: &str) -> Vec<BlockOp>` on `BlockDoc`.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `crates/cairn-domain/src/crdt.rs`:

```rust
#[test]
fn fold_foreign_inserts_a_new_block_after_its_neighbor() {
    let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
    let base = doc.materialize(); // "a\n\nb\n"
    let ops = doc.fold_foreign(&base, "a\n\nmiddle\n\nb\n");
    assert_eq!(doc.materialize(), "a\n\nmiddle\n\nb\n");
    assert!(!ops.is_empty(), "an insert produced ops to fan out");
}

#[test]
fn fold_foreign_substitutes_content_in_place_keeping_the_id() {
    let mut doc = BlockDoc::from_markdown(1, "keep\n\nold\n");
    let id_before = doc.block_ids_in_order();
    let base = doc.materialize();
    doc.fold_foreign(&base, "keep\n\nnew\n");
    assert_eq!(doc.materialize(), "keep\n\nnew\n");
    // The edited block kept its identity (SetContent, not delete+insert).
    let id_after = doc.block_ids_in_order();
    assert_eq!(id_before, id_after, "substitution preserved block IDs");
}

#[test]
fn fold_foreign_deletes_a_removed_block() {
    let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n\nc\n");
    let base = doc.materialize();
    doc.fold_foreign(&base, "a\n\nc\n");
    assert_eq!(doc.materialize(), "a\n\nc\n");
}

#[test]
fn fold_foreign_is_noop_when_disk_equals_base() {
    let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
    let base = doc.materialize();
    let ops = doc.fold_foreign(&base, &base);
    assert!(ops.is_empty(), "identical foreign produces no ops");
    assert_eq!(doc.materialize(), base);
}

#[test]
fn re_fold_against_the_consumed_bytes_does_not_duplicate_inserts() {
    // Models the daemon's `last_written = disk` rule: after folding foreign_1,
    // the NEXT fold uses foreign_1 as the base — so a further edit does not
    // re-insert foreign_1's block.
    let mut doc = BlockDoc::from_markdown(1, "a\n");
    let base0 = doc.materialize(); // "a\n"
    doc.fold_foreign(&base0, "a\n\nb\n"); // absorb b
    let base1 = "a\n\nb\n".to_string(); // the consumed bytes become the new base
    doc.fold_foreign(&base1, "a\n\nb\n\nc\n"); // absorb c
    let out = doc.materialize();
    assert_eq!(out, "a\n\nb\n\nc\n");
    assert_eq!(out.matches("b").count(), 1, "b appears exactly once");
}

#[test]
fn fold_foreign_preserves_every_foreign_block_when_base_diverged() {
    // A peer advanced the doc (materialize != base): the fallback path must not
    // lose any foreign block's text (the "no silent loss" floor).
    let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
    let base = doc.materialize();
    // Peer edits the doc so it no longer equals `base`.
    let id_b = doc.block_ids_in_order()[1];
    doc.apply_local(Edit::UpdateText {
        id: id_b,
        text: "b (peer)".into(),
        author: Author::Human,
    });
    // Foreign edit (made against the original `base`) adds a block.
    doc.fold_foreign(&base, "a\n\nb\n\nc from disk\n");
    let out = doc.materialize();
    assert!(out.contains("c from disk"), "foreign addition preserved");
    assert!(out.contains("a"), "untouched block preserved");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-domain fold_foreign`
Expected: FAIL — `fold_foreign` not found (won't compile).

- [ ] **Step 3: Implement `fold_foreign` and `text_of`**

Add these two methods inside `impl BlockDoc` in `crates/cairn-domain/src/crdt.rs` (place `fold_foreign` after `apply_local`; `text_of` can go just below it):

```rust
    /// Fold a foreign on-disk revision of this note back into the live document:
    /// diff `foreign` against `base` (the markdown the daemon last wrote) and
    /// apply the delta as ops so no foreign work is lost. Every produced op is
    /// authored `Human` (an external editor is a human surface). Returns the ops
    /// to fan out to peers. See design spec §13.3.
    pub fn fold_foreign(&mut self, base: &str, foreign: &str) -> Vec<BlockOp> {
        let base_texts: Vec<String> = crate::block::parse_blocks(base)
            .into_iter()
            .map(|b| b.text)
            .collect();
        let foreign_blocks = crate::block::parse_blocks(foreign);
        let foreign_texts: Vec<String> =
            foreign_blocks.iter().map(|b| b.text.clone()).collect();
        let live = self.block_ids_in_order();

        // Positional alignment is exact only when the live doc is byte-identical
        // to `base`: then base_texts[i] corresponds to live[i]. Otherwise a peer
        // advanced the doc concurrently — fall back to content matching so no
        // foreign block is lost (spec §13.3).
        let index_aligned = self.materialize() == base && base_texts.len() == live.len();

        // PASS 1 (no mutation): resolve the edit script into concrete actions,
        // pairing each gap's deletes/inserts into content substitutions. Base
        // IDs are resolved against the pre-mutation doc so a SetContent cannot
        // perturb a later block's content-match.
        enum Action {
            Delete(BlockId),
            SetContent(BlockId, String),
            Insert {
                anchor: Option<BlockId>,
                kind: crate::block::BlockKind,
                text: String,
            },
        }
        let mut consumed = vec![false; live.len()];
        let mut resolve_base = |bi: usize, this: &BlockDoc| -> Option<BlockId> {
            if index_aligned {
                return Some(live[bi]);
            }
            for (k, id) in live.iter().enumerate() {
                if !consumed[k] && this.text_of(*id) == Some(base_texts[bi].as_str()) {
                    consumed[k] = true;
                    return Some(*id);
                }
            }
            None
        };

        let steps = crate::blockdiff::lcs_edit_script(&base_texts, &foreign_texts);
        let mut actions: Vec<Action> = Vec::new();
        let mut anchor: Option<BlockId> = None; // last kept live id before the gap
        let mut gap_del: Vec<usize> = Vec::new();
        let mut gap_ins: Vec<usize> = Vec::new();

        let mut flush_gap =
            |anchor: Option<BlockId>,
             gap_del: &mut Vec<usize>,
             gap_ins: &mut Vec<usize>,
             actions: &mut Vec<Action>,
             resolve_base: &mut dyn FnMut(usize, &BlockDoc) -> Option<BlockId>,
             this: &BlockDoc| {
                let pair = gap_del.len().min(gap_ins.len());
                for k in 0..pair {
                    let fi = gap_ins[k];
                    match resolve_base(gap_del[k], this) {
                        // Substitution in place: keep the block's identity.
                        Some(id) => actions.push(Action::SetContent(id, foreign_texts[fi].clone())),
                        // Base block vanished but foreign has content here: insert
                        // it rather than drop it (floor).
                        None => actions.push(Action::Insert {
                            anchor,
                            kind: foreign_blocks[fi].kind,
                            text: foreign_texts[fi].clone(),
                        }),
                    }
                }
                for &bi in &gap_del[pair..] {
                    // Only apply a delete when the base block is uniquely matched;
                    // losing a delete never loses content, so ambiguity is skipped.
                    if let Some(id) = resolve_base(bi, this) {
                        actions.push(Action::Delete(id));
                    }
                }
                for &fi in &gap_ins[pair..] {
                    actions.push(Action::Insert {
                        anchor,
                        kind: foreign_blocks[fi].kind,
                        text: foreign_texts[fi].clone(),
                    });
                }
                gap_del.clear();
                gap_ins.clear();
            };

        for step in &steps {
            match *step {
                DiffStep::Keep { bi, .. } => {
                    flush_gap(
                        anchor,
                        &mut gap_del,
                        &mut gap_ins,
                        &mut actions,
                        &mut resolve_base,
                        self,
                    );
                    // Advance the anchor to this kept block's live id.
                    anchor = resolve_base(bi, self);
                }
                DiffStep::Delete { bi } => gap_del.push(bi),
                DiffStep::Insert { fi } => gap_ins.push(fi),
            }
        }
        flush_gap(
            anchor,
            &mut gap_del,
            &mut gap_ins,
            &mut actions,
            &mut resolve_base,
            self,
        );

        // PASS 2 (mutation): apply the actions, chaining consecutive inserts that
        // share a gap anchor so multi-block insertions land in order.
        let mut produced: Vec<BlockOp> = Vec::new();
        let mut prev_anchor: Option<BlockId> = None;
        let mut prev_insert_id: Option<BlockId> = None;
        for action in actions {
            match action {
                Action::Delete(id) => {
                    prev_insert_id = None;
                    produced.extend(self.apply_local(Edit::Remove { id }));
                }
                Action::SetContent(id, text) => {
                    prev_insert_id = None;
                    produced.extend(self.apply_local(Edit::UpdateText {
                        id,
                        text,
                        author: Author::Human,
                    }));
                }
                Action::Insert { anchor, kind, text } => {
                    let after = if prev_insert_id.is_some() && prev_anchor == anchor {
                        prev_insert_id
                    } else {
                        anchor
                    };
                    let ops = self.apply_local(Edit::InsertAfter {
                        after,
                        kind,
                        text,
                        author: Author::Human,
                    });
                    if let Some(BlockOp::Insert { id, .. }) = ops.first() {
                        prev_insert_id = Some(*id);
                    }
                    prev_anchor = anchor;
                    produced.extend(ops);
                }
            }
        }
        produced
    }

    /// Current text of a live (non-tombstoned) block, for content-match folding.
    fn text_of(&self, id: BlockId) -> Option<&str> {
        self.entries
            .get(&id)
            .filter(|e| !e.tombstone)
            .map(|e| e.text.as_str())
    }
```

Add `DiffStep` to the imports at the top of `crdt.rs` (it already has `use crate::block::{join_blocks, BlockKind};`):

```rust
use crate::blockdiff::DiffStep;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-domain fold_foreign`
Expected: PASS (6 new tests). Then `cargo test -p cairn-domain` to confirm the existing CRDT property tests still pass.

- [ ] **Step 5: Fmt/clippy and commit**

```bash
cargo fmt --all && cargo clippy -p cairn-domain --all-targets --locked -- -D warnings
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(domain): BlockDoc::fold_foreign block-diff fold-back (Option A)"
```

---

### Task 3: `collab::fold_foreign` critical section (`cairn-daemon`)

The single collab-lock step that merges a foreign edit into the session replica, fans the ops to peers, and advances the baseline. Added first (non-breaking) so Task 4 can switch the flush pass onto it.

**Files:**
- Modify: `crates/cairn-daemon/src/collab.rs` (add `fold_foreign` + a test-only fanout subscriber helper)
- Test: `#[cfg(test)] mod flush_tests` in `crates/cairn-daemon/src/collab.rs`

**Interfaces:**
- Consumes: `Collab`, `Session`, `Fanout`, `lock`, `DAEMON_REPLICA`, `block_op_to_wire`, `CollabServerMsg`, `BlockDoc::fold_foreign` (Task 2).
- Produces: `pub(crate) fn fold_foreign(collab: &Collab, path: &NotePath, foreign: &str)`; test-only `pub(crate) fn test_subscribe(collab: &Collab, path: &NotePath) -> broadcast::Receiver<Fanout>` and `pub(crate) fn fanout_op(f: &Fanout) -> Option<cairn_domain::BlockOp>`.

- [ ] **Step 1: Write the failing test**

Add to `mod flush_tests` in `crates/cairn-daemon/src/collab.rs`:

```rust
#[test]
fn fold_foreign_merges_disk_edit_fans_out_and_advances_baseline() {
    let reg = registry();
    let p = NotePath::new("n.md").unwrap();
    // Session seeded + last_written == "a\n" (one block "a").
    insert_dirty_session(&reg, &p, "a\n", vec![]);
    add_participant(&reg, &p, 7);
    // A peer is subscribed to the fan-out channel.
    let mut rx = test_subscribe(&reg, &p);

    // Foreign on-disk edit: appended a block "b".
    fold_foreign(&reg, &p, "a\n\nb\n");

    // (1) Merged into the daemon replica.
    {
        let reg = lock(&reg);
        let sess = reg.get(&p).unwrap();
        assert!(sess.doc.materialize().contains("b"), "foreign edit in replica");
        // (2) Baseline advanced to the consumed disk bytes.
        assert_eq!(sess.last_written, "a\n\nb\n");
        // (3) Session stays dirty so the next pass writes the merged result.
        assert!(sess.dirty);
    }
    // (4) Fanned out to peers: at least one Insert op arrived.
    let f = rx.try_recv().expect("a folded op was fanned out");
    assert!(matches!(fanout_op(&f), Some(cairn_domain::BlockOp::Insert { .. })));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-daemon fold_foreign_merges`
Expected: FAIL — `fold_foreign` / `test_subscribe` / `fanout_op` not found.

- [ ] **Step 3: Implement `fold_foreign` and the test helpers**

Add to `crates/cairn-daemon/src/collab.rs` (near `settle_flush`):

```rust
/// Fold a foreign on-disk edit into a session's live replica, under the collab
/// lock only. Merges the block-diff of `foreign` against the session's baseline
/// into `doc`, fans the produced ops out to peers, advances `last_written` to the
/// consumed `foreign` bytes, and leaves the session dirty so the next flush pass
/// writes the merged result. A no-op if the session was reaped meanwhile. This is
/// the fold-back critical section that replaces A1's conflict-skip (spec §13.1/§13.2).
pub(crate) fn fold_foreign(collab: &Collab, path: &NotePath, foreign: &str) {
    let mut reg = lock(collab);
    let Some(sess) = reg.get_mut(path) else {
        return;
    };
    let base = sess.last_written.clone();
    let ops = sess.doc.fold_foreign(&base, foreign);
    for op in ops {
        let _ = sess.peers.send(Fanout {
            origin: DAEMON_REPLICA,
            msg: CollabServerMsg::Op {
                note: path.as_str().to_string(),
                op: block_op_to_wire(op),
            },
        });
    }
    // The consumed disk bytes are the new baseline: a re-fold on the next pass
    // diffs foreign→newer-foreign, never re-minting these Insert IDs (spec §13.1).
    sess.last_written = foreign.to_string();
    sess.dirty = true;
    sess.last_op = Instant::now();
}
```

Add the test-only helpers (next to `add_participant`):

```rust
/// Subscribe to a session's fan-out channel to observe folded/relayed ops. Test-only.
#[cfg(test)]
pub(crate) fn test_subscribe(collab: &Collab, path: &NotePath) -> broadcast::Receiver<Fanout> {
    let reg = lock(collab);
    reg.get(path).expect("session exists").peers.subscribe()
}

/// Extract the domain op from a fan-out envelope. Test-only.
#[cfg(test)]
pub(crate) fn fanout_op(f: &Fanout) -> Option<cairn_domain::BlockOp> {
    match &f.msg {
        CollabServerMsg::Op { op, .. } => Some(block_op_from_wire(op.clone())),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-daemon fold_foreign_merges`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/collab.rs
git commit -m "feat(collab): fold_foreign critical section — merge foreign edit + fan out"
```

---

### Task 4: Wire fold-back into the flush pass; remove `Conflict` (`cairn-daemon`)

Replace the A1 skip-and-warn branch with the fold call, delete the now-dead `FlushOutcome::Conflict` variant and its test, and prove the end-to-end fold + fan-out at the `AppState` level.

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (`run_collab_flush_pass`)
- Modify: `crates/cairn-daemon/src/collab.rs` (remove `FlushOutcome::Conflict` + its arm in `settle_flush`; delete `settle_conflict_reaps_abandoned_but_keeps_active`)
- Test: `#[cfg(test)] mod` in `crates/cairn-daemon/src/lib.rs`

**Interfaces:**
- Consumes: `collab::fold_foreign` (Task 3), `collab::drain_due`, `collab::settle_flush`, `collab::test_subscribe`, `collab::fanout_op`.
- Produces: unchanged public signature of `run_collab_flush_pass`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod` in `crates/cairn-daemon/src/lib.rs` (the module that already has `engine_over`, `ins`, `ins_at`):

```rust
#[test]
fn flush_folds_foreign_disk_edit_into_replica_and_fans_out() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(engine_over(tmp.path()));
    let path = NotePath::new("n.md").unwrap();

    // Open session (insert_dirty_session already sets dirty=true); baseline == "seed\n".
    collab::insert_dirty_session(&state.collab, &path, "seed\n", vec![]);
    collab::add_participant(&state.collab, &path, 1);
    // Subscribe a peer BEFORE the fold to observe fan-out.
    let mut rx = collab::test_subscribe(&state.collab, &path);

    // A foreign editor rewrites the file, adding a line.
    std::fs::write(tmp.path().join("n.md"), "seed\n\nforeign line\n").unwrap();

    // Flush pass 1: disk != baseline ⇒ fold (no write this pass).
    state.run_collab_flush_pass(std::time::Duration::ZERO);

    // Folded into the daemon replica; baseline advanced to the consumed bytes.
    let (markdown, baseline) =
        collab::test_session_markdown_and_baseline(&state.collab, &path).unwrap();
    assert!(markdown.contains("foreign line"));
    assert_eq!(baseline, "seed\n\nforeign line\n");
    // Fanned out to the peer.
    let mut saw_insert = false;
    while let Ok(f) = rx.try_recv() {
        if matches!(collab::fanout_op(&f), Some(cairn_domain::BlockOp::Insert { .. })) {
            saw_insert = true;
        }
    }
    assert!(saw_insert, "foreign block fanned out as an Insert");

    // Flush pass 2: now disk == baseline ⇒ the merged result is written+committed.
    state.run_collab_flush_pass(std::time::Duration::ZERO);
    let guard = state.engine();
    assert!(guard
        .note_at(&path, "HEAD")
        .unwrap()
        .contains("foreign line"));
}
```

This uses a `collab::` test helper so `Session`'s fields stay private. Add it to `crates/cairn-daemon/src/collab.rs` (near the other `#[cfg(test)]` helpers):

```rust
/// Current materialized replica text + baseline for a session. Test-only.
#[cfg(test)]
pub(crate) fn test_session_markdown_and_baseline(
    collab: &Collab,
    path: &NotePath,
) -> Option<(String, String)> {
    let reg = lock(collab);
    reg.get(path)
        .map(|s| (s.doc.materialize(), s.last_written.clone()))
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-daemon flush_folds_foreign`
Expected: FAIL — the flush still skips-and-warns; `sess.last_written` stays `"seed\n"` and nothing is committed (or the helper is missing → compile error). Add the `Collab` helper first if you chose that route.

- [ ] **Step 3: Rewrite the flush branch and remove `Conflict`**

In `crates/cairn-daemon/src/lib.rs`, replace the `if disk != item.baseline { … }` block in `run_collab_flush_pass` (currently lines ~295-303) with:

```rust
            if disk != item.baseline {
                drop(guard); // release the engine lock before taking the collab lock
                // Foreign on-disk edit: fold it into the live replica and fan it
                // out (collab lock only). No write this pass — the fold leaves the
                // session dirty, so the NEXT pass re-materializes the merged result
                // and writes it via the normal disk==baseline path (spec §13.1).
                tracing::info!(
                    note = %item.path.as_str(),
                    "collab flush: foreign on-disk edit; folding back"
                );
                collab::fold_foreign(&self.collab, &item.path, &disk);
                continue;
            }
```

In `crates/cairn-daemon/src/collab.rs`, delete the `Conflict` variant from `FlushOutcome`:

```rust
pub(crate) enum FlushOutcome {
    /// `write_note` landed these bytes on disk (commit may have failed — the
    /// bytes are still the on-disk truth, so they become the new baseline).
    Committed(String),
    /// `write_note` itself failed; nothing landed.
    WriteError,
}
```

Delete the `FlushOutcome::Conflict => { … }` arm in `settle_flush` (the whole match arm at lines ~319-330). Update the `settle_flush` doc-comment's "A2 fold-back" sentence to read: `// Foreign edits are folded back before the write (see `fold_foreign`), so a settled outcome is only Committed or WriteError.` Delete the test `settle_conflict_reaps_abandoned_but_keeps_active` (lines ~479-495) — its scenario is now the fold path, covered by Task 3.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon` and confirm `flush_folds_foreign_disk_edit_into_replica_and_fans_out` passes and no test references `FlushOutcome::Conflict`.
Expected: PASS. (`cargo build -p cairn-daemon` must be clean — no unused-variant warning.)

- [ ] **Step 5: Fmt/clippy and commit**

```bash
cargo fmt --all && cargo clippy -p cairn-daemon --all-targets --locked -- -D warnings
git add crates/cairn-daemon/src/lib.rs crates/cairn-daemon/src/collab.rs
git commit -m "feat(collab): fold foreign edits in the flush pass; drop Conflict skip"
```

---

### Task 5: Watcher arbitration — defer sessioned `Changed(N)` to the flush (`cairn-daemon`)

A foreign `Changed(N)` for a note with an open session must not run the generic re-index/auto-commit; it marks the session so the flush folds it, and self-write echoes are dropped. Enables prompt fan-out (spec §3.1, §13.5) and removes the double-commit under `auto_commit=true` (spec §12.6).

**Files:**
- Modify: `crates/cairn-daemon/src/collab.rs` (add `is_sessioned` + `note_foreign_edit`)
- Modify: `crates/cairn-daemon/src/lib.rs` (`apply_change_blocking` short-circuit)
- Test: `crates/cairn-daemon/tests/watch.rs` (new arbitration test) and `mod flush_tests` in `collab.rs`

**Interfaces:**
- Produces: `pub fn is_sessioned(collab: &Collab, path: &NotePath) -> bool`; `pub fn note_foreign_edit(collab: &Collab, path: &NotePath, disk: &str)` (marks the session dirty iff `disk != last_written`).
- Consumes: `AppState::{engine, collab}`, `cairn_ports::FsChange`.

- [ ] **Step 1: Write the failing tests**

Add to `mod flush_tests` in `crates/cairn-daemon/src/collab.rs`:

```rust
#[test]
fn note_foreign_edit_marks_dirty_only_on_real_divergence() {
    let reg = registry();
    let p = NotePath::new("n.md").unwrap();
    insert_dirty_session(&reg, &p, "base\n", vec![]);
    // Clear dirty to model a settled session.
    lock(&reg).get_mut(&p).unwrap().dirty = false;

    // A self-write echo (disk == last_written) must NOT re-arm the session.
    note_foreign_edit(&reg, &p, "base\n");
    assert!(!lock(&reg).get(&p).unwrap().dirty, "self-echo ignored");

    // A real foreign edit (disk != last_written) re-arms it for the flush fold.
    note_foreign_edit(&reg, &p, "base\n\nforeign\n");
    assert!(lock(&reg).get(&p).unwrap().dirty, "foreign edit marks dirty");
    assert!(is_sessioned(&reg, &p));
    assert!(!is_sessioned(&reg, &NotePath::new("other.md").unwrap()));
}
```

Add to `crates/cairn-daemon/tests/watch.rs` (this file already builds an `AppState` and calls `apply_change_blocking`; reuse its harness — check its top for the `state`/`changed(...)` helpers and mirror them):

```rust
#[test]
fn watcher_defers_sessioned_note_to_the_flush_not_generic_ingest() {
    // A Changed(N) for a note with an open session must NOT auto-commit via the
    // generic path; it marks the session dirty so the collab flush folds it.
    let (state, tmp) = state_with_tmp(); // mirror this file's existing setup helper
    let path = cairn_domain::NotePath::new("n.md").unwrap();

    // Open a session; baseline == on-disk == "base\n".
    std::fs::write(tmp.path().join("n.md"), "base\n").unwrap();
    cairn_daemon::collab_test_open(&state, &path, "base\n"); // see helper note below

    // Foreign edit on disk, then the watcher fires.
    std::fs::write(tmp.path().join("n.md"), "base\n\nforeign\n").unwrap();
    state.apply_change_blocking(&FsChange::Changed(path.clone()));

    // The flush now folds it (disk != baseline), then writes+commits on pass 2.
    state.run_collab_flush_pass(std::time::Duration::ZERO); // fold
    state.run_collab_flush_pass(std::time::Duration::ZERO); // write merged
    let guard = state.engine();
    assert!(guard.note_at(&path, "HEAD").unwrap().contains("foreign"));
}
```

> Helper note: the integration test needs to open a session and read the tmp dir. If `watch.rs` has no `state_with_tmp`, add a small module-local helper mirroring `serve()`-style setup in `collab.rs` tests (build `AppState::new(engine_over(tmp.path()))`, return `(state, tmp)`), and expose a thin `#[cfg(feature = "test-collab")]`-free public shim `cairn_daemon::collab_test_open(&AppState, &NotePath, &str)` that inserts a dirty-then-cleared session with one participant. If exposing a public shim is undesirable, move this test **into** the `lib.rs` `#[cfg(test)] mod` (same crate) where `collab::insert_dirty_session` + `collab::add_participant` are already reachable — preferred, keeps the API surface unchanged.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-daemon note_foreign_edit_marks_dirty` then `cargo test -p cairn-daemon watcher_defers`
Expected: FAIL — `note_foreign_edit` / `is_sessioned` not found; watcher still runs generic ingest.

- [ ] **Step 3: Implement the arbitration**

Add to `crates/cairn-daemon/src/collab.rs`:

```rust
/// Whether a live session is open on this note (the daemon owns `N.md`).
#[must_use]
pub fn is_sessioned(collab: &Collab, path: &NotePath) -> bool {
    lock(collab).contains_key(path)
}

/// Record a foreign on-disk edit detected by the watcher: if `disk` diverges from
/// the session's baseline, mark it dirty so the flush pass folds it (spec §13.5).
/// A no-op when there is no session or when `disk` equals the last self-write
/// (echo suppression — the daemon's own materialize writes must not re-arm it).
pub fn note_foreign_edit(collab: &Collab, path: &NotePath, disk: &str) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        if sess.last_written != disk {
            sess.dirty = true;
            sess.last_op = Instant::now();
        }
    }
}
```

In `crates/cairn-daemon/src/lib.rs`, add a session short-circuit at the top of `apply_change_blocking` (before `let mut guard = self.engine();`):

```rust
    pub fn apply_change_blocking(&self, change: &cairn_ports::FsChange) {
        // A sessioned note is owned by the daemon: defer a foreign `Changed` to
        // the collab flush (which folds it) instead of the generic re-index /
        // auto-commit. Whole-file `Removed` is not intercepted — content-edit
        // fold-back only (spec §13.5); a spurious delete self-heals on re-flush.
        if let cairn_ports::FsChange::Changed(path) = change {
            if collab::is_sessioned(&self.collab, path) {
                // Read disk OUTSIDE the collab lock (engine lock), then mark.
                let disk = self.engine().read_note(path).unwrap_or_default();
                collab::note_foreign_edit(&self.collab, path, &disk);
                return;
            }
        }
        let mut guard = self.engine();
        // …existing body unchanged…
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon note_foreign_edit_marks_dirty` and the watcher-arbitration test. Then `cargo test -p cairn-daemon` to confirm the existing `watch.rs` tests (non-sessioned notes still ingest generically) stay green.
Expected: PASS.

- [ ] **Step 5: Fmt/clippy and commit**

```bash
cargo fmt --all && cargo clippy -p cairn-daemon --all-targets --locked -- -D warnings
git add crates/cairn-daemon/src/collab.rs crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/watch.rs
git commit -m "feat(collab): watcher defers sessioned edits to the flush (arbitration)"
```

---

### Task 6: Seed from git HEAD + reconcile uncommitted worktree (`cairn-daemon`)

Seed the session replica from HEAD (not the working tree) and, when the worktree already differs from HEAD at open, mark the session dirty so the first flush folds that pre-existing edit through the same fold-back path (spec §13.4).

**Files:**
- Modify: `crates/cairn-daemon/src/collab.rs` (`run_collab` seed type + `Session` construction)
- Modify: `crates/cairn-daemon/src/lib.rs` (the `/collab` handler's seed closure)
- Test: `#[cfg(test)] mod` in `crates/cairn-daemon/src/lib.rs`

**Interfaces:**
- Produces: `pub struct Seed { pub markdown: String, pub dirty: bool }`; `run_collab`'s bound changes to `S: Fn(&NotePath) -> Seed + Clone + Send + 'static`.
- Consumes: `Engine::note_at(path, "HEAD")` and `Engine::read_note` in the daemon handler.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod` in `crates/cairn-daemon/src/lib.rs`:

```rust
#[test]
fn opening_a_session_reconciles_a_pre_existing_uncommitted_worktree_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(engine_over(tmp.path()));
    let path = NotePath::new("n.md").unwrap();

    // Commit a base version to HEAD, then leave an uncommitted worktree edit.
    {
        let mut guard = state.engine();
        let mut tap = EventTap { tx: state.events.clone(), collected: Vec::new() };
        guard.write_note(&path, "base\n", &mut tap).unwrap();
        guard.commit("seed", &mut tap).unwrap();
    }
    std::fs::write(tmp.path().join("n.md"), "base\n\nuncommitted\n").unwrap();

    // Seed a session the way the /collab handler will (HEAD + dirty-if-diverged).
    let head = state.engine().note_at(&path, "HEAD").unwrap_or_default();
    let worktree = state.engine().read_note(&path).unwrap_or_default();
    collab::insert_seeded_session(&state.collab, &path, &head, worktree != head);
    collab::add_participant(&state.collab, &path, 1);

    // First flush folds the uncommitted edit; second writes the merged result.
    state.run_collab_flush_pass(std::time::Duration::ZERO);
    state.run_collab_flush_pass(std::time::Duration::ZERO);
    assert!(state
        .engine()
        .note_at(&path, "HEAD")
        .unwrap()
        .contains("uncommitted"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-daemon opening_a_session_reconciles`
Expected: FAIL — `collab::insert_seeded_session` not found.

- [ ] **Step 3: Implement `Seed`, the seed-closure change, and the test helper**

In `crates/cairn-daemon/src/collab.rs`, add the `Seed` type and change `run_collab`'s bound + the `or_insert_with` block. Replace the seed generic and the session construction:

```rust
/// What the `/collab` seed closure returns: the HEAD markdown to seed the replica
/// (and initialize `last_written`), plus whether the working tree already diverges
/// from HEAD so the first flush folds that pre-existing edit (spec §13.4).
pub struct Seed {
    pub markdown: String,
    pub dirty: bool,
}
```

Change the signature:

```rust
pub async fn run_collab<S>(socket: WebSocket, collab: Collab, seed: S)
where
    S: Fn(&NotePath) -> Seed + Clone + Send + 'static,
```

Update the `spawn_blocking` seed and the `or_insert_with`:

```rust
                let seeded: Seed = tokio::task::spawn_blocking(move || seed_fn(&seed_path))
                    .await
                    .unwrap_or(Seed { markdown: String::new(), dirty: false });
                let joined = {
                    let mut reg = lock(&collab);
                    let sess = reg.entry(path.clone()).or_insert_with(|| {
                        let (tx, _rx) = broadcast::channel(256);
                        Session {
                            doc: BlockDoc::from_markdown(DAEMON_REPLICA, &seeded.markdown),
                            peers: tx,
                            participants: HashSet::new(),
                            // Diverged worktree ⇒ dirty so the first flush folds it.
                            dirty: seeded.dirty,
                            last_op: Instant::now(),
                            last_written: seeded.markdown.clone(),
                        }
                    });
```

Add the test helper (near `insert_dirty_session`):

```rust
/// Insert a session seeded from `head`, dirty iff the worktree diverged. Test-only.
#[cfg(test)]
pub(crate) fn insert_seeded_session(collab: &Collab, path: &NotePath, head: &str, dirty: bool) {
    let (tx, _rx) = broadcast::channel(256);
    let mut reg = lock(collab);
    reg.insert(
        path.clone(),
        Session {
            doc: BlockDoc::from_markdown(DAEMON_REPLICA, head),
            peers: tx,
            participants: HashSet::new(),
            dirty,
            last_op: Instant::now(),
            last_written: head.to_string(),
        },
    );
}
```

In `crates/cairn-daemon/src/lib.rs`, update the `/collab` handler's seed closure (currently lines ~610-613) to return a `Seed`:

```rust
        collab::run_collab(socket, collab, move |path| {
            // Seed from git HEAD (the snapshot boundary); mark dirty if the working
            // tree already diverges so the first flush folds the uncommitted edit
            // (spec §13.4). Empty when the note is not yet in HEAD.
            let guard = seed_state.engine();
            let head = guard.note_at(path, "HEAD").unwrap_or_default();
            let worktree = guard.read_note(path).unwrap_or_default();
            collab::Seed { dirty: worktree != head, markdown: head }
        })
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon opening_a_session_reconciles` then `cargo test -p cairn-daemon` (the WS harness in `tests/collab.rs` still seeds correctly — an empty note yields `Seed { markdown: "", dirty: false }`, so the existing `Snapshot { .. }` empty-seed assertions still hold).
Expected: PASS.

- [ ] **Step 5: Fmt/clippy and commit**

```bash
cargo fmt --all && cargo clippy -p cairn-daemon --all-targets --locked -- -D warnings
git add crates/cairn-daemon/src/collab.rs crates/cairn-daemon/src/lib.rs
git commit -m "feat(collab): seed sessions from git HEAD + reconcile uncommitted worktree"
```

---

### Task 7: Headline WS fold-back integration test (spec §8.2)

Two real WS clients on one note; a foreign disk write mid-session appears in **both** replicas over the wire (no lost work) — the DoD's integration proof. Extends the PR-1 harness.

**Files:**
- Modify: `crates/cairn-daemon/tests/collab.rs` (expose state+tmp from `serve()`; add the fold-back test)

**Interfaces:**
- Consumes: the existing `connect/send/recv` helpers, `AppState::{apply_change_blocking, run_collab_flush_pass}`, `cairn_ports::FsChange`.

- [ ] **Step 1: Write the failing test**

In `crates/cairn-daemon/tests/collab.rs`, add a sibling to `serve()` that returns the state + tempdir (keep `serve()` for the existing tests):

```rust
async fn serve_with_state() -> (std::net::SocketAddr, cairn_daemon::AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine)
        .with_allowed_origins(vec![ORIGIN.to_string()])
        .with_token(TOKEN);
    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, state, tmp)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_disk_edit_mid_session_reaches_both_peers() {
    use cairn_ports::FsChange;
    let (addr, state, tmp) = serve_with_state().await;
    let note = "n.md";
    let path = cairn_domain::NotePath::new(note).unwrap();

    // Two peers join; each gets Joined + (empty) Snapshot.
    let mut c1 = connect(addr).await;
    let mut c2 = connect(addr).await;
    for (c, r) in [(&mut c1, 1u64), (&mut c2, 2u64)] {
        send(c, &CollabClientMsg::Join { note: note.into(), replica: r }).await;
        assert!(matches!(recv(c).await, CollabServerMsg::Joined { .. }));
        assert!(matches!(recv(c).await, CollabServerMsg::Snapshot { .. }));
    }

    // A foreign editor writes N.md directly, then the watcher fires.
    std::fs::write(tmp.path().join(note), "foreign para\n").unwrap();
    let s = state.clone();
    let p = path.clone();
    tokio::task::spawn_blocking(move || {
        s.apply_change_blocking(&FsChange::Changed(p.clone())); // arbitration → dirty
        s.run_collab_flush_pass(std::time::Duration::ZERO); // fold + fan out
    })
    .await
    .unwrap();

    // Both peers receive the folded Insert over the wire.
    let mut d1 = BlockDoc::from_markdown(1, "");
    let mut d2 = BlockDoc::from_markdown(2, "");
    for (c, d) in [(&mut c1, &mut d1), (&mut c2, &mut d2)] {
        match recv(c).await {
            CollabServerMsg::Op { op, .. } => d.merge(block_op_from_wire(op)),
            other => panic!("expected folded Op, got {other:?}"),
        }
    }
    assert_eq!(d1.materialize(), d2.materialize());
    assert!(d1.materialize().contains("foreign para"), "no lost work");
}
```

- [ ] **Step 2: Run the test to verify it fails (then passes once Tasks 3-6 landed)**

Run: `cargo test -p cairn-daemon --test collab foreign_disk_edit_mid_session`
Expected: If Tasks 3-6 are already merged, this should PASS immediately (it is the end-to-end proof). If run in isolation before them, it FAILs at the fold step. Since this task is last, expect PASS.

- [ ] **Step 3: (no implementation — integration test only)**

If the test is flaky on the two `recv` awaits, confirm the fan-out ordering: the daemon sends one `Op` per folded block; a single-block foreign edit sends exactly one `Op` to each non-daemon peer. For a multi-block foreign edit, drain with a loop until an `Insert` containing the text arrives (mirror Task 4's `while let Ok(f) = rx.try_recv()` pattern but over `recv`).

- [ ] **Step 4: Run the full workspace gate**

Run:
```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace
```
Expected: all green. (Known-flaky, pre-existing, unrelated to this change: `invoke_times_out_and_kills_plugin`, `cairn-sandbox-win::exec_and_pipe_stdout` — re-run, do not code-fix.)

- [ ] **Step 5: Commit and open the PR**

```bash
git add crates/cairn-daemon/tests/collab.rs
git commit -m "test(collab): headline foreign-edit fold-back over the wire (spec §8.2)"
git push -u origin crdt-foreign-edit-foldback
gh pr create --base main --title "feat(collab): CRDT foreign-edit fold-back (A2)" \
  --body "Implements A2 (spec §13): replaces A1's skip-and-warn with a block-diff fold-back. See docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md §13."
# Enqueue via the merge queue (queue owns the squash strategy — no --squash):
gh pr merge --auto
```

---

## Self-Review

**Spec coverage (§13):**
- §13.1 two-cycle fold, `last_written = disk` → Task 3 (`fold_foreign` advances baseline, stays dirty) + Task 4 (flush pass folds, no write; pass 2 writes). Re-fold no-duplication → Task 2 test `re_fold_against_the_consumed_bytes_does_not_duplicate_inserts`. ✓
- §13.2 fold as its own collab-lock critical section; `Conflict` removed; never reap a just-folded session → Task 3 (`fold_foreign` sets `dirty=true`; empty+dirty kept by `drain_due`/`settle_flush`) + Task 4 (remove `Conflict`). ✓
- §13.3 Option A block-diff (index-align + LCS; content-match fallback; Insert-never-drop) → Tasks 1-2, with the diverged-base fallback test. ✓
- §13.4 HEAD seed + reconcile → Task 6. ✓
- §13.5 watcher arbitration → Task 5. ✓
- §13.6 tests 1-7 → headline (Task 7), diverged fallback (Task 2), re-fold (Task 2), HEAD-seed reconcile (Task 6), watcher single-commit (Task 5 defers generic auto-commit; the merged result is the sole commit), `fold_foreign` unit/property (Task 2), existing A1 + CRDT tests unchanged (verified green in Tasks 4-7 gates). ✓

**Placeholder scan:** No TBD/TODO. Every code step is concrete. The two "helper note" blocks (Task 4, Task 5) give an explicit primary route (keep `Session` fields private / move the test into the same-crate `#[cfg(test)] mod`) — pick the primary; they are decision guidance, not placeholders.

**Type consistency:** `fold_foreign` (domain: `&mut self, base:&str, foreign:&str -> Vec<BlockOp>`) vs `collab::fold_foreign` (daemon: `&Collab, &NotePath, &str`) — deliberately distinct namespaces (`BlockDoc::` vs `collab::`). `Seed { markdown, dirty }` used identically in Task 6's closure and helper. `note_foreign_edit`/`is_sessioned` signatures match between definition (Task 5 step 3) and call sites (Task 5 step 1, lib.rs). `FlushOutcome` after Task 4 has exactly `Committed(String)` + `WriteError`, and no code constructs `Conflict`.

**Documented ambiguous cases (floor, not solved):** moved block → delete+insert (text kept, ID/stash lost); duplicate-identical blocks under fallback; peer-retyped block → foreign version Insert'd (duplicate, both texts kept). All preserve text. Whole-file `Removed` of a sessioned note is not folded (content edits only) — the session re-materializes it on the next flush.
