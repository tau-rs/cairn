# `cairn ask` engine seam Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the engine's note-grounded streaming answers (`cairn ask`) over the wire as a contract-native, lock-safe seam, with the daemon's `POST /ask` (SSE) as the demonstrable client.

**Architecture:** Add two contract types (`AskRequest`, `AnswerEvent`); split the service's `augmented_answer` into a lock-scoped context-gather plus a port→wire event mapper; add a token-gated `POST /ask` SSE endpoint on the daemon that gathers context under the engine lock, releases it, then streams the agent run (off the async reactor via `spawn_blocking`, off the engine mutex) as `data: {AnswerEvent}` frames. The runtime is a port (`Arc<dyn AgentRuntime + Send + Sync>`) injected at the daemon's composition root from `TAU_BIN`, defaulting to `NullRuntime`.

**Tech Stack:** Rust, axum 0.8 (SSE + `ws`), tokio (`mpsc` + `spawn_blocking`), `futures-util` (`stream::unfold`), serde, `ts-rs` (contract codegen).

**Design spec:** `docs/superpowers/specs/2026-06-14-cairn-ask-engine-seam-design.md`

**Branch:** `feat/ask-engine-seam`

---

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `crates/cairn-contract/src/lib.rs` | Wire DTOs | **Modify** — add `AskRequest` struct + `AnswerEvent` enum + an inline wire-tag test module |
| `crates/cairn-contract/tests/codegen.rs` | ts-rs export check | **Modify** — assert + export the two new types |
| `crates/cairn-service/src/lib.rs` | Dispatch + answer orchestration | **Modify** — add `gather_answer_context` + `agent_event_to_wire`; rewrite `augmented_answer` to compose them; add tests |
| `crates/cairn-daemon/Cargo.toml` | Daemon deps | **Modify** — add `futures-util` (already a workspace dep) |
| `crates/cairn-daemon/src/lib.rs` | HTTP/WS transport | **Modify** — `AppState.runtime` field + `with_runtime`; `AnswerStreamSink`; `ask_handler`; `/ask` route |
| `crates/cairn-daemon/src/main.rs` | Composition root | **Modify** — build runtime from `TauConfig::from_env()`, inject via `with_runtime` |
| `crates/cairn-daemon/tests/ask.rs` | `/ask` integration tests | **Create** — happy path, NullRuntime→failed, auth gate |

---

## Task 1: Contract types `AskRequest` + `AnswerEvent`

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (after the `Event` enum, ~line 129)
- Modify: `crates/cairn-contract/tests/codegen.rs`

- [ ] **Step 1: Write the failing wire-tag test**

Add to the bottom of `crates/cairn-contract/src/lib.rs` (the file currently has no test module):

```rust
#[cfg(test)]
mod ask_wire_format {
    use super::{AnswerEvent, AskRequest};

    #[test]
    fn answer_event_tags_match_the_track04_mock() {
        let cases = [
            (serde_json::to_value(AnswerEvent::Sources { paths: vec!["a.md".into()] }).unwrap(), "sources"),
            (serde_json::to_value(AnswerEvent::TextDelta { text: "hi".into() }).unwrap(), "text_delta"),
            (serde_json::to_value(AnswerEvent::ToolStarted { tool: "grep".into() }).unwrap(), "tool_started"),
            (serde_json::to_value(AnswerEvent::ToolCompleted { tool: "grep".into(), ok: true }).unwrap(), "tool_completed"),
            (serde_json::to_value(AnswerEvent::TurnCompleted).unwrap(), "turn_completed"),
            (serde_json::to_value(AnswerEvent::Completed).unwrap(), "completed"),
            (serde_json::to_value(AnswerEvent::Failed { message: "boom".into() }).unwrap(), "failed"),
        ];
        for (json, tag) in cases {
            assert_eq!(json["type"], tag, "wire tag drift for {tag}");
        }
        // Field names the UI mock depends on.
        let delta = serde_json::to_value(AnswerEvent::TextDelta { text: "x".into() }).unwrap();
        assert_eq!(delta["text"], "x");
    }

    #[test]
    fn ask_request_top_k_is_optional() {
        let r: AskRequest = serde_json::from_str(r#"{"query":"q"}"#).unwrap();
        assert_eq!(r.query, "q");
        assert_eq!(r.top_k, None);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-contract ask_wire_format`
Expected: FAIL — `cannot find type 'AnswerEvent'` / `'AskRequest'`.

- [ ] **Step 3: Add the two types**

Insert after the `Event` enum (after line 129) in `crates/cairn-contract/src/lib.rs`:

```rust
/// A streaming, note-grounded question. Its own shape — not a `Command` (no
/// mutation) and not a `Query` (no single response).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct AskRequest {
    /// The question to answer.
    pub query: String,
    /// How many top search hits to ground the answer in. `None` ⇒ 5.
    pub top_k: Option<usize>,
}

/// One increment of an answer stream — cairn's own closed wire vocabulary,
/// mirroring `cairn_ports::AgentEvent` plus a leading `Sources` frame. Struct
/// variants are required: `#[serde(tag = "type")]` cannot tag a newtype variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerEvent {
    /// The cited notes grounding the answer; emitted first.
    Sources {
        /// Relative note paths, in rank order.
        paths: Vec<String>,
    },
    /// A chunk of answer text.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// The agent began a tool call.
    ToolStarted {
        /// Tool name.
        tool: String,
    },
    /// A tool call finished; `ok` is false if it reported an error.
    ToolCompleted {
        /// Tool name.
        tool: String,
        /// Whether the call succeeded.
        ok: bool,
    },
    /// One agent turn completed; a run may span several.
    TurnCompleted,
    /// The run finished successfully.
    Completed,
    /// The run failed; `message` is human-readable.
    Failed {
        /// Failure detail.
        message: String,
    },
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-contract ask_wire_format`
Expected: PASS (both tests).

- [ ] **Step 5: Extend the codegen export test**

In `crates/cairn-contract/tests/codegen.rs`: add `AnswerEvent, AskRequest` to the `use cairn_contract::{...}` import (keep alphabetical), then add inside `exports_typescript_bindings`:

```rust
    assert!(AskRequest::decl().contains("AskRequest"));
    assert!(AnswerEvent::decl().contains("AnswerEvent"));
```
and below the existing `export_all()` calls:
```rust
    AskRequest::export_all().unwrap();
    AnswerEvent::export_all().unwrap();
```

- [ ] **Step 6: Run codegen + confirm bindings generated**

Run: `cargo test -p cairn-contract exports_typescript_bindings && ls crates/cairn-contract/bindings/AskRequest.ts crates/cairn-contract/bindings/AnswerEvent.ts`
Expected: PASS, and both `.ts` files listed.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/tests/codegen.rs crates/cairn-contract/bindings/
git commit -m "feat(contract): add AskRequest + AnswerEvent wire types"
```

---

## Task 2: Service seam — lock-scoped gather + port→wire mapper

**Files:**
- Modify: `crates/cairn-service/src/lib.rs` (rewrite `augmented_answer` at lines 270-305; add two pub fns; add tests in the existing `#[cfg(test)] mod` near line 1000)

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing test module in `crates/cairn-service/src/lib.rs` (the module that defines `RecordingRuntime` / `VecSink`, ~line 1000), after `prompt_without_context_omits_notes_section`:

```rust
    #[test]
    fn gather_returns_prompt_with_context_and_cited_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut events: Vec<Event> = Vec::new();
        let mut engine = cairn_startup::build_engine(dir.path()).unwrap();
        dispatch_command(
            &mut engine,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "ownership moves by default".into(),
            },
            &mut events,
        )
        .unwrap();
        engine.reindex(&mut events).unwrap();

        let (prompt, cited) = gather_answer_context(&engine, "ownership", 5).unwrap();
        assert_eq!(cited, vec!["a.md".to_string()]);
        assert!(prompt.contains("ownership moves by default"));
        assert!(prompt.contains("Question: ownership"));
    }

    #[test]
    fn agent_event_maps_every_variant_to_wire() {
        use cairn_contract::AnswerEvent;
        assert_eq!(
            agent_event_to_wire(AgentEvent::TextDelta("hi".into())),
            Some(AnswerEvent::TextDelta { text: "hi".into() })
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::ToolStarted { tool: "g".into() }),
            Some(AnswerEvent::ToolStarted { tool: "g".into() })
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::ToolCompleted { tool: "g".into(), ok: false }),
            Some(AnswerEvent::ToolCompleted { tool: "g".into(), ok: false })
        );
        assert_eq!(agent_event_to_wire(AgentEvent::TurnCompleted), Some(AnswerEvent::TurnCompleted));
        assert_eq!(agent_event_to_wire(AgentEvent::Completed), Some(AnswerEvent::Completed));
        assert_eq!(
            agent_event_to_wire(AgentEvent::Failed { message: "x".into() }),
            Some(AnswerEvent::Failed { message: "x".into() })
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p cairn-service gather_returns_prompt_with_context_and_cited_paths agent_event_maps_every_variant_to_wire`
Expected: FAIL — `cannot find function 'gather_answer_context'` / `'agent_event_to_wire'`.

- [ ] **Step 3: Add the two functions and rewrite `augmented_answer`**

Replace the body of `augmented_answer` (lines 270-305) and add the two new fns. The final shape:

```rust
pub fn augmented_answer(
    engine: &Engine,
    query: &str,
    runtime: &dyn AgentRuntime,
    sink: &mut dyn AgentSink,
    top_k: usize,
) -> Result<Vec<String>, ServiceError> {
    let (prompt, cited) = gather_answer_context(engine, query, top_k)?;
    runtime.answer(&prompt, sink)?;
    Ok(cited)
}

/// The engine-touching half of an answer: search the top `top_k` hits, read them
/// into context, and build the agent prompt. Returns `(prompt, cited_paths)`.
/// Pull this out so a transport can run it under the engine lock and then stream
/// the (long, lock-free) agent run separately.
///
/// # Errors
/// [`ServiceError`] if a search/read dispatch fails.
pub fn gather_answer_context(
    engine: &Engine,
    query: &str,
    top_k: usize,
) -> Result<(String, Vec<String>), ServiceError> {
    let cited: Vec<String> = match dispatch_query(
        engine,
        &Query::Search {
            query: query.to_string(),
        },
    )? {
        QueryResponse::SearchResults { results } => {
            results.into_iter().take(top_k).map(|r| r.path).collect()
        }
        _ => Vec::new(),
    };

    let mut context = String::new();
    for path in &cited {
        if let QueryResponse::Note { contents } =
            dispatch_query(engine, &Query::GetNote { path: path.clone() })?
        {
            context.push_str("## ");
            context.push_str(path);
            context.push('\n');
            context.push_str(&contents);
            context.push_str("\n\n");
        }
    }

    let prompt = build_answer_prompt(&context, query);
    Ok((prompt, cited))
}

/// Map a port [`AgentEvent`] to its wire [`AnswerEvent`]. `None` for kinds with
/// no wire form — `AgentEvent` is `#[non_exhaustive]`, so unknown upstream kinds
/// are skipped rather than panicking (mirroring the CLI's wildcard arm).
#[must_use]
pub fn agent_event_to_wire(e: cairn_ports::AgentEvent) -> Option<cairn_contract::AnswerEvent> {
    use cairn_contract::AnswerEvent as W;
    use cairn_ports::AgentEvent as A;
    Some(match e {
        A::TextDelta(text) => W::TextDelta { text },
        A::ToolStarted { tool } => W::ToolStarted { tool },
        A::ToolCompleted { tool, ok } => W::ToolCompleted { tool, ok },
        A::TurnCompleted => W::TurnCompleted,
        A::Completed => W::Completed,
        A::Failed { message } => W::Failed { message },
        _ => return None,
    })
}
```

Note: `agent_event_to_wire` intentionally has **no** `Sources` arm — `Sources` is produced by the transport from `cited`, not from an `AgentEvent`.

- [ ] **Step 4: Run the new + existing answer tests**

Run: `cargo test -p cairn-service gather_returns_prompt_with_context_and_cited_paths agent_event_maps_every_variant_to_wire retrieves_context_builds_prompt_and_streams`
Expected: PASS (all three — the pre-existing `augmented_answer` test still passes unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-service/src/lib.rs
git commit -m "refactor(service): split augmented_answer into gather + port→wire mapper"
```

---

## Task 3: Daemon `POST /ask` SSE endpoint (happy path)

**Files:**
- Modify: `crates/cairn-daemon/Cargo.toml`
- Modify: `crates/cairn-daemon/src/lib.rs`
- Modify: `crates/cairn-daemon/src/main.rs`
- Create: `crates/cairn-daemon/tests/ask.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/cairn-daemon/tests/ask.rs`:

```rust
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
use http_body_util::BodyExt; // for `.collect()`
use tower::ServiceExt; // for `oneshot`

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn engine(dir: &std::path::Path) -> Engine {
    Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    )
}

/// A runtime that ignores the prompt and emits a fixed, scripted run.
struct StubRuntime;
impl AgentRuntime for StubRuntime {
    fn answer(&self, _prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
        sink.emit(AgentEvent::TextDelta("hello".into()));
        sink.emit(AgentEvent::Completed);
        Ok(())
    }
}

fn ask_request(auth: Option<&str>, query: &str) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/ask")
        .header("content-type", "application/json");
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    b.body(Body::from(
        serde_json::json!({ "query": query }).to_string(),
    ))
    .unwrap()
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn ask_streams_sources_then_text_then_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(
        AppState::new(engine(tmp.path()))
            .with_token(TOKEN)
            .with_runtime(Arc::new(StubRuntime)),
    );
    let resp = app.oneshot(ask_request(Some(TOKEN), "anything")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // SSE frames are `data: {json}\n\n`; assert order and content.
    let sources = body.find("\"type\":\"sources\"").expect("sources frame");
    let delta = body.find("\"type\":\"text_delta\"").expect("text_delta frame");
    let completed = body.find("\"type\":\"completed\"").expect("completed frame");
    assert!(sources < delta && delta < completed, "frames out of order:\n{body}");
    assert!(body.contains("\"text\":\"hello\""), "missing delta text:\n{body}");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-daemon --test ask ask_streams_sources_then_text_then_completed`
Expected: FAIL — `no method named 'with_runtime'` (and `http_body_util` may be missing from dev-deps).

- [ ] **Step 3: Add `futures-util` to daemon deps**

In `crates/cairn-daemon/Cargo.toml`, under `[dependencies]` (after `tokio`), add:

```toml
futures-util = { workspace = true }
```

Confirm the `[dev-dependencies]` section already has `http-body-util`, `tower`, `tempfile`; if `http-body-util` is absent, add `http-body-util = "0.1"` there (it is the body-collect helper used by the existing tests — check `crates/cairn-daemon/tests/http.rs` for the exact crate already in use and mirror it).

- [ ] **Step 4: Add the `runtime` field + `with_runtime` to `AppState`**

In `crates/cairn-daemon/src/lib.rs`:

Add imports near the existing `use cairn_service::...` line:
```rust
use cairn_contract::{AnswerEvent, AskRequest};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink};
use cairn_service::{agent_event_to_wire, gather_answer_context};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
```

Add the field to the `AppState` struct (after `token`):
```rust
    /// Agent runtime backing `POST /ask`. Defaults to `NullRuntime` (which errors
    /// until `TAU_BIN` is set); the binary injects `TauServeRuntime` via
    /// [`AppState::with_runtime`].
    runtime: Arc<dyn AgentRuntime + Send + Sync>,
```

In `AppState::new`, set the default (add to the struct literal):
```rust
            runtime: Arc::new(cairn_infra::NullRuntime),
```

Add the builder after `with_token`:
```rust
    /// Inject the agent runtime backing `POST /ask`.
    #[must_use]
    pub fn with_runtime(mut self, runtime: Arc<dyn AgentRuntime + Send + Sync>) -> Self {
        self.runtime = runtime;
        self
    }
```

- [ ] **Step 5: Add the streaming sink + `ask_handler`**

In `crates/cairn-daemon/src/lib.rs`, add near the other sinks (after `EventTap`):

```rust
/// An `AgentSink` that forwards each agent increment as a wire `AnswerEvent`
/// over an mpsc channel. `blocking_send` gives natural backpressure and is
/// lossless (unlike the broadcast `/events` channel); a closed receiver (client
/// gone) turns sends into no-ops — the run still finishes (no v1 cancellation).
struct AnswerStreamSink {
    tx: tokio::sync::mpsc::Sender<AnswerEvent>,
}
impl AgentSink for AnswerStreamSink {
    fn emit(&mut self, event: AgentEvent) {
        if let Some(wire) = agent_event_to_wire(event) {
            let _ = self.tx.blocking_send(wire);
        }
    }
}
```

Add the handler near `query_handler`:

```rust
/// `POST /ask`: gather note context under the engine lock, release it, then
/// stream the agent answer as Server-Sent `AnswerEvent` frames. The agent run
/// (seconds; spawns a subprocess) is kept off the async reactor (`spawn_blocking`)
/// and off the engine mutex (released after the gather).
async fn ask_handler(State(state): State<AppState>, Json(req): Json<AskRequest>) -> Response {
    let top_k = req.top_k.unwrap_or(5);

    // 1. Gather context under the lock, in a blocking task (engine work blocks).
    let gather_state = state.clone();
    let query = req.query;
    let gathered =
        tokio::task::spawn_blocking(move || {
            let guard = gather_state.engine();
            gather_answer_context(&guard, &query, top_k)
        })
        .await;
    let (prompt, cited) = match gathered {
        Ok(Ok(v)) => v,
        Ok(Err(svc)) => return service_response::<()>(Ok(Err(svc))),
        Err(join) => return service_response::<()>(Err(join)),
    };

    // 2. Stream the agent run, lock-free.
    let (tx, rx) = tokio::sync::mpsc::channel::<AnswerEvent>(64);
    let runtime = state.runtime.clone();
    tokio::task::spawn_blocking(move || {
        // Sources first, then the agent increments.
        let _ = tx.blocking_send(AnswerEvent::Sources { paths: cited });
        let mut sink = AnswerStreamSink { tx };
        // A run that starts then fails reports via AgentEvent::Failed on the sink;
        // an Err means it failed before any event (e.g. NullRuntime) — surface it.
        if let Err(e) = runtime.answer(&prompt, &mut sink) {
            let _ = sink.tx.blocking_send(AnswerEvent::Failed {
                message: e.to_string(),
            });
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        let ev = rx.recv().await?;
        let frame = SseEvent::default()
            .json_data(&ev)
            .unwrap_or_else(|_| SseEvent::default().comment("serialize error"));
        Some((Ok::<_, std::convert::Infallible>(frame), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::new()).into_response()
}
```

- [ ] **Step 6: Register the route (token-gated)**

In `build_router`, add `/ask` to the `protected` group:
```rust
    let protected = Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route("/ask", post(ask_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));
```

- [ ] **Step 7: Run the integration test**

Run: `cargo test -p cairn-daemon --test ask ask_streams_sources_then_text_then_completed`
Expected: PASS.

- [ ] **Step 8: Wire the runtime at the composition root**

In `crates/cairn-daemon/src/main.rs`: ensure `use std::sync::Arc;` is present (add it to the imports if not). Before the `let state = AppState::new(engine)` block (~line 138), add:

```rust
    // Agent runtime for `POST /ask`: tau when configured, else NullRuntime (which
    // errors until TAU_BIN is set). Mirrors the CLI's `cairn ask` wiring.
    let runtime: Arc<dyn cairn_ports::AgentRuntime + Send + Sync> =
        match cairn_infra::TauConfig::from_env() {
            Some(cfg) => {
                tracing::info!("ask: tau runtime enabled");
                Arc::new(cairn_infra::TauServeRuntime::new(cfg))
            }
            None => {
                tracing::info!("ask: no TAU_BIN; /ask returns a configuration error");
                Arc::new(cairn_infra::NullRuntime)
            }
        };
```

Then extend the builder chain:
```rust
    let state = AppState::new(engine)
        .with_allowed_origins(cors_origins.clone())
        .with_token(token)
        .with_runtime(runtime);
```

If `cairn-ports` is not a direct dependency of the daemon binary's `main.rs` usage, it already is (listed in `Cargo.toml`), so the `cairn_ports::AgentRuntime` path resolves.

- [ ] **Step 9: Build the whole daemon crate (bin + lib)**

Run: `cargo build -p cairn-daemon`
Expected: builds clean (no warnings that fail CI).

- [ ] **Step 10: Commit**

```bash
git add crates/cairn-daemon/Cargo.toml crates/cairn-daemon/src/lib.rs crates/cairn-daemon/src/main.rs crates/cairn-daemon/tests/ask.rs Cargo.lock
git commit -m "feat(daemon): POST /ask SSE endpoint streaming AnswerEvents"
```

---

## Task 4: Daemon `/ask` error + auth paths

**Files:**
- Modify: `crates/cairn-daemon/tests/ask.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/cairn-daemon/tests/ask.rs`:

```rust
#[tokio::test]
async fn ask_without_token_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(
        AppState::new(engine(tmp.path()))
            .with_token(TOKEN)
            .with_runtime(Arc::new(StubRuntime)),
    );
    let resp = app.oneshot(ask_request(None, "q")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ask_with_null_runtime_emits_failed_frame() {
    let tmp = tempfile::tempdir().unwrap();
    // No `.with_runtime(...)` → AppState defaults to NullRuntime.
    let app = build_router(AppState::new(engine(tmp.path())).with_token(TOKEN));
    let resp = app.oneshot(ask_request(Some(TOKEN), "q")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK); // stream opened; failure is in-band
    let body = body_string(resp).await;
    assert!(body.contains("\"type\":\"sources\""), "expected sources frame:\n{body}");
    assert!(body.contains("\"type\":\"failed\""), "expected failed frame:\n{body}");
    assert!(body.contains("TAU_BIN"), "failed message should name TAU_BIN:\n{body}");
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p cairn-daemon --test ask`
Expected: PASS for all four tests in the file (no code changes needed — these exercise existing behavior: the token gate from Task 3's `protected` route, and the NullRuntime default that surfaces its `Err` as a `Failed` frame).

If `ask_with_null_runtime_emits_failed_frame` fails because the `Sources` frame is absent, that is correct behavior to keep — `Sources` is sent before `runtime.answer` is called, so it must appear even when the runtime errors. Investigate only if the ordering assertion or the `TAU_BIN` substring fails.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-daemon/tests/ask.rs
git commit -m "test(daemon): /ask auth gate + NullRuntime failed-frame"
```

---

## Task 5: Full gate + regenerate bindings for UI re-sync

**Files:** none new — verification + generated bindings.

- [ ] **Step 1: Regenerate the contract TypeScript bindings**

Run: `cargo test -p cairn-contract exports_typescript_bindings`
Expected: PASS; `git status` shows `crates/cairn-contract/bindings/AskRequest.ts` and `AnswerEvent.ts` present (committed in Task 1; confirm no further diff).

- [ ] **Step 2: Run the full local gate**

Run the repo's gate exactly as CI does (see `lefthook.yml` / the `just` recipes). At minimum:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
Expected: all green. Fix any fmt/clippy findings and amend the relevant task's commit.

- [ ] **Step 3: Push the branch and open the PR**

```bash
git push -u origin feat/ask-engine-seam
gh pr create --base main --title "feat: cairn ask engine seam (POST /ask SSE)" --body "Implements docs/superpowers/specs/2026-06-14-cairn-ask-engine-seam-design.md. Adds AskRequest/AnswerEvent contract types, splits augmented_answer into a lock-scoped gather + port→wire mapper, and adds a token-gated POST /ask SSE endpoint. Runtime injected from TAU_BIN at the daemon composition root (NullRuntime default)."
```

- [ ] **Step 4: Cross-repo follow-up (note, not executed here)**

After this PR merges in the engine repo, the **cairn-ui repo** re-syncs the vendored contract: run its contract-sync script so `web/src/contract/` gains `AskRequest.ts` + `AnswerEvent.ts` (committed raw; in `web/.prettierignore`). That satisfies this track's "Done" and unblocks Track 04's Wave-2 wiring. Track 04's mock must already use the exact tags/fields locked by Task 1's `answer_event_tags_match_the_track04_mock` test.

---

## Self-Review

**Spec coverage:**
- §1 Contract `AskRequest` + `AnswerEvent` → Task 1. ✓ (struct variants, closed enum, `Sources` wire-only — all encoded)
- §2 Service `gather_answer_context` + `agent_event_to_wire`, `augmented_answer` kept/composed → Task 2. ✓
- §3 Runtime injection (`Arc<dyn AgentRuntime + Send + Sync>`, composition root, `NullRuntime` default) → Task 3 steps 4 & 8. ✓
- §4 Daemon `POST /ask` SSE, lock released before agent run, `spawn_blocking` → Task 3 step 5. ✓
- §5 Error handling: pre-stream → HTTP error (`service_response`); NullRuntime → `Failed` frame; in-run `Failed` passthrough → Task 3 step 5 + Task 4. ✓
- §6 Testing: contract codegen + wire-tag (Task 1), service unit (Task 2), daemon integration incl. auth + NullRuntime (Tasks 3-4). ✓
- §7 Re-sync + Track 04 tag-lock → Task 1 test + Task 5 step 4. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. Task 3 step 3 asks the implementer to confirm `http-body-util` in dev-deps by mirroring an existing test — this is verification of an existing fact, not a placeholder (the crate is already used by `tests/http.rs`).

**Type consistency:** `gather_answer_context(&Engine, &str, usize) -> Result<(String, Vec<String>), ServiceError>`, `agent_event_to_wire(AgentEvent) -> Option<AnswerEvent>`, `AppState::with_runtime(Arc<dyn AgentRuntime + Send + Sync>)`, `AnswerStreamSink { tx: mpsc::Sender<AnswerEvent> }`, and the `AnswerEvent` variant/field names (`Sources{paths}`, `TextDelta{text}`, `ToolCompleted{tool,ok}`, `Failed{message}`) are used identically across Tasks 1-4. ✓
