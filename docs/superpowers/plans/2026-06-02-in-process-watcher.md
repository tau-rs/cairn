# In-Process Watcher + Stat-Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring live file-watching to the in-process path via a reusable `run_watch_loop` (daemon refactors onto it, new `cairn watch` consumes it), plus a `(mtime,len)` stat-guard that lets `apply_change` skip reads for spurious watcher events.

**Architecture:** A thin drain primitive in `cairn-service`; the engine-apply/output stays in each caller's closure (daemon locks a shared engine, CLI owns it). A `FileStamp` port capability + `Engine.stamps` map adds the stat-guard. `NotifyWatcher` (the existing real OS watcher) is now hosted in-process by `cairn watch`.

**Tech Stack:** Rust, `std::sync::mpsc`, `std::fs::metadata`, clap, serde_json (CLI `--json`).

**Branch:** `feat/in-process-watcher` (already created; the spec is committed there).

**Spec:** `docs/superpowers/specs/2026-06-02-in-process-watcher-design.md`.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/cairn-ports/src/lib.rs` | `FileStamp` struct + `VaultStore::stamp` method | 1 |
| `crates/cairn-infra/src/localfs.rs` | `LocalFsStore::stamp` impl + test | 1 |
| `crates/cairn-app/src/lib.rs` | `Engine.stamps`, reindex seeds stamps, `apply_change` stat-guard, `apply_removal` drops stamp + tests | 2 |
| `crates/cairn-service/src/lib.rs` | `run_watch_loop` + test | 3 |
| `crates/cairn-daemon/src/main.rs` | refactor watch loop onto `run_watch_loop` | 4 |
| `crates/cairn-cli/src/main.rs` + `Cargo.toml` | `cairn watch` subcommand + `WatchSink` + serde_json + unit tests | 5 |
| `docs/decisions/0005-in-process-watcher.md`, `docs/handoffs/...` | ADR + handoff update | 6 |

Each task ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

**Commit convention:** use `git -c commit.gpgsign=false commit` if signing fails.

---

### Task 1: `FileStamp` port + `LocalFsStore::stamp`

Adding a trait method breaks any `VaultStore` impl lacking it; `LocalFsStore` is the only impl, so add both together.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-infra/src/localfs.rs`

- [ ] **Step 1: Add `FileStamp` + the trait method in ports**

In `crates/cairn-ports/src/lib.rs`, add the struct just above `pub trait VaultStore` (it has no domain imports — uses `std::time::SystemTime`):
```rust
/// Cheap file-change fingerprint: a note's last-modified time and byte length,
/// obtained without reading contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// Last modification time.
    pub modified: std::time::SystemTime,
    /// File length in bytes.
    pub len: u64,
}
```
Add this method to the `VaultStore` trait (after `list`):
```rust
    /// Stat a note's change-fingerprint without reading its contents.
    ///
    /// # Errors
    /// `NotFound` if the note is missing; `Adapter` on other failures.
    fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError>;
```

- [ ] **Step 2: Build ports**

Run: `cargo build -p cairn-ports`
Expected: PASS.

- [ ] **Step 3: Write the failing infra test**

In `crates/cairn-infra/src/localfs.rs`, inside the existing `#[cfg(test)] mod tests`, add (the test module already imports `cairn_domain::NotePath`, `tempfile`, and `PortError` is in scope via the file's `use`):
```rust
    #[test]
    fn stamp_reflects_writes_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let a = NotePath::new("a.md").unwrap();
        assert!(matches!(store.stamp(&a), Err(PortError::NotFound(_))));

        store.write(&a, "hello").unwrap();
        let s1 = store.stamp(&a).unwrap();
        assert_eq!(s1.len, 5);

        // Different length guarantees a different stamp regardless of mtime resolution.
        store.write(&a, "hello world!!").unwrap();
        let s2 = store.stamp(&a).unwrap();
        assert_ne!(s1, s2);
    }
```

- [ ] **Step 4: Run it — fails to compile (no `stamp` impl yet)**

Run: `cargo test -p cairn-infra stamp_reflects_writes_and_missing`
Expected: compile error — `stamp` not implemented for `LocalFsStore`.

- [ ] **Step 5: Implement `LocalFsStore::stamp`**

In `crates/cairn-infra/src/localfs.rs`, add to `impl VaultStore for LocalFsStore` (after `list`). `FileStamp` must be added to the file's `use cairn_ports::{...}` import:
```rust
    fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError> {
        let full = self.full(path);
        let meta = match std::fs::metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PortError::NotFound(path.as_str().to_string()));
            }
            Err(e) => return Err(PortError::Adapter(e.to_string())),
        };
        let modified = meta.modified().map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(FileStamp {
            modified,
            len: meta.len(),
        })
    }
```

- [ ] **Step 6: Run the test + gate**

Run: `cargo test -p cairn-infra stamp_reflects_writes_and_missing` → PASS.
Then: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/localfs.rs
git commit -m "feat(ports,infra): FileStamp + VaultStore::stamp"
```

---

### Task 2: Engine stamps + stat-guard

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Write the failing test (read-counting store proves the read-skip)**

The stat-guard's only distinguishing behavior is *skipping the read* — the existing memo
already suppresses events for unchanged content. So prove it with a `VaultStore` wrapper
that counts reads. In `crates/cairn-app/src/lib.rs`, inside `#[cfg(test)] mod tests`, add
the wrapper and the test (the test module already imports `cairn_infra::{GitVcs,
InMemoryIndex, LocalFsStore}`):
```rust
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A `VaultStore` that counts `read` calls, delegating everything else to
    /// an inner `LocalFsStore`.
    struct CountingStore {
        inner: LocalFsStore,
        reads: Arc<AtomicUsize>,
    }
    impl VaultStore for CountingStore {
        fn read(&self, path: &NotePath) -> Result<String, PortError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.read(path)
        }
        fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError> {
            self.inner.write(path, contents)
        }
        fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
            self.inner.delete(path)
        }
        fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError> {
            self.inner.rename(from, to)
        }
        fn list(&self) -> Result<Vec<NotePath>, PortError> {
            self.inner.list()
        }
        fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError> {
            self.inner.stamp(path)
        }
    }

    #[test]
    fn stat_guard_skips_read_when_stamp_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let store = CountingStore {
            inner: LocalFsStore::open(tmp.path()).unwrap(),
            reads: reads.clone(),
        };
        let mut eng = Engine::new(
            store,
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );

        std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
        let mut ev = Vec::new();
        eng.reindex(&mut ev).unwrap(); // reads a.md once, seeds stamp
        let before = reads.load(Ordering::SeqCst);

        // Unchanged file: the stat-guard must skip the read AND emit nothing.
        let a = NotePath::new("a.md").unwrap();
        let mut e2 = Vec::new();
        eng.apply_change(&FsChange::Changed(a), &mut e2).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), before, "stat-guard must skip the read");
        assert!(e2.is_empty());
    }
```

- [ ] **Step 2: Run it — fails (read still happens)**

Run: `cargo test -p cairn-app stat_guard_skips_read_when_stamp_unchanged`
Expected: FAIL — before the stat-guard, `apply_change(Changed)` reads `a.md`, so the read
count increases past `before`. (Once the `stamps` field + reindex seeding + guard land in
Steps 3–5, the read is skipped and it passes.)

- [ ] **Step 3: Add the `stamps` field + import**

In `crates/cairn-app/src/lib.rs`:
- Change the import `use cairn_ports::{FsChange, PortError, SearchHit, SearchIndex, VaultStore, Vcs};` to add `FileStamp`:
  `use cairn_ports::{FileStamp, FsChange, PortError, SearchHit, SearchIndex, VaultStore, Vcs};`
- Add a field to the `Engine` struct (after `memo`):
  ```rust
      stamps: HashMap<NotePath, FileStamp>,
  ```
- In `Engine::new`, initialize it (after `memo: HashMap::new(),`):
  ```rust
          stamps: HashMap::new(),
  ```

- [ ] **Step 4: Seed stamps in `reindex`**

In `reindex`, after the `self.memo = …` assignment and before `sink.emit(Event::Reindexed(...))`, add:
```rust
        let mut stamps = HashMap::with_capacity(notes.len());
        for n in &notes {
            stamps.insert(n.path.clone(), self.store.stamp(&n.path)?);
        }
        self.stamps = stamps;
```

- [ ] **Step 5: Add the stat-guard to `apply_change` + drop stamp in `apply_removal`**

Replace the `FsChange::Changed(path)` arm body in `apply_change` with:
```rust
            FsChange::Changed(path) => {
                // Stat-guard: skip the read entirely when the file's (mtime,len)
                // is unchanged (a spurious/duplicate watcher event).
                let stamp = match self.store.stamp(path) {
                    Ok(s) => s,
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                if self.stamps.get(path) == Some(&stamp) {
                    return Ok(());
                }
                let raw = match self.store.read(path) {
                    Ok(raw) => raw,
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                let note = Note::parse(path.clone(), &raw);
                let hash = note.content_hash();
                // Record the new stamp even if content reverted, so the next
                // unchanged event short-circuits.
                self.stamps.insert(path.clone(), stamp);
                if self.memo.get(path) == Some(&hash) {
                    return Ok(());
                }
                self.index.upsert(&note)?;
                self.memo.insert(path.clone(), hash);
                sink.emit(Event::NoteChanged(path.clone()));
                sink.emit(Event::Reindexed(self.memo.len()));
                Ok(())
            }
```
In `apply_removal`, inside the `if self.memo.contains_key(path)` block, add `self.stamps.remove(path);` after `self.memo.remove(path);`:
```rust
            self.index.remove(path)?;
            self.memo.remove(path);
            self.stamps.remove(path);
            sink.emit(Event::NoteDeleted(path.clone()));
            sink.emit(Event::Reindexed(self.memo.len()));
```

- [ ] **Step 6: Run app tests**

Run: `cargo test -p cairn-app`
Expected: all pass, including `stat_guard_skips_read_when_stamp_unchanged` (read count now unchanged) and the existing `apply_change_dedups_self_writes_and_emits_on_real_change` (the echo step is now skipped by the stat-guard → still empty; the external write of `"changed"` differs in length from `"hello"`, so the stamp differs → it reads and emits as before).

- [ ] **Step 7: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): stat-guard apply_change via per-note FileStamp"
```

---

### Task 3: `run_watch_loop` primitive

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/cairn-service/src/lib.rs`, inside `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn run_watch_loop_drains_until_sender_drops() {
        use cairn_ports::{FsChange, WatchHandle};
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = WatchHandle::new(rx, Box::new(()));
        tx.send(FsChange::Changed(NotePath::new("a.md").unwrap())).unwrap();
        tx.send(FsChange::Removed(NotePath::new("b.md").unwrap())).unwrap();
        drop(tx); // close the channel → loop ends

        let mut seen = Vec::new();
        run_watch_loop(&handle, |c| seen.push(c.clone()));
        assert_eq!(
            seen,
            vec![
                FsChange::Changed(NotePath::new("a.md").unwrap()),
                FsChange::Removed(NotePath::new("b.md").unwrap()),
            ]
        );
    }
```
(`NotePath` is already imported at the top of `cairn-service/src/lib.rs`.)

- [ ] **Step 2: Run it — fails (no `run_watch_loop`)**

Run: `cargo test -p cairn-service run_watch_loop_drains_until_sender_drops`
Expected: compile error — `run_watch_loop` not found.

- [ ] **Step 3: Implement `run_watch_loop`**

In `crates/cairn-service/src/lib.rs`, add at the top level (after the imports; add `FsChange` and `WatchHandle` to the existing `use cairn_ports::{...}` line):
```rust
/// Drain a watch handle until its sender drops, invoking `on_change` for each
/// debounced change. Blocking — run on a dedicated thread (CLI `watch`) or via
/// `tokio::task::spawn_blocking` (daemon).
///
/// The engine-apply + event-forwarding lives in the caller's `on_change`: the
/// daemon locks a shared engine per change while the CLI owns it, and output
/// differs — centralizing only the drain keeps this testable.
pub fn run_watch_loop(handle: &WatchHandle, mut on_change: impl FnMut(&FsChange)) {
    while let Ok(change) = handle.changes.recv() {
        on_change(&change);
    }
}
```

- [ ] **Step 4: Run the test + gate**

Run: `cargo test -p cairn-service run_watch_loop_drains_until_sender_drops` → PASS.
Then: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): run_watch_loop drain primitive"
```

---

### Task 4: Daemon refactors onto `run_watch_loop`

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs`

- [ ] **Step 1: Replace the hand-rolled loop**

In `crates/cairn-daemon/src/main.rs`, the watch block currently reads:
```rust
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    while let Ok(change) = handle.changes.recv() {
                        watch_state.apply_change_blocking(&change);
                    }
                });
```
Replace the `tokio::task::spawn_blocking` closure body with `run_watch_loop`:
```rust
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    cairn_service::run_watch_loop(&handle, |change| {
                        watch_state.apply_change_blocking(change)
                    });
                });
```
(`apply_change_blocking` takes `&FsChange`; `change` is already `&FsChange` here.)

- [ ] **Step 2: Verify the daemon watcher test still passes**

Run: `cargo test -p cairn-daemon`
Expected: PASS (no behavior change; the existing `tests/watch.rs` integration test still passes).

- [ ] **Step 3: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "refactor(daemon): host watcher via run_watch_loop"
```

---

### Task 5: `cairn watch` command

**Files:**
- Modify: `crates/cairn-cli/Cargo.toml`
- Modify: `crates/cairn-cli/src/main.rs`

- [ ] **Step 1: Add serde_json to the CLI**

In `crates/cairn-cli/Cargo.toml`, under `[dependencies]`, add:
```toml
serde_json = { workspace = true }
```

- [ ] **Step 2: Add imports + the `WatchSink`**

In `crates/cairn-cli/src/main.rs`:
- Extend the imports:
  - `use cairn_app::{Engine, Event};` → `use cairn_app::{Engine, Event, EventSink};`
  - `use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};` → add `NotifyWatcher`: `use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex};`
  - `use cairn_service::{dispatch_command, dispatch_query};` → add `app_event_to_wire, run_watch_loop`: `use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, run_watch_loop};`
  - add `use cairn_ports::Watcher;` and `use std::io::Write;`
- Add the sink type near the top of the file (after the imports, before `Cli`):
```rust
/// Renders engine events for `cairn watch`. Generic over the writer so it is
/// unit-testable without spawning the blocking command.
struct WatchSink<W: Write> {
    json: bool,
    out: W,
}

impl<W: Write> EventSink for WatchSink<W> {
    fn emit(&mut self, event: Event) {
        if self.json {
            let wire = app_event_to_wire(event);
            let _ = writeln!(
                self.out,
                "{}",
                serde_json::to_string(&wire).expect("wire event serializes")
            );
        } else {
            match event {
                Event::NoteChanged(p) => {
                    let _ = writeln!(self.out, "changed {}", p.as_str());
                }
                Event::NoteDeleted(p) => {
                    let _ = writeln!(self.out, "removed {}", p.as_str());
                }
                // Reindexed / Committed are noise for a human watch view.
                _ => {}
            }
        }
    }
}
```

- [ ] **Step 3: Add the `Watch` subcommand variant**

In the `enum Command`, add (e.g. after `Read`):
```rust
    /// Watch the cairn for changes and stream them until interrupted.
    Watch {
        /// Emit one JSON event per line instead of human-readable lines.
        #[arg(long)]
        json: bool,
    },
```

- [ ] **Step 4: Handle it in `run`**

The `run` function already builds `engine` and calls `engine.reindex(&mut events)` before the `match cli.command`. Add this arm to that `match` (it owns the engine and blocks until Ctrl-C):
```rust
        Command::Watch { json } => {
            let handle = NotifyWatcher
                .watch(&root)
                .map_err(|e| format!("file watcher: {e}"))?;
            eprintln!("watching {} for changes", root.display());
            let mut sink = WatchSink {
                json,
                out: std::io::stdout(),
            };
            run_watch_loop(&handle, |change| {
                if let Err(e) = engine.apply_change(change, &mut sink) {
                    eprintln!("watch: {e}");
                }
            });
        }
```
Note: confirm the variable holding the cairn path in `run` is named `root` (it is in the existing arms, e.g. `Command::Init` uses `root.display()`); if it is `cli.cairn`, use that instead — match the surrounding arms.

- [ ] **Step 5: Add WatchSink unit tests**

At the bottom of `crates/cairn-cli/src/main.rs`, add:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::NotePath;

    fn render(json: bool, events: Vec<Event>) -> String {
        let mut sink = WatchSink {
            json,
            out: Vec::<u8>::new(),
        };
        for e in events {
            sink.emit(e);
        }
        String::from_utf8(sink.out).unwrap()
    }

    #[test]
    fn human_lines_skip_reindexed() {
        let out = render(
            false,
            vec![
                Event::NoteChanged(NotePath::new("a.md").unwrap()),
                Event::NoteDeleted(NotePath::new("b.md").unwrap()),
                Event::Reindexed(3),
            ],
        );
        assert_eq!(out, "changed a.md\nremoved b.md\n");
    }

    #[test]
    fn json_lines_include_all_wire_events() {
        let out = render(
            true,
            vec![
                Event::NoteChanged(NotePath::new("a.md").unwrap()),
                Event::Reindexed(2),
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"type\":\"note_changed\""));
        assert!(lines[0].contains("\"path\":\"a.md\""));
        assert!(lines[1].contains("\"type\":\"reindexed\""));
        assert!(lines[1].contains("\"count\":2"));
    }
}
```
`cairn-cli` needs `cairn-domain` as a dependency for the test (it is already listed in `[dependencies]`).

- [ ] **Step 6: Run CLI tests + gate**

Run: `cargo test -p cairn-cli` → pass (incl. the 2 new unit tests).
Then: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
Manual smoke (optional, not automated — it blocks): `cargo run -p cairn-cli -- --cairn ./demo watch` then edit a `.md` in another terminal and confirm `changed <path>` prints.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-cli/Cargo.toml crates/cairn-cli/src/main.rs
git commit -m "feat(cli): cairn watch command (human + --json output)"
```

---

### Task 6: ADR + handoff + final gate

**Files:**
- Create: `docs/decisions/0005-in-process-watcher.md`
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Write ADR-0005**

Create `docs/decisions/0005-in-process-watcher.md` (mirror the structure of `docs/decisions/0004-daemon-cors.md` — read it first for the heading style):
```markdown
# 5. In-process watcher and stat-guard

Date: 2026-06-02

## Status

Accepted

## Context

Live file-watching existed only in the daemon, which hand-rolled a drain loop over
`WatchHandle.changes`. The CLI was one-shot, and any in-process embedder (a future desktop
UI hosting the engine without HTTP) had no way to watch and react. Separately, every
watcher-driven `apply_change` re-read the changed file even for spurious/duplicate debounced
events.

## Decision

Add `run_watch_loop(handle, on_change)` in `cairn-service` — a thin, tested drain primitive.
The daemon refactors onto it; a new long-running `cairn watch [--json]` consumes it. The
engine-apply and output stay in each caller's closure because the daemon locks a shared
`Arc<Mutex<Engine>>` per change while the CLI owns the engine outright.

Add a per-note `(mtime, len)` `FileStamp` (`VaultStore::stamp`) and an `Engine.stamps` map.
`apply_change` stats first and skips the read when the stamp is unchanged.

## Consequences

- The in-process path now has live watching, sharing one tested loop with the daemon.
- The stat-guard avoids reads on spurious events. Its larger payoff (skipping reads across
  restarts) needs on-disk index/stamp persistence, which is deferred; with the in-memory
  index a cold start still reads everything.
- Tradeoff: an external edit preserving the exact `(mtime, len)` is skipped. On
  nanosecond-resolution filesystems distinct edits do not collide; the content-hash memo
  remains the backstop on the read path.
```

- [ ] **Step 2: Update the handoff doc**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`:
- In the §2 CLI demo block, add a line after the existing commands:
  ```
  cargo run -p cairn-cli -- --cairn ./demo watch        # stream live changes (Ctrl-C to stop); --json for machine output
  ```
- In §2's capabilities list, update the file-watching bullet (or add one) to note it now works in-process too:
  ```
  - **Live watching:** external `.md` edits / `git pull` push `note_changed`/`note_deleted`
    events — over the daemon WebSocket and, in-process, via `cairn watch` or by driving
    `cairn_service::run_watch_loop` + `Engine::apply_change` in an embedder.
  ```

- [ ] **Step 3: Final whole-workspace gate**

Run:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check licenses bans sources
```
Expected: all green (`deny` licenses/bans/sources ok; the `advisories` sub-check may crash on an old local cargo-deny — that is a tooling issue, CI runs a newer one).

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0005-in-process-watcher.md docs/handoffs/2026-06-01-ui-session-handoff.md
git commit -m "docs: ADR-0005 in-process watcher + handoff update"
```

---

## Notes for the implementer

- **`run_watch_loop` blocks forever** until the watch handle's sender drops; for `cairn watch` that means until Ctrl-C ends the process. That is intended.
- **Do not move the engine-apply into `run_watch_loop`** — the daemon (shared, locked engine) and CLI (owned engine) need different access; the closure is the seam.
- **Stamp is recorded before the hash check** in `apply_change` so a revert-to-indexed-content event still updates the stamp (next unchanged event short-circuits).
- **`apply_change_blocking` already prints its own errors**; the daemon refactor must not double-handle them.
- **`WatchSink` is generic over `Write`** specifically so the formatting is unit-testable without spawning the blocking command — keep it that way.
