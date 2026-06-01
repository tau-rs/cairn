# File Watcher + Content-Hash Memo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The daemon pushes precise `note_changed`/`note_deleted`/`reindexed` events on external `.md` edits, with an engine content-hash memo that dedupes cairn's own writes and no-op rewrites.

**Architecture:** A `Watcher` port redesign (`FsChange` + `WatchHandle`) with a `notify`-based adapter; the engine gains a `memo: HashMap<NotePath,u64>` and an `apply_change` primitive that is the single source of all change-events (commands and the watcher both route through it); the daemon hosts the watcher and feeds `FsChange`es into `apply_change`.

**Tech Stack:** Rust 1.85, `notify` + `notify-debouncer-full`, `std::hash::DefaultHasher`, axum/tokio (daemon), nextest.

**Verified current shapes:** `Note { path: NotePath, frontmatter: Option<String>, body: String }`, `Note::parse(NotePath,&str)`. `SearchIndex { reindex(&mut,&[Note]), search(&self,&str)->Vec<SearchHit> }`; `InMemoryIndex { docs: Vec<Note> }`. `Watcher { start(&mut self)->Result<(),PortError> }` + `NoopWatcher` in `crates/cairn-infra/src/seams.rs` (its test `seams_have_expected_neutral_behavior` asserts `NoopWatcher.start()` — must be updated). `Engine { store, index, vcs }`, `Engine::new(store,index,vcs)`; `write_note` = `store.write` + `emit NoteChanged` + `reindex`; `delete_note` = `store.delete` + `emit NoteDeleted` + `reindex`; `reindex` = `load_all_notes` + `index.reindex` + `emit Reindexed(len)`; private `load_all_notes`. `Event::{NoteChanged(NotePath),NoteDeleted(NotePath),Committed(String),Reindexed(usize)}`. Daemon `AppState { engine: Arc<Mutex<CairnEngine>>, events: broadcast::Sender<WireEvent> }`, `BroadcastSink`, `run_command_blocking`; `main.rs run()` builds engine → `reindex` → `AppState::new` → `build_router` → bind → `axum::serve`.

---

## Task 1: Domain — `Note::content_hash`

**Files:** Modify `crates/cairn-domain/src/note.rs`

- [ ] **Step 1: Write the failing test**

In `crates/cairn-domain/src/note.rs`, inside `impl Note` (after `display_title`), add:
```rust
    /// A stable non-cryptographic hash of the note's content (frontmatter +
    /// body), for change detection / memoization. Not for security.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.frontmatter.hash(&mut h);
        self.body.hash(&mut h);
        h.finish()
    }
```
Add to the `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let p = NotePath::new("a.md").unwrap();
        let a1 = Note::parse(p.clone(), "---\ntitle: X\n---\nbody");
        let a2 = Note::parse(p.clone(), "---\ntitle: X\n---\nbody");
        let b = Note::parse(p, "---\ntitle: X\n---\nDIFFERENT");
        assert_eq!(a1.content_hash(), a2.content_hash());
        assert_ne!(a1.content_hash(), b.content_hash());
    }
```

- [ ] **Step 2: Run + lint**

Run: `cargo test -p cairn-domain content_hash` then `cargo clippy -p cairn-domain --all-targets -- -D warnings` and `cargo fmt --all -- --check`. All pass.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(domain): Note::content_hash for change detection"
```

---

## Task 2: Ports — `FsChange`, `Watcher` redesign, `SearchIndex` upsert/remove (+ keep infra compiling)

**Files:** Modify `crates/cairn-ports/src/lib.rs`, `crates/cairn-infra/src/seams.rs`, `crates/cairn-infra/src/index.rs`

- [ ] **Step 1: Redesign the `Watcher` port + add `FsChange`/`WatchHandle`**

In `crates/cairn-ports/src/lib.rs`, replace the existing `Watcher` trait (the `pub trait Watcher { fn start(...) }`) with:
```rust
/// A change to a note detected on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    /// A note was created or modified.
    Changed(NotePath),
    /// A note was removed.
    Removed(NotePath),
}

/// Owns the OS watcher and delivers debounced changes; dropping it stops
/// watching.
pub struct WatchHandle {
    /// Debounced note changes.
    pub changes: std::sync::mpsc::Receiver<FsChange>,
    // Keeps the underlying OS watcher alive for the handle's lifetime.
    _keepalive: Box<dyn Send>,
}

impl WatchHandle {
    /// Build a handle from a change receiver and an opaque keepalive (the
    /// adapter's live watcher).
    #[must_use]
    pub fn new(changes: std::sync::mpsc::Receiver<FsChange>, keepalive: Box<dyn Send>) -> Self {
        Self { changes, _keepalive: keepalive }
    }
}

/// Detects external changes to the cairn.
pub trait Watcher {
    /// Begin watching `root`; returns a handle delivering debounced changes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the OS watcher cannot be created.
    fn watch(&self, root: &std::path::Path) -> Result<WatchHandle, PortError>;
}
```

- [ ] **Step 2: Add incremental primitives to `SearchIndex`**

In `crates/cairn-ports/src/lib.rs`, add to the `SearchIndex` trait (alongside `reindex`/`search`):
```rust
    /// Insert or replace a single note in the index.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
    /// Remove a single note from the index.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
```

- [ ] **Step 3: Update `NoopWatcher` + its seam test (infra)**

In `crates/cairn-infra/src/seams.rs`: update the import to `use cairn_ports::{AgentRuntime, CollabSession, Executor, PortError, Watcher, WatchHandle};` and replace the `impl Watcher for NoopWatcher` with:
```rust
impl Watcher for NoopWatcher {
    fn watch(&self, _root: &std::path::Path) -> Result<WatchHandle, PortError> {
        // Park the sender in the keepalive so the receiver never yields and
        // never disconnects: a no-op watcher reports no changes, ever.
        let (tx, rx) = std::sync::mpsc::channel::<cairn_ports::FsChange>();
        Ok(WatchHandle::new(rx, Box::new(tx)))
    }
}
```
In the `#[cfg(test)] mod tests` of `seams.rs`, replace the `NoopWatcher.start()` assertion in `seams_have_expected_neutral_behavior` with:
```rust
        let handle = NoopWatcher.watch(std::path::Path::new(".")).unwrap();
        assert!(handle
            .changes
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
```
(Keep the `NoCollab`/`NullRuntime` assertions in that test as they are.)

- [ ] **Step 4: Implement `upsert`/`remove` on `InMemoryIndex`**

In `crates/cairn-infra/src/index.rs`, add to `impl SearchIndex for InMemoryIndex` (after `search`):
```rust
    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        if let Some(slot) = self.docs.iter_mut().find(|d| d.path == note.path) {
            *slot = note.clone();
        } else {
            self.docs.push(note.clone());
        }
        Ok(())
    }

    fn remove(&mut self, path: &cairn_domain::NotePath) -> Result<(), PortError> {
        self.docs.retain(|d| &d.path != path);
        Ok(())
    }
```
Add to the `index.rs` `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn upsert_then_remove() {
        let mut idx = InMemoryIndex::default();
        idx.upsert(&note("a.md", "hello target")).unwrap();
        assert_eq!(idx.search("target").unwrap(), vec![SearchHit { path: NotePath::new("a.md").unwrap() }]);
        idx.upsert(&note("a.md", "changed")).unwrap(); // replace, not duplicate
        assert!(idx.search("target").unwrap().is_empty());
        assert_eq!(idx.search("changed").unwrap().len(), 1);
        idx.remove(&NotePath::new("a.md").unwrap()).unwrap();
        assert!(idx.search("changed").unwrap().is_empty());
    }
```

- [ ] **Step 5: Build, test, lint, commit**

Run: `cargo test -p cairn-ports -p cairn-infra`, `cargo clippy -p cairn-ports -p cairn-infra --all-targets -- -D warnings`, `cargo fmt --all -- --check`. (Workspace won't fully build yet — `cairn-app` still calls the old paths; that's fixed in Task 3. These two crates compile + test in isolation.)
```bash
git add -A && git commit -m "feat(ports): FsChange + Watcher::watch redesign + SearchIndex upsert/remove"
```

---

## Task 3: App — memo + `apply_change` (single event source)

**Files:** Modify `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Add the memo field + import**

In `crates/cairn-app/src/lib.rs`: update imports to include `FsChange` and `std::collections::HashMap`:
```rust
use cairn_ports::{FsChange, PortError, SearchHit, SearchIndex, VaultStore, Vcs};
use std::collections::HashMap;
```
(Keep the existing `cairn_domain` import of `Graph, Note, NotePath`.)

Add a `memo` field to `Engine`:
```rust
pub struct Engine<S, I, V> {
    store: S,
    index: I,
    vcs: V,
    memo: HashMap<NotePath, u64>,
}
```
Update `Engine::new` to initialize it:
```rust
    pub fn new(store: S, index: I, vcs: V) -> Self {
        Self { store, index, vcs, memo: HashMap::new() }
    }
```

- [ ] **Step 2: Replace `reindex`, `write_note`, `delete_note`; add `apply_change`**

Replace the existing `reindex`, `write_note`, and `delete_note` methods with the following, and add `apply_change` + the private `apply_removal`:
```rust
    /// Rebuild the index and the content-hash memo from the store (startup /
    /// full rescan). Emits [`Event::Reindexed`].
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        self.memo = notes.iter().map(|n| (n.path.clone(), n.content_hash())).collect();
        sink.emit(Event::Reindexed(notes.len()));
        Ok(())
    }

    /// Apply a single filesystem change, deduped via the content-hash memo.
    /// This is the single source of change-events: it emits only when the
    /// content actually differs from what is indexed.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn apply_change(
        &mut self,
        change: &FsChange,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        match change {
            FsChange::Changed(path) => {
                let raw = match self.store.read(path) {
                    Ok(raw) => raw,
                    // The file vanished between the event and the read.
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                let note = Note::parse(path.clone(), &raw);
                let hash = note.content_hash();
                if self.memo.get(path) == Some(&hash) {
                    return Ok(()); // no real change (self-write echo / no-op rewrite)
                }
                self.index.upsert(&note)?;
                self.memo.insert(path.clone(), hash);
                sink.emit(Event::NoteChanged(path.clone()));
                sink.emit(Event::Reindexed(self.memo.len()));
                Ok(())
            }
            FsChange::Removed(path) => self.apply_removal(path, sink),
        }
    }

    fn apply_removal(&mut self, path: &NotePath, sink: &mut dyn EventSink) -> Result<(), PortError> {
        if self.memo.remove(path).is_some() {
            self.index.remove(path)?;
            sink.emit(Event::NoteDeleted(path.clone()));
            sink.emit(Event::Reindexed(self.memo.len()));
        }
        Ok(())
    }

    /// Create or overwrite a note; emits via the memo diff (see [`Engine::apply_change`]).
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn write_note(
        &mut self,
        path: &NotePath,
        contents: &str,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.write(path, contents)?;
        self.apply_change(&FsChange::Changed(path.clone()), sink)
    }

    /// Delete a note; emits via the memo diff (see [`Engine::apply_change`]).
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn delete_note(
        &mut self,
        path: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.delete(path)?;
        self.apply_change(&FsChange::Removed(path.clone()), sink)
    }
```
(Leave `read_note`, `search`, `backlinks`, `list_notes`, `graph`, `load_all_notes` unchanged.)

- [ ] **Step 3: Add memo/apply_change tests**

In the `#[cfg(test)] mod tests` of `cairn-app/src/lib.rs`, add:
```rust
    #[test]
    fn apply_change_dedups_self_writes_and_emits_on_real_change() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let a = NotePath::new("a.md").unwrap();

        // First write -> one NoteChanged + Reindexed.
        let mut e1 = Vec::new();
        eng.write_note(&a, "hello", &mut e1).unwrap();
        assert_eq!(e1, vec![Event::NoteChanged(a.clone()), Event::Reindexed(1)]);

        // Echo: same content already on disk -> apply_change emits nothing.
        let mut e2 = Vec::new();
        eng.apply_change(&FsChange::Changed(a.clone()), &mut e2).unwrap();
        assert!(e2.is_empty());

        // Real external change -> emits again.
        std::fs::write(tmp.path().join("a.md"), "changed").unwrap();
        let mut e3 = Vec::new();
        eng.apply_change(&FsChange::Changed(a.clone()), &mut e3).unwrap();
        assert_eq!(e3, vec![Event::NoteChanged(a.clone()), Event::Reindexed(1)]);

        // Removal -> NoteDeleted; removing again -> nothing.
        std::fs::remove_file(tmp.path().join("a.md")).unwrap();
        let mut e4 = Vec::new();
        eng.apply_change(&FsChange::Removed(a.clone()), &mut e4).unwrap();
        assert_eq!(e4, vec![Event::NoteDeleted(a.clone()), Event::Reindexed(0)]);
        let mut e5 = Vec::new();
        eng.apply_change(&FsChange::Removed(a.clone()), &mut e5).unwrap();
        assert!(e5.is_empty());
    }
```
(The existing `write_then_search_and_backlinks` test — which asserts the `[NoteChanged(a), Reindexed(1), NoteChanged(b), Reindexed(2)]` sequence — must still pass unchanged. If it fails, the refactor is wrong; fix the code, not the test.)

- [ ] **Step 4: Build, test, lint, commit**

Run: `cargo test -p cairn-app`, `cargo clippy -p cairn-app --all-targets -- -D warnings`, `cargo build --workspace` (the whole workspace compiles again now), `cargo fmt --all -- --check`.
```bash
git add -A && git commit -m "feat(app): content-hash memo + apply_change as single event source"
```

---

## Task 4: Infra — `NotifyWatcher`

**Files:** Create `crates/cairn-infra/src/notify_watcher.rs`; modify `crates/cairn-infra/src/lib.rs`, `crates/cairn-infra/Cargo.toml`, root `Cargo.toml`

- [ ] **Step 1: Add deps**

In the root `Cargo.toml` `[workspace.dependencies]`, add:
```toml
notify = "6"
notify-debouncer-full = "0.3"
```
In `crates/cairn-infra/Cargo.toml` `[dependencies]`, add:
```toml
notify = { workspace = true }
notify-debouncer-full = { workspace = true }
```
(If these versions don't build on Rust 1.85 or fail `cargo-deny`, pin a compatible version — verify with `cargo build --locked` + `cargo +nightly? no` — just `cargo build`. Report what you pinned.)

- [ ] **Step 2: Implement `NotifyWatcher`**

Create `crates/cairn-infra/src/notify_watcher.rs`:
```rust
//! A `Watcher` backed by `notify` + `notify-debouncer-full`. Watches a cairn
//! root recursively, reports debounced changes to `.md` files (ignoring
//! `.git/`), and classifies each by current on-disk existence.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use cairn_domain::NotePath;
use cairn_ports::{FsChange, PortError, WatchHandle, Watcher};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

/// Filesystem watcher over a cairn directory.
#[derive(Debug, Default)]
pub struct NotifyWatcher;

/// Map an absolute changed path to an `FsChange`, or `None` to ignore it.
/// Keeps only `.md` files under `root`, skipping any `.git/` segment.
/// Classifies by existence: present -> Changed, absent -> Removed.
fn classify(root: &Path, path: &Path) -> Option<FsChange> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.to_str()?.replace('\\', "/");
    if rel.split('/').any(|seg| seg == ".git") {
        return None;
    }
    if !rel.ends_with(".md") {
        return None;
    }
    let note_path = NotePath::new(&rel).ok()?;
    if path.exists() {
        Some(FsChange::Changed(note_path))
    } else {
        Some(FsChange::Removed(note_path))
    }
}

impl Watcher for NotifyWatcher {
    fn watch(&self, root: &Path) -> Result<WatchHandle, PortError> {
        let (tx, rx) = mpsc::channel::<FsChange>();
        let root = root.to_path_buf();
        let cb_root = root.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            None,
            move |result: notify_debouncer_full::DebounceEventResult| {
                let Ok(events) = result else { return };
                for event in events {
                    for path in &event.paths {
                        if let Some(change) = classify(&cb_root, path) {
                            let _ = tx.send(change);
                        }
                    }
                }
            },
        )
        .map_err(|e| PortError::Adapter(e.to_string()))?;
        debouncer
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(WatchHandle::new(rx, Box::new(debouncer)))
    }
}
```
NOTE on the `notify-debouncer-full` API: signature and the debounced-event shape vary by version. Adapt minimally to the installed version while preserving behavior: ~200 ms debounce, recursive watch of `root`, iterate the debounced events' paths, run them through `classify`, send `FsChange` on the channel, and store the live debouncer in the `WatchHandle` keepalive (`Box::new(debouncer)`). The debouncer must be `Send`; if it isn't directly, wrap as needed. `event.paths` / `event.event.paths` — use whichever the version exposes.

Add to `crates/cairn-infra/src/lib.rs`:
```rust
pub mod notify_watcher;
pub use notify_watcher::NotifyWatcher;
```

- [ ] **Step 3: Integration test (tempdir)**

Create `crates/cairn-infra/tests/notify_watcher.rs`:
```rust
use cairn_domain::NotePath;
use cairn_infra::NotifyWatcher;
use cairn_ports::{FsChange, Watcher};
use std::time::Duration;

fn drain_for(rx: &std::sync::mpsc::Receiver<FsChange>, want: &FsChange) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(c) if &c == want => return true,
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn reports_md_create_and_ignores_git_and_non_md() {
    let tmp = tempfile::tempdir().unwrap();
    let handle = NotifyWatcher.watch(tmp.path()).unwrap();

    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join(".git/config"), "x").unwrap();
    std::fs::write(tmp.path().join("note.txt"), "x").unwrap();
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();

    // The .md create is reported.
    assert!(drain_for(&handle.changes, &FsChange::Changed(NotePath::new("a.md").unwrap())));
}

#[test]
fn reports_md_removal() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    let handle = NotifyWatcher.watch(tmp.path()).unwrap();
    std::fs::remove_file(tmp.path().join("a.md")).unwrap();
    assert!(drain_for(&handle.changes, &FsChange::Removed(NotePath::new("a.md").unwrap())));
}
```
(`tempfile` is already a dev-dependency of `cairn-infra`.)

- [ ] **Step 4: Build, test, lint, commit**

Run: `cargo test -p cairn-infra` (first build compiles notify; may be slow), `cargo clippy -p cairn-infra --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cargo build --locked --workspace` (pin deps if MSRV-1.85 fails; report).
```bash
git add -A && git commit -m "feat(infra): NotifyWatcher (notify + debouncer)"
```

---

## Task 5: Daemon — host the watcher

**Files:** Modify `crates/cairn-daemon/src/lib.rs`, `crates/cairn-daemon/src/main.rs`; create `crates/cairn-daemon/tests/watch.rs`

- [ ] **Step 1: Add `apply_change_blocking` to `AppState`**

In `crates/cairn-daemon/src/lib.rs`, extend the `cairn_*` imports to bring in `FsChange` (from `cairn_contract`? no — `FsChange` is in `cairn_ports`; add `use cairn_ports::FsChange;` or reference it fully). Then add to `impl AppState` (after `run_query_blocking`):
```rust
    /// Apply a watcher-reported filesystem change, publishing any resulting
    /// events. Best-effort: errors are logged, not propagated (a transient
    /// read failure must not kill the watch loop).
    pub fn apply_change_blocking(&self, change: &cairn_ports::FsChange) {
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
        let mut sink = BroadcastSink(self.events.clone());
        if let Err(e) = guard.apply_change(change, &mut sink) {
            eprintln!("watch: apply_change failed: {e}");
        }
    }
```
Add `cairn-ports` to `crates/cairn-daemon/Cargo.toml` `[dependencies]` if not already present:
```toml
cairn-ports = { path = "../cairn-ports" }
```

- [ ] **Step 2: Start the watcher in the binary**

In `crates/cairn-daemon/src/main.rs`, add a `--no-watch` flag to `Cli`:
```rust
    /// Disable the filesystem watcher (no live events on external edits).
    #[arg(long)]
    no_watch: bool,
```
In `run()`, after `let app = build_router(...)` — change it to keep a clone of the state and start the watcher before `axum::serve`:
```rust
    let state = AppState::new(engine);
    let app = build_router(state.clone());

    if !cli.no_watch {
        match cairn_infra::NotifyWatcher.watch(&cli.cairn) {
            Ok(handle) => {
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    while let Ok(change) = handle.changes.recv() {
                        watch_state.apply_change_blocking(&change);
                    }
                });
            }
            Err(e) => eprintln!("warning: file watcher disabled: {e}"),
        }
    }
```
(`AppState` is `Clone`; `cairn_infra` and `cairn_ports` are deps of the daemon. Adjust the existing `build_router(AppState::new(engine))` line to the two-line `state` form above. The `Watcher` trait must be in scope: `use cairn_infra::NotifyWatcher;` and `use cairn_ports::Watcher;` at the top of `main.rs`.)

- [ ] **Step 3: Deterministic integration test (no fs-timing)**

Create `crates/cairn-daemon/tests/watch.rs` — drives `apply_change_blocking` directly (the real `NotifyWatcher` is covered by Task 4; this proves the daemon publishes + dedups):
```rust
use cairn_app::Engine;
use cairn_domain::NotePath;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_ports::FsChange;
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_change_pushes_event_then_dedups() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine);
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = axum::serve(listener, app).await; });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Simulate an external edit: write the file, then notify the engine.
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    let a = NotePath::new("a.md").unwrap();
    state.apply_change_blocking(&FsChange::Changed(a.clone()));

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out")
        .expect("stream ended")
        .expect("ws error");
    let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(json["type"], "note_changed");
    assert_eq!(json["path"], "a.md");

    // Same content again -> memo dedups -> no further frame.
    state.apply_change_blocking(&FsChange::Changed(a));
    assert!(tokio::time::timeout(Duration::from_millis(800), ws.next()).await.is_err());
}
```
(`futures-util`, `tokio-tungstenite`, `tempfile` are already dev-deps; add `cairn-ports` to `[dev-dependencies]` of the daemon if the test needs it directly — it does, for `FsChange`. If `cairn-ports` is a normal dep from Step 1, no dev-dep entry is needed.)

- [ ] **Step 4: Build, test, lint, commit**

Run: `cargo test -p cairn-daemon`, `cargo clippy -p cairn-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`. Confirm `cargo run -p cairn-daemon -- --help` shows `--no-watch`.
```bash
git add -A && git commit -m "feat(daemon): host the file watcher (default on, --no-watch)"
```

---

## Task 6: ADR + handoff + full gate

**Files:** Create `docs/decisions/0003-file-watcher.md`; modify `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: ADR-0003**

Create `docs/decisions/0003-file-watcher.md` (mirror ADR-0002 style): Context (the `Watcher` seam; UI needs live external-edit events); Decision (`FsChange`/`WatchHandle` port redesign; engine `memo` + `apply_change` as the single event source; existence-based `.md` classification ignoring `.git/`; `notify`+debouncer adapter; daemon hosts it default-on/`--no-watch`, resilient); Consequences (external edits + git pulls now emit events; self-writes/no-op rewrites deduped; `SearchIndex` gained incremental `upsert`/`remove`; deferred: incremental *reads*, in-process/Tauri watcher, graph memo). Reference the spec.

- [ ] **Step 2: Update the handoff**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, update §6's "Real file-watcher (today: no push on external edits...)" bullet to state that the **daemon now pushes `note_changed`/`note_deleted` on external `.md` edits and `git pull`** (default on; `--no-watch` to disable), and that cairn's own writes/no-op rewrites are deduped by a content-hash memo. Move the watcher out of the §7 "not here yet" list (or note it's done for the daemon; in-process/Tauri watcher still pending).

- [ ] **Step 3: Full workspace gate**

Run and confirm green:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
Report the total test count. If `cargo-deny` is available, run it; otherwise CI's cargo-deny job will vet the new `notify` deps.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "docs: ADR-0003 file watcher + handoff update"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** §3 content_hash → Task 1; §4 FsChange/Watcher/SearchIndex → Task 2; §5 memo+apply_change+refactors → Task 3; §6 NotifyWatcher → Task 4; §7 daemon hosting → Task 5; §8 tests → Tasks 1–5; §9 deps → Task 4; docs → Task 6. (Note: the spec wrote `content_hash(raw)` as a free fn; implemented as `Note::content_hash(&self)` so the command path and `reindex` hash the *same* parsed representation — consistency requirement; documented here.)
- **Type consistency:** `FsChange::{Changed,Removed}`, `WatchHandle::new(changes, keepalive)`, `Watcher::watch(&self,&Path)->Result<WatchHandle,_>`, `SearchIndex::{upsert(&Note),remove(&NotePath)}`, `Note::content_hash()->u64`, `Engine.memo: HashMap<NotePath,u64>`, `Engine::apply_change(&FsChange,&mut dyn EventSink)`, `AppState::apply_change_blocking(&FsChange)` are used consistently across Tasks 1–5.
- **Placeholder scan:** no TBD/TODO; every code step is complete. The notify-debouncer-full API note is an explicit adaptation point (version-dependent), not a placeholder — behavior + tests are fully specified.
- **Compile ordering:** Task 2 changes the `Watcher`/`SearchIndex` traits and updates the infra adapters in the same task so each crate compiles in isolation; the full workspace recompiles in Task 3 once `cairn-app` is migrated off the old `start()`/explicit-emit paths.
```
