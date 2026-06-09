# In-Memory Note Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve `list_notes`/`graph`/`backlinks`/`list_tags`/`notes_by_tag` from a lazy, watcher-maintained in-memory parsed-note cache instead of re-reading the whole vault on every call.

**Architecture:** `Engine` gains a `RefCell<Option<HashMap<NotePath, Note>>>` populated on first query and kept live by the single-note apply paths (no bulk invalidation — index rebuilds don't change note files). `Graph::build` takes an iterator so graph/backlinks build from `cache.values()` without cloning. Change is isolated to `cairn-app` + a one-line domain refactor.

**Tech Stack:** Rust (`std::cell::RefCell` interior mutability).

**Branch:** `feat/note-cache` (already created; the spec is committed there).

**Spec:** `docs/superpowers/specs/2026-06-02-note-cache-design.md`.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/cairn-domain/src/graph.rs` | `Graph::build` takes an iterator (+ its tests) | 1 |
| `crates/cairn-app/src/lib.rs` | cache field, `with_notes`, query rewrites, apply-path updates (+ callers of `Graph::build`) | 1, 2 |
| `docs/decisions/0007-note-cache.md`, handoff | ADR + docs | 3 |

Each task ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

**Commit convention:** use `git -c commit.gpgsign=false commit` if signing fails.

---

### Task 1: `Graph::build` takes an iterator

Refactor the domain signature first (so Task 2 can build the graph from `cache.values()`), updating its callers so the workspace stays green.

**Files:**
- Modify: `crates/cairn-domain/src/graph.rs`
- Modify: `crates/cairn-app/src/lib.rs` (the two `Graph::build` callers — temporary `.iter()` form)

- [ ] **Step 1: Change the signature + body**

In `crates/cairn-domain/src/graph.rs`, replace the `build` method with:
```rust
    /// Build a graph from all notes. Targets are resolved to a note whose
    /// stem equals the target text; unresolved targets are dropped.
    #[must_use]
    pub fn build<'a>(notes: impl IntoIterator<Item = &'a Note>) -> Self {
        let notes: Vec<&Note> = notes.into_iter().collect();
        // Last note wins when two notes share a stem; callers should keep
        // note stems unique within a cairn.
        let by_stem: BTreeMap<&str, &NotePath> = notes
            .iter()
            .copied()
            .map(|n| (n.path.stem(), &n.path))
            .collect();

        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();

        for note in notes.iter().copied() {
            let mut targets: Vec<NotePath> = extract_links(&note.body)
                .into_iter()
                .filter_map(|t| by_stem.get(t.0.as_str()).copied().cloned())
                .collect();
            targets.sort();
            targets.dedup();
            for t in &targets {
                backward
                    .entry(t.clone())
                    .or_default()
                    .push(note.path.clone());
            }
            forward.insert(note.path.clone(), targets);
        }
        for v in backward.values_mut() {
            v.sort();
            v.dedup();
        }
        Self { forward, backward }
    }
```

- [ ] **Step 2: Update the graph unit tests**

In the same file's `#[cfg(test)] mod tests`, the three tests call `Graph::build(&notes)`. Change each to `Graph::build(notes.iter())`:
- `resolves_forward_and_backlinks_by_stem`: `let g = Graph::build(notes.iter());`
- `drops_unresolved_targets`: `let g = Graph::build(notes.iter());`
- `nodes_and_edges_expose_the_graph`: `let g = Graph::build(notes.iter());`

- [ ] **Step 3: Update the engine callers (temporary `.iter()` form)**

In `crates/cairn-app/src/lib.rs`, update the two callers so they compile against the new signature (Task 2 will switch these to the cache):
```rust
    pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
        let notes = self.load_all_notes()?;
        let graph = Graph::build(notes.iter());
        Ok(graph.backlinks(path).to_vec())
    }
    pub fn graph(&self) -> Result<Graph, PortError> {
        Ok(Graph::build(self.load_all_notes()?.iter()))
    }
```

- [ ] **Step 4: Run + gate + commit**

- `cargo test -p cairn-domain graph` → 3 graph tests pass.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-domain/src/graph.rs crates/cairn-app/src/lib.rs
git commit -m "refactor(domain): Graph::build takes an iterator of &Note"
```

---

### Task 2: The note cache in `Engine`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Write the failing cache tests**

In `crates/cairn-app/src/lib.rs` `#[cfg(test)] mod tests`, add (the module already has `CountingStore`, `Arc`, `AtomicUsize`, `Ordering`, and imports `cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore}`):
```rust
    #[test]
    fn note_cache_serves_queries_and_stays_live() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "see [[b]]").unwrap();
        std::fs::write(tmp.path().join("b.md"), "hi").unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );

        // First query populates the cache (reads both notes).
        assert_eq!(eng.list_notes().unwrap().len(), 2);
        let after_first = reads.load(Ordering::SeqCst);
        assert!(after_first >= 2);

        // Second query (a different one): cache hit, no new reads.
        assert_eq!(eng.graph().unwrap().edges().len(), 1);
        assert_eq!(reads.load(Ordering::SeqCst), after_first, "cache hit: no re-read");

        // write_note keeps the cache live without a full re-read.
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("c.md").unwrap(), "from c to [[b]]", &mut ev)
            .unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 3);
        assert_eq!(reads.load(Ordering::SeqCst), after_first, "write kept cache live");

        // delete_note removes from the cache.
        eng.delete_note(&NotePath::new("a.md").unwrap(), &mut ev).unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 2);
        assert_eq!(reads.load(Ordering::SeqCst), after_first, "delete kept cache live");
    }

    #[test]
    fn reindex_does_not_invalidate_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "x").unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.list_notes().unwrap(); // populate cache
        let base = reads.load(Ordering::SeqCst);
        eng.reindex(&mut Vec::new()).unwrap(); // reads to rebuild the index
        let after_reindex = reads.load(Ordering::SeqCst);
        assert!(after_reindex > base, "reindex reads for the index");
        eng.list_notes().unwrap(); // cache still valid
        assert_eq!(
            reads.load(Ordering::SeqCst),
            after_reindex,
            "reindex did not invalidate the cache"
        );
    }
```
Run `cargo test -p cairn-app note_cache_serves_queries_and_stays_live` → FAILS (the second query re-reads, count increases) — confirming the cache isn't there yet.

- [ ] **Step 2: Add the import + struct field + init**

In `crates/cairn-app/src/lib.rs`:
- Add `use std::cell::RefCell;` near the other `std` imports.
- Add the field to the `Engine` struct (after `stamps`):
```rust
    notes_cache: RefCell<Option<HashMap<NotePath, Note>>>,
```
- In `Engine::new`, after `stamps: HashMap::new(),` add:
```rust
            notes_cache: RefCell::new(None),
```

- [ ] **Step 3: Add the `with_notes` helper**

Add this private method inside the `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` block (e.g. right after `load_all_notes`):
```rust
    /// Ensure the parsed-note cache is populated (reading the vault once if
    /// empty), then run `f` over it.
    fn with_notes<R>(
        &self,
        f: impl FnOnce(&HashMap<NotePath, Note>) -> R,
    ) -> Result<R, PortError> {
        if self.notes_cache.borrow().is_none() {
            let map: HashMap<NotePath, Note> = self
                .load_all_notes()?
                .into_iter()
                .map(|n| (n.path.clone(), n))
                .collect();
            *self.notes_cache.borrow_mut() = Some(map);
        }
        let guard = self.notes_cache.borrow();
        Ok(f(guard.as_ref().expect("cache populated above")))
    }
```

- [ ] **Step 4: Rewrite the five query methods to use the cache**

Replace `backlinks`, `list_notes`, `graph`, `list_tags`, `notes_by_tag` with:
```rust
    /// Backlinks for a note, computed from the cached notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
        self.with_notes(|m| Graph::build(m.values()).backlinks(path).to_vec())
    }

    /// All parsed notes in the cairn (from the cache).
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn list_notes(&self) -> Result<Vec<Note>, PortError> {
        self.with_notes(|m| m.values().cloned().collect())
    }

    /// The link graph derived from the cached notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn graph(&self) -> Result<Graph, PortError> {
        self.with_notes(|m| Graph::build(m.values()))
    }

    /// All tags across the cairn with note counts, sorted by tag.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn list_tags(&self) -> Result<Vec<(String, usize)>, PortError> {
        self.with_notes(|m| {
            let mut counts: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for note in m.values() {
                for tag in note.tags() {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            }
            counts.into_iter().collect()
        })
    }

    /// Notes carrying `tag`, sorted by path.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn notes_by_tag(&self, tag: &str) -> Result<Vec<NotePath>, PortError> {
        self.with_notes(|m| {
            let mut out: Vec<NotePath> = m
                .values()
                .filter(|n| n.tags().iter().any(|t| t == tag))
                .map(|n| n.path.clone())
                .collect();
            out.sort();
            out
        })
    }
```

- [ ] **Step 5: Keep the cache live in the apply paths**

Add the guarded cache update to each apply path (each is `&mut self`, so use `get_mut()`):

In `apply_change`, in the `FsChange::Changed(path)` arm, immediately AFTER `self.stamps.insert(path.clone(), stamp);` and BEFORE the `if self.memo.get(path) == Some(&hash)` line, add:
```rust
                if let Some(map) = self.notes_cache.get_mut() {
                    map.insert(path.clone(), note.clone());
                }
```

In `apply_write`, immediately AFTER `self.stamps.insert(path.clone(), self.store.stamp(path)?);` and BEFORE `if self.memo.get(path) == Some(&hash)`, add:
```rust
        if let Some(map) = self.notes_cache.get_mut() {
            map.insert(path.clone(), note.clone());
        }
```

In `apply_removal`, immediately AFTER `self.stamps.remove(path);`, add:
```rust
        if let Some(map) = self.notes_cache.get_mut() {
            map.remove(path);
        }
```

- [ ] **Step 6: Run + gate + commit**

- `cargo test -p cairn-app` → all pass (the 2 new cache tests + the existing query/correctness tests `list_tags_and_notes_by_tag`, `list_notes_and_graph_expose_engine_state`, etc.).
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green. (Watch for a clippy lint on the `RefCell` field or `borrow()`; the code is idiomatic, but if clippy flags anything, address minimally.)
```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): lazy in-memory note cache for metadata queries"
```

---

### Task 3: ADR-0007 + handoff + final gate

**Files:**
- Create: `docs/decisions/0007-note-cache.md`
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Write ADR-0007**

Read `docs/decisions/0006-on-disk-persistence.md` first to match the heading/section format, then create `docs/decisions/0007-note-cache.md` with that format and this content:
- **Context:** `list`/`graph`/`tags`/`backlinks` called `load_all_notes`, re-reading + re-parsing the whole vault on every call — in the daemon, on every UI refresh.
- **Decision:** add a lazy `RefCell<Option<HashMap<NotePath, Note>>>` cache on `Engine`, populated on first query and kept live by the single-note apply paths (`apply_write`/`apply_change`/`apply_removal`). No bulk invalidation — `reindex`/`reconcile` rebuild the index but don't change note files, so a populated cache stays valid. `RefCell` keeps the change inside `cairn-app` (queries stay `&self`, `dispatch_query` untouched). `Graph::build` now takes an iterator so graph/backlinks build from `cache.values()` without cloning.
- **Consequences:** metadata queries are O(in-memory) after the first; the cache holds all parsed notes in RAM (~vault text size, like Obsidian); the watcher keeps it current across external edits. Out of scope: caching the built `Graph` (backlinks still constructs it per call), persisting the cache, eviction.

- [ ] **Step 2: Update the handoff**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, add a capability note (place it near the existing list/graph/tags description or the §2 capabilities list, matching the surrounding style):
```
- **In-memory note cache:** `list_notes`, `graph`, `get_backlinks`, `list_tags`, and
  `notes_by_tag` are served from an in-memory cache of parsed notes (populated on first
  use, kept live by the watcher) instead of re-reading the vault per call — so polling
  these from the UI is cheap after the first call.
```

- [ ] **Step 3: Final gate**

Run:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check licenses bans sources
```
Expected: all green (no new deps; `deny` licenses/bans/sources ok). Do NOT run plain `cargo deny check` (its advisories sub-check can crash on an old local cargo-deny — tooling issue, not code).

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0007-note-cache.md docs/handoffs/2026-06-01-ui-session-handoff.md
git commit -m "docs: ADR-0007 in-memory note cache + handoff update"
```

---

## Notes for the implementer

- **No bulk invalidation by design.** `reindex`/`reconcile` read the vault to rebuild the *index* but never change note files, so a populated cache stays valid across them. The cache is kept consistent purely by the apply paths, which are the only routes that change note files (`write_note`/`delete_note`/`rename_note`/the watcher).
- **Cache update sits next to the stamp update** in `apply_change`/`apply_write` (before the memo-dedup early return) so the cache refreshes even when the index is deduped.
- **`get_mut()` in apply paths, `borrow()`/`borrow_mut()` in `with_notes`:** apply paths are `&mut self` (use `RefCell::get_mut`, no runtime borrow check); `with_notes` is `&self` (uses `borrow`). The `if … borrow().is_none()` temporary drops before `borrow_mut()`, so no double-borrow panic.
- **`Engine: Send` is preserved** — `RefCell<Option<HashMap<…>>>` is `Send` because its contents are `Send`; the daemon only touches the engine under its `Mutex`. The workspace compiling (daemon builds) confirms it.
- **No service/daemon/cli changes** — the cache is internal to `Engine`; `dispatch_query` stays `&Engine`.
