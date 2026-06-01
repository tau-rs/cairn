# ADR-0001: Walking skeleton — hexagonal workspace, git-backed engine

**Status:** Accepted
**Date:** 2026-06-01

## Context

Cairn is an open-source, git-backed, Obsidian-class note-management engine
built in Rust. It lives in the `tau-rs` GitHub org and mirrors tau's
engineering conventions: hexagonal architecture, `forbid(unsafe_code)`,
dual MIT/Apache-2.0 license, ADR + Diátaxis docs, and GitHub Actions CI.

The full design is specified in
`docs/superpowers/specs/2026-06-01-cairn-engine-design.md`.

Before any UI, plugin host, daemon transport, or tau integration can be built,
the team needs confidence that:

1. The port/adapter hexagon actually works end-to-end for real operations
   (read, write, search, backlinks, commit).
2. The typed contract (Commands / Queries / Events) is locked down and
   generates correct TypeScript bindings — the artifact the UI session imports.
3. Every planned capability (plugin host, tau adapter, daemon, CRDT collab)
   has a proven seam — a real port trait with a Null/stub adapter — so later
   sub-projects are additive rather than structural.

A **walking skeleton** satisfies all three goals with the minimum real code:
one working end-to-end path through every layer, placeholders for everything
else.

## Decision

### Crate layout

The workspace is split into six focused crates:

| Crate | Role |
|---|---|
| `cairn-domain` | Pure model: `Note`, `NotePath`, `Graph`, `Link` — no I/O, no deps outside std |
| `cairn-ports` | Trait ports: `VaultStore`, `SearchIndex`, `Vcs`, `Watcher`, `Executor`, `CollabSession`, `AgentRuntime` |
| `cairn-infra` | Concrete adapters implementing those ports |
| `cairn-contract` | Serde Command / Query / Event DTOs + generated TypeScript bindings |
| `cairn-app` | `Engine<S, I, V>`: use-cases that orchestrate ports, emit events |
| `cairn-cli` | Binary: in-process consumer of the engine, validates the contract end-to-end |

`cairn-domain` and `cairn-ports` have zero I/O dependencies. `cairn-app`
depends only on `cairn-domain` and `cairn-ports` — never on `cairn-infra`
directly. Adapters are injected at the construction site in `cairn-cli`.

### Port catalog and adapter status

**Real adapters** (fully wired in the skeleton):

| Port | Adapter | Notes |
|---|---|---|
| `VaultStore` | `LocalFsStore` | Reads/writes plain `.md` files from a directory |
| `Vcs` | `GitVcs` via `git2` | `commit_all`: stages everything, creates a git commit |
| `SearchIndex` | `InMemoryIndex` | Substring match; full reindex on every write |

**Seams** (trait + Null/stub adapter only; behaviour documented, not implemented):

| Port | Seam adapter | Behaviour |
|---|---|---|
| `Watcher` | `NoopWatcher` | `start()` is a no-op |
| `Executor` | `BlockingExecutor` | Runs closures inline on the calling thread |
| `CollabSession` | `NoCollab` | `is_active()` always returns `false` |
| `AgentRuntime` | `NullRuntime` | Returns a descriptive error (tau not yet wired) |

### Skeleton-only choices that are deliberately swappable

The design spec explicitly names these as open questions; the skeleton picks the
simplest option that fits MSRV 1.85 and keeps the port boundary clean:

- **`git2` (libgit2)** instead of `gix`: mature C library with good Rust
  bindings; swappable behind `Vcs` when `gix` is ready.
- **`InMemoryIndex`** instead of Tantivy: zero schema complexity for the
  skeleton; swappable behind `SearchIndex` when full-text ranking is needed.
- **Synchronous, in-process handlers**: no async runtime in the skeleton;
  the `Executor` seam makes a future tokio/rayon swap non-invasive.

### The contract

`cairn-contract` defines three serde enums — `Command`, `Query`, `Event` —
annotated with `#[derive(TS)]` from the `ts-rs` crate. Running
`cargo test -p cairn-contract` generates TypeScript union types into
`crates/cairn-contract/bindings/` (`Command.ts`, `Query.ts`, `Event.ts`).
These are the artifact the UI session imports; the types are intentionally
decoupled from `cairn-domain` internals so the wire format can stabilize
independently of domain refactors.

## Consequences

### What this enables

- The UI session has a real, working engine and a fully typed TypeScript
  contract it can import immediately — no stubs required.
- Later sub-projects (plugin host, tau adapter, daemon transport, CRDT collab)
  are purely additive: swap a Null adapter for a real one behind an existing
  port, or add new commands/events to the contract without breaking existing
  shapes.
- The hexagonal split is validated by a real binary (`cairn-cli`) that
  exercises every layer; regressions are caught by `cargo test --workspace`.

### Known limitations carried forward

- **Full reindex per write**: every `write_note` call rebuilds the entire
  in-memory index from scratch. Acceptable for the skeleton; a Tantivy adapter
  with incremental updates is the fix.
- **No real watcher**: external file changes (e.g. editing notes with another
  editor) are not detected at runtime. The `NoopWatcher` seam is the
  placeholder.
- **No daemon / async transport**: the CLI is purely synchronous and
  in-process. The `Executor` seam and the transport-blind contract design mean
  async/daemon support is additive.
- **No collab**: `NoCollab` always reports no live session.
- **No tau**: `NullRuntime` returns an error for every agent action.
- **MSRV / dependency note**: a transitive dependency in the `git2` tree
  required pinning entries in `Cargo.lock` to keep the workspace building on
  Rust 1.85. These pins should be revisited when upgrading the toolchain or
  migrating to `gix`.
