//! Bidirectional CRDT op-relay over `/collab`. The daemon is a dumb fan-out
//! that also holds one `BlockDoc` replica per open note (the git bridge). PR-1:
//! relay + catch-up snapshot only — no disk writes (materialize/commit and the
//! client adapter land in PR-2). See docs/superpowers/specs/
//! 2026-07-19-crdt-collaboration-transport-design.md.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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
pub async fn run_collab<S>(socket: WebSocket, collab: Collab, seed: S)
where
    S: Fn(&NotePath) -> String + Send + 'static,
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
                // Get-or-create the session; take a snapshot + a subscription.
                let joined = {
                    let mut reg = lock(&collab);
                    let sess = reg.entry(path.clone()).or_insert_with(|| {
                        let (tx, _rx) = broadcast::channel(256);
                        Session {
                            doc: BlockDoc::from_markdown(DAEMON_REPLICA, &seed(&path)),
                            peers: tx,
                            participants: HashSet::new(),
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
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
                    // PR-1: no materialize/commit — the daemon replica stays in memory.
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

/// Remove `replica` from a note's session; drop the session when empty.
fn leave(collab: &Collab, path: &NotePath, replica: Option<u64>) {
    let Some(replica) = replica else { return };
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.participants.remove(&replica);
        if sess.participants.is_empty() {
            reg.remove(path);
        }
    }
}
