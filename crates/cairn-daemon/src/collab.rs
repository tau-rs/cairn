//! Bidirectional CRDT op-relay over `/collab`. The daemon is a dumb fan-out
//! that also holds one `BlockDoc` replica per open note (the git bridge). PR-1:
//! relay + catch-up snapshot only — no disk writes (materialize/commit and the
//! client adapter land in PR-2). See docs/superpowers/specs/
//! 2026-07-19-crdt-collaboration-transport-design.md.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use cairn_contract::{CollabClientMsg, CollabServerMsg};
use cairn_domain::{BlockDoc, NotePath};
use cairn_service::{block_op_from_wire, block_op_to_wire};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};

/// Replica id reserved for the daemon's own commit-agent replica. Clients must
/// choose other ids (the app assigns small positive ints).
pub const DAEMON_REPLICA: u64 = u64::MAX;

/// A fan-out envelope carrying the originating replica so a peer skips its own
/// echo (merge is idempotent, but skipping avoids redundant traffic).
#[derive(Clone)]
struct Fanout {
    origin: u64,
    msg: CollabServerMsg,
}

/// One live note session: the daemon's replica + a fan-out channel + the set of
/// joined replica ids (last one out tears the session down).
pub struct Session {
    doc: BlockDoc,
    peers: broadcast::Sender<Fanout>,
    participants: HashSet<u64>,
    /// Set when an op has merged since the last successful flush. Cleared
    /// optimistically when a flush captures the session; re-set by
    /// `remark_dirty` if that flush is skipped (foreign-edit conflict) or fails.
    dirty: bool,
    /// Monotonic time of the last merged op; the debounce measures quiescence
    /// against it.
    last_op: Instant,
    /// The exact markdown the daemon last wrote to `N.md` (initialized to the
    /// seed). A pre-write disk read that differs from this signals a foreign
    /// edit — the flush skips rather than clobbers it (A2 folds it back).
    last_written: String,
}

/// The daemon's collab registry: one `Session` per open note.
pub type Collab = Arc<Mutex<HashMap<NotePath, Session>>>;

/// Build an empty collab registry.
#[must_use]
pub fn registry() -> Collab {
    Arc::new(Mutex::new(HashMap::new()))
}

fn lock(collab: &Collab) -> std::sync::MutexGuard<'_, HashMap<NotePath, Session>> {
    collab.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drive one upgraded `/collab` socket. `seed` reads a note's current markdown
/// to seed a fresh session (empty string when the note does not exist yet).
///
/// Assumes ONE replica id per connection: `my_replica` and the disconnect
/// cleanup below are connection-global, so multiplexing distinct replica ids
/// over a single socket is out of scope for PR-1 (to be enforced in PR-2).
pub async fn run_collab<S>(socket: WebSocket, collab: Collab, seed: S)
where
    S: Fn(&NotePath) -> String + Clone + Send + 'static,
{
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<CollabServerMsg>(64);

    // One writer task owns the socket sink.
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "collab: drop msg, serialize failed"),
            }
        }
    });

    let mut my_replica: Option<u64> = None;
    let mut forwarders: Vec<(NotePath, tokio::task::JoinHandle<()>)> = Vec::new();

    while let Some(Ok(frame)) = stream.next().await {
        let Message::Text(text) = frame else { continue };
        let msg: CollabClientMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "collab: drop unparseable client frame");
                continue;
            }
        };
        match msg {
            CollabClientMsg::Join { note, replica } => {
                let Ok(path) = NotePath::new(&note) else {
                    let _ = out_tx
                        .send(CollabServerMsg::Error {
                            note,
                            message: "invalid note path".into(),
                        })
                        .await;
                    continue;
                };
                my_replica = Some(replica);
                // Seed a fresh session's replica OFF the executor and OUTSIDE the
                // collab lock, so a large note's read cannot stall other notes'
                // relay traffic. Discarded if another Join created the session
                // first.
                let seed_fn = seed.clone();
                let seed_path = path.clone();
                let seeded = tokio::task::spawn_blocking(move || seed_fn(&seed_path))
                    .await
                    .unwrap_or_default();
                // Get-or-create the session; take a snapshot + a subscription.
                let joined = {
                    let mut reg = lock(&collab);
                    let sess = reg.entry(path.clone()).or_insert_with(|| {
                        let (tx, _rx) = broadcast::channel(256);
                        Session {
                            doc: BlockDoc::from_markdown(DAEMON_REPLICA, &seeded),
                            peers: tx,
                            participants: HashSet::new(),
                            dirty: false,
                            last_op: Instant::now(),
                            last_written: seeded.clone(),
                        }
                    });
                    if sess.participants.insert(replica) {
                        Some((sess.doc.state_as_ops(), sess.peers.subscribe()))
                    } else {
                        None
                    }
                };
                let Some((ops, rx)) = joined else {
                    let _ = out_tx
                        .send(CollabServerMsg::Error {
                            note,
                            message: "replica id already joined".into(),
                        })
                        .await;
                    continue;
                };
                let _ = out_tx
                    .send(CollabServerMsg::Joined { note: note.clone() })
                    .await;
                let wire_ops = ops.into_iter().map(block_op_to_wire).collect();
                let _ = out_tx
                    .send(CollabServerMsg::Snapshot {
                        note: note.clone(),
                        ops: wire_ops,
                    })
                    .await;

                // Per-note forwarder: fan the session broadcast to this socket,
                // skipping this connection's own ops.
                let out2 = out_tx.clone();
                let handle = tokio::spawn(async move {
                    let mut rx = rx;
                    loop {
                        match rx.recv().await {
                            Ok(f) => {
                                if f.origin == replica {
                                    continue;
                                }
                                if out2.send(f.msg).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                // A lagged collab subscriber has permanently missed
                                // state-critical ops; its replica diverges until it
                                // re-Joins (reconnect-resync is PR-2).
                                tracing::warn!(skipped, "collab: subscriber lagged, dropped ops");
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
                forwarders.push((path, handle));
            }
            CollabClientMsg::Op { note, op } => {
                let Ok(path) = NotePath::new(&note) else {
                    continue;
                };
                let domain_op = block_op_from_wire(op);
                let mut reg = lock(&collab);
                if let Some(sess) = reg.get_mut(&path) {
                    sess.doc.merge(domain_op.clone());
                    let _ = sess.peers.send(Fanout {
                        origin: my_replica.unwrap_or(DAEMON_REPLICA),
                        msg: CollabServerMsg::Op {
                            note,
                            op: block_op_to_wire(domain_op),
                        },
                    });
                    // Mark the session for the debounced flush ticker (spec §12).
                    sess.dirty = true;
                    sess.last_op = Instant::now();
                }
            }
            CollabClientMsg::Leave { note } => {
                if let Ok(path) = NotePath::new(&note) {
                    leave(&collab, &path, my_replica);
                    forwarders.retain(|(p, h)| {
                        if *p == path {
                            h.abort();
                            false
                        } else {
                            true
                        }
                    });
                }
            }
        }
    }

    // Disconnect: drop this replica from every joined note; stop tasks.
    for (path, handle) in forwarders {
        handle.abort();
        leave(&collab, &path, my_replica);
    }
    writer.abort();
}

/// Remove `replica` from a note's session; drop the session when empty and
/// clean. An empty session with unflushed edits is kept so the flush ticker can
/// finalize (materialize + commit) then reap it (spec §12.5).
fn leave(collab: &Collab, path: &NotePath, replica: Option<u64>) {
    let Some(replica) = replica else { return };
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.participants.remove(&replica);
        if sess.participants.is_empty() && !sess.dirty {
            reg.remove(path);
        }
    }
}

/// One note due for materialize-and-commit: captured under the collab lock,
/// written under the engine lock (never both at once).
pub(crate) struct FlushItem {
    pub path: NotePath,
    pub markdown: String,
    pub baseline: String,
}

/// Flush pass, phase 1: under the collab lock only, materialize every dirty
/// session that is quiescent (no op for `debounce`) or abandoned (no
/// participants), returning the write set. `dirty` is cleared optimistically
/// (re-armed by `remark_dirty` on a skipped/failed write). Empty sessions are
/// finalized here and reaped. No engine call happens under the lock.
pub(crate) fn drain_due(collab: &Collab, debounce: Duration) -> Vec<FlushItem> {
    let now = Instant::now();
    let mut items = Vec::new();
    let mut reg = lock(collab);
    reg.retain(|path, sess| {
        let empty = sess.participants.is_empty();
        let due = empty || now.duration_since(sess.last_op) >= debounce;
        if sess.dirty && due {
            items.push(FlushItem {
                path: path.clone(),
                markdown: sess.doc.materialize(),
                baseline: sess.last_written.clone(),
            });
            sess.dirty = false;
        }
        // Keep active sessions; drop empty ones (already finalized above).
        !empty
    });
    items
}

/// Record a successful flush: the bytes now on disk become the new baseline.
/// Does not touch `dirty`, so ops that merged mid-flush keep it set and the next
/// pass re-flushes them. A no-op if the session was already reaped.
pub(crate) fn record_flush(collab: &Collab, path: &NotePath, written: String) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.last_written = written;
    }
}

/// Re-arm a session after a skipped or failed flush (foreign-edit conflict or a
/// write error) so its pending edits are retried rather than lost. A no-op if
/// the session was already reaped.
pub(crate) fn remark_dirty(collab: &Collab, path: &NotePath) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.dirty = true;
    }
}

#[cfg(test)]
pub(crate) fn insert_dirty_session(
    collab: &Collab,
    path: &NotePath,
    seed: &str,
    ops: Vec<cairn_domain::BlockOp>,
) {
    let (tx, _rx) = broadcast::channel(256);
    let mut doc = BlockDoc::from_markdown(DAEMON_REPLICA, seed);
    for op in ops {
        doc.merge(op);
    }
    let mut reg = lock(collab);
    reg.insert(
        path.clone(),
        Session {
            doc,
            peers: tx,
            participants: HashSet::new(),
            dirty: true,
            last_op: Instant::now(),
            last_written: seed.to_string(),
        },
    );
}

#[cfg(test)]
mod flush_tests {
    use super::*;
    use cairn_domain::{block::BlockKind, BlockId, BlockOp};

    fn ins(text: &str) -> BlockOp {
        BlockOp::Insert {
            id: BlockId {
                replica: 1,
                counter: 0,
            },
            after: None,
            lamport: 1,
            kind: BlockKind::Paragraph,
            text: text.into(),
        }
    }

    #[test]
    fn drain_due_flushes_dirty_abandoned_session_and_reaps_it() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("hello")]);
        // No participants ⇒ due regardless of debounce; dirty ⇒ flush.
        let items = drain_due(&reg, Duration::from_secs(3600));
        assert_eq!(items.len(), 1);
        assert!(items[0].markdown.contains("hello"));
        assert_eq!(items[0].baseline, "");
        // Empty session finalized ⇒ reaped.
        assert!(lock(&reg).is_empty());
    }

    #[test]
    fn drain_due_respects_debounce_for_active_sessions() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("hi")]);
        lock(&reg).get_mut(&p).unwrap().participants.insert(7);
        // Fresh last_op + large debounce ⇒ not quiescent ⇒ no flush, session kept.
        let items = drain_due(&reg, Duration::from_secs(3600));
        assert!(items.is_empty());
        assert!(lock(&reg).contains_key(&p));
        assert!(lock(&reg).get(&p).unwrap().dirty);
    }

    #[test]
    fn record_flush_and_remark_dirty_settle_a_session() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("x")]);
        lock(&reg).get_mut(&p).unwrap().participants.insert(1);
        // debounce 0 ⇒ due ⇒ drain clears dirty (session kept, has a participant).
        let _ = drain_due(&reg, Duration::ZERO);
        assert!(!lock(&reg).get(&p).unwrap().dirty);
        remark_dirty(&reg, &p);
        assert!(lock(&reg).get(&p).unwrap().dirty);
        record_flush(&reg, &p, "written".into());
        assert_eq!(lock(&reg).get(&p).unwrap().last_written, "written");
    }

    #[test]
    fn leave_keeps_empty_dirty_session_for_the_ticker() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("z")]);
        lock(&reg).get_mut(&p).unwrap().participants.insert(9);
        // Last peer leaves while dirty ⇒ session must NOT be dropped (final flush pending).
        leave(&reg, &p, Some(9));
        assert!(
            lock(&reg).contains_key(&p),
            "empty+dirty session kept for ticker"
        );
    }
}
