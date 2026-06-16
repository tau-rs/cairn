//! `LocalCrdt`: an in-memory `CollabSession` adapter holding one `BlockDoc`
//! per open note. No transport — ops are returned to the caller. See ADR-0011.

use cairn_domain::{BlockDoc, BlockOp, Edit, NotePath};
use cairn_ports::CollabSession;
use std::collections::HashMap;

/// In-memory collaboration session: a `BlockDoc` per open note.
#[derive(Debug, Default)]
pub struct LocalCrdt {
    replica: u64,
    docs: HashMap<NotePath, BlockDoc>,
}

impl LocalCrdt {
    /// Create a session for a given replica id (unique per writer/surface).
    #[must_use]
    pub fn new(replica: u64) -> Self {
        Self {
            replica,
            docs: HashMap::new(),
        }
    }
}

impl CollabSession for LocalCrdt {
    fn is_active(&self) -> bool {
        !self.docs.is_empty()
    }
    fn open(&mut self, path: &NotePath, markdown: &str) {
        self.docs.insert(
            path.clone(),
            BlockDoc::from_markdown(self.replica, markdown),
        );
    }
    fn edit(&mut self, path: &NotePath, edit: Edit) -> Vec<BlockOp> {
        self.docs
            .get_mut(path)
            .map(|d| d.apply_local(edit))
            .unwrap_or_default()
    }
    fn merge_remote(&mut self, path: &NotePath, op: BlockOp) {
        if let Some(d) = self.docs.get_mut(path) {
            d.merge(op);
        }
    }
    fn materialize(&self, path: &NotePath) -> Option<String> {
        self.docs.get(path).map(BlockDoc::materialize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::{Author, BlockKind};

    #[test]
    fn two_replicas_converge_through_the_port() {
        let path = NotePath::new("note.md").unwrap();
        let seed = "shared line\n";
        let mut a = LocalCrdt::new(1);
        let mut b = LocalCrdt::new(2);
        a.open(&path, seed);
        b.open(&path, seed);

        // A appends a block; B appends a different block. Exchange ops.
        let a_ops = a.edit(
            &path,
            Edit::InsertAfter {
                after: None,
                kind: BlockKind::Paragraph,
                text: "from A".into(),
                author: Author::Human,
            },
        );
        let b_ops = b.edit(
            &path,
            Edit::InsertAfter {
                after: None,
                kind: BlockKind::Paragraph,
                text: "from B".into(),
                author: Author::Human,
            },
        );
        for op in &b_ops {
            a.merge_remote(&path, op.clone());
        }
        for op in &a_ops {
            b.merge_remote(&path, op.clone());
        }

        assert_eq!(a.materialize(&path), b.materialize(&path));
    }

    #[test]
    fn is_active_reflects_open_docs() {
        let mut s = LocalCrdt::new(1);
        assert!(!s.is_active());
        s.open(&NotePath::new("a.md").unwrap(), "x\n");
        assert!(s.is_active());
    }
}
