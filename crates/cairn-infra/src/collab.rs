//! `LocalCrdt`: an in-memory `CollabSession` adapter. A single **owner thread**
//! holds one `BlockDoc` per open note (decision ③); the `LocalCrdt` value is a
//! cheap cloneable **handle** that talks to the owner over a channel. Because
//! only the owner ever touches a `BlockDoc`, convergence logic runs
//! single-threaded and is data-race-free by construction — no shared mutex.
//!
//! The `CollabSession` methods stay synchronous: `edit`/`materialize`/`is_active`
//! send a command carrying a reply channel and block on the answer (a
//! synchronous blocking bridge), so call sites do not become `async`. No
//! transport yet — ops are returned to the caller. See ADR-0011.

use cairn_domain::{BlockDoc, BlockOp, Edit, NotePath};
use cairn_ports::CollabSession;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A command to the owner thread. Reply-carrying variants make the handle's
/// method block until the owner answers.
enum Cmd {
    Open {
        path: NotePath,
        markdown: String,
    },
    Edit {
        path: NotePath,
        edit: Edit,
        reply: Sender<Vec<BlockOp>>,
    },
    Merge {
        path: NotePath,
        op: BlockOp,
    },
    Materialize {
        path: NotePath,
        reply: Sender<Option<String>>,
    },
    IsActive {
        reply: Sender<bool>,
    },
}

/// Keeps the owner thread joinable and joins it once the last handle drops.
/// Field order in [`LocalCrdt`] guarantees the last `tx` is dropped before this,
/// closing the channel so the owner's `recv` loop ends and the join returns.
#[derive(Debug)]
struct Owner {
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(h) = self.handle.lock().expect("owner lock").take() {
            let _ = h.join();
        }
    }
}

/// In-memory collaboration session handle. Cloning yields another handle to the
/// same owner thread (and thus the same documents).
#[derive(Debug, Clone)]
pub struct LocalCrdt {
    tx: Sender<Cmd>,
    _owner: Arc<Owner>,
}

impl LocalCrdt {
    /// Create a session for a given replica id (unique per writer/surface).
    /// Spawns the owner thread that solely holds the documents.
    #[must_use]
    pub fn new(replica: u64) -> Self {
        let (tx, rx) = channel::<Cmd>();
        let handle = std::thread::spawn(move || {
            let mut docs: HashMap<NotePath, BlockDoc> = HashMap::new();
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Open { path, markdown } => {
                        docs.insert(path, BlockDoc::from_markdown(replica, &markdown));
                    }
                    Cmd::Edit { path, edit, reply } => {
                        let ops = docs
                            .get_mut(&path)
                            .map(|d| d.apply_local(edit))
                            .unwrap_or_default();
                        let _ = reply.send(ops);
                    }
                    Cmd::Merge { path, op } => {
                        if let Some(d) = docs.get_mut(&path) {
                            d.merge(op);
                        }
                    }
                    Cmd::Materialize { path, reply } => {
                        let _ = reply.send(docs.get(&path).map(BlockDoc::materialize));
                    }
                    Cmd::IsActive { reply } => {
                        let _ = reply.send(!docs.is_empty());
                    }
                }
            }
        });
        Self {
            tx,
            _owner: Arc::new(Owner {
                handle: Mutex::new(Some(handle)),
            }),
        }
    }
}

impl CollabSession for LocalCrdt {
    fn is_active(&self) -> bool {
        let (reply, rx) = channel();
        self.tx.send(Cmd::IsActive { reply }).expect("owner alive");
        rx.recv().expect("owner replies")
    }
    fn open(&self, path: &NotePath, markdown: &str) {
        self.tx
            .send(Cmd::Open {
                path: path.clone(),
                markdown: markdown.to_owned(),
            })
            .expect("owner alive");
    }
    fn edit(&self, path: &NotePath, edit: Edit) -> Vec<BlockOp> {
        let (reply, rx) = channel();
        self.tx
            .send(Cmd::Edit {
                path: path.clone(),
                edit,
                reply,
            })
            .expect("owner alive");
        rx.recv().expect("owner replies")
    }
    fn merge_remote(&self, path: &NotePath, op: BlockOp) {
        self.tx
            .send(Cmd::Merge {
                path: path.clone(),
                op,
            })
            .expect("owner alive");
    }
    fn materialize(&self, path: &NotePath) -> Option<String> {
        let (reply, rx) = channel();
        self.tx
            .send(Cmd::Materialize {
                path: path.clone(),
                reply,
            })
            .expect("owner alive");
        rx.recv().expect("owner replies")
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
        let a = LocalCrdt::new(1);
        let b = LocalCrdt::new(2);
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
        let s = LocalCrdt::new(1);
        assert!(!s.is_active());
        s.open(&NotePath::new("a.md").unwrap(), "x\n");
        assert!(s.is_active());
    }

    #[test]
    fn concurrent_handles_edit_one_doc_without_races() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        const THREADS: usize = 2;
        const PER_THREAD: usize = 50;

        let path = NotePath::new("note.md").unwrap();
        let s = LocalCrdt::new(1);
        s.open(&path, "seed\n");

        // Two handles (clones) sharing one owner hammer InsertAfter on the same
        // doc. A racy read-modify-write on the block counter would hand two
        // inserts the same BlockId and silently drop one; single-owner
        // serialization keeps every counter unique, so all inserts survive.
        let barrier = Arc::new(Barrier::new(THREADS));
        let workers: Vec<_> = (0..THREADS)
            .map(|t| {
                let s = s.clone();
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..PER_THREAD {
                        s.edit(
                            &path,
                            Edit::InsertAfter {
                                after: None,
                                kind: BlockKind::Paragraph,
                                text: format!("t{t}-{i}"),
                                author: Author::Human,
                            },
                        );
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }

        let md = s.materialize(&path).expect("doc is open");
        assert!(md.contains("seed"));
        for t in 0..THREADS {
            for i in 0..PER_THREAD {
                assert!(md.contains(&format!("t{t}-{i}")), "lost insert t{t}-{i}");
            }
        }
    }
}
