# In-Process Watcher + Stat-Guard — Design Spec

**Date:** 2026-06-02
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main`.

---

## 1. Goal

Bring live file-watching to the **in-process** path (CLI, and future embedders that
host the engine without HTTP), not just the daemon. Deliver:

1. A small **reusable watch-drive primitive** (`run_watch_loop`) that the daemon refactors
   onto and a new `cairn watch` CLI command consumes.
2. A long-running **`cairn watch`** command that streams live changes (human lines, or
   `--json`).
3. A narrow **incremental-read** optimization: a per-note `(mtime, len)` stamp that lets
   `apply_change` skip the read for spurious/duplicate watcher events (a **stat-guard**).

The real OS watcher (`NotifyWatcher`) already exists; today only the daemon hosts it. This
closes the in-process gap and shares one tested loop between both transports.

---

## 2. Decisions (locked during brainstorming)

1. **Scope:** reusable in-process watch API + daemon refactor onto it + `cairn watch`.
2. **Incremental reads:** included, **narrow** — a `(mtime,len)` stat-guard in
   `apply_change` (not the broad in-memory Note cache).
3. **`cairn watch` output:** human-readable lines by default, plus a `--json` flag emitting
   the wire `Event` shape (one JSON object per line).
4. **Honest caveat (informs §6):** with the in-memory Tantivy index (rebuilt each startup)
   plus the existing debouncer and content-hash dedup, the stamp's payoff is modest — a
   cold start reads everything regardless. Its working benefit here is skipping the read on
   spurious watcher events; the larger win needs on-disk persistence (out of scope).

---

## 3. Reusable watch-drive primitive (`cairn-service`)

```rust
use cairn_ports::{FsChange, WatchHandle};

/// Drain a watch handle until its sender drops, invoking `on_change` for each
/// debounced change. Blocking — run on a dedicated thread (CLI `watch`) or via
/// `tokio::task::spawn_blocking` (daemon).
///
/// The engine-apply + event-forwarding deliberately lives in the caller's
/// `on_change`: the daemon locks a shared `Arc<Mutex<Engine>>` per change while
/// the CLI/embedder owns the engine outright, and output differs. Centralizing
/// only the drain keeps the loop testable without coupling to either model.
pub fn run_watch_loop(handle: &WatchHandle, mut on_change: impl FnMut(&FsChange)) {
    while let Ok(change) = handle.changes.recv() {
        on_change(&change);
    }
}
```
`WatchHandle.changes` is a `std::sync::mpsc::Receiver<FsChange>`; `recv()` returns `Err`
once the sender (held by the adapter's keepalive inside the handle) drops, ending the loop.

---

## 4. Daemon refactor (`cairn-daemon/src/main.rs`)

Replace the hand-rolled drain loop inside the `spawn_blocking` closure with a call to
`run_watch_loop`. Today it is shaped like:
```rust
tokio::task::spawn_blocking(move || {
    for change in handle.changes.iter() {
        watch_state.apply_change_blocking(&change);
    }
});
```
becomes:
```rust
tokio::task::spawn_blocking(move || {
    cairn_service::run_watch_loop(&handle, |change| watch_state.apply_change_blocking(change));
});
```
Pure DRY; no behavior change. (`apply_change_blocking` already locks the shared engine and
forwards events to WebSocket subscribers.)

---

## 5. CLI `cairn watch` (`cairn-cli`)

New subcommand:
```rust
/// Watch the cairn for changes and stream them until interrupted.
Watch {
    /// Emit one JSON event per line instead of human-readable lines.
    #[arg(long)]
    json: bool,
},
```
Behavior (mirrors how every CLI command builds + reindexes the engine first):
1. Build the engine (`TantivyIndex::in_memory()`), `reindex` once (seeds memo + stamps).
2. `NotifyWatcher.watch(&root)?` — on error, exit with `error: file watcher: {e}`.
3. Print banner `watching {root} for changes` to **stderr** (so `--json` stdout stays clean).
4. `run_watch_loop(&handle, |change| { … })` on the main thread: for each change,
   `engine.apply_change(change, &mut sink)`; on `Err`, print `watch: {e}` to stderr and
   continue. Runs until Ctrl-C ends the process.

The sink is a small `EventSink` that renders each emitted event:
```rust
struct WatchSink {
    json: bool,
}
impl cairn_app::EventSink for WatchSink {
    fn emit(&mut self, event: cairn_app::Event) {
        if self.json {
            let wire = cairn_service::app_event_to_wire(event);
            // wire is `cairn_contract::Event` (serde-serializable, tagged snake_case)
            println!("{}", serde_json::to_string(&wire).expect("event serializes"));
        } else {
            match event {
                cairn_app::Event::NoteChanged(p) => println!("changed {}", p.as_str()),
                cairn_app::Event::NoteDeleted(p) => println!("removed {}", p.as_str()),
                // Reindexed/Committed are noise for a human watch view.
                _ => {}
            }
        }
    }
}
```
Human mode prints only real, post-dedup changes (spurious events emit nothing). `--json`
mode emits every wire event including `reindexed`, one per line, for piping.

`serde_json` is already a workspace dependency; add it to `cairn-cli` if not present.

---

## 6. Incremental reads — the stat-guard

### 6.1 Ports (`cairn-ports`)
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
Add to `VaultStore`:
```rust
/// Stat a note's change-fingerprint without reading its contents.
///
/// # Errors
/// `NotFound` if the note is missing; `Adapter` on other failures.
fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError>;
```

### 6.2 Infra (`cairn-infra` — `LocalFsStore::stamp`)
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
    Ok(FileStamp { modified, len: meta.len() })
}
```

### 6.3 Application (`cairn-app` — `Engine`)
- Add a field `stamps: HashMap<NotePath, FileStamp>` (initialized empty in `new`).
- `reindex`: after (re)building `memo`, record a stamp per current note —
  `self.stamps = paths.iter().map(|p| Ok((p.clone(), self.store.stamp(p)?))).collect::<Result<_, PortError>>()?;`
  (or stat inside the existing per-note loop). Emits `Reindexed` as today; no per-note events.
- `apply_change(FsChange::Changed(path))` gains a stat-guard at the top:
  ```rust
  let stamp = match self.store.stamp(path) {
      Ok(s) => s,
      Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
      Err(e) => return Err(e),
  };
  if self.stamps.get(path) == Some(&stamp) {
      return Ok(()); // unchanged on disk → skip the read entirely
  }
  // … existing: read, parse, hash; record the new stamp regardless …
  self.stamps.insert(path.clone(), stamp);
  // … existing memo/hash dedup decides whether to upsert + emit …
  ```
  The existing content-hash memo check stays (it still suppresses re-indexing when a file
  changed then reverted to indexed content). The stat-guard is a cheaper earlier short-circuit.
- `apply_removal`: also `self.stamps.remove(path)` (alongside the existing memo/index removal).

### 6.4 Tradeoff (documented)
An external edit preserving the exact `(mtime, len)` is skipped by the guard. On
nanosecond-resolution filesystems (APFS, ext4, NTFS) distinct edits don't collide; only a
same-length edit within coarse mtime granularity could be missed. This is the standard
mtime-watcher caveat and acceptable for a watcher optimization. `write_note`/`delete_note`
route through `apply_change`/`apply_removal`; a self-write sets a fresh mtime, so the guard
does not suppress command-path writes in practice.

---

## 7. Error handling

- `run_watch_loop` returns when the handle's sender drops (process/handle gone) — a clean
  end, not an error.
- `cairn watch`: a per-change `apply_change` error prints to stderr and the loop continues
  (one unreadable file must not kill the watch). A failure to *start* the watcher exits
  non-zero with a clear message.
- All new port operations map IO failures to `PortError::Adapter`; missing files to
  `PortError::NotFound`.

---

## 8. Testing

- **service:** `run_watch_loop` drains a synthetic `WatchHandle` built from an mpsc pair
  (send `Changed`/`Removed`, collect via `on_change`, drop the sender → loop ends); asserts
  order and termination.
- **infra:** `LocalFsStore::stamp` returns mtime+len for an existing note; `NotFound` for a
  missing one; the stamp differs after the note is rewritten with new content.
- **app:**
  - stat-guard: with a seeded stamp, `apply_change(Changed)` on an unchanged file emits
    nothing and performs no re-index;
  - a genuine change (new stamp + new content) still emits `NoteChanged` + `Reindexed`;
  - `apply_removal` drops the stamp (a later recreate is treated as new);
  - `reindex` populates `stamps` for all notes.
- **cli:** unit-test `WatchSink::emit` formatting for both modes (human lines for
  `NoteChanged`/`NoteDeleted`, nothing for `Reindexed`; JSON line for each in `--json`),
  rather than spawning the blocking `watch` command.
- **daemon:** existing watcher integration test still passes after the `run_watch_loop`
  refactor (no behavior change).

---

## 9. Docs

- **ADR-0005** (`docs/decisions/0005-in-process-watcher.md`): the in-process watch host via
  `run_watch_loop`; the stat-guard and why incremental-read's larger payoff needs on-disk
  persistence; the `(mtime,len)` tradeoff.
- **Handoff update:** `cairn watch [--json]` exists; embedders that host the engine
  in-process drive it with `run_watch_loop` + `apply_change` (the daemon and CLI are the two
  reference consumers).

---

## 10. Out of scope

On-disk stamp/index persistence (where stat-skip across restarts actually pays off); true
byte-range/partial reads (meaningless for small notes); the broad in-memory Note cache
(declined — would also serve list/graph/tags/backlinks from memory); watcher-triggered
periodic full rescans; an in-process watcher adapter other than `NotifyWatcher` (it already
works in-process).
