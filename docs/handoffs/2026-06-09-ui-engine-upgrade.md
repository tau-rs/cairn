# UI ← Engine Upgrade Guide: from `079f9f9` to current `main`

**Date:** 2026-06-09
**From:** the engine session
**To:** the `cairn-web-ui` (Tauri in-process) session
**Engine target rev:** `dca454f` (current `main`)
**Your current pin:** `079f9f9` — **~99 commits behind**

---

## TL;DR

Your Tauri app pins the engine at `079f9f9` (the original handoff commit). Since then the
engine gained a lot that you can adopt with **one breaking change** and some **in-process
wiring** — most importantly **live external-edit updates** (you have none today) and
**ranked search with snippets**. Nothing new needs to be built in the engine; this is an
adopt-what-exists upgrade.

Bump the git `rev` in `src-tauri/Cargo.toml` from `079f9f9` to `dca454f`, re-vendor the TS
bindings, and apply the changes below.

> **MSRV:** the engine now requires **Rust 1.88** (was lower). Your Tauri build/CI
> environment must have `rustc >= 1.88`. (`rust-toolchain.toml` in the engine pins it.)

---

## 1. What's new since `079f9f9` (what you can adopt)

| Capability | What it gives the UI | Adoption cost |
|---|---|---|
| **Ranked full-text search + snippets** | `search` now returns score + a highlighted snippet per hit, BM25-ranked, mid-word substring still matches | **Breaking** — response shape changed (see §2) |
| **File watcher → live events** | The webview live-refreshes on external edits / `git pull` — today you only refresh on your own writes | In-process wiring (§4) |
| **On-disk persistence + reconcile** | Fast startup on large vaults — re-index only what changed since last run | In-process wiring (§3) |
| **In-memory note cache** | `list_notes` / `get_graph` / `get_backlinks` / `list_tags` / `notes_by_tag` are now served from memory (no vault re-read per call) — your graph view + backlinks panel get cheaper | Free (automatic) |
| **Link-aware rename/move** | A "rename note" UI feature: moves the file and rewrites `[[wikilinks]]` pointing at it | New command (§5) |
| **Frontmatter tags** | `list_tags` / `notes_by_tag`; `NoteSummary` now carries `tags: string[]` | New queries (§5) |

---

## 2. The one breaking change: `search` → `search_results`

At `079f9f9`, `Query::Search` returned `QueryResponse::Paths { paths: string[] }`. It now
returns a richer variant:

```ts
// QueryResponse gained:
| { type: "search_results"; results: SearchResult[] }   // <- search

interface SearchResult {
  path: string;
  score: number;            // BM25 (relative ordering only; not normalized)
  snippet: string;          // plain-text excerpt around the match
  highlights: [number, number][];  // [start, end) byte ranges within `snippet`
}
```

`get_backlinks` and `notes_by_tag` still return `{ type: "paths"; paths }` — only `search`
moved. Update your search handler to read `results` (each `SearchResult.path` for the list;
`snippet` + `highlights` for a result preview). Re-vendoring the bindings (§6) brings the
new `SearchResult` type.

---

## 3. On-disk persistence (faster startup)

Today `src-tauri` uses `InMemoryIndex` + `engine.reindex(...)` — it rebuilds the whole
Tantivy index in memory on every open. Switch to the persistent index + `reconcile`:

```rust
// type alias
type CairnEngine = Engine<LocalFsStore, TantivyIndex, GitVcs>;   // was InMemoryIndex

use cairn_infra::{ensure_cairn_dir, GitVcs, LocalFsStore, TantivyIndex};

fn open_engine(dir: &Path) -> Result<CairnEngine, ServiceError> {
    let store = LocalFsStore::open(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let vcs = GitVcs::open_or_init(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
    ensure_cairn_dir(dir).map_err(|e| ServiceError::Internal(e.to_string()))?; // creates <dir>/.cairn/ (+ .gitignore)
    let index = TantivyIndex::open_at(&dir.join(".cairn").join("index"))
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(Engine::new(store, index, vcs))
}

// in `open_at`, replace `engine.reindex(&mut sink)` with:
engine.reconcile(&mut sink).map_err(|e| ServiceError::Internal(e.to_string()))?;
```

`reconcile` loads `<dir>/.cairn/state.json`, stat-diffs against disk, and re-indexes only
changed/added/removed notes (full build the first time). It persists `<dir>/.cairn/`
(auto-gitignored, so it never enters the user's notes repo).

**Concurrency (important):** an on-disk Tantivy index has a single exclusive **writer**.
The Tauri app holds it for the open cairn's lifetime, so it must be the **sole writer** —
do **not** run `cairn-daemon` against the same cairn at the same time (it would fail to
acquire the lock). On cairn-close, drop the engine to release the lock. (If you ever want
the daemon + app to share a cairn, that's the deferred "reader-shared" Phase-2 work — not
done; ask the engine session.)

---

## 4. Live external-edit updates (the watcher) — your biggest functional gap

Today your `TauriSink` only emits when *you* mutate (write/delete via `dispatch_command`).
External changes (another editor, `git pull`, `git checkout`) are invisible until reopen.
The engine ships a real OS watcher; host it in-process and drive `Engine::apply_change`,
which emits `note_changed` / `note_deleted` through your existing `TauriSink` →
`cairn://event` (the webview already listens).

**Pieces:** `cairn_infra::NotifyWatcher` (impl of `cairn_ports::Watcher`), its
`WatchHandle { changes: mpsc::Receiver<FsChange> }`, and `Engine::apply_change(&mut self,
&FsChange, &mut dyn EventSink)`. The engine also exposes `cairn_service::run_watch_loop`,
but that blocks on `recv()` forever — fine for the daemon (one cairn, never stops), **but
your app opens different cairns, so you need a *stoppable* loop.** `WatchHandle.changes` is
`pub`, so write the loop with `recv_timeout` + a stop flag:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use cairn_infra::NotifyWatcher;
use cairn_ports::Watcher;

// Add a stop flag + join handle to CairnState so cairn-switch/close can stop the watcher.
// On open (after recording the engine + path):
let handle = NotifyWatcher.watch(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
let stop = Arc::new(AtomicBool::new(false));
let watch_state = state.clone();          // Arc<Mutex<Option<(CairnEngine, PathBuf)>>>
let app_for_watch = app.clone();
let stop_for_thread = stop.clone();
std::thread::spawn(move || {
    use std::sync::mpsc::RecvTimeoutError;
    loop {
        if stop_for_thread.load(Ordering::Relaxed) {
            break;
        }
        match handle.changes.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(change) => {
                if let Ok(mut guard) = watch_state.inner.lock() {
                    if let Some((engine, _)) = guard.as_mut() {
                        let mut sink = TauriSink(app_for_watch.clone());
                        let _ = engine.apply_change(&change, &mut sink);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(handle); // releases the OS watcher
});
// store `stop` (and optionally the JoinHandle) in CairnState; on close/switch:
//   old_stop.store(true, Ordering::Relaxed);  // the thread exits within ~250ms and drops the handle
```

Notes:
- Hold the **engine lock only per-change** (as shown) — never across the blocking
  `recv_timeout`, or you'd block `send_command`/`run_query`.
- `apply_change` is idempotent and deduped (content-hash + a `(mtime,len)` stat-guard), so
  spurious/echo events are cheap no-ops; your own writes won't double-emit.
- The watcher is `.md`-only, ignores `.git/` and `.cairn/`, and is debounced (~200ms).

---

## 5. New commands/queries you can wire (optional features)

All flow through your existing `send_command` / `run_query` (they call `dispatch_*`), so no
new Tauri commands are needed — just new `Command`/`Query` payloads from the bindings:

- **Rename/move:** `{ type: "rename_note", from, to }` → `{ type: "done" }`. Emits
  `note_deleted(from)`, `note_changed(to)`, then a `note_changed` per note whose
  `[[wikilink]]` was rewritten. Wire a rename action in the note context menu.
- **Tags:** `{ type: "list_tags" }` → `{ type: "tags", tags: TagCount[] }`
  (`TagCount { tag, count }`); `{ type: "notes_by_tag", tag }` → `{ type: "paths", paths }`.
  `NoteSummary` (from `list_notes`) now includes `tags: string[]`. Enables a tag panel /
  tag filter.

---

## 6. Mechanical steps

1. **Bump the engine rev** in `src-tauri/Cargo.toml`: change all five `rev = "079f9f9..."`
   to `rev = "dca454f3b14948d2fbf6f5d66954d33d53f53159"`. Run `cargo build` in `src-tauri`
   (needs Rust 1.88).
2. **Re-vendor the TS bindings** from the engine's
   `crates/cairn-contract/bindings/*.ts` at `dca454f` (they now include `SearchResult`, the
   `search_results` arm of `QueryResponse`, `rename_note` in `Command`, `TagCount`, and
   `tags` on `NoteSummary`). Your `scripts/` vendoring step should already do this — point it
   at the new rev.
3. **Adapt the search handler** to the `search_results` shape (§2).
4. **Switch to persistence** (§3) and **wire the watcher** (§4).
5. Run your Vitest/Playwright suite; the contract shapes are typed, so the compiler/TS will
   flag the search-shape change.

---

## 7. What the engine still does NOT have (your later roadmap phases)

- **Plugin host** (your Phase 6): the engine's out-of-process plugin system is **not built**
  yet. When you reach it, coordinate — it's real engine work.
- **tau `AgentRuntime` actions** (your Phase 7): the `AgentRuntime` port is a `NullRuntime`
  stub; surfacing agent actions needs both the engine seam fleshed out and tau to firm up.
- **Reader-shared on-disk index** (daemon + app sharing one cairn's index concurrently):
  designed but deferred (Phase 1 of that work — daemon-as-sole-writer — is what's shipped).

Everything in §1–5 is ready today.
