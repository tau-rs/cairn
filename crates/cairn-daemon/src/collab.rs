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
use cairn_service::{block_op_from_wire, block_op_to_wire, recoverable_block_to_wire};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};

/// Replica id reserved for the daemon's own commit-agent replica. Clients must
/// choose other ids (the app assigns small positive ints).
pub const DAEMON_REPLICA: u64 = u64::MAX;

/// A fan-out envelope carrying the originating replica so a peer skips its own
/// echo (merge is idempotent, but skipping avoids redundant traffic).
#[derive(Clone)]
pub(crate) struct Fanout {
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
    /// optimistically when a flush captures the session; re-set by `settle_flush`
    /// if that flush is skipped (foreign-edit conflict) or fails.
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

/// What the `/collab` seed closure returns: the HEAD markdown to seed the replica
/// (and initialize `last_written`), plus whether the working tree already diverges
/// from HEAD so the first flush folds that pre-existing edit (spec §13.4).
pub struct Seed {
    pub markdown: String,
    pub dirty: bool,
}

/// Drive one upgraded `/collab` socket. `seed` reads a note's HEAD markdown to
/// seed a fresh session (empty when the note does not exist yet at HEAD), plus
/// whether the working tree already diverges from that HEAD snapshot.
///
/// Assumes ONE replica id per connection: `my_replica` and the disconnect
/// cleanup below are connection-global, so multiplexing distinct replica ids
/// over a single socket is out of scope for PR-1 (to be enforced in PR-2).
pub async fn run_collab<S>(socket: WebSocket, collab: Collab, seed: S)
where
    S: Fn(&NotePath) -> Seed + Clone + Send + 'static,
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
                let seeded: Seed = tokio::task::spawn_blocking(move || seed_fn(&seed_path))
                    .await
                    .unwrap_or(Seed {
                        markdown: String::new(),
                        dirty: false,
                    });
                // Get-or-create the session; take a snapshot + a subscription.
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
            CollabClientMsg::Recover { note } => {
                let Ok(path) = NotePath::new(&note) else {
                    let _ = out_tx
                        .send(CollabServerMsg::Error {
                            note,
                            message: "invalid note path".into(),
                        })
                        .await;
                    continue;
                };
                let blocks = recoverable_blocks(&collab, &path)
                    .into_iter()
                    .map(recoverable_block_to_wire)
                    .collect();
                let _ = out_tx
                    .send(CollabServerMsg::Recoverable { note, blocks })
                    .await;
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

/// The result of the engine-side (phase 2) write for one `FlushItem`, fed back
/// to `settle_flush` so it can update the baseline and decide reaping.
pub(crate) enum FlushOutcome {
    /// `write_note` landed these bytes on disk (commit may have failed — the
    /// bytes are still the on-disk truth, so they become the new baseline).
    Committed(String),
    /// `write_note` itself failed; nothing landed.
    WriteError,
}

/// Flush pass, phase 1: under the collab lock only, materialize every dirty
/// session that is quiescent (no op for `debounce`) or abandoned (no
/// participants), returning the write set. `dirty` is cleared optimistically —
/// a session captured here is **kept** (not reaped) so `settle_flush` can reap
/// it only after the write is confirmed; an idle empty+clean session is reaped
/// here since it has nothing to persist. No engine call happens under the lock.
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
            // Keep it alive through phase 2; settle_flush reaps after a
            // confirmed write so a failed write can still re-arm (Finding A) and
            // a concurrent Join reuses this session rather than reseeding a
            // stale duplicate from pre-write disk (Finding B).
            true
        } else {
            // Not flushing: reap an idle abandoned (empty+clean) session; keep
            // active or dirty ones.
            !empty
        }
    });
    items
}

/// Settle one flushed session after its phase-2 write, under the collab lock.
/// A no-op if the session was reaped meanwhile. Reaps an abandoned session only
/// once its edits are safely on disk, never before. Foreign edits are folded
/// back before the write (see `fold_foreign`), so a settled outcome is only
/// Committed or WriteError.
pub(crate) fn settle_flush(collab: &Collab, path: &NotePath, outcome: FlushOutcome) {
    let mut reg = lock(collab);
    let Some(sess) = reg.get_mut(path) else {
        return;
    };
    let reap = match outcome {
        // The write landed: these bytes are the new on-disk baseline (even if the
        // commit failed — the next op re-flushes and re-commits without a false
        // foreign-edit conflict). Reap only a still-abandoned, settled session.
        FlushOutcome::Committed(written) => {
            sess.last_written = written;
            sess.participants.is_empty() && !sess.dirty
        }
        // Transient write failure: keep the edits and retry next pass.
        FlushOutcome::WriteError => {
            sess.dirty = true;
            false
        }
    };
    if reap {
        reg.remove(path);
    }
}

/// Fold a foreign on-disk edit into a session's live replica, under the collab
/// lock only. Merges the block-diff of `foreign` against the session's baseline
/// into `doc`, fans the produced ops out to peers, advances `last_written` to the
/// consumed `foreign` bytes, and leaves the session dirty so the next flush pass
/// writes the merged result. A no-op if the session was reaped meanwhile. This is
/// the fold-back critical section that replaces A1's conflict-skip (spec §13.1/§13.2),
/// called from `run_collab_flush_pass` when the on-disk bytes diverge from baseline.
pub(crate) fn fold_foreign(collab: &Collab, path: &NotePath, foreign: &str) {
    let mut reg = lock(collab);
    let Some(sess) = reg.get_mut(path) else {
        return;
    };
    let base = sess.last_written.clone();
    // Spec §13.3: if the live replica already diverged from the baseline (a peer
    // edited concurrently), the block-diff falls back to content-matching; make
    // that observable.
    if sess.doc.materialize() != base {
        tracing::warn!(
            note = %path.as_str(),
            "collab fold-back: live replica diverged from baseline; content-match fallback"
        );
    }
    let ops = sess.doc.fold_foreign(&base, foreign);
    let produced = !ops.is_empty();
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
    // Always reconcile the baseline — even for a fold that produced no ops (a
    // foreign edit differing only in whitespace that normalizes away) — so the
    // same divergence is not re-detected on every subsequent pass.
    sess.last_written = foreign.to_string();
    // Only re-arm the flush when the fold actually changed the doc; a zero-op fold
    // has nothing to rewrite, so it must not trigger a needless `collab sync`
    // commit. Never *clears* dirty, so a real pending edit still flushes.
    if produced {
        sess.dirty = true;
        sess.last_op = Instant::now();
    }
}

/// Whether a live session is open on this note (the daemon owns `N.md`).
#[must_use]
pub(crate) fn is_sessioned(collab: &Collab, path: &NotePath) -> bool {
    lock(collab).contains_key(path)
}

/// The recoverable content of a session's live replica (view-only). Empty when
/// there is no session for `path`. Read-only: never merges, marks dirty, or fans
/// out — the caller replies to the requesting socket only.
pub(crate) fn recoverable_blocks(
    collab: &Collab,
    path: &NotePath,
) -> Vec<cairn_domain::RecoverableBlock> {
    lock(collab)
        .get(path)
        .map(|sess| sess.doc.recoverable_blocks())
        .unwrap_or_default()
}

/// Record a foreign on-disk edit detected by the watcher: if `disk` diverges from
/// the session's baseline, mark it dirty so the flush pass folds it (spec §13.5).
/// A no-op when there is no session or when `disk` equals the last self-write
/// (echo suppression — the daemon's own materialize writes must not re-arm it).
pub(crate) fn note_foreign_edit(collab: &Collab, path: &NotePath, disk: &str) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        if sess.last_written != disk {
            sess.dirty = true;
            sess.last_op = Instant::now();
        }
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

/// Merge an op into an existing session and mark it dirty, exactly as the WS
/// `Op` arm does. Test-only.
#[cfg(test)]
pub(crate) fn merge_op(collab: &Collab, path: &NotePath, op: cairn_domain::BlockOp) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.doc.merge(op);
        sess.dirty = true;
        sess.last_op = Instant::now();
    }
}

/// Join a replica to a session so it is not treated as abandoned. Test-only.
#[cfg(test)]
pub(crate) fn add_participant(collab: &Collab, path: &NotePath, replica: u64) {
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.participants.insert(replica);
    }
}

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
    fn drain_due_captures_abandoned_session_but_keeps_it_until_settled() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("hello")]);
        // No participants ⇒ due regardless of debounce; dirty ⇒ flush.
        let items = drain_due(&reg, Duration::from_secs(3600));
        assert_eq!(items.len(), 1);
        assert!(items[0].markdown.contains("hello"));
        assert_eq!(items[0].baseline, "");
        // Kept alive (dirty cleared) until settle_flush confirms the write —
        // NOT reaped in drain (Finding A: a failed write must be re-armable).
        assert!(lock(&reg).contains_key(&p));
        assert!(!lock(&reg).get(&p).unwrap().dirty);
        // A confirmed write reaps the now-settled abandoned session.
        settle_flush(&reg, &p, FlushOutcome::Committed("hello\n".into()));
        assert!(lock(&reg).is_empty());
    }

    #[test]
    fn drain_due_reaps_idle_empty_clean_session() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("z")]);
        // Clear dirty (as a prior successful flush would) ⇒ idle empty+clean.
        lock(&reg).get_mut(&p).unwrap().dirty = false;
        let items = drain_due(&reg, Duration::ZERO);
        assert!(items.is_empty());
        assert!(lock(&reg).is_empty(), "idle empty+clean session reaped");
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
    fn settle_committed_updates_baseline_and_keeps_active_session() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("x")]);
        lock(&reg).get_mut(&p).unwrap().participants.insert(1);
        // debounce 0 ⇒ due ⇒ drain clears dirty (session kept, has a participant).
        let _ = drain_due(&reg, Duration::ZERO);
        assert!(!lock(&reg).get(&p).unwrap().dirty);
        settle_flush(&reg, &p, FlushOutcome::Committed("written".into()));
        // Active session: baseline updated, NOT reaped.
        assert!(lock(&reg).contains_key(&p));
        assert_eq!(lock(&reg).get(&p).unwrap().last_written, "written");
    }

    #[test]
    fn settle_write_error_rearms_abandoned_session_no_loss() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "", vec![ins("keep me")]);
        let _ = drain_due(&reg, Duration::ZERO); // captured, dirty cleared, kept
                                                 // A failed write must re-arm (dirty) and keep the session — the edits
                                                 // live nowhere else (Finding A).
        settle_flush(&reg, &p, FlushOutcome::WriteError);
        assert!(lock(&reg).contains_key(&p));
        assert!(lock(&reg).get(&p).unwrap().dirty);
    }

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
            assert!(
                sess.doc.materialize().contains("b"),
                "foreign edit in replica"
            );
            // (2) Baseline advanced to the consumed disk bytes.
            assert_eq!(sess.last_written, "a\n\nb\n");
            // (3) Session stays dirty so the next pass writes the merged result.
            assert!(sess.dirty);
        }
        // (4) Fanned out to peers: at least one Insert op arrived.
        let f = rx.try_recv().expect("a folded op was fanned out");
        assert!(matches!(
            fanout_op(&f),
            Some(cairn_domain::BlockOp::Insert { .. })
        ));
    }

    #[test]
    fn fold_foreign_whitespace_only_edit_does_not_mark_dirty() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        // Clean session; baseline is the canonical two-block form.
        insert_seeded_session(&reg, &p, "a\n\nb\n", false);
        add_participant(&reg, &p, 7);
        let mut rx = test_subscribe(&reg, &p);

        // Foreign edit differs only in whitespace that normalizes away (extra
        // blank separator lines): parse_blocks yields the same block texts, so
        // the fold produces zero ops.
        fold_foreign(&reg, &p, "a\n\n\n\nb\n");

        let reg = lock(&reg);
        let sess = reg.get(&p).unwrap();
        // No semantic change ⇒ session must NOT be re-armed for a rewrite/commit.
        assert!(!sess.dirty, "whitespace-only fold must not mark dirty");
        // But the baseline IS reconciled to the consumed bytes, so the divergence
        // is not re-detected on every subsequent pass.
        assert_eq!(sess.last_written, "a\n\n\n\nb\n");
        // Nothing was fanned out to peers.
        assert!(rx.try_recv().is_err(), "a zero-op fold fans out nothing");
    }

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
        assert!(
            lock(&reg).get(&p).unwrap().dirty,
            "foreign edit marks dirty"
        );
        assert!(is_sessioned(&reg, &p));
        assert!(!is_sessioned(&reg, &NotePath::new("other.md").unwrap()));
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

    #[test]
    fn recoverable_blocks_reads_session_and_does_not_fan_out() {
        let reg = registry();
        let p = NotePath::new("n.md").unwrap();
        insert_dirty_session(&reg, &p, "gone\n", vec![]);
        add_participant(&reg, &p, 7);
        // Delete the only block so its content is recoverable-but-hidden.
        let id = {
            let reg = lock(&reg);
            reg.get(&p).unwrap().doc.block_ids_in_order()[0]
        };
        merge_op(&reg, &p, BlockOp::Delete { id, lamport: 5 });

        // A peer is subscribed; a read-only recovery query must NOT fan anything out.
        let mut rx = test_subscribe(&reg, &p);
        let blocks = recoverable_blocks(&reg, &p);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].tombstoned);
        assert!(blocks[0].versions.contains(&"gone".to_string()));
        assert!(
            rx.try_recv().is_err(),
            "recovery is read-only, nothing fanned out"
        );
    }

    #[test]
    fn recoverable_blocks_absent_session_is_empty() {
        let reg = registry();
        let p = NotePath::new("nope.md").unwrap();
        assert!(recoverable_blocks(&reg, &p).is_empty());
    }
}
