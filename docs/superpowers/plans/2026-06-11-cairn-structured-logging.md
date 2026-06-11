# Structured Logging (Daemon, First Increment) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `tracing` into `cairn-daemon`: an env-driven subscriber, a per-request span on `/command` and `/query`, and conversion of the daemon's ad-hoc `println!`/`eprintln!` to leveled log macros.

**Architecture:** Initialize a `tracing-subscriber` `fmt` subscriber once at the top of `main()` (text format, `RUST_LOG`-driven, default `info`). Each request handler opens an `info_span!("request", …)`, times the `spawn_blocking` work, records `status`/`duration_ms`/`outcome`, and emits one completion event inside the span. Existing startup/warning prints become `info!`/`warn!`/`error!`.

**Tech Stack:** Rust, `tracing` 0.1, `tracing-subscriber` 0.3 (env-filter), `tracing-test` (dev), axum 0.8, tokio.

**Spec:** `docs/superpowers/specs/2026-06-11-cairn-structured-logging-design.md`

---

## File Structure

- **`Cargo.toml`** (workspace root) — add `tracing`, `tracing-subscriber`, `tracing-test` to `[workspace.dependencies]`.
- **`crates/cairn-daemon/Cargo.toml`** — add `tracing` + `tracing-subscriber` deps, `tracing-test` dev-dep.
- **`crates/cairn-daemon/src/main.rs`** — subscriber init in `main()`; convert `println!`/`eprintln!` to log macros.
- **`crates/cairn-daemon/src/lib.rs`** — `command_kind`/`query_kind` helpers; per-request spans in `command_handler`/`query_handler`; convert the watch `eprintln!`.
- **`crates/cairn-daemon/tests/logging.rs`** (new) — regression test that a request emits a span/event with the command kind.

---

## Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/cairn-daemon/Cargo.toml`

- [ ] **Step 1: Add to workspace `[workspace.dependencies]`**

In `Cargo.toml`, after the existing `toml = "1"` line, add:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-test = "0.2"
```

- [ ] **Step 2: Add to `cairn-daemon` dependencies**

In `crates/cairn-daemon/Cargo.toml`, under `[dependencies]` (after the `toml = { workspace = true }` line), add:

```toml
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

Under `[dev-dependencies]` (after `tempfile = { workspace = true }`), add:

```toml
tracing-test = { workspace = true }
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo fetch && cargo build -p cairn-daemon`
Expected: builds successfully (no code uses the crates yet, so no warnings about them as they are declared deps).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/cairn-daemon/Cargo.toml
git commit -m "build(daemon): add tracing, tracing-subscriber, tracing-test"
```

---

## Task 2: Initialize the subscriber in `main()`

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs:158-167` (the `main` fn)

- [ ] **Step 1: Add subscriber init at the top of `main()`**

Replace the `main` function body so the subscriber is set up before `run()`:

```rust
#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
```

(This also converts the fatal `eprintln!("error: {e}")` at line 163 to `tracing::error!`.)

- [ ] **Step 2: Verify it builds**

Run: `cargo build -p cairn-daemon`
Expected: builds successfully.

- [ ] **Step 3: Smoke-check the subscriber emits**

Run: `cargo run -p cairn-daemon -- --cairn /tmp/does-not-exist`
Expected: a single `ERROR` line (timestamp + level) like
`... ERROR cairn_daemon: not a cairn at /tmp/does-not-exist ...` and a non-zero exit.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): init tracing subscriber, env-driven level filter"
```

---

## Task 3: Convert `main.rs` startup prints to leveled macros

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs` (lines 79, 84, 91, 105, 114, 116, 122, 127, 144, 146)

- [ ] **Step 1: Convert the index lines (info)**

Line 79: `println!("persisting index at {}", index_dir.display());`
→ `tracing::info!("persisting index at {}", index_dir.display());`

Line 84: `println!("index: in-memory (not persisted)");`
→ `tracing::info!("index: in-memory (not persisted)");`

- [ ] **Step 2: Convert the plugin-timeout warning (warn)**

The `eprintln!` block starting at line 91:

```rust
            eprintln!(
                "warning: [plugins] timeout_secs = 0 is invalid; using default {:?}",
                cairn_infra::DEFAULT_PLUGIN_TIMEOUT
            );
```

→

```rust
            tracing::warn!(
                "[plugins] timeout_secs = 0 is invalid; using default {:?}",
                cairn_infra::DEFAULT_PLUGIN_TIMEOUT
            );
```

(Drop the `warning: ` text prefix — the level conveys it.)

- [ ] **Step 3: Convert the plugin lines (info / warn)**

The `println!` block at line 105:

```rust
        println!(
            "plugins: none trusted (add [plugins].trusted = [\"<dir>\"] to {}/cairn.toml to enable)",
            cli.cairn.display()
        );
```

→ same text with `tracing::info!(` replacing `println!(`.

Line 114: `println!("plugins: read timeout {plugin_timeout:?}");`
→ `tracing::info!("plugins: read timeout {plugin_timeout:?}");`

Line 116: `Err(e) => eprintln!("warning: plugin host disabled: {e}"),`
→ `Err(e) => tracing::warn!("plugin host disabled: {e}"),`

- [ ] **Step 4: Convert the CORS lines (info)**

The `println!` block at line 122:

```rust
        println!(
            "CORS: no cross-origin origins allowed (add [cors].origins to {}/cairn.toml or pass --cors-origin)",
            cli.cairn.display()
        );
```

→ same text with `tracing::info!(`.

Line 127: `println!("CORS: allowing {}", cors_origins.join(", "));`
→ `tracing::info!("CORS: allowing {}", cors_origins.join(", "));`

- [ ] **Step 5: Convert the watcher + listening lines (info / warn)**

Line 144: `println!("watching {} for changes", cli.cairn.display());`
→ `tracing::info!("watching {} for changes", cli.cairn.display());`

Line 146: `Err(e) => eprintln!("warning: file watcher disabled: {e}"),`
→ `Err(e) => tracing::warn!("file watcher disabled: {e}"),`

Line 154: `println!("cairn-daemon listening on http://{addr}");`
→ `tracing::info!("cairn-daemon listening on http://{addr}");`

- [ ] **Step 6: Verify no prints remain in main.rs and it builds**

Run: `grep -n 'println!\|eprintln!' crates/cairn-daemon/src/main.rs; cargo build -p cairn-daemon`
Expected: grep prints nothing; build succeeds.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): convert startup println/eprintln to tracing levels"
```

---

## Task 4: Per-request span — failing test first

**Files:**
- Create: `crates/cairn-daemon/tests/logging.rs`

- [ ] **Step 1: Write the failing regression test**

Create `crates/cairn-daemon/tests/logging.rs`:

```rust
use axum::body::Body;
use axum::http::Request;
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use tower::ServiceExt; // for `oneshot`

fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    AppState::new(engine)
}

#[tokio::test]
#[tracing_test::traced_test]
async fn command_request_emits_span_with_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    // The per-request span carries the command kind and a completion event.
    assert!(logs_contain("request completed"));
    assert!(logs_contain("command=\"write_note\""));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-daemon --test logging -- --nocapture`
Expected: FAIL — `logs_contain("request completed")` is false (no span/event emitted yet).

---

## Task 5: Per-request span — implement

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (add helpers; rewrite `command_handler` at 185-188 and `query_handler` at 190-193)

- [ ] **Step 1: Add the kind helpers**

In `crates/cairn-daemon/src/lib.rs`, add above `command_handler` (after `service_response`, around line 184):

```rust
/// The wire `type` tag for a command (matches the serde `rename_all = "snake_case"`),
/// used as the `command` span field so a logged request matches what a client sent.
fn command_kind(command: &Command) -> &'static str {
    match command {
        Command::WriteNote { .. } => "write_note",
        Command::DeleteNote { .. } => "delete_note",
        Command::RenameNote { .. } => "rename_note",
        Command::Commit { .. } => "commit",
        Command::RestoreNote { .. } => "restore_note",
        Command::InvokePluginCommand { .. } => "invoke_plugin_command",
    }
}

/// The wire `type` tag for a query (matches the serde `rename_all = "snake_case"`).
fn query_kind(query: &Query) -> &'static str {
    match query {
        Query::GetNote { .. } => "get_note",
        Query::Search { .. } => "search",
        Query::GetBacklinks { .. } => "get_backlinks",
        Query::ListNotes => "list_notes",
        Query::GetGraph => "get_graph",
        Query::ListTags => "list_tags",
        Query::NotesByTag { .. } => "notes_by_tag",
        Query::ListPlugins => "list_plugins",
        Query::NoteHistory { .. } => "note_history",
        Query::NoteAt { .. } => "note_at",
    }
}
```

- [ ] **Step 2: Rewrite `command_handler`**

Replace (lines 185-188):

```rust
async fn command_handler(State(state): State<AppState>, Json(command): Json<Command>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_command_blocking(&command)).await;
    service_response(result)
}
```

with:

```rust
async fn command_handler(State(state): State<AppState>, Json(command): Json<Command>) -> Response {
    let span = tracing::info_span!(
        "request",
        method = "POST",
        path = "/command",
        command = command_kind(&command),
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    let _enter = span.enter();
    let start = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || state.run_command_blocking(&command)).await;
    let response = service_response(result);
    span.record("status", response.status().as_u16());
    span.record("duration_ms", start.elapsed().as_millis() as u64);
    span.record(
        "outcome",
        if response.status().is_success() { "ok" } else { "error" },
    );
    tracing::info!("request completed");
    response
}
```

- [ ] **Step 3: Rewrite `query_handler`**

Replace (lines 190-193):

```rust
async fn query_handler(State(state): State<AppState>, Json(query): Json<Query>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_query_blocking(&query)).await;
    service_response(result)
}
```

with:

```rust
async fn query_handler(State(state): State<AppState>, Json(query): Json<Query>) -> Response {
    let span = tracing::info_span!(
        "request",
        method = "POST",
        path = "/query",
        command = query_kind(&query),
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    let _enter = span.enter();
    let start = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || state.run_query_blocking(&query)).await;
    let response = service_response(result);
    span.record("status", response.status().as_u16());
    span.record("duration_ms", start.elapsed().as_millis() as u64);
    span.record(
        "outcome",
        if response.status().is_success() { "ok" } else { "error" },
    );
    tracing::info!("request completed");
    response
}
```

- [ ] **Step 4: Run the regression test to verify it passes**

Run: `cargo test -p cairn-daemon --test logging -- --nocapture`
Expected: PASS — both `logs_contain` assertions hold.

- [ ] **Step 5: Verify the whole daemon still builds and tests pass**

Run: `cargo test -p cairn-daemon`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/logging.rs
git commit -m "feat(daemon): per-request tracing span on /command and /query"
```

---

## Task 6: Convert the `lib.rs` watch print

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs:147`

- [ ] **Step 1: Convert the watch failure print (warn)**

Line 147: `eprintln!("watch: apply_change failed: {e}");`
→ `tracing::warn!("watch: apply_change failed: {e}");`

- [ ] **Step 2: Verify no prints remain in the daemon crate and build is clean**

Run: `grep -rn 'println!\|eprintln!' crates/cairn-daemon/src; cargo build -p cairn-daemon`
Expected: grep prints nothing; build succeeds.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-daemon/src/lib.rs
git commit -m "feat(daemon): log watch apply_change failure via tracing::warn"
```

---

## Task 7: Full verification & clippy

- [ ] **Step 1: Workspace build, clippy, and tests**

Run: `cargo clippy -p cairn-daemon --all-targets -- -D warnings && cargo test -p cairn-daemon`
Expected: no clippy warnings; all tests pass.

- [ ] **Step 2: Capture real log output for the PR (info level)**

```bash
TMP=$(mktemp -d) && (cd "$TMP" && git init -q && cargo run -q -p cairn-daemon -- --cairn "$TMP" --no-watch --port 7799 &) ; sleep 3
curl -s -XPOST localhost:7799/command -H 'content-type: application/json' \
  -d '{"type":"write_note","path":"a.md","contents":"hello"}' >/dev/null
curl -s -XPOST localhost:7799/query -H 'content-type: application/json' \
  -d '{"type":"search","query":"hello"}' >/dev/null
```

Expected (paste into PR): `info` startup lines (`index: in-memory…`, `plugins: none trusted…`, `CORS: no cross-origin…`, `cairn-daemon listening…`) and two `request` span lines with `method`, `path`, `command`, `status`, `duration_ms`, `outcome`. Stop the daemon afterward (`kill %1` or `pkill -f cairn-daemon`).

- [ ] **Step 3: Capture a `RUST_LOG=warn` run to show the filter is honored**

Re-run Step 2 prefixed with `RUST_LOG=warn`. Expected: the `info` startup and `request` lines are suppressed; only `warn`/`error` (if any) appear. Paste into PR.

---

## Self-Review notes

- **Spec coverage:** deps (T1), subscriber init + env filter (T2), per-request span fields method/path/command/status/duration_ms/outcome (T4-T5), level mapping for all listed sites (T2 fatal, T3 startup, T6 watch), regression test (T4-T5), manual verification incl. `RUST_LOG=warn` (T7). Deferred items (plugin_host, G4/G5, JSON) are explicitly not tasks.
- **No placeholders:** every code step shows full code.
- **Type consistency:** `command_kind`/`query_kind` defined in T5 and used in the same handlers; `logs_contain`/`traced_test` come from `tracing_test`.
