# CRDT Collaboration Transport — PR-2 / A1 (Daemon Commit-Agent) Implementation Plan

> **Historical record — 2026-08-27.** This plan shipped as written; it is kept
> as the record of what was built, not as a description of current behavior.
> PR #179 moved commit policy into the engine, so the flush described below no
> longer commits: it writes, calls `mark_activity()`, and a seal loop commits
> once per idle session. `config.sync.quiet_period_ms` is now
> `config.sync.idle_seconds` (deprecated alias retained), `auto_commit` defaults
> **true**, and `FlushOutcome::Committed` is `FlushOutcome::Written`. See
> `docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md` §14
> and ADR-0012 §Update.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the daemon the git commit-agent for a live collab session — debounce-materialize its `BlockDoc` replica to `N.md` and git-commit it, with the file-watcher ignoring the self-write and no silent clobber of foreign on-disk edits.

**Architecture:** A single testable pass, `AppState::run_collab_flush_pass(debounce)`, does two phases under one global lock order: phase 1 materializes every dirty+quiescent session **under the collab lock only** (no engine call); phase 2 takes the **engine lock** per note to write (via `Engine::write_note`, which records the stat-stamp so the watcher self-suppresses) and commit. `main.rs` spawns a 250ms ticker calling that pass, symmetric with the existing watcher auto-commit loop. The write is guarded by a disk-vs-`last_written` comparison so a foreign edit is never overwritten (fold-back = A2). Design: `docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md` §12.

**Tech Stack:** Rust (MSRV 1.88), axum/tokio daemon, `std::time::{Duration,Instant}`, existing `Engine`/`GitVcs`/`LocalFsStore`/`TantivyIndex`, `tempfile` (dev-dep, already used by `tests/collab.rs`).

## Global Constraints

- MSRV 1.88; `#![forbid(unsafe_code)]` — no `unsafe`.
- `cairn-domain` stays **serde-free**; this slice does **not** touch `cairn-domain`, `cairn-contract`, or `cairn-service` — daemon crate only, plus one doc file.
- `thiserror` at boundaries, `anyhow` internally. This slice adds **no new error types** — the flush is best-effort (log-and-continue), mirroring `commit_external_blocking`.
- **No new crate dependencies** ⇒ no `Cargo.lock` change.
- Lock order is a single global order: **never hold the collab lock across an engine call; never acquire the collab lock while holding the engine guard.** Every `collab::*` call in phase 2 happens *after* `drop(guard)`.
- Merge queue: branch off `main` → PR → `gh pr merge --auto --squash`. No manual rebase/local-merge. Shared working dir → check `git branch` before every commit.
- DoD: `cargo test --workspace` + `cargo clippy --workspace --all-targets --locked` + `cargo fmt --check` all green. (`invoke_times_out_and_kills_plugin` is known-flaky in this sandbox — see project memory — and is unrelated.)
- Conventional commits, imperative, scoped.

---

## File Structure

- `crates/cairn-daemon/src/collab.rs` — **modify**: add per-session flush state (`dirty`, `last_op`, `last_written`) to `Session`; mark dirty in the `Op` arm; change `leave` to keep empty+dirty sessions; add the collab-side flush API (`FlushItem`, `drain_due`, `record_flush`, `remark_dirty`) and a `#[cfg(test)]` injector; unit tests for the debounce/reap semantics. **Session fields stay private to this module** — the engine phase reaches them only through `drain_due`/`record_flush`/`remark_dirty`.
- `crates/cairn-daemon/src/lib.rs` — **modify**: add `AppState::run_collab_flush_pass(&self, debounce: Duration)` and the DoD unit test (materialize→commit→watcher-non-reingest).
- `crates/cairn-daemon/src/main.rs` — **modify**: spawn the debounced flush ticker.
- `docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md` — **already modified** (§12 commit-boundary delta). No task; committed with Task 1.

---

## Task 1: Session flush-state + collab-side flush API

**Files:**
- Modify: `crates/cairn-daemon/src/collab.rs`
- Test: `crates/cairn-daemon/src/collab.rs` (`#[cfg(test)] mod flush_tests`)

**Interfaces:**
- Consumes: existing `Session`, `Collab`, `lock`, `registry`, `DAEMON_REPLICA`, `Fanout`; `cairn_domain::{BlockDoc, BlockOp, NotePath}`.
- Produces (all `pub(crate)`, used by `lib.rs` Task 2):
  - `struct FlushItem { path: NotePath, markdown: String, baseline: String }`
  - `fn drain_due(collab: &Collab, debounce: Duration) -> Vec<FlushItem>`
  - `fn record_flush(collab: &Collab, path: &NotePath, written: String)`
  - `fn remark_dirty(collab: &Collab, path: &NotePath)`
  - `#[cfg(test)] fn insert_dirty_session(collab: &Collab, path: &NotePath, seed: &str, ops: Vec<BlockOp>)`

- [ ] **Step 1: Write the failing tests**

Append to `crates/cairn-daemon/src/collab.rs`:

```rust
#[cfg(test)]
mod flush_tests {
    use super::*;
    use cairn_domain::{block::BlockKind, BlockId, BlockOp};

    fn ins(text: &str) -> BlockOp {
        BlockOp::Insert {
            id: BlockId { replica: 1, counter: 0 },
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
        assert!(lock(&reg).contains_key(&p), "empty+dirty session kept for ticker");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-daemon --lib flush_tests`
Expected: FAIL to compile — `cannot find function drain_due` / `insert_dirty_session` / no field `dirty` on `Session`.

- [ ] **Step 3: Add the flush-state fields and imports**

At the top of `crates/cairn-daemon/src/collab.rs`, add to the `std` imports:

```rust
use std::time::{Duration, Instant};
```

Extend `Session` (add three fields after `participants`):

```rust
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
```

- [ ] **Step 4: Initialize the fields on session creation and mark dirty on `Op`**

In the `Join` arm's `or_insert_with`, replace the `Session { … }` literal so it initializes the new fields. The current closure body is:

```rust
                    let sess = reg.entry(path.clone()).or_insert_with(|| {
                        let (tx, _rx) = broadcast::channel(256);
                        Session {
                            doc: BlockDoc::from_markdown(DAEMON_REPLICA, &seeded),
                            peers: tx,
                            participants: HashSet::new(),
                        }
                    });
```

Replace with:

```rust
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
```

In the `CollabClientMsg::Op` arm, inside `if let Some(sess) = reg.get_mut(&path)`, after the `sess.peers.send(...)` fan-out (replacing the `// PR-1: no materialize/commit …` comment), add:

```rust
                    // Mark the session for the debounced flush ticker (spec §12).
                    sess.dirty = true;
                    sess.last_op = Instant::now();
```

- [ ] **Step 5: Change `leave` to keep empty+dirty sessions**

Replace the body of `fn leave`:

```rust
fn leave(collab: &Collab, path: &NotePath, replica: Option<u64>) {
    let Some(replica) = replica else { return };
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.participants.remove(&replica);
        // Empty + clean: nothing to persist, drop now. Empty + dirty: keep so
        // the flush ticker can finalize (materialize + commit) then reap it.
        if sess.participants.is_empty() && !sess.dirty {
            reg.remove(path);
        }
    }
}
```

- [ ] **Step 6: Add the flush API + the `#[cfg(test)]` injector**

Add these to `crates/cairn-daemon/src/collab.rs` (e.g. after `fn leave`):

```rust
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
```

> Reap corner (documented, A1-acceptable): an abandoned (empty) session that is dirty is finalized and reaped in the same pass, so a phase-2 foreign-edit conflict on it loses the session's *unpersisted live edits* (the foreign on-disk edit is preserved — the "never lose foreign work" floor holds). A2's fold-back removes this corner.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon --lib flush_tests`
Expected: PASS (4 tests). Then `cargo test -p cairn-daemon --lib` to confirm no regression in the existing collab code.

- [ ] **Step 8: Commit** (design doc rides along — see Global Constraints on when to commit)

```bash
git branch   # confirm: crdt-daemon-commit-agent
git add crates/cairn-daemon/src/collab.rs docs/superpowers/specs/2026-07-19-crdt-collaboration-transport-design.md
git commit -m "feat(collab): per-session flush state + collab-side flush API

Add dirty/last_op/last_written to Session, mark dirty on merged ops, and
keep empty+dirty sessions for the ticker to finalize. drain_due captures
due sessions (materialize under the collab lock only), record_flush /
remark_dirty settle them after the engine-side write. Documents the A1
commit boundary in the design spec (§12). Unit-tested.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `AppState::run_collab_flush_pass` + the commit-boundary test

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs`
- Test: `crates/cairn-daemon/src/lib.rs` (`#[cfg(test)] mod collab_flush_tests`)

**Interfaces:**
- Consumes: `collab::{drain_due, record_flush, remark_dirty, insert_dirty_session}` (Task 1); existing `AppState.engine()`, `AppState.events`, `AppState.collab`, `EventTap`; `Engine::{read_note, write_note, has_uncommitted_changes, commit, note_at, apply_change}`; `cairn_ports::FsChange`.
- Produces: `pub fn AppState::run_collab_flush_pass(&self, debounce: std::time::Duration)` — one flush pass over the collab registry (called by the `main.rs` ticker in Task 3 and directly by tests).

- [ ] **Step 1: Write the failing test**

Append to `crates/cairn-daemon/src/lib.rs`:

```rust
#[cfg(test)]
mod collab_flush_tests {
    use super::*;
    use cairn_app::Engine;
    use cairn_domain::{block::BlockKind, BlockId, BlockOp, NotePath};
    use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
    use cairn_ports::FsChange;

    fn ins(text: &str) -> BlockOp {
        BlockOp::Insert {
            id: BlockId { replica: 1, counter: 0 },
            after: None,
            lamport: 1,
            kind: BlockKind::Paragraph,
            text: text.into(),
        }
    }

    #[test]
    fn flush_materializes_commits_and_watcher_ignores_self_write() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = Engine::new(
            LocalFsStore::open(tmp.path()).unwrap(),
            TantivyIndex::in_memory().unwrap(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        let state = AppState::new(engine);
        let path = NotePath::new("n.md").unwrap();

        // A live edit sitting in the daemon's replica, as the WS `Op` arm leaves it.
        collab::insert_dirty_session(&state.collab, &path, "", vec![ins("live edit")]);

        // One flush pass; debounce 0 ⇒ flush immediately (deterministic, no sleeps).
        state.run_collab_flush_pass(std::time::Duration::ZERO);

        // (1) Materialized to disk.
        let on_disk = std::fs::read_to_string(tmp.path().join("n.md")).unwrap();
        assert!(on_disk.contains("live edit"), "materialized markdown on disk");

        // (2) Committed: the note is readable at git HEAD.
        {
            let guard = state.engine();
            let at_head = guard.note_at(&path, "HEAD").expect("note committed at HEAD");
            assert!(at_head.contains("live edit"), "flush created a commit");
        }

        // (3) The watcher does NOT re-ingest the self-write: apply_change on the
        // just-written file emits nothing (engine stat-guard), exactly as the
        // real watcher path would see it.
        let mut guard = state.engine();
        let mut tap = EventTap { tx: state.events.clone(), collected: Vec::new() };
        guard
            .apply_change(&FsChange::Changed(path.clone()), &mut tap)
            .unwrap();
        assert!(tap.collected.is_empty(), "self-write must not re-ingest");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-daemon --lib flush_materializes_commits`
Expected: FAIL to compile — `no method named run_collab_flush_pass found for struct AppState`.

- [ ] **Step 3: Add `run_collab_flush_pass`**

If not already present, add near the top of `crates/cairn-daemon/src/lib.rs`:

```rust
use std::time::Duration;
```

Add this method to `impl AppState` (place it next to `commit_external_blocking`):

```rust
    /// One collab commit-agent pass: materialize + git-commit every session that
    /// is dirty and quiescent (or abandoned). Phase 1 (`drain_due`) works under
    /// the collab lock only; phase 2 takes the engine lock per note. The write
    /// goes through `write_note` so the file-watcher self-suppresses the echo,
    /// and a disk-vs-baseline check skips (never clobbers) a foreign edit — A2
    /// folds those back instead. Best-effort: every failure logs and continues.
    /// Run from a blocking context (`spawn_blocking`), like the watch loop.
    /// See the design spec §12.
    pub fn run_collab_flush_pass(&self, debounce: Duration) {
        let items = collab::drain_due(&self.collab, debounce);
        for item in items {
            let mut guard = self.engine();
            let disk = guard.read_note(&item.path).unwrap_or_default();
            if disk != item.baseline {
                drop(guard);
                tracing::warn!(
                    note = %item.path.as_str(),
                    "collab flush: foreign on-disk edit; skipping write (fold-back is A2)"
                );
                collab::remark_dirty(&self.collab, &item.path);
                continue;
            }
            let mut tap = EventTap { tx: self.events.clone(), collected: Vec::new() };
            if let Err(e) = guard.write_note(&item.path, &item.markdown, &mut tap) {
                drop(guard);
                tracing::warn!(note = %item.path.as_str(), error = %e, "collab flush: write failed");
                collab::remark_dirty(&self.collab, &item.path);
                continue;
            }
            match guard.has_uncommitted_changes() {
                Ok(true) => {
                    let msg = format!("cairn: collab sync {}", item.path.as_str());
                    if let Err(e) = guard.commit(&msg, &mut tap) {
                        tracing::warn!(error = %e, "collab flush: commit failed");
                    }
                }
                Ok(false) => {} // materialize matched disk; nothing to commit
                Err(e) => tracing::warn!(error = %e, "collab flush: dirty-check failed"),
            }
            drop(guard);
            collab::record_flush(&self.collab, &item.path, item.markdown);
        }
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-daemon --lib flush_materializes_commits`
Expected: PASS. Then `cargo test -p cairn-daemon` (lib + `tests/collab.rs` integration) to confirm PR-1's relay tests still pass.

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: crdt-daemon-commit-agent
git add crates/cairn-daemon/src/lib.rs
git commit -m "feat(daemon): run_collab_flush_pass — materialize + commit sessions

The daemon commit-agent: materialize each dirty/quiescent session's
BlockDoc to N.md via write_note (so the watcher self-suppresses the echo)
and git-commit it, skipping a foreign on-disk edit rather than clobbering
it (fold-back is A2). Single global lock order (collab then engine, never
nested). Tested: a live edit materializes, commits at HEAD, and the
watcher's apply_change does not re-ingest the self-write.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Spawn the debounced flush ticker in `main.rs`

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs`

**Interfaces:**
- Consumes: `AppState::run_collab_flush_pass` (Task 2); existing `state`, `config.sync.quiet_period_ms`, `Duration`.
- Produces: a background tokio task (no new public surface). Behavior is covered by Task 2's pass test; scheduling glue has no separate unit test.

- [ ] **Step 1: Add the ticker**

In `crates/cairn-daemon/src/main.rs`, immediately after the `if !cli.no_watch { … }` block (before the `let addr = …` line), add:

```rust
    // Collab commit-agent: debounce-materialize + commit sessioned notes. Runs
    // independently of the file watcher — the daemon is the sole disk writer for
    // a live session (design spec §12). Ticks every 250ms; a session commits
    // after `quiet_period_ms` of no ops (or immediately once abandoned).
    {
        let flush_state = state.clone();
        let quiet = Duration::from_millis(config.sync.quiet_period_ms);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                let s = flush_state.clone();
                if tokio::task::spawn_blocking(move || s.run_collab_flush_pass(quiet))
                    .await
                    .is_err()
                {
                    break; // runtime shutting down
                }
            }
        });
    }
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build -p cairn-daemon`
Expected: clean build (no warnings on the new code).

- [ ] **Step 3: Commit**

```bash
git branch   # confirm: crdt-daemon-commit-agent
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): spawn the collab flush ticker

Run run_collab_flush_pass on a 250ms tick (debounce = quiet_period_ms),
symmetric with the watcher auto-commit loop, so live sessions commit on
quiescence and abandoned ones flush immediately.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Full guardrails + PR

- [ ] **Step 1: Run the full DoD suite**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --locked
cargo fmt --check
```
Expected: all green (modulo the known-flaky `invoke_times_out_and_kills_plugin`). Fix any clippy/fmt findings on the new code and amend the relevant commit.

- [ ] **Step 2: Push and open the PR against `main`**

```bash
git push -u origin crdt-daemon-commit-agent
gh pr create --base main --title "feat(collab): daemon commit-agent — PR-2 / A1 (materialize + commit)" \
  --body "Implements PR-2 / A1 of the CRDT collaboration transport (design spec §12, roadmap Epic A). The daemon becomes the git commit-agent for a live session: it debounce-materializes its BlockDoc replica to N.md via Engine::write_note (so the file-watcher self-suppresses the echo) and git-commits it, skipping — never clobbering — a foreign on-disk edit (fold-back is A2). No new deps; cairn-domain untouched.

Tested: a live edit materializes, commits at HEAD, and the watcher's apply_change does not re-ingest the self-write; plus collab-side unit tests for the debounce/reap/settle semantics.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

- [ ] **Step 3: Enqueue via the merge queue (only after review approval)**

```bash
gh pr merge --auto --squash
```

---

## Self-Review

**1. Spec coverage (design §12):**
- §12.1 centralized debounced ticker + testable pass → Task 2 (`run_collab_flush_pass`) + Task 3 (ticker). ✅
- §12.1 debounce = `quiet_period_ms`, 250ms tick, `cairn: collab sync {note}` message → Task 3 + Task 2 commit line. ✅
- §12.2 self-write suppression via `write_note` (stat-guard) → Task 2 `run_collab_flush_pass` + the DoD test's `apply_change` assertion. ✅
- §12.3 non-clobber (disk vs `last_written`, warn+skip) → Task 1 `last_written` + Task 2 conflict branch. ✅
- §12.4 seed from working tree → unchanged (`read_note` in the existing handler); nothing to modify. ✅
- §12.5 teardown (empty+clean drop / empty+dirty finalize+reap) → Task 1 `leave` change + `drain_due` reap + `leave_keeps_empty_dirty_session…` test. ✅
- §12.6 watcher/session no-conflict under default `auto_commit=false` → no code needed (documented). ✅
- Lock order single global (collab then engine, never nested) → Task 2 `drop(guard)` before every `collab::*`; Global Constraints. ✅

**2. Placeholder scan:** No TBD/TODO/"handle errors"/"similar to". Every code step is complete literal code. The reap corner is a documented design limit, not a placeholder. ✅

**3. Type consistency:** `Session.{dirty,last_op,last_written}` defined in Task 1, initialized in the `Join`/`insert_dirty_session` literals and read in `drain_due`/`leave`. `FlushItem{path,markdown,baseline}` produced by `drain_due` (Task 1), consumed field-for-field in `run_collab_flush_pass` (Task 2). `record_flush`/`remark_dirty` signatures match call sites. `run_collab_flush_pass(&self, Duration)` defined Task 2, called in Task 3 ticker and the Task 2 test. `EventTap{tx,collected}`, `Engine::{read_note,write_note,has_uncommitted_changes,commit,note_at,apply_change}`, `FsChange::Changed`, `NotePath::as_str` all match the verified signatures. No new deps ⇒ no `Cargo.lock` churn. ✅

## Notes carried to A2 (not this plan)

- Non-clobber currently *skips* on divergence; A2 replaces it with a block-diff fold-back (spec §3.2) that merges the foreign edit into the live doc as `BlockOp`s.
- Seed switches to `Engine::note_at(path, "HEAD")` + reconciliation of pre-existing uncommitted working-tree changes when the commit boundary requires HEAD as the baseline.
- Watcher/session commit arbitration (watcher defers per-note to the session flush under `auto_commit=true`, spec §3.1) is A2.
- The abandoned-session reap corner (empty+dirty + foreign-edit conflict loses unpersisted live edits) disappears once fold-back lands.
