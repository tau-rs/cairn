# WebSocket `/events` Origin Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reject WebSocket upgrades on `/events` whose `Origin` is missing or not in the daemon's configured allowlist, closing the cross-origin event-stream leak (audit finding S2).

**Architecture:** The CORS allowlist (`cors_origins`, computed in `main.rs` from `cairn.toml` ∪ `--cors-origin`) becomes a single source of truth shared with the WS handler. `AppState` gains an `allowed_origins` field set via a builder; `events_handler` extracts the `Origin` request header and returns `403 FORBIDDEN` before `ws.on_upgrade` unless the origin is present *and* in the allowlist (same deny-by-default policy the CORS layer enforces). Browsers always send `Origin` on WS handshakes, so a missing `Origin` is rejected too.

**Tech Stack:** Rust, axum (`WebSocketUpgrade`, `HeaderMap`), tokio, `tokio-tungstenite` (test client).

---

### Task 1: Thread the origin allowlist into `AppState`

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (`AppState` struct, `AppState::new`, add builder)
- Modify: `crates/cairn-daemon/src/main.rs:110-123` (set origins on state)

- [ ] **Step 1: Add an `allowed_origins` field + builder to `AppState`**

In `crates/cairn-daemon/src/lib.rs`, extend the struct and constructor. Store as `Arc<[String]>` so cloning `AppState` (it derives `Clone`, cloned per connection) stays cheap.

```rust
/// Shared daemon state: the engine behind a mutex + an event broadcast.
#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<CairnEngine>>,
    events: broadcast::Sender<WireEvent>,
    /// Origins permitted to open the `/events` WebSocket. Same allowlist the
    /// CORS layer enforces; empty denies all (deny-by-default).
    allowed_origins: Arc<[String]>,
}
```

In `AppState::new`, initialize it empty:

```rust
        Self {
            engine: Arc::new(Mutex::new(engine)),
            events,
            allowed_origins: Arc::from([]),
        }
```

Add a builder method (place it right after `new`):

```rust
    /// Set the origins permitted to open the `/events` WebSocket. Reuse the
    /// daemon's CORS allowlist so HTTP and WS share one origin policy.
    #[must_use]
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = Arc::from(origins.into_boxed_slice());
        self
    }
```

- [ ] **Step 2: Wire it in `main.rs`**

In `crates/cairn-daemon/src/main.rs`, after `cors_origins` is computed and before/around the existing `let app = ...`, build state with the allowlist. Replace `let state = AppState::new(engine);` (line 110) so the WS check sees the same list. Note `cors_origins` is computed *after* the current `AppState::new` line, so move the state construction below the merge, or clone the origins. Concretely, change line 110 from:

```rust
    let state = AppState::new(engine);
```

to keep `engine` and defer state creation until after `cors_origins` exists. After the `cors_origins` block (the `if cors_origins.is_empty() { ... } else { ... }`), construct:

```rust
    let state = AppState::new(engine).with_allowed_origins(cors_origins.clone());
```

and leave `let app = build_router(state.clone()).layer(cors_layer(&cors_origins));` as is. Verify `engine` is still in scope (it is — it's only consumed by `AppState::new`).

- [ ] **Step 3: Compile**

Run: `cargo build -p cairn-daemon`
Expected: builds (the watch loop and serve code already use `state.clone()`).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-daemon/src/lib.rs crates/cairn-daemon/src/main.rs
git commit -m "refactor(daemon): thread CORS allowlist into AppState for WS reuse"
```

---

### Task 2: Reject disallowed/missing Origin on the WS upgrade (TDD)

**Files:**
- Test: `crates/cairn-daemon/tests/ws.rs` (add rejection + permitted-origin tests; fix existing test to send an Origin)
- Modify: `crates/cairn-daemon/src/lib.rs:183-186` (`events_handler`) + a helper

- [ ] **Step 1: Write the failing tests**

In `crates/cairn-daemon/tests/ws.rs`, add a helper to build a WS request carrying an `Origin`, and two tests. Add imports at the top:

```rust
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;
```

Add a helper that spawns a server whose state has a fixed allowlist and returns its address:

```rust
async fn serve_with_origins(origins: Vec<String>) -> std::net::SocketAddr {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    // Keep tmp alive for the duration of the server task.
    let state = AppState::new(engine).with_allowed_origins(origins);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
        drop(tmp); // hold the tempdir until the server stops
    });
    addr
}

fn ws_request(addr: std::net::SocketAddr, origin: Option<&str>) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut req = format!("ws://{addr}/events").into_client_request().unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    req
}
```

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_origin_is_rejected() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let req = ws_request(addr, Some("http://evil.example"));
    let err = tokio_tungstenite::connect_async(req)
        .await
        .expect_err("foreign origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected HTTP 403, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_origin_is_rejected() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let req = ws_request(addr, None);
    let err = tokio_tungstenite::connect_async(req)
        .await
        .expect_err("missing origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected HTTP 403, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permitted_origin_upgrades() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let req = ws_request(addr, Some("http://localhost:5173"));
    let (_ws, resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("permitted origin must upgrade");
    assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
}
```

Also fix the existing `write_command_pushes_event_over_websocket` test: it builds `AppState::new(engine)` (empty allowlist) and connects with no `Origin`, which now gets rejected. Change its state to allow an origin and connect with it. Replace its `let state = AppState::new(engine);` with:

```rust
    let state = AppState::new(engine).with_allowed_origins(vec!["http://localhost:5173".to_string()]);
```

and replace its `connect_async(format!("ws://{addr}/events"))` call with:

```rust
    let req = ws_request(addr, Some("http://localhost:5173"));
    let (mut ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .unwrap();
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-daemon --test ws`
Expected: `foreign_origin_is_rejected` and `missing_origin_is_rejected` FAIL (the handler currently upgrades unconditionally, so `connect_async` succeeds instead of erroring). `permitted_origin_upgrades` passes.

- [ ] **Step 3: Implement the Origin check in `events_handler`**

In `crates/cairn-daemon/src/lib.rs`, extend the imports to bring in `HeaderMap` and the `ORIGIN` header name, and rewrite `events_handler` plus add a helper. Update the axum import block (`http`) to:

```rust
    http::{header, HeaderMap, StatusCode},
```

Rewrite the handler:

```rust
/// True if `origin` (the request's `Origin` header value) is present and in the
/// allowlist. Browsers always send `Origin` on a WS handshake; a missing or
/// non-UTF-8 header is treated as disallowed (deny-by-default, mirroring CORS).
fn ws_origin_allowed(allowed: &[String], origin: Option<&axum::http::HeaderValue>) -> bool {
    match origin.and_then(|o| o.to_str().ok()) {
        Some(value) => allowed.iter().any(|a| a == value),
        None => false,
    }
}

async fn events_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Browsers do not apply CORS to WebSocket upgrades, so validate Origin here
    // against the same allowlist (audit S2). Reject before upgrading.
    if !ws_origin_allowed(&state.allowed_origins, headers.get(header::ORIGIN)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
}
```

Note: `HeaderMap` is a non-body extractor, so it may precede `WebSocketUpgrade` (which consumes the request) in the argument list — axum requires the body-consuming extractor last.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-daemon --test ws`
Expected: all four tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/ws.rs
git commit -m "fix(daemon): validate Origin on /events WS upgrade (audit S2)"
```

---

### Task 3: Correct the stale ADR claim

**Files:**
- Modify: `docs/decisions/0004-daemon-cors.md` (the "WebSocket `/events` needs no CORS config" bullet)

- [ ] **Step 1: Update the misleading consequence**

The audit cites ADR-0004 as asserting "CORS is the only gate" and the consequences list says `/events` "needs no CORS config". Replace that bullet to record that `/events` now validates `Origin` against the same allowlist (browsers bypass CORS on WS). Replace the bullet beginning `**WebSocket `/events` needs no CORS config.**` with:

```markdown
- **WebSocket `/events` validates `Origin` directly.** Browsers do not apply
  CORS to WebSocket upgrades, so the daemon checks the `Origin` header against
  the same allowlist inside `events_handler` and rejects (HTTP 403) a missing or
  non-allowlisted origin before upgrading. The UI's origin must be allowlisted
  (via `cairn.toml` or `--cors-origin`) for the event stream just as for HTTP.
```

- [ ] **Step 2: Commit**

```bash
git add docs/decisions/0004-daemon-cors.md
git commit -m "docs(adr-0004): /events now validates Origin (audit S2)"
```

---

### Task 4: Full verification

- [ ] **Step 1: Run the whole daemon test suite + lints**

Run: `cargo test -p cairn-daemon && cargo clippy -p cairn-daemon --all-targets -- -D warnings`
Expected: all tests green, no clippy warnings.
