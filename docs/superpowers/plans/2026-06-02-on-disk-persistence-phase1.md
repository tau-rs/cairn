# On-Disk Persistence Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The daemon persists its Tantivy index under `<cairn>/.cairn/` and reconciles on startup (re-indexing only notes changed while it was off), with the daemon as sole writer.

**Architecture:** A held `IndexWriter` makes `TantivyIndex` lock-holding and adds `open_at` (MmapDirectory). A sidecar `state.json` (per-note hash + `(mtime,len)` stamp) lets `Engine::reconcile` stat-diff instead of full-rebuild. `VaultStore` gains `read_meta`/`write_meta`. Default-on; `--no-persist`/`[index] persist=false` opt out.

**Tech Stack:** Rust, Tantivy 0.26 (`MmapDirectory`, `Index::open_or_create`, held `IndexWriter`), serde/serde_json (app-layer state DTO), toml (daemon config).

**Branch:** `feat/on-disk-index` (already created; the spec is committed there).

**Spec:** `docs/superpowers/specs/2026-06-02-on-disk-persistence-phase1-design.md`.

**Validated:** the on-disk Tantivy API (held writer, reopen-finds-content, second-writer-conflicts) was compile-and-test-verified in a probe; the adapter code in Task 2 is that verified shape.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/cairn-ports/src/lib.rs` | `VaultStore::read_meta`/`write_meta` | 1 |
| `crates/cairn-infra/src/localfs.rs` + `lib.rs` | impls + `ensure_cairn_dir` helper (exported) | 1 |
| `crates/cairn-app/src/lib.rs` | update `CountingStore` test mock for the new methods | 1 |
| `crates/cairn-infra/src/tantivy_index.rs` | held-writer refactor + `TantivyIndex::open_at` + fallback | 2 |
| `crates/cairn-app/src/lib.rs` + `Cargo.toml` | `Engine::reconcile`, state DTO, serde deps | 3 |
| `crates/cairn-daemon/src/config.rs` + `main.rs` | `[index]` config, `--no-persist`, wiring | 4 |
| `docs/decisions/0006-on-disk-persistence.md`, handoff | ADR + docs | 5 |

Each task ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

**Cargo.lock:** Task 3 adds serde/serde_json to `cairn-app`. CI runs `clippy`/`locked-check` with `--locked`, so **commit `Cargo.lock`** in that task or CI fails.

**Commit convention:** use `git -c commit.gpgsign=false commit` if signing fails.

---

### Task 1: `VaultStore` metadata blob + `ensure_cairn_dir`

Adding trait methods breaks every `VaultStore` impl: `LocalFsStore` (real) and `CountingStore` (the test mock in `cairn-app/src/lib.rs`). Update all three together.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-infra/src/localfs.rs`, `crates/cairn-infra/src/lib.rs`
- Modify: `crates/cairn-app/src/lib.rs` (CountingStore mock)

- [ ] **Step 1: Add the trait methods**

In `crates/cairn-ports/src/lib.rs`, add to the `VaultStore` trait (after `stamp`):
```rust
    /// Read the persisted engine metadata blob (`<root>/.cairn/state.json`), if present.
    ///
    /// # Errors
    /// `Adapter` on a read/IO failure. A missing file is `Ok(None)`, not an error.
    fn read_meta(&self) -> Result<Option<String>, PortError>;

    /// Write the engine metadata blob, creating `<root>/.cairn/` if needed.
    ///
    /// # Errors
    /// `Adapter` on an IO failure.
    fn write_meta(&self, data: &str) -> Result<(), PortError>;
```

- [ ] **Step 2: Add `ensure_cairn_dir` + impls in infra**

In `crates/cairn-infra/src/localfs.rs`, add a module-level public helper (above `impl LocalFsStore` or near the top after imports):
```rust
/// Create `<root>/.cairn/` and a `.gitignore` (`*`) so the cache never enters
/// the user's notes repo. Idempotent. Returns the `.cairn` directory path.
///
/// # Errors
/// `Adapter` if the directory or `.gitignore` cannot be created.
pub fn ensure_cairn_dir(root: &Path) -> Result<PathBuf, PortError> {
    let dir = root.join(".cairn");
    fs::create_dir_all(&dir).map_err(|e| PortError::Adapter(e.to_string()))?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "*\n").map_err(|e| PortError::Adapter(e.to_string()))?;
    }
    Ok(dir)
}
```
Add to `impl VaultStore for LocalFsStore` (after `stamp`):
```rust
    fn read_meta(&self) -> Result<Option<String>, PortError> {
        let path = self.root.join(".cairn").join("state.json");
        match fs::read_to_string(&path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PortError::Adapter(e.to_string())),
        }
    }

    fn write_meta(&self, data: &str) -> Result<(), PortError> {
        let dir = ensure_cairn_dir(&self.root)?;
        fs::write(dir.join("state.json"), data).map_err(|e| PortError::Adapter(e.to_string()))
    }
```
In `crates/cairn-infra/src/lib.rs`, export the helper alongside `LocalFsStore` (it currently has `pub use localfs::LocalFsStore;`):
```rust
pub use localfs::{ensure_cairn_dir, LocalFsStore};
```

- [ ] **Step 3: Update the `CountingStore` test mock**

In `crates/cairn-app/src/lib.rs`, the `#[cfg(test)] mod tests` has `struct CountingStore` impl `VaultStore`. Add the two methods (delegating to inner):
```rust
        fn read_meta(&self) -> Result<Option<String>, PortError> {
            self.inner.read_meta()
        }
        fn write_meta(&self, data: &str) -> Result<(), PortError> {
            self.inner.write_meta(data)
        }
```

- [ ] **Step 4: Write the infra test**

In `crates/cairn-infra/src/localfs.rs` `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn meta_roundtrips_and_creates_gitignored_cairn_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        assert!(store.read_meta().unwrap().is_none());

        store.write_meta("{\"x\":1}").unwrap();
        assert_eq!(store.read_meta().unwrap().as_deref(), Some("{\"x\":1}"));

        let ignore = tmp.path().join(".cairn").join(".gitignore");
        assert_eq!(std::fs::read_to_string(ignore).unwrap(), "*\n");
    }
```

- [ ] **Step 5: Run + gate + commit**

- `cargo test -p cairn-infra meta_roundtrips_and_creates_gitignored_cairn_dir` → PASS.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/localfs.rs crates/cairn-infra/src/lib.rs crates/cairn-app/src/lib.rs
git commit -m "feat(ports,infra): VaultStore meta blob + ensure_cairn_dir"
```

---

### Task 2: `TantivyIndex` held writer + `open_at`

Refactor `TantivyIndex` to hold a long-lived `IndexWriter` (so the daemon keeps Tantivy's exclusive lock) and add `open_at` for an on-disk `MmapDirectory` index with a corruption-rebuild fallback.

**Files:**
- Modify: `crates/cairn-infra/src/tantivy_index.rs`

- [ ] **Step 1: Imports + struct field**

In `crates/cairn-infra/src/tantivy_index.rs`:
- Add to imports: `use std::path::Path;` and `use tantivy::directory::MmapDirectory;`.
- Add a `writer: IndexWriter` field to the `TantivyIndex` struct:
```rust
pub struct TantivyIndex {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    path: Field,
    path_text: Field,
    body: Field,
}
```

- [ ] **Step 2: Factor schema/finish helpers + two constructors**

Replace the existing `in_memory` (and the private `fn writer(&self)` helper, which is removed) with shared builders + both constructors:
```rust
    fn schema() -> (Schema, Field, Field, Field) {
        let mut sb = Schema::builder();
        let path = sb.add_text_field("path", STRING | STORED);
        let text_opts = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ngram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let path_text = sb.add_text_field("path_text", text_opts.clone());
        let body = sb.add_text_field("body", text_opts | STORED);
        (sb.build(), path, path_text, body)
    }

    fn finish(index: Index, path: Field, path_text: Field, body: Field) -> Result<Self, PortError> {
        let ngram = NgramTokenizer::new(NGRAM_MIN, 3, false).map_err(adapt)?;
        let analyzer = TextAnalyzer::builder(ngram).filter(LowerCaser).build();
        index.tokenizers().register("ngram", analyzer);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(adapt)?;
        let writer = index.writer(WRITER_HEAP).map_err(adapt)?;
        Ok(Self {
            index,
            reader,
            writer,
            path,
            path_text,
            body,
        })
    }

    /// Build an in-memory index (RamDirectory). Load notes via [`SearchIndex::reindex`].
    ///
    /// # Errors
    /// Returns [`PortError`] if the schema, tokenizer, writer, or reader cannot be built.
    pub fn in_memory() -> Result<Self, PortError> {
        let (schema, path, path_text, body) = Self::schema();
        let index = Index::create_in_ram(schema);
        Self::finish(index, path, path_text, body)
    }

    /// Open (or create) a persistent index under `dir` (a `MmapDirectory`). The
    /// caller holds the exclusive writer for the value's lifetime.
    ///
    /// # Errors
    /// Returns [`PortError`] if the directory can't be created/opened even after a
    /// rebuild attempt, or the writer lock is held by another process.
    pub fn open_at(dir: &Path) -> Result<Self, PortError> {
        let (schema, path, path_text, body) = Self::schema();
        let index = open_or_rebuild(dir, &schema)?;
        Self::finish(index, path, path_text, body)
    }
```
Add the free function (after `fn adapt`):
```rust
fn open_or_rebuild(dir: &Path, schema: &Schema) -> Result<Index, PortError> {
    std::fs::create_dir_all(dir).map_err(adapt)?;
    let open = |schema: Schema| -> tantivy::Result<Index> {
        let mmap = MmapDirectory::open(dir)?;
        Index::open_or_create(mmap, schema)
    };
    match open(schema.clone()) {
        Ok(index) => Ok(index),
        Err(_) => {
            // Corrupt or schema-mismatched index: wipe the directory and recreate.
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let _ = std::fs::remove_dir_all(&p);
                    } else {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            open(schema.clone()).map_err(adapt)
        }
    }
}
```

- [ ] **Step 3: Use the held writer in the mutating methods**

Replace the bodies of `reindex`, `upsert`, `remove` to use `self.writer` instead of a per-op writer:
```rust
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.writer.delete_all_documents().map_err(adapt)?;
        for n in notes {
            let p = n.path.as_str().to_string();
            self.writer
                .add_document(doc!(
                    self.path => p.clone(),
                    self.path_text => p,
                    self.body => n.body.clone(),
                ))
                .map_err(adapt)?;
        }
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }
```
```rust
    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        let term = self.term(&note.path);
        self.writer.delete_term(term);
        let p = note.path.as_str().to_string();
        self.writer
            .add_document(doc!(
                self.path => p.clone(),
                self.path_text => p,
                self.body => note.body.clone(),
            ))
            .map_err(adapt)?;
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }
```
```rust
    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        let term = self.term(path);
        self.writer.delete_term(term);
        self.writer.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }
```
`search` is unchanged (it uses `self.reader`/`self.index`, never the writer). Update the module doc comment's "an on-disk constructor can be added later" line to reflect that `open_at` now exists.

- [ ] **Step 4: Add on-disk tests**

In the `#[cfg(test)] mod tests` of the same file, add:
```rust
    #[test]
    fn open_at_persists_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("index");
        {
            let mut idx = TantivyIndex::open_at(&dir).unwrap();
            idx.reindex(&[note("a.md", "the borrow checker enforces ownership")])
                .unwrap();
            assert!(idx
                .search("ownership")
                .unwrap()
                .iter()
                .any(|h| h.path.as_str() == "a.md"));
        } // drop releases the writer lock

        // Reopen the SAME dir: content is still present, no re-index.
        let idx2 = TantivyIndex::open_at(&dir).unwrap();
        assert!(idx2
            .search("nersh")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
    }

    #[test]
    fn open_at_rebuilds_on_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("index");
        {
            let _ = TantivyIndex::open_at(&dir).unwrap();
        }
        // Corrupt the index metadata.
        std::fs::write(dir.join("meta.json"), "not valid json").unwrap();
        // open_at must wipe + recreate rather than error.
        let idx = TantivyIndex::open_at(&dir).unwrap();
        assert!(idx.search("anything").unwrap().is_empty());
    }
```
(The `note` helper already exists in this test module.) If `meta.json` corruption does not trip `open_or_create` on your Tantivy build, adjust the corruption to truncate/garble a segment file under `dir`; the assertion (rebuild succeeds, index empty) stays the same.

- [ ] **Step 5: Run + gate + commit**

- `cargo test -p cairn-infra tantivy_index` → all pass (existing + 2 new).
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green (the held-writer refactor keeps `in_memory()` behavior identical, so CLI/daemon/app tests are unaffected).
```bash
git add crates/cairn-infra/src/tantivy_index.rs
git commit -m "feat(infra): TantivyIndex::open_at persistent index + held writer"
```

---

### Task 3: `Engine::reconcile` + state persistence

**Files:**
- Modify: `crates/cairn-app/Cargo.toml`
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Add serde deps to cairn-app**

In `crates/cairn-app/Cargo.toml`, under `[dependencies]`, add:
```toml
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 2: Imports + state DTOs**

In `crates/cairn-app/src/lib.rs`:
- Ensure these are imported at the top: `use std::collections::{HashMap, HashSet};` (add `HashSet`), and `use std::time::{Duration, UNIX_EPOCH};`.
- Add the serde DTOs near the top of the file (after the imports, before `Engine`):
```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct StateEntry {
    path: String,
    hash: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    len: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StatePayload {
    entries: Vec<StateEntry>,
}
```

- [ ] **Step 3: Extract `rebuild`, add `reconcile` + helpers**

Refactor `reindex` to call a private `rebuild` (so `reconcile_cold` can reuse it), and add the reconcile machine. Replace the existing `reindex` with:
```rust
    fn rebuild(&mut self) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        self.memo = notes
            .iter()
            .map(|n| (n.path.clone(), n.content_hash()))
            .collect();
        let mut stamps = HashMap::with_capacity(notes.len());
        for n in &notes {
            stamps.insert(n.path.clone(), self.store.stamp(&n.path)?);
        }
        self.stamps = stamps;
        Ok(())
    }

    /// Rebuild the index and the content-hash memo from the store (startup /
    /// full rescan). Emits [`Event::Reindexed`].
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        self.rebuild()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    /// Startup reconcile against a persisted index: load `state.json`, seed memo
    /// + stamps, then stat each current note and (re)index only what changed,
    /// removing notes gone from disk. Saves the refreshed state. Emits a single
    /// [`Event::Reindexed`]. Falls back to a full rebuild if no/invalid state.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reconcile(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        match self.store.read_meta()? {
            Some(json) => match parse_state(&json) {
                Ok(restored) => self.reconcile_warm(restored, sink),
                Err(()) => self.reconcile_cold(sink),
            },
            None => self.reconcile_cold(sink),
        }
    }

    fn reconcile_cold(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        self.rebuild()?;
        self.save_state()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    fn reconcile_warm(
        &mut self,
        restored: HashMap<NotePath, (u64, FileStamp)>,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.memo = restored.iter().map(|(p, (h, _))| (p.clone(), *h)).collect();
        self.stamps = restored.iter().map(|(p, (_, s))| (p.clone(), *s)).collect();

        let current = self.store.list()?;
        let current_set: HashSet<&NotePath> = current.iter().collect();
        let removed: Vec<NotePath> = restored
            .keys()
            .filter(|p| !current_set.contains(*p))
            .cloned()
            .collect();
        for p in removed {
            self.index.remove(&p)?;
            self.memo.remove(&p);
            self.stamps.remove(&p);
        }

        for path in current {
            let stamp = self.store.stamp(&path)?;
            if self.stamps.get(&path) == Some(&stamp) {
                continue; // unchanged on disk → trust the persisted index
            }
            let raw = self.store.read(&path)?;
            let note = Note::parse(path.clone(), &raw);
            let hash = note.content_hash();
            self.index.upsert(&note)?;
            self.memo.insert(path.clone(), hash);
            self.stamps.insert(path, stamp);
        }

        self.save_state()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    fn save_state(&self) -> Result<(), PortError> {
        let mut entries = Vec::with_capacity(self.stamps.len());
        for (path, stamp) in &self.stamps {
            let hash = self.memo.get(path).copied().unwrap_or(0);
            let dur = stamp
                .modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            entries.push(StateEntry {
                path: path.as_str().to_string(),
                hash,
                mtime_secs: dur.as_secs(),
                mtime_nanos: dur.subsec_nanos(),
                len: stamp.len,
            });
        }
        let json = serde_json::to_string(&StatePayload { entries })
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        self.store.write_meta(&json)
    }
```
Add the free `parse_state` function (after the `Engine` impl block, near the DTOs):
```rust
fn parse_state(json: &str) -> Result<HashMap<NotePath, (u64, FileStamp)>, ()> {
    let payload: StatePayload = serde_json::from_str(json).map_err(|_| ())?;
    let mut map = HashMap::with_capacity(payload.entries.len());
    for e in payload.entries {
        let path = NotePath::new(&e.path).map_err(|_| ())?;
        let modified = UNIX_EPOCH + Duration::new(e.mtime_secs, e.mtime_nanos);
        map.insert(path, (e.hash, FileStamp { modified, len: e.len }));
    }
    Ok(map)
}
```

- [ ] **Step 4: Write reconcile tests (over a persistent on-disk index)**

Warm reconcile is only meaningful with a persistent index, so these use `TantivyIndex::open_at` across two engine instances (a simulated restart). Add to `#[cfg(test)] mod tests` (the module already imports `cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore}` and has `CountingStore`; add `TantivyIndex` to that import and `Arc`/`AtomicUsize` are already imported from the stat-guard test):
```rust
    #[test]
    fn reconcile_cold_builds_and_writes_state() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "ownership rules").unwrap();
        let mut eng = Engine::new(
            LocalFsStore::open(tmp.path()).unwrap(),
            TantivyIndex::open_at(&tmp.path().join(".cairn/index")).unwrap(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        assert!(eng
            .search("ownership")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
        // state.json was written — assert via a fresh store reading the same dir.
        let store = LocalFsStore::open(tmp.path()).unwrap();
        assert!(store.read_meta().unwrap().is_some());
    }

    #[test]
    fn reconcile_warm_skips_unchanged_and_catches_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let idx_dir = tmp.path().join(".cairn/index");
        std::fs::write(tmp.path().join("a.md"), "alpha body").unwrap();
        std::fs::write(tmp.path().join("b.md"), "beta body").unwrap();

        // First run (cold): build + write state. Drop to release the writer lock.
        {
            let mut eng = Engine::new(
                LocalFsStore::open(tmp.path()).unwrap(),
                TantivyIndex::open_at(&idx_dir).unwrap(),
                GitVcs::open_or_init(tmp.path()).unwrap(),
            );
            eng.reconcile(&mut Vec::new()).unwrap();
        }

        // Simulate "while off": change a.md, delete b.md.
        std::fs::write(tmp.path().join("a.md"), "alpha CHANGED body").unwrap();
        std::fs::remove_file(tmp.path().join("b.md")).unwrap();

        // Second run (warm) with a read-counting store: only a.md is re-read.
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            TantivyIndex::open_at(&idx_dir).unwrap(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1, "only the changed note is re-read");
        // a.md reflects new content; b.md is gone from the index.
        assert!(eng
            .search("CHANGED")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
        assert!(eng.search("beta").unwrap().is_empty());
    }
```

- [ ] **Step 5: Run, regenerate Cargo.lock, gate, commit**

- `cargo test -p cairn-app` → all pass (incl. the 2 reconcile tests).
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
- **Regenerate the lockfile** (serde/serde_json are new edges for cairn-app) and verify `--locked`:
  `cargo check --workspace --all-targets --locked` → must succeed.
```bash
git add crates/cairn-app/Cargo.toml crates/cairn-app/src/lib.rs Cargo.lock
git commit -m "feat(app): Engine::reconcile with sidecar state persistence"
```

---

### Task 4: Daemon config + wiring

**Files:**
- Modify: `crates/cairn-daemon/src/config.rs`
- Modify: `crates/cairn-daemon/src/main.rs`

- [ ] **Step 1: Add `[index]` config**

In `crates/cairn-daemon/src/config.rs`, add the section to `Config` and the new struct:
```rust
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// CORS settings.
    #[serde(default)]
    pub cors: CorsConfig,
    /// On-disk index settings.
    #[serde(default)]
    pub index: IndexConfig,
}

/// On-disk index persistence settings.
#[derive(Debug, Deserialize)]
pub struct IndexConfig {
    /// Persist the index under `<cairn>/.cairn/index` (default true).
    #[serde(default = "default_true")]
    pub persist: bool,
    /// Override the index directory (defaults to `<cairn>/.cairn/index`).
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self { persist: true, path: None }
    }
}

fn default_true() -> bool {
    true
}
```
Add config tests in that file's `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn index_persist_defaults_true() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.index.persist);
        let c: Config = toml::from_str("[index]\n").unwrap();
        assert!(c.index.persist);
    }

    #[test]
    fn index_persist_can_be_disabled() {
        let c: Config = toml::from_str("[index]\npersist = false").unwrap();
        assert!(!c.index.persist);
    }
```

- [ ] **Step 2: Add the `--no-persist` flag**

In `crates/cairn-daemon/src/main.rs`, in the `Cli` struct (next to `no_watch`), add:
```rust
    /// Disable the on-disk index (use an ephemeral in-memory index).
    #[arg(long)]
    no_persist: bool,
```
Also ensure `PathBuf` is imported (it is, used by `config: Option<PathBuf>`).

- [ ] **Step 3: Load config before building the engine, and branch on persist**

In `run()`, the config is currently loaded *after* the engine. Move the config load up to just after the `.git` guard, then replace the `build_engine` + `reindex` block. The new flow:
```rust
    // Load config first — it decides index persistence.
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::load_default(&cli.cairn)?,
    };

    let mut startup: Vec<Event> = Vec::new();
    let persist = config.index.persist && !cli.no_persist;
    let engine = if persist {
        let index_dir = config
            .index
            .path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| cli.cairn.join(".cairn").join("index"));
        cairn_infra::ensure_cairn_dir(&cli.cairn).map_err(|e| e.to_string())?;
        let store = LocalFsStore::open(&cli.cairn).map_err(|e| e.to_string())?;
        let vcs = GitVcs::open_or_init(&cli.cairn).map_err(|e| e.to_string())?;
        let index = TantivyIndex::open_at(&index_dir).map_err(|e| e.to_string())?;
        let mut eng = Engine::new(store, index, vcs);
        eng.reconcile(&mut startup).map_err(|e| e.to_string())?;
        println!("persisting index at {}", index_dir.display());
        eng
    } else {
        let mut eng = build_engine(&cli.cairn)?;
        eng.reindex(&mut startup).map_err(|e| e.to_string())?;
        println!("index: in-memory (not persisted)");
        eng
    };

    let state = AppState::new(engine);
```
Then DELETE the now-duplicate later `let config = match &cli.config { ... }` block (config is already loaded above) and the old `let mut engine = build_engine(...)` / `engine.reindex(...)` / `let state = AppState::new(engine);` lines. The CORS code that follows keeps using `config.cors`. `build_engine` (the in-memory helper) stays for the `else` branch.
Imports: `TantivyIndex` is already imported in `main.rs`; ensure `Engine` and `LocalFsStore`/`GitVcs` are too (they are, used by `build_engine`).

- [ ] **Step 4: Run daemon tests + gate**

- `cargo test -p cairn-daemon` → pass (config tests + existing http/watch/cors/ws tests; the daemon still defaults to building an engine, now via reconcile on a temp `.cairn/`... note the existing daemon integration tests construct `AppState` directly via their own `state()` helper using `in_memory()`, so they are unaffected; only `main.rs::run` changed, which isn't exercised by those tests).
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/config.rs crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): persist index by default with reconcile; --no-persist"
```

---

### Task 5: ADR-0006 + handoff + final gate

**Files:**
- Create: `docs/decisions/0006-on-disk-persistence.md`
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Write ADR-0006**

Read `docs/decisions/0005-in-process-watcher.md` first to match the exact heading format, then create `docs/decisions/0006-on-disk-persistence.md` with that format and this content:
- **Context:** the in-memory index rebuilt fully on every daemon start; the stat-guard couldn't pay off across restarts without persistence.
- **Decision:** the daemon persists a Tantivy `MmapDirectory` index under `<cairn>/.cairn/index` (default-on, gitignored) and reconciles on startup against a sidecar `state.json` (per-note hash + `(mtime,len)`), re-indexing only changed/added/removed notes. It holds Tantivy's exclusive writer lock (sole writer). State storage is a sidecar (not extra index fields) to keep the search schema/`upsert` untouched. `--no-persist` / `[index] persist=false` opt out.
- **Consequences:** daemon startup is O(changed notes); the user's notes repo stays clean (`.cairn/.gitignore`). `state.json` is saved at reconcile time only — mid-session edits are re-stat-reconciled next start (correct, slightly redundant). CLI read-only access to the persisted index is Phase 2. Corrupt/ schema-mismatched index → wipe + rebuild.

- [ ] **Step 2: Update the handoff**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, add a short note (in the §2 capabilities list or a "Running the daemon" area):
```
- **Persistent index:** the daemon persists its search index under `<cairn>/.cairn/`
  (auto-gitignored) and reconciles on startup, so it starts fast after the first run.
  Pass `--no-persist` (or `[index] persist = false` in `cairn.toml`) for an ephemeral
  in-memory index. (CLI read access to the persisted index is a later phase.)
```

- [ ] **Step 3: Final gate**

Run:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check licenses bans sources
```
Expected: all green (no new external deps beyond serde/serde_json, already in the tree; `deny` licenses/bans/sources ok). Do NOT run plain `cargo deny check` (its advisories sub-check can crash on an old local cargo-deny — tooling issue, not code).

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0006-on-disk-persistence.md docs/handoffs/2026-06-01-ui-session-handoff.md
git commit -m "docs: ADR-0006 on-disk persistence + handoff update"
```

---

## Notes for the implementer

- **Held writer = held lock.** `open_at` acquires Tantivy's exclusive writer lock at construction and holds it until the `TantivyIndex` is dropped. Tests that reopen the same dir MUST drop the first instance first (use a `{ }` block).
- **Warm reconcile trusts the persisted index** for unchanged notes (no re-read). This is only correct with a persistent index — that's why the reconcile tests use `open_at`, not `InMemoryIndex`.
- **`reindex` stays** for the `--no-persist` path; `reconcile` is the persistent path. Both emit only `Reindexed`.
- **Cargo.lock:** Task 3 adds serde/serde_json to cairn-app — commit the regenerated `Cargo.lock` or CI's `--locked` jobs fail.
- **Corruption test:** if garbling `meta.json` doesn't trip the fallback on your Tantivy build, garble a segment file instead; the rebuild-succeeds assertion is what matters.
