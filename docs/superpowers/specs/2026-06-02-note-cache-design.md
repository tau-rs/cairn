# In-Memory Note Cache — Design Spec

**Date:** 2026-06-02
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main` (live watcher, stat-guard, on-disk persistence).

---

## 1. Goal

Stop re-reading and re-parsing the entire vault on every metadata query. Today
`list_notes`, `graph`, `backlinks`, `list_tags`, and `notes_by_tag` each call
`load_all_notes`, which reads + parses every note from disk per call — in the daemon, on
every UI refresh. Add a **lazy, watcher-maintained in-memory cache of parsed notes** so
those queries serve from memory after a single load.

`search` (Tantivy index) and `read_note` (single file) already avoid `load_all_notes` and
are unchanged.

---

## 2. Decisions (locked during brainstorming)

1. **Lazy, not eager.** The cache is populated on first use and kept live by the single-note
   apply paths — it does **not** touch the Phase-1 `reconcile`/stat-guard (eager population
   would force `reconcile_warm` to read every note, undoing the read-skip).
2. **`RefCell` interior mutability.** Keeps the `&self` query methods and `dispatch_query`'s
   signature unchanged; the change stays inside `cairn-app` (+ a tiny domain refactor).
3. **`Graph::build` takes an iterator** so `graph`/`backlinks` build from `cache.values()`
   without cloning every note.

---

## 3. The cache (`cairn-app` — `Engine`)

Add a field:
```rust
notes_cache: std::cell::RefCell<Option<HashMap<NotePath, Note>>>,
```
Initialized `RefCell::new(None)` in `Engine::new`. (`Note: Clone`, already.)

A private helper populates it on demand (reusing the existing `load_all_notes` for the
full read) and runs the body with the populated map:
```rust
/// Ensure the parsed-note cache is populated (reading the vault once if empty),
/// then run `f` over it.
fn with_notes<R>(&self, f: impl FnOnce(&HashMap<NotePath, Note>) -> R) -> Result<R, PortError> {
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
(The `if … borrow().is_none()` temporary is dropped before the body's `borrow_mut()`, so
there is no double-borrow panic.) `Engine: Send` still holds (`RefCell<Option<HashMap<…>>>`
is `Send` because its contents are `Send`); the daemon only touches the engine under its
`Mutex`, so there is no concurrent `RefCell` access.

`load_all_notes` **stays** — it is still the full-read used by `rebuild`/`reconcile_cold`
(and now `with_notes`). Only the five query methods stop calling it directly.

---

## 4. Cache coherence

**No bulk invalidation is needed.** `reindex`/`reconcile`/`rebuild` read the vault to rebuild
the *index*; they do not change the note files on disk, so a populated cache stays valid
across them (and at startup the cache is `None` until the first query anyway). Every actual
disk-note mutation already routes through a single-note apply path, so the cache is kept
consistent there:

- `apply_write(path, contents)` (used by `write_note` and rename's link-rewrite):
  `map.insert(path.clone(), Note::parse(path.clone(), contents));`
- `apply_change(FsChange::Changed)` (watcher / external edits, and rename's moved note):
  after the note is read + parsed, **before** the memo-dedup early return — alongside the
  existing `self.stamps.insert(...)` — `map.insert(path.clone(), note.clone());` (so the
  cache refreshes even when the index is deduped).
- `apply_removal(path)` (used by `delete_note`, rename's old path, and external deletes):
  `map.remove(path);`
- A stat-guard **skip** (unchanged file) leaves the existing cache entry — already correct.

Each update is guarded by `if let Some(map) = self.notes_cache.get_mut() { ... }`, so it is a
no-op when the cache hasn't been loaded yet (the next query reads fresh from disk). Because
`write_note`/`delete_note`/`rename_note` and the watcher are the *only* ways note files
change, the cache cannot drift from disk while loaded.

---

## 5. Query methods (`cairn-app`)

Rewrite the five to use `with_notes`:
```rust
pub fn list_notes(&self) -> Result<Vec<Note>, PortError> {
    self.with_notes(|m| m.values().cloned().collect())
}

pub fn graph(&self) -> Result<Graph, PortError> {
    self.with_notes(|m| Graph::build(m.values()))
}

pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
    self.with_notes(|m| Graph::build(m.values()).backlinks(path).to_vec())
}

pub fn list_tags(&self) -> Result<Vec<(String, usize)>, PortError> {
    self.with_notes(|m| {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for note in m.values() {
            for tag in note.tags() {
                *counts.entry(tag).or_insert(0) += 1;
            }
        }
        counts.into_iter().collect()
    })
}

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
Note: `list_notes` returns owned `Vec<Note>` (a clone out of the cache — inherent to its
API; still far cheaper than disk read + parse). `graph`/`backlinks` build from `&Note`
references (no note clones). Result ordering is unchanged: `Graph` sorts internally;
`notes_by_tag` sorts; `list_tags` uses a `BTreeMap`; `list_notes` ordering was already
unspecified (HashMap iteration) — the dispatcher/UI does not rely on `list_notes` order
(the contract notes "ordering deterministic" only for the graph).

---

## 6. Domain refactor (`cairn-domain` — `Graph::build`)

Change the signature from `pub fn build(notes: &[Note]) -> Self` to:
```rust
pub fn build<'a>(notes: impl IntoIterator<Item = &'a Note>) -> Self {
    let notes: Vec<&Note> = notes.into_iter().collect();
    // ... existing body, iterating `notes` (now `&&Note`; deref where needed) ...
}
```
Internally collect once to `Vec<&Note>` (the body iterates twice: the `by_stem` map and the
link-building loop). Update callers: `Engine::graph`/`backlinks` pass `m.values()`; the
graph unit tests pass `slice.iter()`. The reindex path does not call `Graph::build`.

---

## 7. Memory

The cache holds every parsed note (path + frontmatter + body) in RAM — roughly the vault's
text size, the same model as Obsidian. Acceptable for a note app; documented in the ADR.
No eviction/size cap (a vault fits in memory). Tantivy holds the index separately.

---

## 8. Error handling

`with_notes` propagates `PortError` from `store.list`/`store.read` during population. A
populated cache makes the query methods infallible at read time (they still return
`Result` for the population step). No new error variants.

---

## 9. Testing

- **app (cache behavior, via a read-counting `VaultStore`):**
  - first `list_notes` reads the vault (N reads); an immediate second `list_notes` (or
    `graph`/`tags`) performs **zero** additional reads (cache hit);
  - after `write_note(p, ...)` (which uses `apply_write` — no read-back), a `list_notes`
    reflects the new/updated note with **no** additional reads;
  - after `delete_note(p)`, the note is gone from `list_notes`/`graph` with no re-read;
  - after an external `apply_change(Changed)`/`apply_change(Removed)`, a subsequent
    `list_notes` reflects it (the apply read the one changed file; the query adds no reads);
  - calling `reindex` after the cache is populated does **not** force the next query to
    re-read (the cache is unaffected by index rebuilds) and results stay correct.
- **app (correctness unchanged):** existing `list_tags_and_notes_by_tag` and
  `list_notes_and_graph_expose_engine_state` tests still pass.
- **domain:** `Graph::build` produces the same graph from a `&[Note]` slice's `.iter()` and
  from a `HashMap::values()` iterator (the existing graph tests, adapted to the new
  signature, still pass).

---

## 10. Docs

- **ADR-0007** (`docs/decisions/0007-note-cache.md`): the lazy watcher-maintained note
  cache; why lazy + `RefCell` (preserve the stat-guard, isolate to `cairn-app`); the
  memory trade-off; `Graph::build` now iterator-based.
- **Handoff update:** a line noting `list`/`graph`/`tags`/`backlinks` are now served from an
  in-memory cache (kept live by the watcher) rather than re-reading the vault per call.

---

## 11. Out of scope

Caching the **built `Graph`** (`backlinks` still constructs the graph from cached notes per
call — this eliminates the disk+parse cost, not the in-memory graph construction);
persisting the note cache; cache eviction / size limits; serving `read_note`/`search` from
the cache (they already avoid `load_all_notes`).
