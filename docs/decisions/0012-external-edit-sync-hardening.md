# ADR-0012: External-edit sync hardening

**Status:** Accepted (Gap 1 superseded 2026-08-27 — see Update below)
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
the boundary explicitly. The policy lives at the daemon edge; the only port
additions are two read-only probes (`Vcs::is_dirty`, `Engine::exists_on_disk`)
that keep the engine pure and synchronous. The `Watcher` port is unchanged.

## Decision

### Gap 1 — coalesced auto-commit of external edits (fix)

> **Superseded 2026-08-27** by the Update below — auto-commit is now on by
> default, covers every edit source, and the config keys have changed. The
> paragraph is kept verbatim as the record of the original decision.

Opt-in, **off by default**. After a quiet period with no further external change,
the daemon commits externally-detected changes with a generic message. Cairn's own
command-path writes stay dirty-until-`Commit` (unchanged): command writes have a
caller-chosen transaction boundary; external edits have none, so a quiet-period
coalesce is the only available signal. The policy lives at the daemon edge (clock/
threads) via a `commit_external_blocking` on `AppState` calling the existing
`Engine::commit`, driven by a testable `run_watch_loop_timeout` sibling in
`cairn-service`. Config: `[sync] auto_commit`, `quiet_period_ms`.

`commit_external_blocking` first checks a new `Vcs::is_dirty` (via
`Engine::has_uncommitted_changes`) and no-ops on a clean tree, so a spurious
watcher event or an already-committed change never produces an empty commit.

`GitVcs::commit_all` stages everything (`add_all(["*"])`), so an auto-commit
sweeps any pending command-path edits too; accepted for v1 with a generic message.
A path-scoped commit (`Vcs::commit_paths`) is deferred.

### Gap 3 — confirm-before-delete (fix)

Before honoring a `Removed`, the daemon waits a short grace and re-checks
existence (`Engine::exists_on_disk` → `VaultStore::stamp`); if the file is back,
it routes `Changed` instead. `apply_removal` is idempotent and the stat-guard
skips no-ops, so the re-check is harmless. Grace is `[sync] confirm_grace_ms`
(default 50). Partial *reads* that parse are not fixed — the
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

## Update — 2026-08-27: engine-owned sealing, auto-commit on by default

Gap 1 is superseded. Commit policy moved into the engine (PR #179, design:
`docs/superpowers/specs/2026-08-22-engine-auto-commit-versioning-design.md`),
which changes three things about the decision above.

**1. Auto-commit is ON by default and no longer external-edit-specific.** The
daemon marks a dirty flag after *any* successful mutating dispatch — the
watcher's external edits as before, plus `WriteNote`/`DeleteNote`/`RenameNote`/
`RestoreNote` and collab flushes. One quiet period seals one *editing session*
into one commit, whatever wrote the bytes. The original asymmetry — external
edits coalesce, command-path writes stay dirty until an explicit `Commit` — is
gone: it produced a history that was part noise, part missing, and the engine is
the only party that sees every change source. An explicit
`Command::Commit { message: None }` ("seal now") seals immediately, and a
long-running session that never goes idle is sealed by a backstop timer.

**2. Config keys changed.** The surface is now:

```toml
[sync]
auto_commit = true        # was false
idle_seconds = 2          # replaces quiet_period_ms
backstop_minutes = 20     # new: seal a never-idle session anyway
confirm_grace_ms = 50     # unchanged (Gap 3)
```

`quiet_period_ms` is still accepted as a deprecated alias (an explicit
`idle_seconds` wins; setting it logs a deprecation warning), so existing
`cairn.toml` files keep working.

**3. The empty-commit guard moved and got sharper.** `Engine::has_uncommitted_changes`
(over `Vcs::is_dirty`) is gone; the seal path guards on
`Vcs::pending_summary().changes` being non-empty, which is the same diff walk
that generates the commit message. Commit messages are engine-generated from
that diff, replacing the generic `"cairn: sync external edits"` — so the
"generic message" limitation recorded above no longer applies.

Gaps 2, 3 and 4 are unaffected and remain accurate: confirm-before-delete still
guards `Removed` events, native renames still do not rewrite wikilinks, and the
write race is still unlocked. Push/sync policy stays decoupled — sealing never
pushes.
