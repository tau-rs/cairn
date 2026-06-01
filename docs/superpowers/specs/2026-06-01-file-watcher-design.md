# File Watcher + Content-Hash Memo — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine + transport + list/graph queries on `main`.
**Fills the seam:** the `Watcher` port (currently `NoopWatcher`).

---

## 1. Goal

The daemon detects external `.md` changes — an editor, `git pull`, another
app — and pushes precise `note_changed` / `note_deleted` / `reindexed` events to
WebSocket subscribers, so a UI stays live. A **content-hash memo** in the engine
makes cairn's own writes (and no-op rewrites) produce no redundant reindex or
duplicate events: the memo is the exact, race-free dedup that the rejected
"recently-written time window" approach tried to be.

**Decision recap (from brainstorming):** the watcher and the command path both
funnel changes through a single engine primitive (`apply_change`) whose
content-hash memo decides whether anything actually changed. Events therefore
have one source (the memo diff), work on every transport (the command path still
emits immediately), and the watcher acts as a safety net for any external write.

---

## 2. Where the memo lives — the engine

A watcher-local memo cannot dedupe self-writes (it doesn't know the command path
already indexed the new content). The authority on "what content is indexed" is
the engine, so the memo lives there:

`Engine` gains `memo: HashMap<NotePath, u64>` (content hash per indexed note),
consulted by both the command path and the watcher drain loop.

---

## 3. `cairn-domain`

- `pub fn content_hash(raw: &str) -> u64` — a pure helper hashing a note's raw
  file contents with `std::hash::DefaultHasher` (no new dependency). Used to key
  the memo. (Non-cryptographic; change-detection only.)

---

## 4. `cairn-ports`

- `enum FsChange { Changed(NotePath), Removed(NotePath) }` (Debug, Clone, PartialEq, Eq).
- Redesign `Watcher`:
  ```rust
  pub trait Watcher {
      /// Begin watching `root`; returns a handle delivering debounced changes.
      ///
      /// # Errors
      /// Returns [`PortError`] if the OS watcher cannot be created.
      fn watch(&self, root: &std::path::Path) -> Result<WatchHandle, PortError>;
  }

  /// Owns the OS watcher and the stream of changes; dropping it stops watching.
  pub struct WatchHandle {
      /// Debounced note changes.
      pub changes: std::sync::mpsc::Receiver<FsChange>,
      _keepalive: Box<dyn Send>,
  }
  impl WatchHandle {
      #[must_use]
      pub fn new(changes: std::sync::mpsc::Receiver<FsChange>, keepalive: Box<dyn Send>) -> Self { ... }
  }
  ```
- `SearchIndex` gains incremental primitives (keep `reindex` for full rebuild):
  ```rust
  fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
  fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
  ```

---

## 5. `cairn-app` — `apply_change` + unified event source

`Engine` gains the memo and one new primitive; `write_note`/`delete_note` route
through it so all change-events originate from the memo diff.

```rust
/// Apply a single filesystem change, deduping via the content-hash memo
/// and emitting precise events only when something actually changed.
pub fn apply_change(&mut self, change: &FsChange, sink: &mut dyn EventSink)
    -> Result<(), PortError>;
```
Behavior:
- `Changed(p)`: `raw = store.read(p)`. If the read is `NotFound`, treat as
  `Removed(p)` (race: the file vanished after the event). Else `h = content_hash(raw)`:
  - if `memo.get(p) == Some(&h)` → **no-op** (self-write echo / no-op rewrite dies here).
  - else: `index.upsert(Note::parse(p, raw))`; `memo.insert(p, h)`;
    emit `NoteChanged(p)` then `Reindexed(memo.len())`.
- `Removed(p)`: if `memo.remove(p).is_some()` → `index.remove(p)`;
  emit `NoteDeleted(p)` then `Reindexed(memo.len())`. Else no-op.

Refactors (behavior-preserving):
- `write_note(p, contents)`: `store.write(p, contents)?; self.apply_change(&FsChange::Changed(p.clone()), sink)`.
- `delete_note(p)`: `store.delete(p)?; self.apply_change(&FsChange::Removed(p.clone()), sink)`.
  (Both stop emitting events directly — `apply_change` is the sole emitter.)
- `reindex(sink)` (startup full rebuild): load all notes; `index.reindex(&notes)`;
  rebuild `memo` = `{path: content_hash(raw)}` for all; emit `Reindexed(notes.len())`.
  Re-reads raw content per note to hash (acceptable at startup).

**Event sequence is preserved:** `write_note(a)` → `NoteChanged(a)`,
`Reindexed(1)`; `write_note(b)` → `NoteChanged(b)`, `Reindexed(2)`;
`delete_note(b)` → `NoteDeleted(b)`, `Reindexed(1)`. Existing cairn-app /
cairn-service / cairn-cli / cairn-daemon tests continue to pass (a no-op rewrite
now correctly emits nothing — a behavior improvement, not a break).

---

## 6. `cairn-infra` — `NotifyWatcher`

- Uses `notify` + `notify-debouncer-full` (debounce ~200 ms). On each debounced
  batch, for each affected path: keep only paths under `root` with a `.md`
  extension, skipping anything under a `.git/` segment.
- **Existence-based classification** (robust across platforms / notify versions,
  avoids brittle event-kind matching): for each kept `.md` path, send
  `FsChange::Changed(rel)` if the path currently exists on disk, else
  `FsChange::Removed(rel)`. (A rename surfaces as a `Removed` of the old path and
  a `Changed` of the new one.)
- Returns `WatchHandle { changes, _keepalive: Box::new(debouncer) }` — the
  debouncer (which owns the OS watch thread) is kept alive by the handle.
- `NoopWatcher.watch` keeps the seam: returns a handle whose sender is parked in
  `_keepalive`, so its receiver never yields and never disconnects.

---

## 7. `cairn-daemon`

- New `AppState::apply_change_blocking(&self, change: &FsChange)`: lock the
  engine, build a `BroadcastSink`, call `engine.apply_change` → events reach WS
  subscribers (same path as commands).
- On startup (default on; `--no-watch` to disable): build a `NotifyWatcher`,
  `watch(cairn_root)`, and `tokio::task::spawn_blocking` a drain loop:
  `while let Ok(change) = handle.changes.recv() { state.apply_change_blocking(&change); }`.
  The loop owns `handle` (keeps watching alive). If `watch()` fails, log a
  warning and continue — the daemon stays up, serving without live external
  events (resilient).
- Startup order: build engine → `engine.reindex` (seeds the memo) → `AppState` →
  start watcher → `axum::serve`.

---

## 8. Testing

- **domain:** `content_hash` is stable + differs on changed content.
- **app:** `apply_change` — `Changed` on new content emits `NoteChanged`+`Reindexed`;
  a second `Changed` with identical content (the self-write echo) emits **nothing**;
  `Removed` of a known note emits `NoteDeleted`+`Reindexed`; `Removed` of an
  unknown note is a no-op; `Changed` whose file is gone behaves as `Removed`.
  Plus: existing event-sequence tests still pass.
- **infra:** `NotifyWatcher` over a tempdir — create/modify/delete a `.md` →
  the matching `FsChange` arrives on the receiver within a timeout; `.git/` and
  non-`.md` files produce nothing; dropping the handle stops watching.
- **daemon:** integration — connect a WS client, write a `.md` file *directly to
  disk* (bypassing the command path), and assert a `note_changed` frame arrives;
  then write the *same* content again and assert no further frame (memo dedup).

---

## 9. Crates & deps

New workspace deps: `notify` and `notify-debouncer-full` (pure Rust). The
implementer pins versions that build on Rust 1.85 and pass `cargo-deny`
(advisories/licenses) + the locked-MSRV check; if a version raises MSRV, pin an
older compatible one (same approach as the git2/idna pins). No new daemon system
deps.

---

## 10. Out of scope

Incremental *reading* (the memo still re-reads on a real change; a full
mtime-skip read optimization is later), Tantivy, the in-process/Tauri watcher
(the daemon is the watcher host for now; the in-process path can adopt the same
`Watcher` port later), CRDT, tau. Graph/backlinks remain computed on-demand
(no memo there yet).
