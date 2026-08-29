//! Block-level CRDT for one note: an RGA sequence of blocks whose content is
//! an author-priority LWW register. Block IDs are live-only and never reach
//! disk. See docs/decisions/0011-crdt-collaboration-model.md.

use crate::block::{join_blocks, BlockKind};
use crate::blockdiff::DiffStep;
use std::collections::{BTreeSet, HashMap};

/// Lamport timestamp.
pub type Lamport = u64;

/// A globally-unique, live-only block identity. Stripped on materialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId {
    pub replica: u64,
    pub counter: u64,
}

/// Who authored an edit. Drives the same-block LWW policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    Human,
    Agent,
}

/// Priority for same-block content LWW: Human beats Agent.
fn author_rank(a: Author) -> u8 {
    match a {
        Author::Human => 1,
        Author::Agent => 0,
    }
}

/// A replicated operation on a note's block document. Commutative + idempotent
/// under `merge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockOp {
    Insert {
        id: BlockId,
        after: Option<BlockId>,
        lamport: Lamport,
        kind: BlockKind,
        text: String,
    },
    Delete {
        id: BlockId,
        lamport: Lamport,
    },
    SetContent {
        id: BlockId,
        text: String,
        lamport: Lamport,
        author: Author,
    },
}

/// A local edit intent. `apply_local` turns it into `BlockOp`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    InsertAfter {
        after: Option<BlockId>,
        kind: BlockKind,
        text: String,
        author: Author,
    },
    UpdateText {
        id: BlockId,
        text: String,
        author: Author,
    },
    Remove {
        id: BlockId,
    },
}

/// One block's recoverable content: versions retained by the CRDT but not shown
/// by `materialize()`. `tombstoned` distinguishes a deleted block (versions = its
/// former content) from a live block (versions = stashed LWW losers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableBlock {
    pub id: BlockId,
    pub tombstoned: bool,
    pub versions: Vec<String>,
}

/// Internal per-block state.
#[derive(Debug, Clone)]
struct Entry {
    id: BlockId,
    after: Option<BlockId>,
    ins_lamport: Lamport,
    /// Block taxonomy carried through inserts; metadata only, does not affect
    /// convergence or materialized output, so not yet read.
    #[allow(dead_code)]
    kind: BlockKind,
    text: String,
    content_lamport: Lamport,
    content_author: Author,
    tombstone: bool,
    /// Loser content versions retained on conflict (never silently dropped),
    /// keyed by each loser's own `(author_rank, lamport, text)`. A `BTreeSet`
    /// makes the stash order a pure function of content — independent of merge
    /// order, so replicas converge byte-for-byte — and dedups re-applied ops.
    stash: BTreeSet<(u8, Lamport, String)>,
}

/// A live, mergeable representation of one note's blocks.
#[derive(Debug, Clone)]
pub struct BlockDoc {
    replica: u64,
    counter: u64,
    clock: Lamport,
    entries: HashMap<BlockId, Entry>,
}

impl BlockDoc {
    /// Seed a fresh document from a note's markdown. Assigns fresh, live-only
    /// block IDs in document order (a simple RGA chain).
    #[must_use]
    pub fn from_markdown(replica: u64, src: &str) -> Self {
        let mut doc = Self {
            replica,
            counter: 0,
            clock: 0,
            entries: HashMap::new(),
        };
        let mut prev: Option<BlockId> = None;
        for block in crate::block::parse_blocks(src) {
            doc.clock += 1;
            let id = BlockId {
                replica,
                counter: doc.counter,
            };
            doc.counter += 1;
            doc.entries.insert(
                id,
                Entry {
                    id,
                    after: prev,
                    ins_lamport: doc.clock,
                    kind: block.kind,
                    text: block.text,
                    content_lamport: doc.clock,
                    content_author: Author::Human,
                    tombstone: false,
                    stash: BTreeSet::new(),
                },
            );
            prev = Some(id);
        }
        doc
    }

    /// Project current state to canonical markdown. Block IDs are stripped;
    /// the output is pure plain markdown.
    #[must_use]
    pub fn materialize(&self) -> String {
        // children[anchor] = entries inserted directly after `anchor`.
        let mut children: HashMap<Option<BlockId>, Vec<&Entry>> = HashMap::new();
        for e in self.entries.values() {
            children.entry(e.after).or_default().push(e);
        }
        // Deterministic sibling order: higher insertion lamport first, id as
        // tiebreak. Total + independent of merge order ⇒ convergent.
        for v in children.values_mut() {
            v.sort_by(|a, b| {
                b.ins_lamport
                    .cmp(&a.ins_lamport)
                    .then_with(|| b.id.cmp(&a.id))
            });
        }
        let mut texts: Vec<String> = Vec::new();
        walk(None, &children, &mut texts);
        join_blocks(&texts)
    }

    /// Merge a replicated op. Commutative and idempotent.
    pub fn merge(&mut self, op: BlockOp) {
        match op {
            BlockOp::Insert {
                id,
                after,
                lamport,
                kind,
                text,
            } => {
                self.clock = self.clock.max(lamport);
                self.entries.entry(id).or_insert(Entry {
                    id,
                    after,
                    ins_lamport: lamport,
                    kind,
                    text,
                    content_lamport: lamport,
                    content_author: Author::Human,
                    tombstone: false,
                    stash: BTreeSet::new(),
                });
            }
            BlockOp::Delete { id, lamport } => {
                self.clock = self.clock.max(lamport);
                if let Some(e) = self.entries.get_mut(&id) {
                    e.tombstone = true;
                }
            }
            BlockOp::SetContent { .. } => {
                self.merge_set_content(op);
            }
        }
    }

    /// The minimal op-set that reconstructs this document's current state when
    /// merged into a fresh replica. Used to catch up a peer joining a live
    /// session (and, later, to persist a `.cairn/` resume sidecar). Every block
    /// is re-created with its live-only `BlockId`, so a joiner adopts the shared
    /// identities rather than minting fresh ones. Order-independent: `merge` is
    /// commutative and idempotent, so the receiver may apply these in any order.
    #[must_use]
    pub fn state_as_ops(&self) -> Vec<BlockOp> {
        let mut ops = Vec::with_capacity(self.entries.len());
        for e in self.entries.values() {
            // Re-establish the block at its position with its current content.
            ops.push(BlockOp::Insert {
                id: e.id,
                after: e.after,
                lamport: e.ins_lamport,
                kind: e.kind,
                text: e.text.clone(),
            });
            // If content advanced past the insert seed, re-affirm it at the right
            // Lamport/author so a joiner's later edits order correctly against it.
            if e.content_lamport != e.ins_lamport || e.content_author != Author::Human {
                ops.push(BlockOp::SetContent {
                    id: e.id,
                    text: e.text.clone(),
                    lamport: e.content_lamport,
                    author: e.content_author,
                });
            }
            // Preserve deletions.
            if e.tombstone {
                ops.push(BlockOp::Delete {
                    id: e.id,
                    lamport: e.content_lamport.max(e.ins_lamport),
                });
            }
        }
        ops
    }

    /// Live (non-tombstoned) block IDs in materialized order. Test/lookup aid.
    #[must_use]
    pub fn block_ids_in_order(&self) -> Vec<BlockId> {
        let mut children: HashMap<Option<BlockId>, Vec<&Entry>> = HashMap::new();
        for e in self.entries.values() {
            children.entry(e.after).or_default().push(e);
        }
        for v in children.values_mut() {
            v.sort_by(|a, b| {
                b.ins_lamport
                    .cmp(&a.ins_lamport)
                    .then_with(|| b.id.cmp(&a.id))
            });
        }
        let mut out = Vec::new();
        fn walk(
            anchor: Option<BlockId>,
            children: &HashMap<Option<BlockId>, Vec<&Entry>>,
            out: &mut Vec<BlockId>,
        ) {
            if let Some(kids) = children.get(&anchor) {
                for e in kids {
                    if !e.tombstone {
                        out.push(e.id);
                    }
                    walk(Some(e.id), children, out);
                }
            }
        }
        walk(None, &children, &mut out);
        out
    }

    fn merge_set_content(&mut self, op: BlockOp) {
        let BlockOp::SetContent {
            id,
            text,
            lamport,
            author,
        } = op
        else {
            return;
        };
        self.clock = self.clock.max(lamport);
        let Some(e) = self.entries.get_mut(&id) else {
            return;
        };
        // Deterministic total order on content: (author_rank, lamport), with the
        // text itself as the final tiebreak. Human always outranks Agent; among
        // equal rank the higher Lamport wins; a true (rank, lamport) tie is
        // broken by text (greater wins). Order-independent ⇒ convergent, and the
        // loser's text is always stashed rather than dropped.
        let incoming = (author_rank(author), lamport);
        let current = (author_rank(e.content_author), e.content_lamport);
        // Each stashed loser is keyed by ITS OWN (author_rank, lamport, text), so
        // the same text always lands under the same key regardless of which side
        // lost — making the stash a convergent set (see `Entry.stash`).
        match incoming.cmp(&current) {
            std::cmp::Ordering::Greater => {
                let displaced = (author_rank(e.content_author), e.content_lamport);
                e.stash.insert((
                    displaced.0,
                    displaced.1,
                    std::mem::replace(&mut e.text, text),
                ));
                e.content_author = author;
                e.content_lamport = lamport;
            }
            std::cmp::Ordering::Less => {
                e.stash.insert((incoming.0, incoming.1, text));
            }
            std::cmp::Ordering::Equal => match text.cmp(&e.text) {
                std::cmp::Ordering::Greater => {
                    let displaced = (author_rank(e.content_author), e.content_lamport);
                    e.stash.insert((
                        displaced.0,
                        displaced.1,
                        std::mem::replace(&mut e.text, text),
                    ));
                }
                std::cmp::Ordering::Less => {
                    e.stash.insert((incoming.0, incoming.1, text));
                }
                std::cmp::Ordering::Equal => {} // identical content: idempotent no-op
            },
        }
    }

    /// Apply a local edit, mutating this doc and returning the op(s) to share.
    pub fn apply_local(&mut self, edit: Edit) -> Vec<BlockOp> {
        self.clock += 1;
        let lamport = self.clock;
        let op = match edit {
            Edit::InsertAfter {
                after,
                kind,
                text,
                author,
            } => {
                let id = BlockId {
                    replica: self.replica,
                    counter: self.counter,
                };
                self.counter += 1;
                let _ = author; // insert content author defaults to Human seed; refined later
                BlockOp::Insert {
                    id,
                    after,
                    lamport,
                    kind,
                    text,
                }
            }
            Edit::UpdateText { id, text, author } => BlockOp::SetContent {
                id,
                text,
                lamport,
                author,
            },
            Edit::Remove { id } => BlockOp::Delete { id, lamport },
        };
        self.merge(op.clone());
        vec![op]
    }

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
        let foreign_texts: Vec<String> = foreign_blocks.iter().map(|b| b.text.clone()).collect();
        let live = self.block_ids_in_order();

        // Positional alignment is exact only when the live doc is byte-identical
        // to `base`: then base_texts[i] corresponds to live[i]. Otherwise a peer
        // advanced the doc concurrently — fall back to content matching so no
        // foreign block is lost (spec §13.3).
        let index_aligned = self.materialize() == base && base_texts.len() == live.len();

        // PASS 1 (no mutation): resolve the edit script into concrete actions,
        // pairing each gap's deletes/inserts into content substitutions. Base IDs
        // are resolved against the pre-mutation doc so a SetContent cannot perturb
        // a later block's content-match. The `FoldPlan` helper owns the pass-1
        // state so `resolve_base`/`flush_gap` can share `consumed` without the
        // nested-closure borrow tangle.
        let steps = crate::blockdiff::lcs_edit_script(&base_texts, &foreign_texts);
        let mut plan = FoldPlan {
            foreign_texts: &foreign_texts,
            foreign_blocks: &foreign_blocks,
            index_aligned,
            live: &live,
            base_texts: &base_texts,
            consumed: vec![false; live.len()],
            actions: Vec::new(),
            anchor: None,
            gap_del: Vec::new(),
            gap_ins: Vec::new(),
        };

        for step in &steps {
            match *step {
                DiffStep::Keep { bi, .. } => {
                    plan.flush_gap(self);
                    // Advance the anchor to this kept block's live id. An
                    // unresolvable kept block must NOT reset the anchor to `None`
                    // (the head): `parse_blocks` splits on blank lines, so a live
                    // block whose text contains one re-parses into several base
                    // blocks that match nothing, and resetting would teleport the
                    // gap's inserts to the top of the note. Hold the last block we
                    // did locate instead — the best available position.
                    plan.anchor = plan.resolve_base(bi, self).or(plan.anchor);
                }
                DiffStep::Delete { bi } => plan.gap_del.push(bi),
                DiffStep::Insert { fi } => plan.gap_ins.push(fi),
            }
        }
        plan.flush_gap(self);
        let actions = plan.actions;

        // PASS 2 (mutation): apply the actions, chaining consecutive inserts that
        // share a gap anchor so multi-block insertions land in order.
        let mut produced: Vec<BlockOp> = Vec::new();
        let mut prev_anchor: Option<BlockId> = None;
        let mut prev_insert_id: Option<BlockId> = None;
        for action in actions {
            match action {
                FoldAction::Delete(id) => {
                    prev_insert_id = None;
                    produced.extend(self.apply_local(Edit::Remove { id }));
                }
                FoldAction::SetContent(id, text) => {
                    prev_insert_id = None;
                    produced.extend(self.apply_local(Edit::UpdateText {
                        id,
                        text,
                        author: Author::Human,
                    }));
                }
                FoldAction::Insert { anchor, kind, text } => {
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

    /// Stashed loser content versions for a block (recoverable). Test/inspect aid.
    #[must_use]
    pub fn stashed(&self, id: BlockId) -> Vec<String> {
        self.entries
            .get(&id)
            .map(|e| e.stash.iter().map(|(_, _, t)| t.clone()).collect())
            .unwrap_or_default()
    }

    /// Content versions retained in the CRDT but NOT shown by `materialize()`:
    /// a live block's stashed losers (its winner is already visible), or ALL of a
    /// tombstoned block's content (winner + losers, since nothing is materialized).
    /// This is what makes a concurrently-edited-then-deleted block — or a foreign
    /// `SetContent` folded onto a live-deleted block — recoverable rather than
    /// silently lost (spec §3.2). Convergent by construction: a pure function of
    /// the (convergent) content register + tombstone, so equal replica state gives
    /// equal output — the `Delete` arm deliberately moves nothing (stashing at
    /// delete time would be order-dependent and diverge). It surfaces a tombstoned
    /// block's content whether or not the delete was truly concurrent with an edit
    /// (distinguishing them needs causality we don't track); over-preserving is
    /// consistent with the floor.
    #[must_use]
    pub fn recoverable(&self, id: BlockId) -> Vec<String> {
        let Some(e) = self.entries.get(&id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if e.tombstone {
            out.push(e.text.clone());
        }
        out.extend(e.stash.iter().map(|(_, _, t)| t.clone()));
        out
    }

    /// Every block with non-empty `recoverable` content, for a recovery view.
    /// Order is deterministic and convergent: live blocks in materialized order
    /// first, then tombstoned blocks by ascending id.
    #[must_use]
    pub fn recoverable_blocks(&self) -> Vec<RecoverableBlock> {
        let mut out = Vec::new();
        // Live blocks, in materialized document order.
        for id in self.block_ids_in_order() {
            let versions = self.recoverable(id);
            if !versions.is_empty() {
                out.push(RecoverableBlock {
                    id,
                    tombstoned: false,
                    versions,
                });
            }
        }
        // Tombstoned blocks, by ascending id (block_ids_in_order excludes them).
        let mut dead: Vec<BlockId> = self
            .entries
            .values()
            .filter(|e| e.tombstone)
            .map(|e| e.id)
            .collect();
        dead.sort();
        for id in dead {
            let versions = self.recoverable(id);
            if !versions.is_empty() {
                out.push(RecoverableBlock {
                    id,
                    tombstoned: true,
                    versions,
                });
            }
        }
        out
    }
}

impl BlockDoc {
    /// Promote a recoverable version of `id` back into the live document by
    /// re-inserting it as a NEW live block, anchored right after the (retained)
    /// source block so it lands in the source's original slot. `version_index`
    /// indexes into `recoverable(id)` — the same ordering the recovery surface
    /// showed (a tombstoned block's `[winner, ..losers]`, or a live block's
    /// stashed losers). Restore is additive and convergent by construction: the
    /// produced op is a plain `Insert` with a fresh `BlockId`, so it inherits
    /// Insert's commutativity + idempotency, and it never mutates the tombstone or
    /// the source register — delete stays delete-wins. Returns the op(s) to fan
    /// out; empty (and inert — no clock advance) when `id` is unknown or
    /// `version_index` is out of range.
    pub fn restore(&mut self, id: BlockId, version_index: usize) -> Vec<BlockOp> {
        let Some(text) = self.recoverable(id).get(version_index).cloned() else {
            return Vec::new();
        };
        let Some(kind) = self.entries.get(&id).map(|e| e.kind) else {
            return Vec::new();
        };
        // Anchor after the source block: its RGA entry is retained even when
        // tombstoned, so the new block sorts ahead of the source's former
        // successor (siblings order by newest ins_lamport first) and reappears in
        // the source's original position.
        self.apply_local(Edit::InsertAfter {
            after: Some(id),
            kind,
            text,
            author: Author::Human,
        })
    }
}

/// A resolved fold action, planned in pass 1 and applied in pass 2 of
/// `fold_foreign`. Owned data only — no borrows into the pre-mutation doc — so
/// pass 2 can freely mutate.
enum FoldAction {
    Delete(BlockId),
    SetContent(BlockId, String),
    Insert {
        anchor: Option<BlockId>,
        kind: BlockKind,
        text: String,
    },
}

/// Pass-1 state for `fold_foreign`. Holding `consumed` here lets `resolve_base`
/// and `flush_gap` share the content-match cursor as ordinary `&mut self`
/// methods, avoiding the `&mut dyn FnMut` nested-closure borrow tangle. All
/// slice fields borrow read-only from the pre-mutation doc's derived data.
struct FoldPlan<'a> {
    foreign_texts: &'a [String],
    foreign_blocks: &'a [crate::block::Block],
    index_aligned: bool,
    live: &'a [BlockId],
    base_texts: &'a [String],
    consumed: Vec<bool>,
    actions: Vec<FoldAction>,
    /// Last kept live id before the current gap.
    anchor: Option<BlockId>,
    gap_del: Vec<usize>,
    gap_ins: Vec<usize>,
}

impl FoldPlan<'_> {
    /// Map a `base` block index to a live `BlockId`. Index-aligned when the doc
    /// equals `base`; otherwise the first unconsumed live block whose current
    /// text matches, marking it consumed so each live block maps at most once.
    fn resolve_base(&mut self, bi: usize, this: &BlockDoc) -> Option<BlockId> {
        if self.index_aligned {
            return Some(self.live[bi]);
        }
        for (k, id) in self.live.iter().enumerate() {
            if !self.consumed[k] && this.text_of(*id) == Some(self.base_texts[bi].as_str()) {
                self.consumed[k] = true;
                return Some(*id);
            }
        }
        None
    }

    /// Resolve the deletes/inserts accumulated in the current gap: pair them into
    /// in-place substitutions, then emit any leftover deletes and inserts.
    fn flush_gap(&mut self, this: &BlockDoc) {
        let anchor = self.anchor;
        let gap_del = std::mem::take(&mut self.gap_del);
        let gap_ins = std::mem::take(&mut self.gap_ins);
        let pair = gap_del.len().min(gap_ins.len());
        for k in 0..pair {
            let fi = gap_ins[k];
            match self.resolve_base(gap_del[k], this) {
                // Substitution in place: keep the block's identity.
                Some(id) => self
                    .actions
                    .push(FoldAction::SetContent(id, self.foreign_texts[fi].clone())),
                // Base block vanished but foreign has content here: insert it
                // rather than drop it (floor).
                None => self.actions.push(FoldAction::Insert {
                    anchor,
                    kind: self.foreign_blocks[fi].kind,
                    text: self.foreign_texts[fi].clone(),
                }),
            }
        }
        for &bi in &gap_del[pair..] {
            // Only apply a delete when the base block is uniquely matched; losing
            // a delete never loses content, so ambiguity is skipped.
            if let Some(id) = self.resolve_base(bi, this) {
                self.actions.push(FoldAction::Delete(id));
            }
        }
        for &fi in &gap_ins[pair..] {
            self.actions.push(FoldAction::Insert {
                anchor,
                kind: self.foreign_blocks[fi].kind,
                text: self.foreign_texts[fi].clone(),
            });
        }
    }
}

/// Pre-order DFS over the RGA tree, emitting live block texts.
fn walk(
    anchor: Option<BlockId>,
    children: &HashMap<Option<BlockId>, Vec<&Entry>>,
    out: &mut Vec<String>,
) {
    if let Some(kids) = children.get(&anchor) {
        for e in kids {
            if !e.tombstone {
                out.push(e.text.clone());
            }
            walk(Some(e.id), children, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_id_is_ordered_and_unique() {
        let a = BlockId {
            replica: 1,
            counter: 0,
        };
        let b = BlockId {
            replica: 1,
            counter: 1,
        };
        assert!(a < b);
        assert_ne!(a, b);
    }

    #[test]
    fn author_human_outranks_agent() {
        assert!(author_rank(Author::Human) > author_rank(Author::Agent));
    }

    #[test]
    fn from_markdown_then_materialize_round_trips() {
        let src = "# Title\n\nFirst para.\n\n- a\n- b\n";
        let doc = BlockDoc::from_markdown(1, src);
        assert_eq!(doc.materialize(), src);
    }

    #[test]
    fn empty_markdown_materializes_empty() {
        let doc = BlockDoc::from_markdown(1, "");
        assert_eq!(doc.materialize(), "");
    }

    #[test]
    fn merge_insert_is_idempotent() {
        let mut doc = BlockDoc::from_markdown(1, "a\n");
        let op = BlockOp::Insert {
            id: BlockId {
                replica: 2,
                counter: 0,
            },
            after: None,
            lamport: 5,
            kind: BlockKind::Paragraph,
            text: "z".into(),
        };
        doc.merge(op.clone());
        let once = doc.materialize();
        doc.merge(op); // applying twice changes nothing
        assert_eq!(doc.materialize(), once);
    }

    #[test]
    fn merge_delete_tombstones_block() {
        let mut doc = BlockDoc::from_markdown(1, "keep\n\ndrop\n");
        // Find the id of the second block ("drop").
        let drop_id = doc.block_ids_in_order()[1];
        doc.merge(BlockOp::Delete {
            id: drop_id,
            lamport: 9,
        });
        assert_eq!(doc.materialize(), "keep\n");
    }

    #[test]
    fn human_edit_beats_agent_edit_same_block_and_stashes_loser() {
        let mut doc = BlockDoc::from_markdown(1, "original\n");
        let id = doc.block_ids_in_order()[0];
        // Agent writes with a HIGHER lamport, human with a lower one.
        doc.merge(BlockOp::SetContent {
            id,
            text: "agent version".into(),
            lamport: 10,
            author: Author::Agent,
        });
        doc.merge(BlockOp::SetContent {
            id,
            text: "human version".into(),
            lamport: 3,
            author: Author::Human,
        });
        assert_eq!(doc.materialize(), "human version\n");
        // Agent's losing text is stashed, not lost.
        assert!(doc.stashed(id).contains(&"agent version".to_string()));
    }

    #[test]
    fn set_content_is_order_independent() {
        let ops = |seed: bool| {
            let mut d = BlockDoc::from_markdown(1, "x\n");
            let id = d.block_ids_in_order()[0];
            let a = BlockOp::SetContent {
                id,
                text: "A".into(),
                lamport: 4,
                author: Author::Human,
            };
            let b = BlockOp::SetContent {
                id,
                text: "B".into(),
                lamport: 7,
                author: Author::Human,
            };
            if seed {
                d.merge(a);
                d.merge(b);
            } else {
                d.merge(b);
                d.merge(a);
            }
            d.materialize()
        };
        assert_eq!(ops(true), ops(false));
    }

    #[test]
    fn set_content_converges_on_equal_author_and_lamport() {
        // Two concurrent edits to the same block with the SAME (author, lamport)
        // must still converge: the text tiebreak makes the order total. Without
        // it, first-writer-wins and the replicas diverge.
        let run = |first_aaa: bool| {
            let mut d = BlockDoc::from_markdown(1, "seed\n");
            let id = d.block_ids_in_order()[0];
            let aaa = BlockOp::SetContent {
                id,
                text: "AAA".into(),
                lamport: 5,
                author: Author::Human,
            };
            let bbb = BlockOp::SetContent {
                id,
                text: "BBB".into(),
                lamport: 5,
                author: Author::Human,
            };
            if first_aaa {
                d.merge(aaa);
                d.merge(bbb);
            } else {
                d.merge(bbb);
                d.merge(aaa);
            }
            d.materialize()
        };
        assert_eq!(run(true), run(false));
        assert_eq!(run(true), "BBB\n"); // greater text wins, deterministically
    }

    #[test]
    fn apply_local_returns_op_and_applies_it() {
        let mut doc = BlockDoc::from_markdown(1, "hello\n");
        let id = doc.block_ids_in_order()[0];
        let ops = doc.apply_local(Edit::UpdateText {
            id,
            text: "hi".into(),
            author: Author::Human,
        });
        assert_eq!(ops.len(), 1);
        assert_eq!(doc.materialize(), "hi\n");
    }

    #[test]
    fn state_as_ops_reconstructs_into_a_fresh_replica() {
        // Build a doc, then mutate: edit a block, append a block, delete a block.
        let mut a = BlockDoc::from_markdown(1, "# Title\n\nbody\n");
        let ids = a.block_ids_in_order();
        let (title, body) = (ids[0], ids[1]);
        a.apply_local(Edit::UpdateText {
            id: body,
            text: "new body".into(),
            author: Author::Human,
        });
        a.apply_local(Edit::InsertAfter {
            after: Some(body),
            kind: crate::block::BlockKind::Paragraph,
            text: "tail".into(),
            author: Author::Human,
        });
        a.apply_local(Edit::Remove { id: title });

        // Replay the snapshot op-set into an empty replica.
        let mut b = BlockDoc::from_markdown(2, "");
        for op in a.state_as_ops() {
            b.merge(op);
        }

        assert_eq!(a.materialize(), b.materialize());
        // Replay is idempotent (the CRDT law): applying twice changes nothing.
        for op in a.state_as_ops() {
            b.merge(op);
        }
        assert_eq!(a.materialize(), b.materialize());
    }

    #[test]
    fn state_as_ops_preserves_ids_so_concurrent_edits_converge() {
        // A seeds a block; B catches up via the snapshot op-set (adopting A's IDs).
        let mut a = BlockDoc::from_markdown(1, "shared block\n");
        let mut b = BlockDoc::from_markdown(2, "");
        for op in a.state_as_ops() {
            b.merge(op);
        }
        let id = a.block_ids_in_order()[0];
        assert_eq!(b.block_ids_in_order(), vec![id]); // shared identity

        // Both edit the SAME block concurrently; exchange the resulting ops.
        let a_ops = a.apply_local(Edit::UpdateText {
            id,
            text: "from A".into(),
            author: Author::Human,
        });
        let b_ops = b.apply_local(Edit::UpdateText {
            id,
            text: "from B".into(),
            author: Author::Human,
        });
        for op in &b_ops {
            a.merge(op.clone());
        }
        for op in &a_ops {
            b.merge(op.clone());
        }

        // Because the ID was shared, the concurrent edits collide on ONE block and
        // converge to a single deterministic winner — not two forked blocks.
        assert_eq!(a.materialize(), b.materialize());
        assert_eq!(a.block_ids_in_order().len(), 1);
    }

    #[test]
    fn fold_foreign_inserts_a_new_block_after_its_neighbor() {
        let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
        let base = doc.materialize(); // "a\n\nb\n"
        let ops = doc.fold_foreign(&base, "a\n\nmiddle\n\nb\n");
        assert_eq!(doc.materialize(), "a\n\nmiddle\n\nb\n");
        assert!(!ops.is_empty(), "an insert produced ops to fan out");
    }

    #[test]
    fn fold_foreign_multi_block_insert_keeps_disk_order() {
        // Regression guard for the fold-back insert-chaining rule. Three foreign
        // blocks land in ONE gap, so pass 1 plans three inserts sharing the anchor
        // "a". `block_ids_in_order` sorts same-anchor siblings DESC by ins_lamport,
        // so emitting all three against that shared anchor would materialize them
        // reversed ("d", "c", "b"). Pass 2 must chain each insert onto the previous
        // inserted id instead.
        let mut doc = BlockDoc::from_markdown(1, "a\n\nz\n");
        let base = doc.materialize();
        doc.fold_foreign(&base, "a\n\nb\n\nc\n\nd\n\nz\n");
        assert_eq!(doc.materialize(), "a\n\nb\n\nc\n\nd\n\nz\n");
    }

    #[test]
    fn fold_foreign_multi_block_insert_at_head_keeps_disk_order() {
        // Same chaining rule with a `None` anchor (insert before the first block):
        // the "same gap" test compares `Option<BlockId>`, so the head case must not
        // fall through to the unchained branch.
        let mut doc = BlockDoc::from_markdown(1, "z\n");
        let base = doc.materialize();
        doc.fold_foreign(&base, "b\n\nc\n\nd\n\nz\n");
        assert_eq!(doc.materialize(), "b\n\nc\n\nd\n\nz\n");
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

    #[test]
    fn fold_foreign_skips_a_delete_whose_base_block_no_longer_matches() {
        // The base→live id mapping must never fall back to positions: when the
        // live doc diverged, a base block is resolved by exact text match only, and
        // an unmatched Delete is DROPPED (losing a delete never loses content,
        // whereas a mis-targeted one does). Here a peer rewrote "b" while the
        // external editor deleted it; the fold must keep the peer's text rather
        // than remove whatever sits at that base position.
        let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
        let base = doc.materialize();
        let id_b = doc.block_ids_in_order()[1];
        doc.apply_local(Edit::UpdateText {
            id: id_b,
            text: "b (peer)".into(),
            author: Author::Human,
        });
        doc.fold_foreign(&base, "a\n");
        let out = doc.materialize();
        assert!(
            out.contains("b (peer)"),
            "peer rewrite not silently removed"
        );
        assert!(out.contains("a"), "untouched block preserved");
    }

    #[test]
    fn fold_foreign_insert_does_not_teleport_to_the_head_on_a_split_block() {
        // `parse_blocks` splits on blank lines, and `BlockDoc::merge` applies wire
        // text verbatim, so a peer can create a live block whose text contains a
        // blank line. `materialize()` then re-parses into MORE blocks than the doc
        // holds, and those extra base blocks match no live block. The fold must
        // still place a foreign append after the content it follows, not at the
        // top of the note — an unresolvable kept block holds the previous anchor
        // rather than resetting it to the head.
        let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n");
        let id_b = doc.block_ids_in_order()[1];
        doc.merge(BlockOp::SetContent {
            id: id_b,
            text: "b1\n\nb2".into(),
            lamport: 50,
            author: Author::Human,
        });
        let base = doc.materialize();
        assert_eq!(base, "a\n\nb1\n\nb2\n");
        // 2 live blocks, but the baseline re-parses into 3 — the misalignment.
        assert_eq!(doc.block_ids_in_order().len(), 2);
        assert_eq!(crate::block::parse_blocks(&base).len(), 3);

        doc.fold_foreign(&base, "a\n\nb1\n\nb2\n\nX\n");
        let out = doc.materialize();
        // The floor: nothing is lost.
        for kept in ["a", "b1", "b2", "X"] {
            assert!(out.contains(kept), "{kept} lost: {out:?}");
        }
        // The fix: the append does not jump ahead of "a".
        assert!(
            out.find('X') > out.find('a'),
            "foreign append teleported to the head: {out:?}"
        );
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    #[test]
    fn deleted_block_keeps_concurrent_edit_recoverable_either_order() {
        // A block is concurrently edited (SetContent) and deleted (Delete). The
        // edit must survive as recoverable regardless of op arrival order — a
        // convergent realization of the "never lose work" floor (spec §3.2).
        let run = |set_first: bool| {
            let mut d = BlockDoc::from_markdown(1, "orig\n");
            let id = d.block_ids_in_order()[0];
            let set = BlockOp::SetContent {
                id,
                text: "edited".into(),
                lamport: 5,
                author: Author::Human,
            };
            let del = BlockOp::Delete { id, lamport: 6 };
            if set_first {
                d.merge(set);
                d.merge(del);
            } else {
                d.merge(del);
                d.merge(set);
            }
            (d.materialize(), sorted(d.recoverable(id)))
        };
        let (mat_a, rec_a) = run(true);
        let (mat_b, rec_b) = run(false);
        assert_eq!(mat_a, ""); // remove-wins: the block is gone from the doc
        assert_eq!(mat_a, mat_b); // materialize converges
        assert_eq!(rec_a, rec_b); // recovery set converges
        assert!(rec_a.contains(&"edited".to_string()), "edit not lost");
    }

    #[test]
    fn foreign_set_content_onto_deleted_block_is_recoverable() {
        // Symmetric case: a live user deletes a block; a folded foreign edit then
        // SetContents it. The foreign winner lands in a tombstoned entry — never
        // materialized, but recoverable, both orders.
        let run = |del_first: bool| {
            let mut d = BlockDoc::from_markdown(1, "orig\n");
            let id = d.block_ids_in_order()[0];
            let del = BlockOp::Delete { id, lamport: 4 };
            let set = BlockOp::SetContent {
                id,
                text: "foreign edit".into(),
                lamport: 9,
                author: Author::Human,
            };
            if del_first {
                d.merge(del);
                d.merge(set);
            } else {
                d.merge(set);
                d.merge(del);
            }
            (d.materialize(), sorted(d.recoverable(id)))
        };
        let (mat_a, rec_a) = run(true);
        let (mat_b, rec_b) = run(false);
        assert_eq!(mat_a, "");
        assert_eq!(mat_a, mat_b);
        assert_eq!(rec_a, rec_b);
        assert!(
            rec_a.contains(&"foreign edit".to_string()),
            "foreign edit onto a deleted block not lost"
        );
    }

    #[test]
    fn recoverable_on_live_block_returns_only_losers() {
        // A live (non-tombstoned) block's winner is already in materialize(), so
        // recoverable() surfaces only the stashed losers, never the visible text.
        let mut doc = BlockDoc::from_markdown(1, "orig\n");
        let id = doc.block_ids_in_order()[0];
        doc.merge(BlockOp::SetContent {
            id,
            text: "loser".into(),
            lamport: 5,
            author: Author::Human,
        });
        doc.merge(BlockOp::SetContent {
            id,
            text: "winner".into(),
            lamport: 7,
            author: Author::Human,
        });
        assert_eq!(doc.materialize(), "winner\n");
        let rec = doc.recoverable(id);
        assert!(
            !rec.contains(&"winner".to_string()),
            "winner is visible, not recovery"
        );
        assert!(rec.contains(&"loser".to_string()), "loser is recoverable");
    }

    #[test]
    fn recoverable_blocks_enumerates_losers_and_tombstoned() {
        // Block 0: live, carries a stashed LWW loser. Block 1: deleted with content.
        let mut doc = BlockDoc::from_markdown(1, "keep\n\ndrop\n");
        let ids = doc.block_ids_in_order();
        let (keep, drop) = (ids[0], ids[1]);
        // Stash a loser on the live block: a lower-ranked SetContent loses to the seed's Human rank.
        doc.merge(BlockOp::SetContent {
            id: keep,
            text: "loser".into(),
            lamport: 3,
            author: Author::Agent,
        });
        // Delete the second block (its content stays recoverable).
        doc.merge(BlockOp::Delete {
            id: drop,
            lamport: 9,
        });

        let rec = doc.recoverable_blocks();
        assert_eq!(rec.len(), 2);
        // Live block first (materialized order), then tombstoned.
        assert_eq!(rec[0].id, keep);
        assert!(!rec[0].tombstoned);
        assert!(rec[0].versions.contains(&"loser".to_string()));
        assert!(!rec[0].versions.contains(&"keep".to_string())); // winner is visible, not recovery
        assert_eq!(rec[1].id, drop);
        assert!(rec[1].tombstoned);
        assert!(rec[1].versions.contains(&"drop".to_string()));
    }

    #[test]
    fn recoverable_blocks_empty_when_nothing_retained() {
        let doc = BlockDoc::from_markdown(1, "a\n\nb\n");
        assert!(doc.recoverable_blocks().is_empty());
    }

    #[test]
    fn stash_order_is_convergent_across_merge_orders() {
        // Multi-loser convergence: the SAME set of losing SetContent ops applied
        // in DIFFERENT orders must yield byte-for-byte equal recovery output, not
        // merely equal sets. The stash is ordered by (author_rank, lamport, text),
        // a pure function of content, so it converges independently of merge order.
        let build = |reversed: bool| {
            let mut d = BlockDoc::from_markdown(1, "seed\n");
            let id = d.block_ids_in_order()[0];
            let mut ops = vec![
                BlockOp::SetContent {
                    id,
                    text: "aaa".into(),
                    lamport: 2,
                    author: Author::Agent,
                },
                BlockOp::SetContent {
                    id,
                    text: "zzz".into(),
                    lamport: 8,
                    author: Author::Agent,
                },
                BlockOp::SetContent {
                    id,
                    text: "mmm".into(),
                    lamport: 3,
                    author: Author::Human,
                },
                BlockOp::SetContent {
                    id,
                    text: "WIN".into(),
                    lamport: 20,
                    author: Author::Human,
                },
            ];
            if reversed {
                ops.reverse();
            }
            for op in ops {
                d.merge(op);
            }
            d
        };
        let fwd = build(false);
        let rev = build(true);
        let id = fwd.block_ids_in_order()[0];
        assert_eq!(fwd.materialize(), "WIN\n");
        assert_eq!(rev.materialize(), "WIN\n");
        // Vec-equality, not just set-equality: (author_rank, lamport, text) order.
        // Agent losers (rank 0) first, then Human losers (rank 1) by lamport.
        let expected = vec![
            "aaa".to_string(),  // (0, 2)
            "zzz".to_string(),  // (0, 8)
            "seed".to_string(), // (1, 1) — displaced seed
            "mmm".to_string(),  // (1, 3)
        ];
        assert_eq!(fwd.recoverable(id), expected);
        assert_eq!(rev.recoverable(id), expected);
        assert_eq!(fwd.recoverable_blocks(), rev.recoverable_blocks());
    }

    #[test]
    fn applying_a_losing_op_twice_leaves_stash_unchanged() {
        // Idempotence: re-merging the same losing op must not duplicate its stash
        // entry (a plain Vec push would; a keyed set dedups).
        let mut doc = BlockDoc::from_markdown(1, "seed\n");
        let id = doc.block_ids_in_order()[0];
        let losing = BlockOp::SetContent {
            id,
            text: "loser".into(),
            lamport: 3,
            author: Author::Agent,
        };
        doc.merge(losing.clone());
        let once = doc.stashed(id);
        doc.merge(losing);
        assert_eq!(
            doc.stashed(id),
            once,
            "re-applying a losing op must not duplicate its stash entry"
        );
        assert_eq!(once, vec!["loser".to_string()]);
    }

    #[test]
    fn restore_reinserts_deleted_content_in_original_slot() {
        // Delete a middle block, then restore v0: the tombstone keeps its RGA
        // anchor, so the re-inserted content lands back between its neighbours.
        let mut doc = BlockDoc::from_markdown(1, "a\n\nb\n\nc\n");
        let b_id = doc.block_ids_in_order()[1];
        doc.merge(BlockOp::Delete {
            id: b_id,
            lamport: 9,
        });
        assert_eq!(doc.materialize(), "a\n\nc\n");
        let ops = doc.restore(b_id, 0);
        assert_eq!(ops.len(), 1, "restore produces one Insert to fan out");
        assert!(matches!(ops[0], BlockOp::Insert { .. }));
        assert_eq!(doc.materialize(), "a\n\nb\n\nc\n");
    }

    #[test]
    fn restore_and_concurrent_delete_converge_either_order() {
        // The restore Insert and the Delete of the source block must converge
        // regardless of arrival order (the convergence law). The restored block
        // survives both ways: it is a fresh id, unaffected by the tombstone.
        let mut src = BlockDoc::from_markdown(1, "a\n\nb\n\nc\n");
        let b_id = src.block_ids_in_order()[1];
        let del = BlockOp::Delete {
            id: b_id,
            lamport: 9,
        };
        src.merge(del.clone());
        let restore_ops = src.restore(b_id, 0);

        let play = |del_first: bool| {
            // Fresh replica 1 mints the same ids as `src` (deterministic counter).
            let mut d = BlockDoc::from_markdown(1, "a\n\nb\n\nc\n");
            if del_first {
                d.merge(del.clone());
                for op in &restore_ops {
                    d.merge(op.clone());
                }
            } else {
                for op in &restore_ops {
                    d.merge(op.clone());
                }
                d.merge(del.clone());
            }
            d.materialize()
        };
        assert_eq!(play(true), play(false));
        assert_eq!(play(true), "a\n\nb\n\nc\n", "restored block survives");
    }

    #[test]
    fn restore_of_live_stash_loser_inserts_adjacent_and_is_idempotent() {
        // A live block's stashed LWW loser is restored as a NEW block adjacent to
        // the winner; replaying the op changes nothing (idempotent).
        let mut doc = BlockDoc::from_markdown(1, "orig\n");
        let id = doc.block_ids_in_order()[0];
        doc.merge(BlockOp::SetContent {
            id,
            text: "loser".into(),
            lamport: 3,
            author: Author::Agent,
        });
        assert_eq!(doc.materialize(), "orig\n"); // human seed wins; "loser" stashed
        assert_eq!(doc.recoverable(id), vec!["loser".to_string()]);
        let ops = doc.restore(id, 0);
        assert_eq!(ops.len(), 1);
        assert_eq!(doc.materialize(), "orig\n\nloser\n");
        // Re-merging the restore op is a no-op (CRDT idempotency law).
        let once = doc.materialize();
        for op in &ops {
            doc.merge(op.clone());
        }
        assert_eq!(doc.materialize(), once);
    }

    #[test]
    fn restore_is_repeatable_two_replicas_converge() {
        // Two replicas independently restore the same deleted block: each mints its
        // own Insert (distinct replica ids), so both copies survive and the
        // replicas converge — restore is additive, not deduped.
        let mut a = BlockDoc::from_markdown(1, "x\n\ny\n");
        let mut b = BlockDoc::from_markdown(2, "");
        for op in a.state_as_ops() {
            b.merge(op); // b adopts a's shared ids
        }
        let y_id = a.block_ids_in_order()[1];
        let del = BlockOp::Delete {
            id: y_id,
            lamport: 50,
        };
        a.merge(del.clone());
        b.merge(del.clone());

        let a_ops = a.restore(y_id, 0);
        let b_ops = b.restore(y_id, 0);
        for op in &b_ops {
            a.merge(op.clone());
        }
        for op in &a_ops {
            b.merge(op.clone());
        }
        assert_eq!(a.materialize(), b.materialize());
        assert_eq!(
            a.materialize().matches("y").count(),
            2,
            "both independent restores survive"
        );
    }

    #[test]
    fn restore_invalid_id_or_index_is_noop() {
        let mut doc = BlockDoc::from_markdown(1, "a\n");
        let id = doc.block_ids_in_order()[0];
        // Live block with nothing stashed ⇒ recoverable is empty ⇒ index 0 misses.
        assert!(doc.restore(id, 0).is_empty());
        // Unknown block id.
        let bogus = BlockId {
            replica: 99,
            counter: 99,
        };
        assert!(doc.restore(bogus, 0).is_empty());
        assert_eq!(doc.materialize(), "a\n", "document unchanged");
    }
}
