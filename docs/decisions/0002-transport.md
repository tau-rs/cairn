# ADR-0002: Transport — dispatcher + daemon, no Tauri in the engine

**Status:** Accepted
**Date:** 2026-06-01

## Context

ADR-0001 delivered a walking skeleton: a working hexagonal engine and a typed
wire contract (`cairn-contract`) with serde DTOs and generated TypeScript
bindings. The skeleton's final review flagged three open gaps:

1. Nothing served the contract — no dispatcher mapped `Command`/`Query` to
   engine use-cases.
2. `Query` had no response DTOs: `GetNote`, `Search`, and `GetBacklinks` all
   lacked a defined return shape.
3. App-internal events (`cairn_app::Event`) had no conversion path to wire
   events (`cairn_contract::Event`).

The design for this sub-project is specified in
`docs/superpowers/specs/2026-06-01-cairn-transport-design.md`.

Tauri is not in scope here. Tauri bundles a webview, a frontend build, and a
Rust backend into a single desktop app — that is a UI concern. The engine repo
ships transport-blind artifacts only: an in-process library and a daemon. The
UI session wires Tauri by calling `cairn-service` in-process. Tau is still in
active development and its integration remains a future seam (`NullRuntime`).

## Decision

### New contract DTOs (`cairn-contract`)

Three new enums were added and TS bindings regenerated (six files total in
`crates/cairn-contract/bindings/`):

- `CommandResponse`: `Done` (write or delete acknowledged) |
  `Committed { commit: String }` (short commit id). Note: the design spec named
  the first variant `Written`; the implementation uses `Done` for accuracy —
  write and delete both succeed with the same acknowledgement shape.
- `QueryResponse`: `Note { contents: String }` | `Paths { paths: Vec<String> }`.
- `ContractError`: `NotFound { what: String }` |
  `InvalidRequest { message: String }` | `Internal { message: String }`.

All types carry `#[serde(tag = "type", rename_all = "snake_case")]` so the
generated TypeScript stays discriminated unions, consistent with `Command`,
`Query`, and `Event`.

### `cairn-service` — the transport-blind dispatcher

A new library crate. No I/O, no async, no transport dependency.

- `dispatch_command(engine, command, sink) -> Result<CommandResponse, ServiceError>`:
  maps `Command::{WriteNote, DeleteNote, Commit}` to engine use-cases; emits
  produced `AppEvent`s to the caller-supplied `&mut dyn EventSink`.
- `dispatch_query(engine, query) -> Result<QueryResponse, ServiceError>`:
  maps `Query::{GetNote, Search, GetBacklinks}` to engine use-cases; queries
  emit no events.
- `ServiceError` (`NotFound` | `InvalidRequest` | `Internal`) converts to
  `ContractError` via `impl From<ServiceError> for ContractError` (1-to-1
  variant mapping).
- `app_event_to_wire(e: AppEvent) -> WireEvent`: a **free function**, not a
  `From` impl. Both `cairn_app::Event` and `cairn_contract::Event` are defined
  in external crates; `impl From<cairn_app::Event> for cairn_contract::Event`
  in `cairn-service` would violate Rust's orphan rule (neither type is local to
  `cairn-service`). The free function is the correct pattern and is documented
  in the source.

### `cairn-daemon` — HTTP + WebSocket transport

A new binary crate. Depends on `cairn-service`, `cairn-infra`, `axum`, and
`tokio`.

Routes:

| Method | Path | Body | Response |
|--------|------|------|----------|
| POST | `/command` | `Command` (JSON) | `CommandResponse` or `ContractError` |
| POST | `/query` | `Query` (JSON) | `QueryResponse` or `ContractError` |
| GET | `/events` | — (WebSocket upgrade) | stream of `Event` JSON text frames |
| GET | `/health` | — | `200 OK` |

`AppState` holds `Arc<Mutex<CairnEngine>>` and a `tokio::sync::broadcast::Sender<WireEvent>`.
Commands and queries run inside `tokio::task::spawn_blocking` so the async
runtime is never stalled by blocking engine work. Command handlers publish
produced events to the broadcast channel; WS clients subscribe and receive
them as JSON text frames. Lagged receivers drop oldest frames (broadcast
semantics) — backpressure refinement is deferred.

The daemon binds `127.0.0.1:<port>` only (LoopbackTrust, no authentication).
CLI flags: `--cairn <path>` (default `.`) and `--port <n>` (default `7777`).
The daemon requires an already-initialized cairn (`.git` must exist), mirroring
the CLI guard. On startup it runs a full reindex before accepting requests.

Mutex poison (an engine panic inside `spawn_blocking`) surfaces as a generic
`ContractError::Internal` with no internal detail leaked to clients.

### CLI retrofit (`cairn-cli`)

The CLI no longer calls `Engine` methods directly. Each subcommand builds a
`Command` or `Query` and calls `cairn_service::dispatch_command` or
`dispatch_query` against an in-process `Engine`. This proves the in-process
transport and validates the dispatcher against the existing integration tests
(behavior is identical; all tests remain green).

### Tauri excluded from the engine repo

Tauri bundles a frontend and belongs in the UI session. The engine repo ships
two transports:

- **In-process** (`cairn-service`): the UI session (Tauri shell) calls this
  directly for the efficient desktop path.
- **Network** (`cairn-daemon`): a pure-browser or remote client connects over
  HTTP/WS for the browser/remote path.

No `cairn-tauri` crate exists in this repo by design.

### MSRV

`axum` and `tokio` were resolved without requiring a pin above Rust 1.85.
`Cargo.lock` was updated and `cargo build --locked` remains green on 1.85.

## Consequences

### What this enables

- The contract is now served: CLI (in-process) and daemon (network) are both
  functional and tested.
- Browser and remote surfaces are unblocked: a browser UI can `POST /command`,
  `POST /query`, and open `GET /events` against a running daemon.
- The UI session can wire Tauri by calling `cairn-service` in-process (desktop)
  or by connecting to `cairn-daemon` over HTTP/WS (browser/remote) — the same
  contract TS types cover both paths.

### Accepted limitations and known seams

- **No auth/TLS:** the daemon is loopback-only (`127.0.0.1`). Exposing it
  over a network, adding token auth, or adding mutual TLS are deferred to a
  later sub-project.
- **Broadcast lag-drop:** slow WS clients lose events silently. Acceptable for
  now; a per-client buffer or SSE fallback is a future refinement.
- **Full reindex per write:** unchanged from ADR-0001; every `write_note` call
  rebuilds the in-memory index from scratch. A Tantivy adapter with incremental
  updates is the fix.
- **No external-change event push:** `NoopWatcher` means files edited by
  external tools (another editor, a git pull) do not trigger `NoteChanged`
  events. The `Watcher` port seam is the placeholder.
- **Mutex-poison panic:** a panic inside the engine surfaces as a generic 500
  with no detail. Structured panic handling is deferred.

### Deferred to future sub-projects

Auth/TLS and network exposure, the Tauri shell (UI session), Tantivy full-text
index, a real file watcher, CRDT collab, tau/`AgentRuntime` integration.
