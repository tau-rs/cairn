# Cairn Transport — Design Spec (dispatcher + daemon)

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Sub-project:** ④ transport, from [`2026-06-01-cairn-engine-design.md`](2026-06-01-cairn-engine-design.md).
**Builds on:** the walking skeleton (`cairn-domain/ports/infra/contract/app/cli`).

---

## 1. Goal

Make the contract **servable**. Today the wire contract (`cairn-contract`) compiles
and round-trips, but nothing routes a `Command`/`Query` to the `Engine`, and the
`Query` variants have no response types. This sub-project adds:

1. `cairn-service` — the transport-blind dispatcher mapping the contract ↔ the engine.
2. `cairn-daemon` — a frontend-agnostic HTTP + WebSocket transport over the dispatcher.
3. Retrofit `cairn-cli` to consume the dispatcher (proving the in-process transport).
4. The missing response/error DTOs in `cairn-contract` (+ regenerated TypeScript).

**Tauri is explicitly NOT built here.** Tauri bundles a webview + frontend + Rust
backend into a desktop app — that is a UI concern and belongs in the UI session,
which will consume `cairn-service` in-process (efficient desktop path) or hit
`cairn-daemon` (browser/remote path). The engine repo ships only frontend-agnostic
transports: the in-process library and the daemon.

---

## 2. New contract DTOs (`cairn-contract`)

Add three serde + `ts-rs` types (regenerate `bindings/`):

```rust
/// Result of a successful command.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandResponse {
    Written,                       // write_note / delete_note ack
    Committed { commit: String },  // commit short id
}

/// Result of a successful query.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryResponse {
    Note { contents: String },     // get_note
    Paths { paths: Vec<String> },  // search + get_backlinks
}

/// A typed error returned across the contract boundary.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContractError {
    NotFound { what: String },
    InvalidRequest { message: String },
    Internal { message: String },
}
```

The existing `Command`/`Query`/`Event` are unchanged. All five enums keep the
`#[serde(tag = "type", rename_all = "snake_case")]` representation so the TS the UI
imports stays a discriminated union.

---

## 3. `cairn-service` — the dispatcher

A transport-blind library. Depends on `cairn-app` + `cairn-contract` (and
transitively `cairn-domain`/`cairn-ports`). No I/O, no async, no transport.

```rust
pub fn dispatch_command<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &mut Engine<S, I, V>,
    command: &Command,
    sink: &mut dyn EventSink,
) -> Result<CommandResponse, ServiceError>;

pub fn dispatch_query<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &Engine<S, I, V>,
    query: &Query,
) -> Result<QueryResponse, ServiceError>;
```

Mapping rules:
- `WriteNote` → validate path → `engine.write_note` → `CommandResponse::Written`.
- `DeleteNote` → validate path → `engine.delete_note` → `Written`.
- `Commit` → `engine.commit` → `Committed { commit }`.
- `GetNote` → validate path → `engine.read_note` → `QueryResponse::Note { contents }`.
- `Search` → `engine.search` → `Paths { paths }` (hit paths as strings).
- `GetBacklinks` → validate path → `engine.backlinks` → `Paths { paths }`.

Errors:
```rust
pub enum ServiceError {
    NotFound(String),       // <- PortError::NotFound
    InvalidRequest(String), // <- NotePath validation failure
    Internal(String),       // <- PortError::Adapter and anything else
}
```
`ServiceError: Into<ContractError>` (1:1 variant mapping).

Event mapping (the missing glue the final review flagged):
```rust
impl From<cairn_app::Event> for cairn_contract::Event {
    // NoteChanged(p)  -> note_changed { path: p.as_str() }
    // NoteDeleted(p)  -> note_deleted { path: p.as_str() }
    // Committed(id)   -> committed   { commit: id }
    // Reindexed(n)    -> reindexed   { count: u32::try_from(n).unwrap_or(u32::MAX) }
}
```

`dispatch_command` takes a `&mut dyn EventSink` so each transport decides what to do
with the produced events (the CLI discards them; the daemon publishes them).

---

## 4. `cairn-daemon` — HTTP + WebSocket transport

A binary crate (`cairn-daemon`). Depends on `cairn-service` + `cairn-infra` +
`axum` + `tokio` + `serde_json`. Pure Rust — **no system/webview deps**, so CI
needs no extra packages.

**Routes:**
| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/command` | `Command` (JSON) | `CommandResponse` or `ContractError` |
| POST | `/query` | `Query` (JSON) | `QueryResponse` or `ContractError` |
| GET | `/events` | — (WebSocket upgrade) | stream of contract `Event` JSON text frames |
| GET | `/health` | — | `200 OK` |

**State:** `AppState { engine: Arc<Mutex<Engine<LocalFsStore, InMemoryIndex, GitVcs>>>, events: tokio::sync::broadcast::Sender<Event> }` (`Event` = contract event).

**Command flow:** deserialize `Command` → `spawn_blocking`: lock the engine, run
`dispatch_command` with a `Vec<cairn_app::Event>` sink → map each produced event to a
contract `Event` and `events.send(..)` it on the broadcast → return the
`CommandResponse`. Serialize result; on `ServiceError`, return the mapped
`ContractError` with an appropriate HTTP status (404 NotFound, 400 InvalidRequest,
500 Internal).

**Query flow:** deserialize `Query` → `spawn_blocking`: lock engine, `dispatch_query`
→ serialize `QueryResponse` (queries emit no events).

**Events (WS):** on upgrade, `events.subscribe()`; forward each broadcast `Event` as a
JSON text frame until the socket closes. Lagged receivers (slow clients) drop oldest
per `broadcast` semantics — acceptable; backpressure refinement deferred.

**Binding & auth:** binds `127.0.0.1:<port>` (configurable via flag/env, default e.g.
`7777`), `LoopbackTrust` — **no authentication, localhost only**. Network exposure,
TLS, and `AuthPolicy` (TokenAuth/MutualTls) are deferred to a later sub-project.

**Cairn location:** the daemon serves a single cairn whose path is given by a
`--cairn <path>` flag (must already be an initialized cairn, mirroring the CLI guard).

**Concurrency:** the engine is sync and its operations block (fs/git); they run inside
`spawn_blocking` under a `std::sync::Mutex` so the async runtime is never stalled. No
async refactor of the engine.

---

## 5. CLI retrofit (`cairn-cli`)

`cairn-cli` stops calling `Engine` methods directly. Each existing subcommand builds a
contract `Command`/`Query` and calls `cairn-service::dispatch_*` against an in-process
`Engine`, then prints from the typed response:
- `write` → `Command::WriteNote` → `Written` → print `wrote <path>`.
- `commit` → `Command::Commit` → `Committed { commit }` → print `committed <commit>`.
- `read` → `Query::GetNote` → `Note { contents }` → print contents (no trailing newline).
- `search` → `Query::Search` → `Paths { paths }` → print each path on its own line.
- `backlinks` → `Query::GetBacklinks` → `Paths { paths }` → print each path on its own line.

The CLI keeps its current subcommand set (no `delete` subcommand is added — the
`Command::DeleteNote` variant exists in the contract and is exercised by the daemon
and `cairn-service` tests, not the CLI). The `init` command and the "must be an
initialized cairn" guard are unchanged.
`reindex`-on-startup stays (it is engine-level, not a contract command). Existing CLI
integration tests must stay green — the observable behavior is identical.

---

## 6. Crates & dependencies

New workspace members: `crates/cairn-service`, `crates/cairn-daemon`.

| Crate | New deps |
|---|---|
| `cairn-contract` | (none — just new types) |
| `cairn-service` | `cairn-app`, `cairn-contract` |
| `cairn-daemon` | `cairn-service`, `cairn-infra`, `cairn-domain`, `axum` (with `ws`), `tokio` (`rt-multi-thread`, `macros`, `sync`), `serde_json`, `clap`; dev: `reqwest`, `tokio-tungstenite`, `tempfile` |
| `cairn-cli` | `cairn-service`, `cairn-contract` (added) |

**MSRV risk:** axum/tokio pull a `hyper`/`http`/`tower` dependency tree that may
include crates raising MSRV above Rust 1.85 (same class as the existing
`git2 → url → idna/icu` pins). The plan pins offending transitive versions in
`Cargo.lock` and keeps `cargo build --locked` green on 1.85. If pinning proves
infeasible, the fallback is to record a documented MSRV bump in an ADR (decision
deferred to implementation; default is to hold 1.85).

---

## 7. Testing

- **`cairn-service`** (pure, fast): dispatch each command/query against real
  `cairn-infra` adapters over a tempdir cairn — `WriteNote→Written`,
  `Commit→Committed{id}`, `GetNote→Note{contents}`, `Search→Paths`,
  `GetBacklinks→Paths`; error cases — missing note `→ NotFound`, invalid path (`../x`)
  `→ InvalidRequest`; and a `From<app::Event> for contract::Event` mapping test
  (incl. the `usize→u32` path).
- **`cairn-daemon`** (integration): boot the axum app on an ephemeral `127.0.0.1`
  port over a tempdir cairn; `POST /command` (write) and `POST /query` (search/get)
  via an HTTP client asserting the typed JSON; connect `GET /events` (WS), issue a
  command, and assert the corresponding `Event` arrives on the socket; assert a
  missing-note query returns `404` + `ContractError::NotFound`.
- **`cairn-cli`**: existing integration tests unchanged (behavior identical post-retrofit).

---

## 8. Docs

- New ADR `docs/decisions/0002-transport.md`: dispatcher + daemon; why Tauri is
  excluded from the engine repo; HTTP/WS shape; loopback-only/no-auth default and
  what's deferred.
- Update `docs/handoffs/2026-06-01-ui-session-handoff.md`: the contract is now served
  (CLI in-process + daemon over HTTP/WS); Tauri lives in the UI session and wires up by
  calling `cairn-service` in-process or hitting `cairn-daemon`; the §4 "gap" is closed.

---

## 9. Out of scope (future sub-projects)

Authentication/TLS/network exposure (`AuthPolicy`), the Tauri shell (UI session),
Tantivy, real file-watching, CRDT collab, tau integration, plugin host. The daemon's
`AuthPolicy` and `Watcher`/event-push-from-external-changes remain seams.
