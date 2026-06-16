# ADR-0012: External-edit sync hardening

**Status:** Proposed (implementation lands in the sync-hardening PR following the MCP server)
**Date:** 2026-06-16

## Context

Alongside the MCP server (ADR-0011), agents may edit note files **natively** on
disk (tau's future `fs.write`) rather than through the MCP/command path. Native
edits surface only through the daemon watcher → `Engine::apply_change`. The
watcher already re-indexes and recomputes the link graph, but three gaps make
native edits unsafe end-to-end:

1. External edits are never committed — only an explicit `Command::Commit` commits.
2. A native `mv a.md b.md` surfaces as `Removed(a)`+`Changed(b)`; the index stays
   correct but `[[a]]` wikilinks are **not** rewritten (only `Engine::rename_note`
   is link-aware).
3. A partial/atomic write can make the watcher fire `Changed` while the file is
   momentarily absent; `apply_change` maps `NotFound → apply_removal`, a spurious
   — possibly terminal — delete.

Plus an inherent race: the filesystem is unguarded, so an external write can race
cairn's own `write_note`.

The MCP/command path is unaffected — it is indexed, link-aware, and race-free
under the engine lock. This ADR hardens the **best-effort native path** and draws
the boundary explicitly. All recommended changes live at the daemon edge; no
`Engine`/`Vcs`/`Watcher` port changes are required.

## Decision

### Gap 1 — coalesced auto-commit of external edits (fix)

Opt-in, **off by default**. After a quiet period with no further external change,
the daemon commits externally-detected changes with a generic message. Cairn's own
command-path writes stay dirty-until-`Commit` (unchanged): command writes have a
caller-chosen transaction boundary; external edits have none, so a quiet-period
coalesce is the only available signal. The policy lives at the daemon edge (clock/
threads) via a `commit_external_blocking` on `AppState` calling the existing
`Engine::commit`, driven by a testable `run_watch_loop_timeout` sibling in
`cairn-service` (injected clock). Config: `[sync] auto_commit`, `quiet_period_ms`.

`GitVcs::commit_all` stages everything (`add_all(["*"])`), so an auto-commit
sweeps any pending command-path edits too; accepted for v1 with a generic message.
A path-scoped commit (`Vcs::commit_paths`) is deferred.

### Gap 3 — confirm-before-delete (fix)

Before deleting on a `Removed` (or a `Changed` the engine would treat as removal),
the daemon re-stats the path after a short grace; if it now exists, it routes
`Changed` instead. `apply_removal` is idempotent and the stat-guard skips no-ops,
so the re-check is harmless. Partial *reads* that parse are not fixed — the
content-hash memo plus the next event self-heal (retrying would be
over-engineering).

### Gap 2 — native rename link-rewrite (document)

A native rename keeps the index correct (old removed, new added) but does **not**
rewrite wikilinks. Link-preserving moves must go through the `rename_note` tool /
`Command::RenameNote`. Rename *detection* in the watcher is non-portable (macOS
FSEvents splits rename events; ADR-0003 chose existence-classification
deliberately) and buys no correctness, so it — and a future `NoteRenamed` event —
is deferred.

### Gap 4 — write race (document)

The engine mutex serializes engine state, not the disk. The content-hash memo
gives eventual consistency (ADR-0005); true lost-update is inherent to concurrent
file writes. Agents should **prefer the MCP write path** (race-free, link-aware)
for writes they originate; native edits are the best-effort sync path.
Filesystem locking is rejected as over-engineering for a window the memo heals.

## Consequences

### What this enables

- Native edits gain durable git history (opt-in) and no longer risk a spurious
  terminal delete mid-write.
- The MCP-vs-native boundary is a deliberate, documented contract: MCP is
  authoritative; native is best-effort with known limitations.

### Accepted limitations and deferred increments

- Auto-commit sweeps pending command-path edits (generic message); path-scoped
  commit deferred.
- Native rename does not rewrite links; rename detection / `NoteRenamed` deferred.
- Partial-read transient content (self-healing) not retried.
- Concurrent file-write lost-update is inherent and not locked against.
