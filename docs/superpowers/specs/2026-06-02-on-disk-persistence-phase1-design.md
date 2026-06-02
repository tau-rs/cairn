# On-Disk Persistence Phase 1 — Daemon Index + Startup Reconcile

**Date:** 2026-06-02
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main` (Tantivy in-memory search + the `(mtime,len)` stat-guard).

---

## 1. Goal

Make the **daemon** persist its Tantivy index on disk and **reconcile** on startup —
re-indexing only the notes that changed while it was off, instead of rebuilding from
scratch. This is where the on-disk index and the stat-guard finally pay off across
restarts: daemon startup becomes O(changed notes), not O(all notes).

Phase 1 is **daemon-only** (sole writer). CLI read-only access to the persisted index is
Phase 2.

---

## 2. Decisions (locked during brainstorming)

1. **Scope:** Phase 1 of a reader-shared design = daemon persists + reconciles; CLI
   unchanged (still ephemeral in-memory per command). Phase 2 (later) adds CLI read-only
   search.
2. **Enable:** **default-on**, persisting under `<cairn>/.cairn/` (auto-gitignored). A
   `--no-persist` flag and `[index] persist = false` in `cairn.toml` disable it (→ in-memory).
3. **State storage:** a **sidecar `state.json`** (not extra Tantivy fields) — keeps the
   search schema and `SearchIndex`/`upsert` signatures unchanged; serialization lives in the
   app layer.
4. **Persistence port surface:** `read_meta`/`write_meta` on the existing `VaultStore` (one
   filesystem port stays one port).
5. **Directory name:** `.cairn/` (mirrors `.git`/`.obsidian`).

---

## 3. On-disk layout

Under `<cairn>/.cairn/` (auto-created when persistence is on):
- `index/` — the Tantivy `MmapDirectory` index.
- `state.json` — sidecar mapping each note path → `{ content_hash, mtime, len }`.
- `.gitignore` — contains `*`, so nothing in `.cairn/` is committed to the user's notes repo.

---

## 4. Ports (`cairn-ports`) — `VaultStore` metadata blob

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
`FileStamp` is unchanged; `cairn-ports` and `cairn-domain` stay serde-free.

---

## 5. Infra (`cairn-infra`)

### 5.1 `LocalFsStore` metadata + `.cairn/` setup
- `read_meta`: read `<root>/.cairn/state.json`; missing file → `Ok(None)`; other IO → `Adapter`.
- `write_meta`: `ensure_cairn_dir(&self.root)?` then write `<root>/.cairn/state.json`.
- A module-level helper:
  ```rust
  /// Create `<root>/.cairn/` and a `.gitignore` (`*`) so the cache never enters
  /// the user's notes repo. Idempotent.
  pub fn ensure_cairn_dir(root: &Path) -> Result<PathBuf, PortError>;
  ```
  Creates `<root>/.cairn/`, writes `.gitignore` with `*\n` if absent, returns the dir path.

### 5.2 `TantivyIndex::open_at`
```rust
/// Open (or create) a persistent index under `dir` (a `MmapDirectory`). The
/// daemon holds the exclusive writer for its lifetime.
///
/// # Errors
/// `Adapter` if the directory can't be created/opened after a rebuild attempt.
pub fn open_at(dir: &Path) -> Result<Self, PortError>;
```
- Build the same schema + register the `ngram` tokenizer (shared with `in_memory`).
- `MmapDirectory::open(dir)` (create `dir` first); `Index::open_or_create(dir_mmap, schema)`.
- **Refactor:** `TantivyIndex` holds a long-lived `writer: IndexWriter` field (instead of
  creating one per op), acquired here. `in_memory()` also acquires one. `reindex`/`upsert`/
  `remove` use the held writer + `commit()` + `reader.reload()`.
- **Corruption / schema-mismatch fallback:** if `open_or_create` (or the first reader build)
  errors, delete the `dir` contents and retry once as a fresh create; if it still fails,
  return `Adapter`.

The `SearchIndex` impl (`reindex`/`search`/`upsert`/`remove`) is otherwise unchanged
(schema, query, snippet, stat-guard interplay all identical).

---

## 6. Application (`cairn-app`) — `Engine::reconcile`

A new state-aware startup path, used only by the persistent daemon path. `reindex`
(existing, full, no state) stays for the in-memory path.

```rust
/// Startup reconcile against a persisted index: load `state.json`, seed memo +
/// stamps, then stat each current note and (re)index only what changed, removing
/// notes gone from disk. Saves the refreshed state. Emits a single `Reindexed`.
///
/// # Errors
/// Propagates [`PortError`] from store/index ops.
pub fn reconcile(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError>;
```
Algorithm:
1. `let restored = self.store.read_meta()?` → parse into `HashMap<NotePath, NoteState>` where
   `NoteState { hash: u64, stamp: FileStamp }`. Missing/parse-fail → treat as cold.
2. **Cold** (`None`): run the existing full build (read all → `index.reindex` → seed memo +
   stamps), then `save_state`, emit `Reindexed(memo.len())`.
3. **Warm**: seed `self.memo` + `self.stamps` from `restored`. Then `store.list()` → current
   paths. For each current path: `store.stamp(path)?`; if it equals the restored stamp, keep
   (no read). Else read+parse, `index.upsert`, update `memo`/`stamps`. For paths in
   `restored` but not current, `index.remove` + drop memo/stamps. Then `save_state` and emit
   `Reindexed(memo.len())`.
4. `save_state`: serialize `{ memo, stamps }` into a JSON DTO and `store.write_meta(&json)`.

### Serde DTO (app layer only)
```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct StateEntry { path: String, hash: u64, mtime_secs: u64, mtime_nanos: u32, len: u64 }
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StatePayload { entries: Vec<StateEntry> }
```
`FileStamp.modified` ↔ `(mtime_secs, mtime_nanos)` via `duration_since(UNIX_EPOCH)` /
`UNIX_EPOCH + Duration::new(secs, nanos)`. A pre-epoch mtime (not expected for real files) is
clamped to the epoch on write. `cairn-app` adds `serde` + `serde_json` deps.

---

## 7. Daemon (`cairn-daemon`)

- `Config` gains an `[index]` section:
  ```rust
  #[derive(Debug, Deserialize)]
  pub struct IndexConfig {
      #[serde(default = "default_true")]
      pub persist: bool,
      #[serde(default)]
      pub path: Option<String>, // overrides <cairn>/.cairn/index
  }
  ```
  `persist` defaults to `true`; `Config` is `#[serde(default)]`-extensible as today.
- CLI flag `--no-persist` overrides `persist` to false.
- Effective behavior at startup:
  - **persist on:** `index_dir = config.index.path or <cairn>/.cairn/index`;
    `ensure_cairn_dir(&cairn)?`; `TantivyIndex::open_at(&index_dir)?`; build the engine;
    `engine.reconcile(&mut sink)`. Startup line notes `persisting index at <dir>`.
  - **persist off:** `TantivyIndex::in_memory()?` + `engine.reindex` (today's path); startup
    line notes `index: in-memory (not persisted)`.
- The watcher + dispatch paths are unchanged: while running, `apply_change` upserts into the
  (now on-disk) index and the daemon should also refresh `state.json` so a later restart is
  warm. **State refresh:** the daemon saves state after the startup reconcile and on a
  graceful-shutdown hook if one exists; otherwise it relies on reconcile to repair drift on
  the next start (saving on every change is unnecessary — reconcile is the safety net). For
  Phase 1, save state at the end of `reconcile` only; document that mid-session changes are
  re-stat-reconciled next start.

---

## 8. Concurrency

- The daemon holds Tantivy's exclusive writer lock for its lifetime (sole writer). A second
  daemon on the same cairn fails to acquire the lock → mapped to a clear `Adapter` error and
  a readable startup message.
- A CLI write while the daemon runs writes only the *file*; the daemon's watcher syncs the
  index. The CLI never opens the on-disk index in Phase 1.

---

## 9. Testing

- **infra:**
  - `LocalFsStore::read_meta` → `Ok(None)` when absent; round-trips after `write_meta`;
    `write_meta` creates `.cairn/` + a `.gitignore` containing `*`.
  - `TantivyIndex::open_at` indexes + searches; **reopening** the same dir finds previously
    indexed content without re-indexing; a corrupted `index/` (e.g. a garbage file where a
    segment is expected) triggers the rebuild fallback rather than erroring.
- **app:**
  - cold `reconcile` (no state) full-builds and writes `state.json`;
  - warm `reconcile` skips unchanged notes (read-counting `VaultStore` proves no read),
    indexes a changed note, removes a note deleted while "off", and rewrites state;
  - an external edit made between two `reconcile` calls (same engine, simulating a restart
    via a fresh engine over the same store + persisted state) is detected and re-indexed.
- **daemon:** with persist on, startup creates `<cairn>/.cairn/` (gitignored) and a second
  `build + reconcile` over the same dir reuses the index; `--no-persist` keeps it in-memory
  (no `.cairn/index` written).

---

## 10. Docs

- **ADR-0006** (`docs/decisions/0006-on-disk-persistence.md`): the daemon on-disk index +
  reconcile; sidecar `state.json` vs in-index fields; default-on + `.cairn/` + gitignore;
  sole-writer concurrency; that CLI reader access is Phase 2.
- **Handoff update:** a short "Persistent index" note — the daemon now keeps `<cairn>/.cairn/`
  (gitignored) and starts fast on warm caches; `--no-persist` opts out.

---

## 11. Out of scope (→ Phase 2 / later)

CLI read-only search against the persisted index (Phase 2); multi-writer coordination;
persisting the link graph / tags / backlinks (still computed from files on demand);
saving `state.json` on every mutation (reconcile repairs drift); segment-merge tuning;
encrypting or compacting the cache.
