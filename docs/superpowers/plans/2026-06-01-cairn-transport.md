# Cairn Transport Implementation Plan (dispatcher + daemon)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Cairn contract servable: a transport-blind dispatcher (`cairn-service`), an HTTP+WebSocket daemon (`cairn-daemon`), the CLI retrofitted onto the dispatcher, and the missing response/error DTOs — with no Tauri in the engine repo.

**Architecture:** `cairn-service` maps contract `Command`/`Query` → the existing `Engine` use-cases and `cairn_app::Event` → `cairn_contract::Event`. `cairn-daemon` is an axum app holding `Arc<Mutex<Engine>>` + a `tokio::broadcast` event channel; it runs the sync engine via `spawn_blocking`, returns typed JSON, and pushes events over a WebSocket. `cairn-cli` builds contract messages and calls the dispatcher. Builds on the walking-skeleton crates; the engine stays synchronous.

**Tech Stack:** Rust 1.85 (`forbid(unsafe_code)`), `axum` 0.7 (`ws`), `tokio` 1, `tower`/`http-body-util` (tests), `tokio-tungstenite` + `futures-util` (WS test), `serde_json`, `thiserror`, `ts-rs`.

**Build-against facts (verified):** `Engine` methods — `write_note(&NotePath,&str,&mut dyn EventSink)`, `delete_note(&NotePath,&mut dyn EventSink)`, `commit(&str,&mut dyn EventSink)->String`, `read_note(&NotePath)->String`, `search(&str)->Vec<SearchHit>`, `backlinks(&NotePath)->Vec<NotePath>`, `reindex(&mut dyn EventSink)`. `SearchHit { path: NotePath }`. `NotePath::new(&str)->Result<_,NotePathError>`, `NotePath::as_str()`. `PortError::{NotFound(String),Adapter(String)}`. `cairn_app::Event::{NoteChanged(NotePath),NoteDeleted(NotePath),Committed(String),Reindexed(usize)}`. `cairn_contract::Event::{NoteChanged{path},NoteDeleted{path},Committed{commit},Reindexed{count:u32}}`.

**Files created/modified:**
```
crates/cairn-contract/src/lib.rs      # + CommandResponse, QueryResponse, ContractError
crates/cairn-contract/bindings/*.ts   # regenerated
crates/cairn-service/                 # NEW: dispatcher
crates/cairn-daemon/                  # NEW: HTTP+WS transport (lib + bin)
crates/cairn-cli/src/main.rs          # retrofit onto cairn-service
Cargo.toml                            # + members + workspace deps
docs/decisions/0002-transport.md      # NEW ADR
docs/handoffs/2026-06-01-ui-session-handoff.md  # updated
```

**MSRV note:** axum/tokio pull a `hyper`/`http`/`tower` tree that may include crates raising MSRV above 1.85 (same class as the existing `git2 → idna/icu` pins). After adding deps, if `cargo build --locked` fails on 1.85, pin the offending transitive crates with `cargo update <crate> --precise <ver>` until green; do NOT bump the channel. axum is pure Rust — no system packages needed, so CI needs no apt changes.

---

## Task 1: Contract response & error DTOs

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- Modify: `crates/cairn-contract/tests/codegen.rs`
- Regenerated: `crates/cairn-contract/bindings/*.ts`

- [ ] **Step 1: Add the three DTOs**

Append to `crates/cairn-contract/src/lib.rs` BEFORE the `#[cfg(test)] mod tests` block:
```rust
/// Result of a successful command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandResponse {
    /// A write or delete succeeded.
    Written,
    /// A commit was created.
    Committed {
        /// Short commit id.
        commit: String,
    },
}

/// Result of a successful query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryResponse {
    /// A note's contents.
    Note {
        /// Full markdown contents.
        contents: String,
    },
    /// A list of note paths (used by search and backlinks).
    Paths {
        /// Relative note paths.
        paths: Vec<String>,
    },
}

/// A typed error returned across the contract boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContractError {
    /// The requested resource does not exist.
    NotFound {
        /// What was missing (e.g. a note path).
        what: String,
    },
    /// The request was malformed or invalid.
    InvalidRequest {
        /// Human-readable reason.
        message: String,
    },
    /// An internal failure occurred.
    Internal {
        /// Human-readable detail.
        message: String,
    },
}
```

- [ ] **Step 2: Extend the codegen test**

In `crates/cairn-contract/tests/codegen.rs`, replace its body with:
```rust
//! Verifies the `#[ts(export)]` bindings generate without error.
use cairn_contract::{Command, CommandResponse, ContractError, Event, Query, QueryResponse};
use ts_rs::TS;

#[test]
fn exports_typescript_bindings() {
    assert!(Command::decl().contains("Command"));
    assert!(Query::decl().contains("Query"));
    assert!(Event::decl().contains("Event"));
    assert!(CommandResponse::decl().contains("CommandResponse"));
    assert!(QueryResponse::decl().contains("QueryResponse"));
    assert!(ContractError::decl().contains("ContractError"));
    Command::export_all().unwrap();
    Query::export_all().unwrap();
    Event::export_all().unwrap();
    CommandResponse::export_all().unwrap();
    QueryResponse::export_all().unwrap();
    ContractError::export_all().unwrap();
}
```
(If the ts-rs 10 API differs from `decl()`/`export_all()`, adapt as needed — the goal is that all six types generate `.ts` files under `bindings/`.)

- [ ] **Step 3: Add a serde round-trip unit test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-contract/src/lib.rs`, add:
```rust
    #[test]
    fn response_and_error_tags_are_snake_case() {
        let r = CommandResponse::Committed { commit: "abc1234".into() };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"committed\""));
        assert_eq!(serde_json::from_str::<CommandResponse>(&j).unwrap(), r);

        let e = ContractError::NotFound { what: "a.md".into() };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"type\":\"not_found\""));
        assert_eq!(serde_json::from_str::<ContractError>(&j).unwrap(), e);
    }
```

- [ ] **Step 4: Run tests + verify bindings generated**

Run: `cargo test -p cairn-contract`
Expected: PASS. New files exist: `crates/cairn-contract/bindings/CommandResponse.ts`, `QueryResponse.ts`, `ContractError.ts`.

- [ ] **Step 5: Commit (incl. generated bindings)**

```bash
git add -A
git commit -m "feat(contract): CommandResponse, QueryResponse, ContractError DTOs"
```

---

## Task 2: `cairn-service` — the dispatcher

**Files:**
- Create: `crates/cairn-service/Cargo.toml`, `crates/cairn-service/src/lib.rs`
- Modify: root `Cargo.toml` (`members`)

- [ ] **Step 1: Create the crate**

Create `crates/cairn-service/Cargo.toml`:
```toml
[package]
name = "cairn-service"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-domain = { path = "../cairn-domain" }
cairn-ports = { path = "../cairn-ports" }
cairn-app = { path = "../cairn-app" }
cairn-contract = { path = "../cairn-contract" }
thiserror = { workspace = true }

[dev-dependencies]
cairn-infra = { path = "../cairn-infra" }
tempfile = { workspace = true }

[lints]
workspace = true
```
Add `"crates/cairn-service"` to the root `Cargo.toml` `members` list.

- [ ] **Step 2: Write the failing tests**

Create `crates/cairn-service/src/lib.rs`:
```rust
//! The transport-blind dispatcher: maps the wire contract to engine
//! use-cases and engine events to wire events. No I/O, no async.

use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    Command, CommandResponse, ContractError, Event as WireEvent, Query, QueryResponse,
};
use cairn_domain::NotePath;
use cairn_ports::{PortError, SearchIndex, VaultStore, Vcs};

/// Errors surfaced when dispatching a contract request.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// A requested note/resource was missing.
    #[error("note not found: {0}")]
    NotFound(String),
    /// The request was malformed (e.g. an invalid note path).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// An internal/adapter failure.
    #[error("{0}")]
    Internal(String),
}

impl From<PortError> for ServiceError {
    fn from(e: PortError) -> Self {
        match e {
            PortError::NotFound(s) => ServiceError::NotFound(s),
            PortError::Adapter(s) => ServiceError::Internal(s),
        }
    }
}

impl From<ServiceError> for ContractError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::NotFound(what) => ContractError::NotFound { what },
            ServiceError::InvalidRequest(message) => ContractError::InvalidRequest { message },
            ServiceError::Internal(message) => ContractError::Internal { message },
        }
    }
}

impl From<AppEvent> for WireEvent {
    fn from(e: AppEvent) -> Self {
        match e {
            AppEvent::NoteChanged(p) => WireEvent::NoteChanged { path: p.as_str().to_string() },
            AppEvent::NoteDeleted(p) => WireEvent::NoteDeleted { path: p.as_str().to_string() },
            AppEvent::Committed(commit) => WireEvent::Committed { commit },
            AppEvent::Reindexed(n) => WireEvent::Reindexed {
                count: u32::try_from(n).unwrap_or(u32::MAX),
            },
        }
    }
}

fn parse_path(raw: &str) -> Result<NotePath, ServiceError> {
    NotePath::new(raw).map_err(|e| ServiceError::InvalidRequest(e.to_string()))
}

/// Dispatch a mutating command, emitting produced events via `sink`.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
pub fn dispatch_command<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &mut Engine<S, I, V>,
    command: &Command,
    sink: &mut dyn EventSink,
) -> Result<CommandResponse, ServiceError> {
    match command {
        Command::WriteNote { path, contents } => {
            let p = parse_path(path)?;
            engine.write_note(&p, contents, sink)?;
            Ok(CommandResponse::Written)
        }
        Command::DeleteNote { path } => {
            let p = parse_path(path)?;
            engine.delete_note(&p, sink)?;
            Ok(CommandResponse::Written)
        }
        Command::Commit { message } => {
            let commit = engine.commit(message, sink)?;
            Ok(CommandResponse::Committed { commit })
        }
    }
}

/// Dispatch a read-only query.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
pub fn dispatch_query<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &Engine<S, I, V>,
    query: &Query,
) -> Result<QueryResponse, ServiceError> {
    match query {
        Query::GetNote { path } => {
            let p = parse_path(path)?;
            let contents = engine.read_note(&p)?;
            Ok(QueryResponse::Note { contents })
        }
        Query::Search { query } => {
            let paths = engine
                .search(query)?
                .into_iter()
                .map(|hit| hit.path.as_str().to_string())
                .collect();
            Ok(QueryResponse::Paths { paths })
        }
        Query::GetBacklinks { path } => {
            let p = parse_path(path)?;
            let paths = engine
                .backlinks(&p)?
                .into_iter()
                .map(|np| np.as_str().to_string())
                .collect();
            Ok(QueryResponse::Paths { paths })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};

    fn engine(dir: &std::path::Path) -> Engine<LocalFsStore, InMemoryIndex, GitVcs> {
        Engine::new(
            LocalFsStore::open(dir).unwrap(),
            InMemoryIndex::default(),
            GitVcs::open_or_init(dir).unwrap(),
        )
    }

    #[test]
    fn write_commit_and_query_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();

        let resp = dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "a.md".into(), contents: "the target [[b]]".into() },
            &mut sink,
        )
        .unwrap();
        assert_eq!(resp, CommandResponse::Written);

        dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "b.md".into(), contents: "second".into() },
            &mut sink,
        )
        .unwrap();

        let got = dispatch_query(&eng, &Query::GetNote { path: "a.md".into() }).unwrap();
        assert_eq!(got, QueryResponse::Note { contents: "the target [[b]]".into() });

        let search = dispatch_query(&eng, &Query::Search { query: "target".into() }).unwrap();
        assert_eq!(search, QueryResponse::Paths { paths: vec!["a.md".into()] });

        let backlinks =
            dispatch_query(&eng, &Query::GetBacklinks { path: "b.md".into() }).unwrap();
        assert_eq!(backlinks, QueryResponse::Paths { paths: vec!["a.md".into()] });

        let commit = dispatch_command(
            &mut eng,
            &Command::Commit { message: "first".into() },
            &mut sink,
        )
        .unwrap();
        assert!(matches!(commit, CommandResponse::Committed { .. }));
    }

    #[test]
    fn missing_note_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let eng = engine(tmp.path());
        let err = dispatch_query(&eng, &Query::GetNote { path: "missing.md".into() }).unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
        assert!(matches!(
            ContractError::from(err),
            ContractError::NotFound { .. }
        ));
    }

    #[test]
    fn invalid_path_is_invalid_request() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        let err = dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "../escape.md".into(), contents: "x".into() },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidRequest(_)));
    }

    #[test]
    fn app_event_maps_to_wire_event() {
        let p = NotePath::new("a.md").unwrap();
        assert_eq!(
            WireEvent::from(AppEvent::NoteChanged(p.clone())),
            WireEvent::NoteChanged { path: "a.md".into() }
        );
        assert_eq!(
            WireEvent::from(AppEvent::Reindexed(3)),
            WireEvent::Reindexed { count: 3 }
        );
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p cairn-service`
Expected: 4 tests PASS.

- [ ] **Step 4: Lint**

Run: `cargo clippy -p cairn-service --all-targets -- -D warnings`
Expected: clean. Run `cargo fmt --all` then `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(service): contract dispatcher + event/error mapping"
```

---

## Task 3: Retrofit the CLI onto the dispatcher

**Files:**
- Modify: `crates/cairn-cli/Cargo.toml`, `crates/cairn-cli/src/main.rs`

- [ ] **Step 1: Add deps**

In `crates/cairn-cli/Cargo.toml` `[dependencies]`, add:
```toml
cairn-contract = { path = "../cairn-contract" }
cairn-service = { path = "../cairn-service" }
```
(Keep the existing deps.)

- [ ] **Step 2: Rewrite `run()` to dispatch through the contract**

In `crates/cairn-cli/src/main.rs`, update the imports at the top to:
```rust
use cairn_app::{Engine, Event};
use cairn_contract::{Command as WireCommand, CommandResponse, Query as WireQuery, QueryResponse};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_service::{dispatch_command, dispatch_query};
use clap::{Parser, Subcommand};
```
(Remove the now-unused `cairn_domain::NotePath` import if present; `build_engine`, the `Cli`/`Command` clap structs, and `main` stay as they are.)

Replace the `match cli.command { ... }` block inside `run()` with:
```rust
    match cli.command {
        Command::Init => {
            println!("initialized cairn at {}", root.display());
        }
        Command::Write { path, contents } => {
            let resp = dispatch_command(
                &mut engine,
                &WireCommand::WriteNote { path: path.clone(), contents },
                &mut events,
            )
            .map_err(|e| e.to_string())?;
            debug_assert!(matches!(resp, CommandResponse::Written));
            println!("wrote {path}");
        }
        Command::Read { path } => {
            match dispatch_query(&engine, &WireQuery::GetNote { path }).map_err(|e| e.to_string())? {
                QueryResponse::Note { contents } => print!("{contents}"),
                QueryResponse::Paths { .. } => unreachable!("GetNote returns Note"),
            }
        }
        Command::Search { query } => {
            if let QueryResponse::Paths { paths } =
                dispatch_query(&engine, &WireQuery::Search { query }).map_err(|e| e.to_string())?
            {
                for p in paths {
                    println!("{p}");
                }
            }
        }
        Command::Backlinks { path } => {
            if let QueryResponse::Paths { paths } =
                dispatch_query(&engine, &WireQuery::GetBacklinks { path })
                    .map_err(|e| e.to_string())?
            {
                for p in paths {
                    println!("{p}");
                }
            }
        }
        Command::Commit { message } => {
            let resp = dispatch_command(&mut engine, &WireCommand::Commit { message }, &mut events)
                .map_err(|e| e.to_string())?;
            if let CommandResponse::Committed { commit } = resp {
                println!("committed {commit}");
            }
        }
    }
```
The surrounding `run()` lines — `let cli = Cli::parse();`, `let root = cli.cairn;`, the init-guard, `let mut events: Vec<Event> = Vec::new();`, `let mut engine = build_engine(&root)?;`, `engine.reindex(&mut events)...`, and the trailing `Ok(())` — stay unchanged.

- [ ] **Step 3: Run the existing CLI tests (behavior must be identical)**

Run: `cargo test -p cairn-cli`
Expected: all 4 existing integration tests still PASS (`write_search_backlinks_commit_flow`, `read_existing_note_prints_contents`, `read_missing_note_fails`, `commands_require_an_initialized_cairn`).

- [ ] **Step 4: Lint**

Run: `cargo clippy -p cairn-cli --all-targets -- -D warnings` and `cargo fmt --all -- --check`. Fix minimally (e.g. remove unused imports).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(cli): consume the contract via cairn-service"
```

---

## Task 4: `cairn-daemon` — state, router, HTTP handlers

**Files:**
- Create: `crates/cairn-daemon/Cargo.toml`, `crates/cairn-daemon/src/lib.rs`
- Create: `crates/cairn-daemon/tests/http.rs`
- Modify: root `Cargo.toml` (`members` + `[workspace.dependencies]`)

- [ ] **Step 1: Add workspace deps + create the crate**

In the root `Cargo.toml` `[workspace.dependencies]`, add:
```toml
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1", features = ["full"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
tokio-tungstenite = "0.24"
futures-util = "0.3"
```
Add `"crates/cairn-daemon"` to `members`.

Create `crates/cairn-daemon/Cargo.toml`:
```toml
[package]
name = "cairn-daemon"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "cairn-daemon"
path = "src/main.rs"

[lib]
path = "src/lib.rs"

[dependencies]
cairn-domain = { path = "../cairn-domain" }
cairn-app = { path = "../cairn-app" }
cairn-infra = { path = "../cairn-infra" }
cairn-contract = { path = "../cairn-contract" }
cairn-service = { path = "../cairn-service" }
axum = { workspace = true }
tokio = { workspace = true }
serde_json = { workspace = true }
clap = { workspace = true }

[dev-dependencies]
tower = { workspace = true }
http-body-util = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
tempfile = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Write the library (state + router + handlers)**

Create `crates/cairn-daemon/src/lib.rs`:
```rust
//! HTTP + WebSocket transport over the cairn dispatcher. Binds localhost
//! only; no authentication (LoopbackTrust). The engine runs synchronously
//! under a mutex via `spawn_blocking`.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{Command, CommandResponse, ContractError, Event as WireEvent, Query, QueryResponse};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_service::{dispatch_command, dispatch_query, ServiceError};
use tokio::sync::broadcast;

/// The concrete engine the daemon serves.
pub type CairnEngine = Engine<LocalFsStore, InMemoryIndex, GitVcs>;

/// Shared daemon state: the engine behind a mutex + an event broadcast.
#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<CairnEngine>>,
    events: broadcast::Sender<WireEvent>,
}

/// An `EventSink` that republishes engine events as wire events.
struct BroadcastSink(broadcast::Sender<WireEvent>);
impl EventSink for BroadcastSink {
    fn emit(&mut self, event: AppEvent) {
        // No subscribers is not an error.
        let _ = self.0.send(WireEvent::from(event));
    }
}

impl AppState {
    /// Build state from an engine.
    #[must_use]
    pub fn new(engine: CairnEngine) -> Self {
        let (events, _rx) = broadcast::channel(256);
        Self { engine: Arc::new(Mutex::new(engine)), events }
    }

    /// Run a command synchronously, publishing produced events. Shared by the
    /// HTTP handler and integration tests.
    ///
    /// # Errors
    /// Returns [`ServiceError`] on invalid input or engine failure.
    pub fn run_command_blocking(&self, command: &Command) -> Result<CommandResponse, ServiceError> {
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
        let mut sink = BroadcastSink(self.events.clone());
        dispatch_command(&mut guard, command, &mut sink)
    }

    /// Run a query synchronously.
    ///
    /// # Errors
    /// Returns [`ServiceError`] on invalid input or engine failure.
    pub fn run_query_blocking(&self, query: &Query) -> Result<QueryResponse, ServiceError> {
        let guard = self.engine.lock().expect("engine mutex poisoned");
        dispatch_query(&guard, query)
    }
}

fn status_for(err: &ServiceError) -> StatusCode {
    match err {
        ServiceError::NotFound(_) => StatusCode::NOT_FOUND,
        ServiceError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn command_handler(State(state): State<AppState>, Json(command): Json<Command>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_command_blocking(&command)).await;
    match result {
        Ok(Ok(resp)) => (StatusCode::OK, Json(resp)).into_response(),
        Ok(Err(svc)) => (status_for(&svc), Json(ContractError::from(svc))).into_response(),
        Err(join) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ContractError::Internal { message: join.to_string() }),
        )
            .into_response(),
    }
}

async fn query_handler(State(state): State<AppState>, Json(query): Json<Query>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_query_blocking(&query)).await;
    match result {
        Ok(Ok(resp)) => (StatusCode::OK, Json(resp)).into_response(),
        Ok(Err(svc)) => (status_for(&svc), Json(ContractError::from(svc))).into_response(),
        Err(join) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ContractError::Internal { message: join.to_string() }),
        )
            .into_response(),
    }
}

async fn events_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
}

async fn forward_events(mut socket: WebSocket, mut rx: broadcast::Receiver<WireEvent>) {
    loop {
        match rx.recv().await {
            Ok(ev) => {
                let Ok(text) = serde_json::to_string(&ev) else { continue };
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

/// Build the axum router for the given state.
#[must_use]
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route("/events", get(events_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}
```
(If axum 0.7's `Message::Text` or `axum::serve` API differs in the installed patch, adapt minimally — the behavior is what matters. `Message::Text` in axum 0.7 takes a `String`.)

- [ ] **Step 3: Write the HTTP integration test (no network, via `oneshot`)**

Create `crates/cairn-daemon/tests/http.rs`:
```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use http_body_util::BodyExt;
use tower::ServiceExt; // for `oneshot`

fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    AppState::new(engine)
}

async fn post_json(app: axum::Router, uri: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn write_then_search_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, body) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hello target"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "written");

    let (status, body) = post_json(
        app.clone(),
        "/query",
        serde_json::json!({"type":"search","query":"target"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "paths");
    assert_eq!(body["paths"][0], "a.md");
}

#[tokio::test]
async fn missing_note_query_is_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));
    let (status, body) = post_json(
        app,
        "/query",
        serde_json::json!({"type":"get_note","path":"missing.md"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], "not_found");
}
```

- [ ] **Step 4: Run the tests + lint**

Run: `cargo test -p cairn-daemon --test http`
Expected: 2 PASS. Then `cargo clippy -p cairn-daemon --all-targets -- -D warnings` and `cargo fmt --all -- --check`.
If `cargo build --locked` fails on Rust 1.85 due to a new transitive dep, pin it: `cargo update <crate> --precise <older-ver>` until `cargo build --locked --workspace` is green; commit the updated `Cargo.lock`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(daemon): axum state, router, and HTTP command/query handlers"
```

---

## Task 5: `cairn-daemon` — WebSocket events + the binary

**Files:**
- Create: `crates/cairn-daemon/src/main.rs`
- Create: `crates/cairn-daemon/tests/ws.rs`

- [ ] **Step 1: Write the binary**

Create `crates/cairn-daemon/src/main.rs`:
```rust
//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_daemon::{build_router, AppState, CairnEngine};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use clap::Parser;

#[derive(Parser)]
#[command(name = "cairn-daemon", about = "Serve a cairn over HTTP + WebSocket on localhost")]
struct Cli {
    /// Path to an existing, initialized cairn.
    #[arg(long, default_value = ".")]
    cairn: PathBuf,
    /// Port to bind on 127.0.0.1.
    #[arg(long, default_value_t = 7777)]
    port: u16,
}

fn build_engine(root: &Path) -> Result<CairnEngine, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    Ok(Engine::new(store, InMemoryIndex::default(), vcs))
}

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if !cli.cairn.join(".git").is_dir() {
        return Err(format!(
            "not a cairn at {0} (run `cairn --cairn {0} init` first)",
            cli.cairn.display()
        ));
    }
    let mut engine = build_engine(&cli.cairn)?;
    let mut startup: Vec<Event> = Vec::new();
    engine.reindex(&mut startup).map_err(|e| e.to_string())?;

    let app = build_router(AppState::new(engine));
    let addr = format!("127.0.0.1:{}", cli.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| e.to_string())?;
    println!("cairn-daemon listening on http://{addr}");
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 2: Write the WebSocket integration test**

Create `crates/cairn-daemon/tests/ws.rs`:
```rust
use cairn_app::Engine;
use cairn_contract::Command;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::test]
async fn write_command_pushes_event_over_websocket() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine);
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Connect the WS first so the broadcast subscription exists.
    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    // Give the server task a moment to register the subscriber.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger a command on the same state -> publishes events to the channel.
    state
        .run_command_blocking(&Command::WriteNote { path: "a.md".into(), contents: "hi".into() })
        .unwrap();

    // The first event should be note_changed for a.md.
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for event")
        .expect("websocket stream ended")
        .expect("websocket error");
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["type"], "note_changed");
    assert_eq!(json["path"], "a.md");
}
```
(If `tokio_tungstenite::connect_async` needs a `&str`/request type, or `Message::into_text` differs by version, adapt minimally — assert the JSON `type`/`path`.)

- [ ] **Step 3: Run the WS test**

Run: `cargo test -p cairn-daemon --test ws`
Expected: PASS. If flaky on the subscription race, increase the sleep to 250ms.

- [ ] **Step 4: Full workspace gate**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```
Expected: all green. Confirm the daemon binary runs: `cargo run -p cairn-daemon -- --help`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(daemon): WebSocket event stream + cairn-daemon binary"
```

---

## Task 6: ADR + handoff update

**Files:**
- Create: `docs/decisions/0002-transport.md`
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Write the ADR**

Create `docs/decisions/0002-transport.md`:
- Title `# ADR-0002: Transport — dispatcher + daemon, no Tauri in the engine`
- `**Status:** Accepted` / `**Date:** 2026-06-01`
- `## Context` — the contract existed but wasn't served (final review of ADR-0001); reference the spec `docs/superpowers/specs/2026-06-01-cairn-transport-design.md`.
- `## Decision` — `cairn-service` dispatcher (Command/Query → Engine, app::Event → contract::Event, ServiceError→ContractError); `cairn-daemon` HTTP (`/command`,`/query`,`/health`) + WS (`/events`) over `Arc<Mutex<Engine>>` + `tokio::broadcast`, run via `spawn_blocking`; binds 127.0.0.1 only, no auth (LoopbackTrust); the CLI now consumes the dispatcher. Explicitly: **Tauri is excluded from the engine repo** — it bundles a frontend and belongs in the UI session, which consumes `cairn-service` in-process or hits `cairn-daemon`.
- `## Consequences` — contract is now served + CLI-proven + network-reachable; browser/remote surfaces are unblocked; deferred: auth/TLS/network exposure, external-change event push (Watcher), Tauri shell. Note the axum/tokio MSRV pins if any were added.

- [ ] **Step 2: Update the handoff**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, update section 4 ("The one real gap") to state the gap is now CLOSED: the contract is served by `cairn-service` (in-process, used by the CLI) and `cairn-daemon` (HTTP `POST /command`,`/query` + WS `/events`). Add that the UI session wires Tauri by calling `cairn-service` in-process (desktop) or hitting `cairn-daemon` (browser/remote), and that `Query` responses are now defined (`QueryResponse`) along with typed `ContractError`. Add `CommandResponse`, `QueryResponse`, `ContractError` to the list of generated TS bindings.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: ADR-0002 transport + handoff update (contract now served)"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** §2 DTOs → Task 1; §3 dispatcher (`dispatch_command`/`dispatch_query`/`From`/`ServiceError`) → Task 2; §5 CLI retrofit → Task 3; §4 daemon (state, routes, spawn_blocking, broadcast, WS, loopback bind) → Tasks 4–5; §6 deps/members → Tasks 2/4; §7 tests → Tasks 2/4/5; §8 docs → Task 6. MSRV risk (§6) → pin steps in Tasks 4/5.
- **Type consistency:** `ServiceError::{NotFound,InvalidRequest,Internal}` used consistently across service, daemon `status_for`, and `ContractError` mapping; `CommandResponse::{Written,Committed}`, `QueryResponse::{Note,Paths}`, `ContractError::{NotFound,InvalidRequest,Internal}` match the contract DTOs in Task 1; `AppState::{new,run_command_blocking,run_query_blocking}` used by handlers (Task 4) and the WS test (Task 5); `build_router`/`CairnEngine` used by the binary and tests.
- **Placeholder scan:** no TBD/TODO; every code step is complete. Task 6's ADR/handoff steps describe prose to write (not code), which is acceptable.
- **Known external-API caveats flagged:** ts-rs (`decl`/`export_all`), axum 0.7 (`Message::Text(String)`, `axum::serve`), tokio-tungstenite (`connect_async`/`into_text`), and the broadcast subscription race (sleep) all carry adaptation notes.
```
