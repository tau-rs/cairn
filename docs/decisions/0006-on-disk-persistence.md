# ADR-0006: On-disk index persistence (daemon, phase 1)

**Status:** Accepted
**Date:** 2026-06-02

## Context

The daemon rebuilt its Tantivy index in memory on every startup (O(all notes)); the
`(mtime,len)` stat-guard couldn't pay off across restarts without persistence. The
browser UI's backend benefits most from fast startup on large vaults.

## Decision

The daemon persists a Tantivy `MmapDirectory` index under `<cairn>/.cairn/index`
(default-on; `--no-persist` / `[index] persist=false` opt out) and **reconciles** on
startup against a sidecar `<cairn>/.cairn/state.json` (per-note content hash +
`(mtime,len)` stamp): seed memo+stamps from state, stat each note, re-index only
changed/added notes and remove deleted ones — startup becomes O(changed notes). The
daemon holds Tantivy's exclusive writer lock for its lifetime (sole writer). State is
a **sidecar** rather than extra index fields, keeping the search schema and `upsert`
signature unchanged. `<cairn>/.cairn/` is auto-created with a `.gitignore` (`*`) so
the cache never enters the user's notes repo. A corrupt/schema-mismatched index is
wiped and rebuilt.

## Consequences

- Daemon startup is O(changed notes), not O(all notes); the user's notes repo stays
  clean.
- `state.json` is saved at reconcile time only — mid-session edits go to the on-disk
  index live (via `apply_change`) but `state.json` lags, so they are
  re-stat-reconciled (correct, slightly redundant) on the next start.
- The daemon is the sole writer; a second daemon on the same cairn fails to acquire
  the lock. **CLI read-only access to the persisted index is Phase 2** (the CLI is
  still ephemeral in-memory per command).
- The `.cairn/` directory is now skipped by note-listing.
