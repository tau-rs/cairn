//! Block-level CRDT for one note: an RGA sequence of blocks whose content is
//! an author-priority LWW register. Block IDs are live-only and never reach
//! disk. See docs/decisions/0011-crdt-collaboration-model.md.

use crate::block::{join_blocks, BlockKind};
use std::collections::HashMap;

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

/// Internal per-block state.
#[derive(Debug, Clone)]
struct Entry {
    id: BlockId,
    after: Option<BlockId>,
    ins_lamport: Lamport,
    kind: BlockKind,
    text: String,
    content_lamport: Lamport,
    content_author: Author,
    content_replica: u64,
    tombstone: bool,
    /// Loser content versions retained on conflict (never silently dropped).
    stash: Vec<String>,
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
            let id = BlockId { replica, counter: doc.counter };
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
                    content_replica: replica,
                    tombstone: false,
                    stash: Vec::new(),
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
            BlockOp::Insert { id, after, lamport, kind, text } => {
                self.clock = self.clock.max(lamport);
                self.entries.entry(id).or_insert(Entry {
                    id,
                    after,
                    ins_lamport: lamport,
                    kind,
                    text,
                    content_lamport: lamport,
                    content_author: Author::Human,
                    content_replica: id.replica,
                    tombstone: false,
                    stash: Vec::new(),
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

    /// Live (non-tombstoned) block IDs in materialized order. Test/lookup aid.
    #[must_use]
    pub fn block_ids_in_order(&self) -> Vec<BlockId> {
        let mut children: HashMap<Option<BlockId>, Vec<&Entry>> = HashMap::new();
        for e in self.entries.values() {
            children.entry(e.after).or_default().push(e);
        }
        for v in children.values_mut() {
            v.sort_by(|a, b| b.ins_lamport.cmp(&a.ins_lamport).then_with(|| b.id.cmp(&a.id)));
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

    fn merge_set_content(&mut self, _op: BlockOp) {
        // Filled in Task 7.
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
        let a = BlockId { replica: 1, counter: 0 };
        let b = BlockId { replica: 1, counter: 1 };
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
            id: BlockId { replica: 2, counter: 0 },
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
        doc.merge(BlockOp::Delete { id: drop_id, lamport: 9 });
        assert_eq!(doc.materialize(), "keep\n");
    }
}
