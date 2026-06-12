# Structured logging — `tracing` in the daemon (first increment)

**Audit finding:** diagnostics (Medium) — *No structured logging / tracing
anywhere* (`audit/diagnostics.md`). The daemon, a long-running localhost network
service, has no logging framework: all operational output is ad-hoc
`println!`/`eprintln!` with no levels, no timestamps, no spans, and no per-request
logging. A request cannot be correlated to the events/plugin calls it produced,
verbosity cannot be raised or lowered, and watch/plugin-dispatch failures are
invisible unless someone is reading stderr.

Locations called out by the finding:

- `crates/cairn-daemon/src/main.rs` — `println!` at startup/config sites and
  `eprintln!` warnings.
- `crates/cairn-daemon/src/lib.rs` — the watch `apply_change` failure `eprintln!`.
- `crates/cairn-infra/src/plugin_host.rs` — two `eprintln!` (deferred, see below).
- workspace `Cargo.toml` — no `tracing`/`log` dependency at all.

## Scope of this PR (smallest viable increment)

Wire the observability framework into the daemon and convert the daemon's own
ad-hoc output:

1. Add `tracing` + `tracing-subscriber` to the workspace and `cairn-daemon`.
2. Initialize the subscriber once, at the top of `main()`.
3. Add a per-request span to the `/command` and `/query` handlers.
4. Convert the `main.rs` `println!`/`eprintln!` sites and the one `lib.rs`
   `eprintln!` to leveled `tracing` macros.

**Out of scope** (listed as follow-ups in the PR body, not built here):

- Instrumenting `cairn-infra/src/plugin_host.rs` (`eprintln!` at the plugin host).
- G4 best-effort drop logging and G5 `JoinError` logging, beyond what the
  per-request span already records (a join failure surfaces as `status = 500`,
  `outcome = error` on the request span).
- JSON log output / a `--log-format` flag. The first PR ships human-readable
  text only; JSON is a clean follow-up if a deployment ever needs it.

## Design

### Dependencies

Add to `[workspace.dependencies]` in the root `Cargo.toml`:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Add `tracing` and `tracing-subscriber` to `cairn-daemon`'s `[dependencies]`, and
`tracing-test` to its `[dev-dependencies]` (declared as a workspace dep too, for
consistency with the existing pattern).

### Subscriber initialization

Initialize the subscriber at the very top of `main()`, **before** `run()`, so
startup and config errors are logged through the same pipeline:

```rust
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    )
    .init();
```

- **Format:** human-readable text (tracing's default `fmt`): timestamp, level,
  target, and span fields rendered as `key=value`. cairn-daemon binds localhost
  only and is read on a terminal; "structured" is satisfied by the span fields.
- **Level filter:** env-driven via the standard `RUST_LOG`
  (e.g. `RUST_LOG=debug`, `RUST_LOG=cairn_daemon=debug`), defaulting to `info`
  when unset or unparseable.

### Per-request span

Add two small helpers in `lib.rs` that map a request to its wire `type` tag (the
snake_case variant name), so the span's `command` field matches what a client
sent:

```rust
fn command_kind(command: &Command) -> &'static str { /* match → "write_note", … */ }
fn query_kind(query: &Query) -> &'static str { /* match → "search", … */ }
```

Rework `command_handler` (and `query_handler`, symmetric) to create a span,
time the work, and emit one completion event inside the span:

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
    span.record("outcome", if response.status().is_success() { "ok" } else { "error" });
    tracing::info!("request completed");
    response
}
```

Span fields cover the finding's requested set: **method, path, command** (the
variant kind), **status, duration_ms, outcome**. `/health` and `/events` are not
instrumented in this PR (no request body to classify; they are not the diagnostic
gap the finding describes).

### Level mapping for existing ad-hoc output

| Site | Now | Becomes |
|---|---|---|
| persisting index at … / index: in-memory | `println!` | `info!` |
| plugins: none trusted / plugins: read timeout | `println!` | `info!` |
| CORS: allowing … / no cross-origin … | `println!` | `info!` |
| watching … for changes | `println!` | `info!` |
| cairn-daemon listening on … | `println!` | `info!` |
| `[plugins] timeout_secs = 0` invalid | `eprintln!` | `warn!` |
| plugin host disabled | `eprintln!` | `warn!` |
| file watcher disabled | `eprintln!` | `warn!` |
| fatal `error: {e}` in `main` | `eprintln!` | `error!` |
| `lib.rs` watch `apply_change failed` | `eprintln!` | `warn!` |
| plugin host: skipping untrusted dir | `eprintln!` | `warn!` |
| plugin host: skipping plugin that failed to spawn | `eprintln!` | `warn!` |

## Testing & verification

### Regression test

Logging is awkward to assert at the unit level, so add **one** natural test: send
a `write_note` command through `build_router(...).oneshot(...)` under
`#[traced_test]`, and assert the request-completed event was emitted carrying the
command kind. `tracing-test` installs a global capturing subscriber that sees
events across the `spawn_blocking` worker threads, which a thread-local subscriber
cannot. The exact assertion (substring vs. field) is pinned via TDD during
implementation; keep it to the one natural assertion rather than over-fitting the
log text.

### Manual verification (primary evidence)

Per the finding, verify with real captured output and paste it into the PR:

1. Build and run the daemon against a temp cairn; confirm the `info`/`warn`
   startup lines.
2. Exercise a `write_note` command and a `search` query; paste the `request`
   span lines showing method / path / command / status / duration_ms / outcome.
3. Re-run with `RUST_LOG=warn` and show the `info` request lines are suppressed,
   demonstrating the level filter is honored.
