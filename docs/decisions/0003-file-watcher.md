# ADR-0003: File watcher + content-hash memo

**Status:** Accepted
**Date:** 2026-06-01

## Context

ADR-0002 delivered a working daemon (HTTP + WebSocket) and a transport-blind
dispatcher. The final review noted one open seam: the `Watcher` port was wired
to a `NoopWatcher`, so external changes to `.md` files — an editor save, a
`git pull`, another app writing to the cairn directory — produced no push
events. WebSocket subscribers saw only cairn's own command-path writes.

The design for this sub-project is specified in
`docs/superpowers/specs/2026-06-01-file-watcher-design.md`.

A naïve fix (watch the filesystem and re-emit unconditionally) introduces a
self-write echo: cairn's own `write_note` call triggers an OS event that the
watcher would re-process, emitting a spurious second `note_changed`. A
"recently-written time window" guard is inherently racy. The correct dedup
authority is the engine itself: it knows exactly what content is already indexed.

## Decision

### `cairn-ports` — redesigned `Watcher` port and `FsChange`

`Watcher` was redesigned to return a `WatchHandle` rather than running a
callback:

```rust
pub trait Watcher {
    fn watch(&self, root: &Path) -> Result<WatchHandle, PortError>;
}

pub struct WatchHandle {
    pub changes: mpsc::Receiver<FsChange>,
    _keepalive: Box<dyn Send>,
}
```

`WatchHandle` owns the OS watcher via `_keepalive`; dropping the handle stops
watching. The receiver delivers debounced `FsChange` values:

```rust
pub enum FsChange { Changed(NotePath), Removed(NotePath) }
```

`SearchIndex` gained two incremental primitives (full `reindex` is retained for
startup):

```rust
fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
```

### `cairn-app` — content-hash memo and `apply_change`

`Engine` gained `memo: HashMap<NotePath, u64>` storing the content hash
(`Note::content_hash`, backed by `DefaultHasher`) for every indexed note. This
memo is the single source of truth for "what is currently indexed".

A new primitive, `apply_change`, is the sole emitter of change-events:

- `FsChange::Changed(p)`: reads the file. A `NotFound` read is treated as
  `Removed`. Otherwise the note is hashed; if `memo[p] == hash`, this is a
  no-op (self-write echo / identical rewrite). If different: `index.upsert`,
  `memo.insert`, emit `NoteChanged` then `Reindexed`.
- `FsChange::Removed(p)`: if the path is in the memo, `index.remove`,
  `memo.remove`, emit `NoteDeleted` then `Reindexed`. Otherwise no-op.

`write_note` and `delete_note` now route through `apply_change` instead of
emitting events directly. `reindex` (startup full rebuild) rebuilds the index
and reconstructs the memo from all notes. The command path and the watcher
drain loop share one race-free dedup: cairn's own writes produce no redundant
events, and no-op rewrites are silently discarded.

### `cairn-infra` — `NotifyWatcher`

`NotifyWatcher` (in `crates/cairn-infra/src/notify_watcher.rs`) implements
`Watcher` using `notify` + `notify-debouncer-full` (~200 ms debounce window).
On each debounced batch it:

1. Keeps only paths under `root` with a `.md` extension, skipping any segment
   equal to `.git`.
2. Canonicalizes `root` at watch time so `strip_prefix` works correctly against
   OS-reported paths (resolves macOS `/tmp → /private/tmp` symlinks via FSEvents).
3. Classifies each remaining path by current on-disk existence: present →
   `FsChange::Changed`, absent → `FsChange::Removed`. This existence-based
   approach is robust across platforms and notify versions; renames surface as
   a `Removed` of the old path and a `Changed` of the new one.

`NoopWatcher::watch` returns a `WatchHandle` whose sender is stored in
`_keepalive`; the receiver never yields and never disconnects, preserving the
seam.

`notify` and `notify-debouncer-full` are pure-Rust crates; no new system
dependencies were added.

### `cairn-daemon` — watcher spawn loop

`AppState` gained `apply_change_blocking`, which locks the engine, builds a
`BroadcastSink`, and calls `engine.apply_change`. This is the same path as
command dispatch, so watcher-produced events reach WebSocket subscribers
identically to command-path events.

On startup (default on; `--no-watch` to disable) the daemon:

1. Runs `engine.reindex` to seed the memo before spawning the watcher.
2. Calls `NotifyWatcher.watch(cairn_root)`.
3. Spawns a `tokio::task::spawn_blocking` drain loop:
   `while let Ok(change) = handle.changes.recv() { state.apply_change_blocking(&change); }`
   The loop owns `handle`, keeping the OS watcher alive.

If `watch()` fails, a warning is logged and the daemon continues serving
without live external events (resilient). The `--no-watch` flag disables the
watcher entirely (useful for testing or headless environments).

## Consequences

### What this enables

- External `.md` edits (an editor, `git pull`, any other writer) now produce
  precise `note_changed` / `note_deleted` push events to WebSocket subscribers,
  without any polling.
- Cairn's own command-path writes and no-op rewrites emit nothing extra: the
  content-hash memo is the exact, race-free dedup that the previously-rejected
  time-window approach aimed to be.
- `SearchIndex` can now be updated incrementally (per-note upsert/remove)
  rather than requiring a full rebuild on every change.

### Accepted limitations and known seams

- **Incremental reads deferred:** the memo eliminates redundant *processing*,
  but on a real change the file is still re-read in full. A mtime-skip read
  optimization is a future refinement.
- **In-process / Tauri watcher deferred:** the daemon is the watcher host
  today. The same `Watcher` port seam can be wired in-process (e.g. inside a
  Tauri backend) in a later sub-project.
- **Rename stitching deferred:** a rename surfaces as `Removed(old)` +
  `Changed(new)` — two separate events. Stitching them into a single
  `NoteRenamed` event is deferred.
- **Graph memo deferred:** backlinks and the graph are still computed on-demand
  from the full note set; no per-note memo there yet.
- **Events remain best-effort:** broadcast lag-drop for slow WS clients
  (inherited from ADR-0002); clients should resync on reconnect.
