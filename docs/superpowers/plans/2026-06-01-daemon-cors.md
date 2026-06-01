# Daemon CORS + Config File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deny-by-default CORS to `cairn-daemon` so a browser UI on an allowlisted origin can call it, with the allowlist coming from a per-cairn `cairn.toml` settings file and/or a `--cors-origin` flag.

**Architecture:** A `Config` (serde/TOML) loaded per-cairn; a `cors_layer(origins)` helper (tower-http) applied to the router in `main.rs` after `build_router` (which stays unchanged); CLI `--config`/`--cors-origin` merged into the effective allowlist.

**Tech Stack:** Rust 1.85, `tower-http` (cors), `toml`, serde, axum/tokio, nextest.

**Verified current shapes:** daemon `lib.rs` exposes `build_router(state: AppState) -> axum::Router`, `AppState`, `CairnEngine`, `cors`-free today. `main.rs` `Cli { cairn: PathBuf, port: u16, no_watch: bool }`; `run()` builds engine → reindex → `let state = AppState::new(engine);` → `let app = build_router(state.clone());` → watcher block (uses `state`) → bind → `axum::serve(listener, app)`. `tests/http.rs` has `fn state(dir)->AppState` and an `oneshot`-based `post_json`. Daemon deps: cairn-* + axum/tokio/serde/serde_json/clap; dev-deps include tower, http-body-util, tokio-tungstenite, futures-util, tempfile. Workspace `serde` has `features=["derive"]`.

---

## Task 1: Config (TOML settings file)

**Files:** Create `crates/cairn-daemon/src/config.rs`; modify `crates/cairn-daemon/src/lib.rs`, `crates/cairn-daemon/Cargo.toml`, root `Cargo.toml`

- [ ] **Step 1: Add deps**

In root `Cargo.toml` `[workspace.dependencies]`, add:
```toml
tower-http = { version = "0.6", features = ["cors"] }
toml = "0.8"
```
In `crates/cairn-daemon/Cargo.toml` `[dependencies]`, add (keep existing; `serde` is already a dep with workspace `derive`):
```toml
tower-http = { workspace = true }
toml = { workspace = true }
```

- [ ] **Step 2: Write the Config module**

Create `crates/cairn-daemon/src/config.rs`:
```rust
//! Daemon settings loaded from a per-cairn `cairn.toml`. Minimal but
//! extensible — add sections as the daemon grows.

use std::path::Path;

use serde::Deserialize;

/// Daemon configuration.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// CORS settings.
    #[serde(default)]
    pub cors: CorsConfig,
}

/// CORS allowlist configuration.
#[derive(Debug, Default, Deserialize)]
pub struct CorsConfig {
    /// Allowed browser origins, e.g. `http://localhost:5173`.
    #[serde(default)]
    pub origins: Vec<String>,
}

impl Config {
    /// Load TOML config from `path`.
    ///
    /// # Errors
    /// Returns an error string if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Config, String> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| format!("read config {}: {e}", path.display()))?;
        toml::from_str(&s).map_err(|e| format!("parse config {}: {e}", path.display()))
    }

    /// Load `<cairn>/cairn.toml` if it exists, else the default (empty) config.
    ///
    /// # Errors
    /// Returns an error string if the file exists but cannot be read/parsed.
    pub fn load_default(cairn: &Path) -> Result<Config, String> {
        let path = cairn.join("cairn.toml");
        if path.exists() {
            Self::load(&path)
        } else {
            Ok(Config::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cors_origins() {
        let c: Config = toml::from_str("[cors]\norigins = [\"http://localhost:5173\"]").unwrap();
        assert_eq!(c.cors.origins, vec!["http://localhost:5173".to_string()]);
    }

    #[test]
    fn empty_or_sectionless_is_empty() {
        assert!(toml::from_str::<Config>("").unwrap().cors.origins.is_empty());
        assert!(toml::from_str::<Config>("[cors]\n").unwrap().cors.origins.is_empty());
    }

    #[test]
    fn load_default_absent_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Config::load_default(tmp.path()).unwrap().cors.origins.is_empty());
    }

    #[test]
    fn load_reads_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("cairn.toml");
        std::fs::write(&p, "[cors]\norigins = [\"http://x\"]").unwrap();
        assert_eq!(Config::load(&p).unwrap().cors.origins, vec!["http://x".to_string()]);
    }
}
```
Add to `crates/cairn-daemon/src/lib.rs` (near the top, with the other `pub` items):
```rust
pub mod config;
pub use config::Config;
```

- [ ] **Step 3: Build, test, lint, commit**

Run: `cargo test -p cairn-daemon config`, `cargo clippy -p cairn-daemon --all-targets -- -D warnings`, `cargo fmt --all` then `cargo fmt --all -- --check`, and `cargo build --locked --workspace` (pin transitive deps if MSRV-1.85 fails; report what you pinned).
```bash
git add -A && git commit -m "feat(daemon): cairn.toml config (Config + cors origins)"
```

---

## Task 2: `cors_layer` helper + CORS tests

**Files:** Modify `crates/cairn-daemon/src/lib.rs`; create `crates/cairn-daemon/tests/cors.rs`

- [ ] **Step 1: Add the helper**

In `crates/cairn-daemon/src/lib.rs`, add a public function (e.g. just above `build_router`):
```rust
/// Build a CORS layer allowing exactly `origins`. Deny-by-default: an empty
/// list allows no cross-origin request. Methods GET/POST/OPTIONS, header
/// `content-type`, no credentials.
#[must_use]
pub fn cors_layer(origins: &[String]) -> tower_http::cors::CorsLayer {
    use axum::http::{header, HeaderValue, Method};
    let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
    tower_http::cors::CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE])
}
```
(If tower-http 0.6's `allow_origin` does not accept `Vec<HeaderValue>` directly, wrap it: `tower_http::cors::AllowOrigin::list(allowed)`. Adapt minimally; behavior must be: only the listed origins are reflected.)

- [ ] **Step 2: CORS integration tests**

Create `crates/cairn-daemon/tests/cors.rs`:
```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, cors_layer, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use tower::ServiceExt; // for `oneshot`

fn app(dir: &std::path::Path, origins: &[String]) -> axum::Router {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    build_router(AppState::new(engine)).layer(cors_layer(origins))
}

#[tokio::test]
async fn allowed_origin_is_reflected() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query")
                .header("content-type", "application/json")
                .header("origin", "http://localhost:5173")
                .body(Body::from("{\"type\":\"list_notes\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:5173"
    );
}

#[tokio::test]
async fn disallowed_origin_gets_no_allow_header() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query")
                .header("content-type", "application/json")
                .header("origin", "http://evil.example")
                .body(Body::from("{\"type\":\"list_notes\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn preflight_options_returns_allow_headers() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/command")
                .header("origin", "http://localhost:5173")
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:5173"
    );
    assert!(resp.headers().get("access-control-allow-methods").is_some());
}
```

- [ ] **Step 3: Run + lint + commit**

Run: `cargo test -p cairn-daemon --test cors`, `cargo clippy -p cairn-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
```bash
git add -A && git commit -m "feat(daemon): cors_layer (deny-by-default allowlist) + tests"
```

---

## Task 3: Wire CORS + config into the binary

**Files:** Modify `crates/cairn-daemon/src/main.rs`

- [ ] **Step 1: Add CLI flags**

In `crates/cairn-daemon/src/main.rs`, add to the `Cli` struct (after `no_watch`):
```rust
    /// Path to a TOML settings file (default: `<cairn>/cairn.toml` if present).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Allow a browser origin to call the daemon (CORS). Repeatable; merged
    /// with `[cors].origins` from the settings file.
    #[arg(long = "cors-origin")]
    cors_origin: Vec<String>,
```
Update the `use cairn_daemon::{...}` import to add `cors_layer` and `Config`:
```rust
use cairn_daemon::{build_router, cors_layer, AppState, CairnEngine, Config};
```

- [ ] **Step 2: Load config, merge, apply the layer**

In `run()`, replace the line `let app = build_router(state.clone());` with:
```rust
    // CORS allowlist: settings file (or default <cairn>/cairn.toml) ∪ --cors-origin.
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::load_default(&cli.cairn)?,
    };
    let mut cors_origins = config.cors.origins;
    cors_origins.extend(cli.cors_origin.iter().cloned());
    cors_origins.sort();
    cors_origins.dedup();
    if cors_origins.is_empty() {
        println!(
            "CORS: no cross-origin origins allowed (add [cors].origins to {}/cairn.toml or pass --cors-origin)",
            cli.cairn.display()
        );
    } else {
        println!("CORS: allowing {}", cors_origins.join(", "));
    }

    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));
```
(The watcher block and `axum::serve(listener, app)` stay as they are — they reference `state` and `app` respectively.)

- [ ] **Step 3: Build, lint, verify --help**

Run: `cargo build -p cairn-daemon`, `cargo clippy -p cairn-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`. Confirm `cargo run -p cairn-daemon -- --help` lists `--config` and `--cors-origin`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(daemon): --config/--cors-origin flags; apply CORS layer"
```

---

## Task 4: ADR + handoff + full gate

**Files:** Create `docs/decisions/0004-daemon-cors.md`; modify `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: ADR-0004**

Create `docs/decisions/0004-daemon-cors.md` (mirror ADR-0003 style): Context (a browser UI on another origin needs CORS; loopback binding doesn't protect against a malicious visited page → CORS is the gate). Decision: **deny-by-default allowlist** (not `*`); allowed origins from `<cairn>/cairn.toml` `[cors].origins` and/or `--cors-origin`, merged; `tower-http` CORS layer applied after `build_router` (unchanged); first config-file surface (`cairn.toml`, TOML, extensible). Consequences: a browser UI must allowlist its dev origin or be blocked; WS `/events` needs no CORS; deferred — auth/TLS/credentials, network exposure, other config keys. Reference the spec.

- [ ] **Step 2: Handoff update**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, in the daemon transport section (§4) and/or gotchas (§6), add a short **"Browser UI: allowlist your origin"** note:
- The daemon denies cross-origin by default. To let a browser UI on e.g. `http://localhost:5173` call it, either create `<cairn>/cairn.toml`:
  ```toml
  [cors]
  origins = ["http://localhost:5173"]
  ```
  or run `cargo run -p cairn-daemon -- --cairn ./demo --cors-origin http://localhost:5173`.
- WebSocket `/events` needs no CORS config (browsers connect cross-origin to WS directly).
Keep edits minimal and accurate.

- [ ] **Step 3: Full workspace gate**

Run and confirm green:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
Report the total test count. Manually verify: start `cargo run -p cairn-daemon -- --cairn /tmp/cairn-cors-demo` (after `cairn init` there) and confirm it prints the "CORS: no cross-origin origins allowed (...)" hint; then with `--cors-origin http://localhost:5173` it prints `CORS: allowing http://localhost:5173`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "docs: ADR-0004 daemon CORS + handoff browser-UI note"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** §3 config → Task 1; §5 cors_layer → Task 2; §4 flags/merge/startup → Task 3; §6 deps → Task 1; §7 tests → Tasks 1–2; §8 docs → Task 4. Deny-by-default (§1/§2) is realized by the empty-allowlist behavior tested in Task 2.
- **Type consistency:** `Config { cors: CorsConfig { origins: Vec<String> } }`, `Config::load`/`load_default`, `cors_layer(&[String]) -> CorsLayer`, `cli.config: Option<PathBuf>`, `cli.cors_origin: Vec<String>` used consistently across Tasks 1–3.
- **build_router untouched:** the layer is applied in `main.rs` and in the `tests/cors.rs` app builder, so existing `http.rs`/`ws.rs`/`watch.rs` tests are unaffected.
- **Placeholder scan:** no TBD/TODO; complete code in every step. The tower-http `allow_origin` API note is an explicit version-adaptation point with the exact required behavior specified.
```
