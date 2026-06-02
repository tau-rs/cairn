# ADR-0005: In-process watcher and stat-guard

**Status:** Accepted
**Date:** 2026-06-02

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

### What this enables

- The in-process path now has live watching, sharing one tested loop with the daemon.
- The stat-guard avoids reads on spurious events. Its larger payoff (skipping reads across
  restarts) needs on-disk index/stamp persistence, which is deferred; with the in-memory
  index a cold start still reads everything.

### Accepted limitations and known seams

- Tradeoff: an external edit preserving the exact `(mtime, len)` is skipped. On
  nanosecond-resolution filesystems distinct edits do not collide; the content-hash memo
  remains the backstop on the read path.
