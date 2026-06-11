# Daemon local bearer-token auth — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate the daemon's `/command` and `/query` routes behind a local bearer token written `0600` to `<cairn>/.cairn/token`, closing audit finding S5 (daemon has no authentication).

**Architecture:** A new `auth` module in `cairn-daemon` generates a fresh 64-hex-char token each startup, writes it `0600`, and provides an axum middleware that constant-time-compares an `Authorization: Bearer <token>` header. `AppState` gains an optional token (`None` = auth off, for the in-process/library/test default); the binary always sets one. `/health` stays open; `/events` keeps its existing Origin gate (S2) untouched.

**Tech Stack:** Rust 1.88, axum 0.8 (`middleware::from_fn_with_state`, `route_layer`), `getrandom` 0.3, tokio.

**Spec:** `docs/superpowers/specs/2026-06-11-daemon-auth-design.md`

---

## File structure

- `Cargo.toml` (workspace root) — add `getrandom` to `[workspace.dependencies]`.
- `crates/cairn-daemon/Cargo.toml` — depend on `getrandom`.
- `crates/cairn-daemon/src/auth.rs` (**new**) — token generation, `0600` writer, `ct_eq`, `bearer_matches`, `require_token` middleware. One responsibility: authentication.
- `crates/cairn-daemon/src/lib.rs` (modify) — `mod auth;`, re-export `generate_token_file`, add `token` field + `with_token` builder to `AppState`, split `build_router` so only `/command`+`/query` carry the auth layer, update the module doc.
- `crates/cairn-daemon/src/main.rs` (modify) — generate the token at startup, print its location, pass it to `AppState`.
- `crates/cairn-daemon/tests/auth.rs` (**new**) — HTTP integration tests for the gate.
- `docs/decisions/0010-daemon-auth.md` (**new**) — ADR.
- `README.md` (modify) — "Daemon trust model" note.

---

## Task 1: Token file generation (`auth` module + `getrandom`)

**Files:**
- Modify: `Cargo.toml` (workspace root, `[workspace.dependencies]`)
- Modify: `crates/cairn-daemon/Cargo.toml`
- Create: `crates/cairn-daemon/src/auth.rs`
- Modify: `crates/cairn-daemon/src/lib.rs` (add `mod auth;` + re-export)

- [ ] **Step 1: Add the `getrandom` dependency**

In the workspace root `Cargo.toml`, under `[workspace.dependencies]`, add (alphabetical-ish, next to the other small crates):

```toml
getrandom = "0.3"
```

In `crates/cairn-daemon/Cargo.toml`, under `[dependencies]`, add after `toml = { workspace = true }`:

```toml
getrandom = { workspace = true }
```

- [ ] **Step 2: Write the failing unit test for the token writer**

Create `crates/cairn-daemon/src/auth.rs` with only the test module first:

```rust
//! Local bearer-token authentication for the daemon (audit S5). The token is a
//! file under `<cairn>/.cairn/token` (mode `0600`); holding it is equivalent to
//! having read access to that file, i.e. being the cairn's owner.

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600_and_64_hex() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let tok = generate_token_file(tmp.path()).unwrap();
        assert_eq!(tok.len(), 64);
        assert!(tok.bytes().all(|b| b.is_ascii_hexdigit()));
        let meta = std::fs::metadata(tmp.path().join(".cairn").join("token")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}
```

In `crates/cairn-daemon/src/lib.rs`, add the module declaration directly under `pub mod config;` / `pub use config::Config;` (top of the file, around line 5-6):

```rust
mod auth;
pub use auth::generate_token_file;
```

- [ ] **Step 3: Run the test to verify it fails to compile**

Run: `cargo test -p cairn-daemon --lib auth::`
Expected: FAIL — `cannot find function 'generate_token_file'` (and the `pub use` in lib.rs fails to resolve).

- [ ] **Step 4: Implement the token generator**

At the **top** of `crates/cairn-daemon/src/auth.rs` (above the `#[cfg(test)]` module), add:

```rust
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Generate a fresh 64-char lowercase-hex bearer token, write it to
/// `<cairn_root>/.cairn/token` with mode `0600` (truncating any prior token),
/// and return it. Creates the `.cairn` directory if absent.
///
/// # Errors
/// Returns an error if the OS RNG is unavailable or the file cannot be written.
pub fn generate_token_file(cairn_root: &Path) -> io::Result<String> {
    let token = random_hex_32()?;
    let dir = cairn_root.join(".cairn");
    fs::create_dir_all(&dir)?;
    write_secret_file(&dir.join("token"), &token)?;
    Ok(token)
}

/// 32 cryptographically-random bytes, lowercase-hex encoded (64 chars).
fn random_hex_32() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut hex = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        // Writing to a String is infallible.
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

/// Write `contents` to `path`, owner-read/write only.
#[cfg(unix)]
fn write_secret_file(path: &Path, contents: &str) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // Enforce 0600 even if the file pre-existed with looser permissions
    // (`.mode()` only applies when the file is newly created).
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    f.write_all(contents.as_bytes())
}

/// Non-Unix fallback: best-effort write with no permission guarantee (noted in
/// the trust-model docs).
#[cfg(not(unix))]
fn write_secret_file(path: &Path, contents: &str) -> io::Result<()> {
    fs::write(path, contents)
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p cairn-daemon --lib auth::`
Expected: PASS — `token_file_is_0600_and_64_hex ... ok`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/cairn-daemon/Cargo.toml crates/cairn-daemon/src/auth.rs crates/cairn-daemon/src/lib.rs
git commit -m "feat(daemon): generate 0600 bearer-token file (audit S5)"
```

---

## Task 2: `AppState` token field + bearer/constant-time helpers

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (`AppState` field, `new`, `with_token`)
- Modify: `crates/cairn-daemon/src/auth.rs` (`ct_eq`, `bearer_matches` + unit tests)

- [ ] **Step 1: Write the failing unit tests for the header helpers**

In `crates/cairn-daemon/src/auth.rs`, replace the existing `#[cfg(test)] mod tests { ... }` block with this expanded version (keeps the file-writer test, adds the helper tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    fn headers_with(auth: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_str(auth).unwrap());
        h
    }

    #[test]
    fn ct_eq_matches_and_rejects() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab")); // differing length
    }

    #[test]
    fn bearer_matches_accepts_correct_token() {
        assert!(bearer_matches(&headers_with("Bearer secret"), "secret"));
    }

    #[test]
    fn bearer_matches_rejects_wrong_scheme_value_and_missing() {
        assert!(!bearer_matches(&headers_with("Bearer nope"), "secret"));
        assert!(!bearer_matches(&headers_with("Basic secret"), "secret"));
        assert!(!bearer_matches(&HeaderMap::new(), "secret"));
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600_and_64_hex() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let tok = generate_token_file(tmp.path()).unwrap();
        assert_eq!(tok.len(), 64);
        assert!(tok.bytes().all(|b| b.is_ascii_hexdigit()));
        let meta = std::fs::metadata(tmp.path().join(".cairn").join("token")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}
```

- [ ] **Step 2: Run the helper tests to verify they fail**

Run: `cargo test -p cairn-daemon --lib auth::`
Expected: FAIL — `cannot find function 'ct_eq'` / `'bearer_matches'`.

- [ ] **Step 3: Implement `ct_eq` and `bearer_matches`**

In `crates/cairn-daemon/src/auth.rs`, add these imports to the top `use` block:

```rust
use axum::http::{header, HeaderMap};
```

Then add the two helpers (above the `#[cfg(test)]` module):

```rust
/// True if `headers` carry `Authorization: Bearer <token>` whose token equals
/// `expected`. Missing, non-UTF-8, or non-`Bearer` headers are rejected
/// (deny-by-default, mirroring the CORS/Origin gates).
pub(crate) fn bearer_matches(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    ct_eq(token.as_bytes(), expected.as_bytes())
}

/// Constant-time byte comparison. The length check leaks only the token length,
/// which is fixed and public; the value comparison itself is timing-independent.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
```

- [ ] **Step 4: Run the helper tests to verify they pass**

Run: `cargo test -p cairn-daemon --lib auth::`
Expected: PASS — all four `auth::tests::*` pass.

- [ ] **Step 5: Add the `token` field and `with_token` builder to `AppState`**

In `crates/cairn-daemon/src/lib.rs`, add the field to the `AppState` struct (after `allowed_origins`):

```rust
    /// Bearer token required on `/command` and `/query`. `None` disables auth
    /// (the in-process/library/test default); the `cairn-daemon` binary always
    /// sets a token via [`AppState::with_token`].
    token: Option<Arc<str>>,
```

In `AppState::new`, add `token: None,` to the returned struct literal (after `allowed_origins: Arc::from([]),`).

Add the builder method inside `impl AppState`, directly after `with_allowed_origins`:

```rust
    /// Require this bearer token on `/command` and `/query`. Reuse the same
    /// optional-builder shape as [`AppState::with_allowed_origins`].
    #[must_use]
    pub fn with_token(mut self, token: impl Into<Arc<str>>) -> Self {
        self.token = Some(token.into());
        self
    }
```

- [ ] **Step 6: Verify the crate still compiles and all tests pass**

Run: `cargo test -p cairn-daemon`
Expected: PASS — existing `http.rs`, `cors.rs`, `events.rs`, `ws.rs`, `watch.rs`, config, and `auth::tests` all green (the new field defaults to `None`, so behavior is unchanged so far).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-daemon/src/auth.rs crates/cairn-daemon/src/lib.rs
git commit -m "feat(daemon): AppState bearer token field + header match helpers"
```

---

## Task 3: Wire the auth middleware into the router

**Files:**
- Modify: `crates/cairn-daemon/src/auth.rs` (`require_token` middleware)
- Modify: `crates/cairn-daemon/src/lib.rs` (`build_router` split, module doc)
- Create: `crates/cairn-daemon/tests/auth.rs`

- [ ] **Step 1: Write the failing integration tests**

Create `crates/cairn-daemon/tests/auth.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use tower::ServiceExt; // for `oneshot`

const TOKEN: &str = "test-token-abc123";

fn app(dir: &std::path::Path) -> axum::Router {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    build_router(AppState::new(engine).with_token(TOKEN))
}

fn write_command(auth: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/command")
        .header("content-type", "application/json");
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    b.body(Body::from(
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}).to_string(),
    ))
    .unwrap()
}

#[tokio::test]
async fn no_token_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path()).oneshot(write_command(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn correct_token_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path())
        .oneshot(write_command(Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn wrong_token_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path())
        .oneshot(write_command(Some("nope")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn malformed_auth_header_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/command")
        .header("content-type", "application/json")
        .header("authorization", format!("Basic {TOKEN}"))
        .body(Body::from(
            serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}).to_string(),
        ))
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_also_requires_token() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"type":"list_notes"}).to_string(),
        ))
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_is_open_without_token() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run the integration tests to verify the gate is missing**

Run: `cargo test -p cairn-daemon --test auth`
Expected: FAIL — `no_token_is_401`, `wrong_token_is_401`, `malformed_auth_header_is_401`, `query_also_requires_token` all report `200 OK` instead of `401` (no middleware wired yet). `correct_token_succeeds` and `health_is_open_without_token` already pass.

- [ ] **Step 3: Implement the `require_token` middleware**

In `crates/cairn-daemon/src/auth.rs`, extend the top `use` block so it reads:

```rust
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::AppState;
```

Then add the middleware (above the `#[cfg(test)]` module):

```rust
/// axum middleware: when the daemon was configured with a token, reject any
/// request that lacks a matching `Authorization: Bearer <token>` header with
/// `401`. With no token configured, every request passes through.
pub(crate) async fn require_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(expected) = &state.token {
        if !bearer_matches(req.headers(), expected) {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
            )
                .into_response();
        }
    }
    next.run(req).await
}
```

- [ ] **Step 4: Split `build_router` so only `/command` and `/query` carry the layer**

In `crates/cairn-daemon/src/lib.rs`, replace the entire `build_router` function:

```rust
/// Build the axum router for the given state.
///
/// `/command` and `/query` require the bearer token (audit S5). `/health` is an
/// open liveness probe; `/events` keeps its own Origin gate (audit S2) and is
/// not token-gated in this increment.
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));
    let open = Router::new()
        .route("/events", get(events_handler))
        .route("/health", get(health_handler));
    protected.merge(open).with_state(state)
}
```

- [ ] **Step 5: Update the module doc to drop the "no authentication" claim**

In `crates/cairn-daemon/src/lib.rs`, replace the top-of-file module doc (lines 1-3):

```rust
//! HTTP + WebSocket transport over the cairn dispatcher. Binds localhost only.
//! `/command` and `/query` require a local bearer token (audit S5; see the
//! `auth` module and [`AppState::with_token`]). The engine runs synchronously
//! under a mutex via `spawn_blocking`.
```

- [ ] **Step 6: Run the full daemon test suite**

Run: `cargo test -p cairn-daemon`
Expected: PASS — all six `auth.rs` integration tests, all `auth::tests` unit tests, and every pre-existing test green.

- [ ] **Step 7: Lint**

Run: `cargo clippy -p cairn-daemon --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-daemon/src/auth.rs crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/auth.rs
git commit -m "feat(daemon): gate /command and /query behind bearer token (audit S5)"
```

---

## Task 4: Wire token generation into the binary

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs`

- [ ] **Step 1: Generate the token at startup and pass it to the state**

In `crates/cairn-daemon/src/main.rs`, find this block (currently around lines 130-133):

```rust
    // The same allowlist gates the /events WS upgrade (browsers bypass CORS on
    // WebSocket handshakes; see events_handler).
    let state = AppState::new(engine).with_allowed_origins(cors_origins.clone());
    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));
```

Replace it with:

```rust
    // Local bearer token: written to <cairn>/.cairn/token (mode 0600) and
    // regenerated each startup. Any client with filesystem access to the cairn
    // reads it and sends `Authorization: Bearer <token>` (audit S5). A write
    // failure is fatal — the daemon never serves unauthenticated.
    let token = cairn_daemon::generate_token_file(&cli.cairn)
        .map_err(|e| format!("write daemon token: {e}"))?;
    println!(
        "auth: bearer token at {}/.cairn/token (clients read this file)",
        cli.cairn.display()
    );

    // The same allowlist gates the /events WS upgrade (browsers bypass CORS on
    // WebSocket handshakes; see events_handler).
    let state = AppState::new(engine)
        .with_allowed_origins(cors_origins.clone())
        .with_token(token);
    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));
```

- [ ] **Step 2: Build the binary**

Run: `cargo build -p cairn-daemon`
Expected: builds clean.

- [ ] **Step 3: Manual smoke test — token is required end-to-end**

```bash
# Fresh cairn + daemon in the background.
TMP=$(mktemp -d)
cargo run -p cairn-cli -- --cairn "$TMP" init
cargo run -p cairn-daemon -- --cairn "$TMP" --port 7799 --no-watch &
DAEMON=$!
sleep 2

# 0600 token file exists:
ls -l "$TMP/.cairn/token"           # expect: -rw------- ... (owner rw only)

# No token -> 401:
curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST http://127.0.0.1:7799/command \
  -H 'content-type: application/json' \
  -d '{"type":"list_notes"}'        # expect: 401

# Correct token -> 200:
TOK=$(cat "$TMP/.cairn/token")
curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST http://127.0.0.1:7799/query \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $TOK" \
  -d '{"type":"list_notes"}'        # expect: 200

# Health is open:
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:7799/health  # expect: 200

kill "$DAEMON"
```

Expected printed codes, in order: file listed with `-rw-------`, `401`, `200`, `200`.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): write + require bearer token in the binary (audit S5)"
```

---

## Task 5: Document the trust model (ADR + README)

**Files:**
- Create: `docs/decisions/0010-daemon-auth.md`
- Modify: `README.md`

- [ ] **Step 1: Write the ADR**

Create `docs/decisions/0010-daemon-auth.md`:

```markdown
# ADR-0010: Daemon authentication — local bearer token

**Status:** Accepted
**Date:** 2026-06-11

## Context

The daemon binds `127.0.0.1` and, until now, authenticated nothing. CORS
(ADR-0004) is enforced by the browser and so constrains only browser clients;
any local process — or any other user on a multi-user host — could `POST
/command` to write/delete/rename/commit notes (reaching code execution via the
plugin findings) or `POST /query` to read the whole cairn. This is audit
finding S5.

The design for this increment is specified in
`docs/superpowers/specs/2026-06-11-daemon-auth-design.md`.

## Decision

### Local bearer token

`/command` and `/query` require an `Authorization: Bearer <token>` header. The
token is 32 cryptographically-random bytes, hex-encoded (64 chars), written to
`<cairn>/.cairn/token` with mode `0600` and regenerated on every startup. The
comparison is constant-time.

We chose a bearer token over a Unix domain socket because it is the smallest
change that fits the existing TCP + CORS + `AppState`-builder architecture, stays
cross-platform, and keeps a future browser UI working through a standard header.

### Trust model

The credential *is* a `0600` file. "Can call the daemon" therefore collapses to
"can read `<cairn>/.cairn/token`" — on a multi-user host, the cairn's owner only.
A request from another user or any process without read access to that file is
rejected with `401`. On non-Unix platforms the `0600` guarantee does not hold;
the token still gates access but the filesystem permission story is weaker.

### Scope

- `/health` stays open (a contentless liveness probe).
- `/events` keeps its Origin gate (ADR-0004 / audit S2) and is **not** token-gated
  in this increment.
- `AppState::new` is token-less by default (`None` = auth off); only the
  `cairn-daemon` binary sets a token, so in-process/library embedding and the
  handler tests are unaffected.

## Consequences

### What this enables

- A non-browser local actor without read access to `.cairn/token` can no longer
  drive the daemon.
- The token sits alongside the CORS allowlist using the same optional-builder
  pattern, so the change is additive and the existing tests are untouched.

### Accepted limitations and deferred increments

- **Unix domain socket transport** — a future alternative that would also drop
  the loopback TCP exposure.
- **Token-gating `/events`** — defense in depth; the WS keeps its Origin gate
  for now.
- **Persistent / rotatable tokens** — today the token is ephemeral per run.
- **Browser-UI token delivery** — how a served web UI obtains the token (it
  cannot read the local filesystem) is a separate sub-project.
- **Non-Unix permissions** — no `0600` equivalent is enforced off Unix.
```

- [ ] **Step 2: Add a trust-model note to the README**

In `README.md`, add this section immediately **before** the `## Vocabulary` section:

```markdown
## Daemon trust model

`cairn-daemon` binds `127.0.0.1` only. Its `/command` and `/query` routes
require a local bearer token: on startup the daemon writes a random token to
`<cairn>/.cairn/token` (mode `0600`) and requires it as an
`Authorization: Bearer <token>` header. Any client with filesystem access to the
cairn reads that file; on a multi-user host the `0600` permissions restrict that
to the cairn's owner, so another local user cannot drive the daemon. The token
is regenerated each startup.

`/health` is an open liveness probe. The `/events` WebSocket is gated by an
Origin allowlist (see [`docs/decisions/0004-daemon-cors.md`](docs/decisions/0004-daemon-cors.md));
cross-origin browser access to the daemon is governed by the same CORS
allowlist. See [`docs/decisions/0010-daemon-auth.md`](docs/decisions/0010-daemon-auth.md)
for the authentication design and its deferred increments (Unix-socket
transport, token-gated events, the browser-UI token channel).
```

- [ ] **Step 3: Verify the whole workspace still builds and tests pass**

Run: `cargo test --workspace`
Expected: PASS across the workspace.

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0010-daemon-auth.md README.md
git commit -m "docs(daemon): document bearer-token trust model (ADR-0010, audit S5)"
```

---

## Self-review notes

- **Spec coverage:** bearer token (Tasks 1-4), `0600` file + regenerate-each-startup (Task 1, Task 4), `Option<Arc<str>>` default-off + `with_token` (Task 2), constant-time `Bearer` match (Task 2), middleware on `/command`+`/query` only with `/health` open and `/events` untouched (Task 3), fail-closed binary wiring + module-doc fix (Tasks 3-4), tests incl. failing-first `no_token_is_401` (Task 3), ADR + README + deferred list (Task 5). All spec sections map to a task.
- **Type consistency:** `generate_token_file(&Path) -> io::Result<String>`, `with_token(impl Into<Arc<str>>)`, `token: Option<Arc<str>>`, `bearer_matches(&HeaderMap, &str) -> bool`, `require_token(State<AppState>, Request, Next) -> Response` are referenced identically across tasks.
- **No placeholders:** every code and command step is concrete.
```
