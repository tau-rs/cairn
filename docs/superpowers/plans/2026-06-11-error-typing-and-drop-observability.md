# Error Typing & Drop Observability Implementation Plan

> **For agentic workers:** implement task-by-task with TDD. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Preserve typed error structure (`#[source]`) at the port boundary (D3), and make the daemon/engine's intentionally best-effort drop/panic paths leave a trace (G4, G5).

**Architecture:** Introduce `cairn_ports::AdapterError` — a message + optional typed `#[source]` — carried by `PortError::Adapter` and `ServiceError::Internal` so a `git2::Error` / `io::Error` survives (downcastable) to the edges while Display stays byte-identical. Surface per-plugin event-handler errors from `PluginHost::dispatch_event` so `Engine::dispatch_plugin_event` can log them via `tracing`. Add `tracing::warn!/error!` on WS lag-drop, WS serialize-failure, and `spawn_blocking` `JoinError`.

**Tech Stack:** Rust, thiserror, tracing, tracing-test.

---

## Task 1: `AdapterError` typed-source carrier (D3, ports)

**Files:** Modify `crates/cairn-ports/src/lib.rs`

- [ ] Add `AdapterError { message: String, source: Option<Box<dyn Error + Send + Sync>> }`,
  `#[error("{message}")]`, with `new(source)` (typed), `message(msg)`, `From<String>`, `From<&str>`.
- [ ] Change `Adapter(String)` → `#[error(transparent)] Adapter(AdapterError)`.
- [ ] Add `EventDispatchError { plugin: String, error: PortError }`.
- [ ] Change `PluginHost::dispatch_event` default to `-> Vec<EventDispatchError> { Vec::new() }`.
- [ ] Test: a wrapped `io::Error` round-trips its `ErrorKind` via `PortError::source().downcast_ref::<io::Error>()`.

## Task 2: Adapter boundaries preserve source (D3, infra + app)

**Files:** `crates/cairn-infra/src/{git,localfs,tantivy_index,plugin_host,notify_watcher,seams}.rs`, `crates/cairn-app/src/lib.rs`

- [ ] `adapt` helpers → `PortError::Adapter(AdapterError::new(e))` (generic over `Error + Send + Sync + 'static`).
- [ ] `.map_err(|e| PortError::Adapter(e.to_string()))` → `.map_err(adapt)` (add `adapt` to localfs).
- [ ] `PortError::Adapter(format!(...))` → `...format!(...).into()`; `Adapter("..".into())` unchanged (From<&str>).
- [ ] cairn-app:237 state-serialize → keep message-only via `.into()`.
- [ ] Fix `matches!(&err, PortError::Adapter(m) if m.contains(..))` test → `m.to_string().contains(..)`.

## Task 3: `ServiceError::Internal` carries source (D3, service)

**Files:** `crates/cairn-service/src/lib.rs`

- [ ] `Internal(String)` → `#[error(transparent)] Internal(cairn_ports::AdapterError)`.
- [ ] `From<PortError>`: `Adapter(a) => Internal(a)`. `From<ServiceError> for ContractError`: `Internal(a) => Internal { message: a.to_string() }` (flatten only at wire).
- [ ] `Internal("boom".into())` test unchanged (From<&str>).

## Task 4: `dispatch_event` reports handler errors (G4, infra + app)

**Files:** `crates/cairn-infra/src/plugin_host.rs`, `crates/cairn-app/src/lib.rs`, deps

- [ ] cairn-app/Cargo.toml: add `tracing` dep, `tracing-test` dev-dep.
- [ ] ProcessPluginHost::dispatch_event returns `Vec<EventDispatchError>` (push instead of `eprintln!`).
- [ ] Engine::dispatch_plugin_event: log each returned error `tracing::warn!`, panic → `tracing::error!` (replaces `eprintln!`).
- [ ] Test (`#[traced_test]`): a `FailingEventHost` returning an `EventDispatchError` → `logs_contain("plugin event handler failed")`.

## Task 5: WS drop + JoinError observability (G4, G5, daemon)

**Files:** `crates/cairn-daemon/src/lib.rs`, `crates/cairn-daemon/tests/`

- [ ] Extract `ws_event_action(Result<WireEvent, RecvError>) -> WsForward` with warn on Lagged + serialize-fail.
- [ ] `service_response` `Err(join)` arm → `tracing::error!(error=%join, "request worker panicked")` before generic 500.
- [ ] Test (`#[traced_test]`): `ws_event_action(Err(Lagged(n)))` → Skip + `logs_contain`; forced `spawn_blocking` panic → `service_response` 500 + `logs_contain("request worker panicked")`.
