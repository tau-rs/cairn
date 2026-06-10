# Cairn — Security & Design Audit

## Project overview

Cairn is a git-backed, Obsidian-class note-management engine in Rust, structured
as a hexagonal Cargo workspace: pure domain types (`cairn-domain`), port traits
(`cairn-ports`), concrete adapters (`cairn-infra`: `LocalFsStore`, `GitVcs`,
`TantivyIndex`, `ProcessPluginHost`), use-cases (`cairn-app`), a transport-blind
contract (`cairn-contract`), an in-process CLI (`cairn-cli`), and an HTTP +
WebSocket daemon (`cairn-daemon`). It is a "walking skeleton": the engine, CLI,
on-disk index, file watcher, and out-of-process plugin host are working; the web
UI, tau agent runtime, and CRDT collaboration are present as seams.

The trust boundaries that matter: the **CLI/daemon write surface**, the
**loopback HTTP+WS daemon** (no auth; CORS-gated for browsers only), and the
**out-of-process plugin host** (spawns arbitrary executables, services
capability-gated host callbacks).

## Findings by severity

| Severity | Count |
|----------|-------|
| Critical | 1 |
| High     | 2 |
| Medium   | 7 |
| Low      | 13 |
| **Total**| **23** |

- Security: 1 Critical, 2 High, 3 Medium, 1 Low (`security.md`)
- Design: 1 Medium, 10 Low (`design.md`)
- Diagnostics: 3 Medium, 2 Low (`diagnostics.md`)
- DevOps & CI/CD: see `audit/devops.md` — 9 recommendations (3 High, 3 Medium, 3 Low) toward the unified tiered pipeline.

## Top 5 issues

1. **S1 (Critical) — Note write → code execution.** `NotePath` accepts leading-dot
   segments, so a write to `.cairn/plugins/<x>/manifest.toml` (or `.git/config`)
   plants a plugin/hook that runs an arbitrary command on the next daemon start /
   git op. Reachable from the CLI, the daemon `/command`, or any `fs:write` plugin.
   `crates/cairn-domain/src/note.rs:15-27`, `crates/cairn-infra/src/localfs.rs:83-89`.

2. **S2 (High) — Cross-origin WebSocket event leak.** Browsers don't apply CORS to
   WebSockets, and `/events` never checks `Origin`, so any visited web page can
   subscribe and exfiltrate the live note-path/commit event stream — defeating
   ADR-0004's "CORS is the only gate." `crates/cairn-daemon/src/lib.rs:183-186`.

3. **S3 (High) — Plugin trust boundary is illusory.** Capabilities are
   self-declared in the plugin's own manifest, and the host spawns an arbitrary
   executable with no approval/signing/sandbox.
   `crates/cairn-infra/src/plugin_host.rs:375-393, 432`.

4. **S5 / S6 (Medium) — Daemon DoS & open access.** No authentication on the
   loopback daemon (any local user can drive `/command`), and a single engine
   panic poisons the mutex and turns every later request into a permanent 500.
   `crates/cairn-daemon/src/lib.rs:93,121,129,140`.

5. **G1 (Medium) — No tracing/structured logging.** The daemon uses ad-hoc
   `println!`/`eprintln!`, no request spans, no levels — effectively undebuggable
   in production; compounded by `read` collapsing all IO errors to `NotFound`
   (G2) and silent state-parse fallback (G3).

## Picking up from here

- **Worktree:** `/Users/titouanlebocq/code/cairn-worktrees/audit`
  (a dedicated git worktree — do **not** touch `/Users/titouanlebocq/code/cairn`).
- **Branch:** `audit/design-security`.
- **State:** This audit added only the `audit/` directory and committed it; no
  source code was modified. The working tree was clean before this commit.
- **Suggested remediation order:** S1 first (close the path-escape invariant in
  `NotePath` + a canonicalize-under-root guard in `LocalFsStore`; this also blunts
  S4). Then S2 (Origin check on the WS upgrade) and S6 (non-poisoning mutex +
  `catch_unwind` around plugin invokes). Then S5 (local token/UDS auth) and the
  diagnostics cluster G1-G3 together (introduce `tracing`, fix the `read` error
  mapping, surface state-parse failures).
- Each finding cites concrete `path:line` locations; the three detail files are
  `security.md`, `design.md`, `diagnostics.md`.
