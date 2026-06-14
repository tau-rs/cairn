# `cairn ask` engine seam — wire transport design

**Date:** 2026-06-14
**Status:** Approved (design); implementation pending
**Branch:** `feat/ask-engine-seam`
**Track:** 03 of the parallel cairn-ui handoff (Rust, engine repo)

## Problem

The engine can answer note-grounded questions (`cairn ask`, PR #64): `cairn_service::augmented_answer`
searches the top-k notes, builds a prompt, and streams `AgentEvent`s (token deltas, tool
start/stop, completed/failed) to an `AgentSink`. Today this is reachable **only from the CLI**,
in-process. It is **not** on the wire contract, so no connected client (daemon, future remote UI)
can reach it.

This track exposes that streaming answer over a **transport-neutral, contract-native seam**, with
the daemon as the demonstrable client.

## Constraints discovered (engine @ `51cdfd1`)

- The wire contract (`crates/cairn-contract/src/lib.rs`) has clean single-response shapes:
  `Command` → `CommandResponse`, `Query` → `QueryResponse`. The `Event` enum is **closed**
  (no `#[non_exhaustive]`) and is delivered over the daemon's WS `/events` as a **lossy,
  fire-and-forget broadcast** (`tokio::broadcast::channel(256)`; lagged subscribers drop frames)
  with **no request correlation**.
- `dispatch_command` runs **synchronously under `Mutex<Engine>`** via `spawn_blocking`.
- `augmented_answer(&Engine, query, &dyn AgentRuntime, &mut dyn AgentSink, top_k)
  -> Result<Vec<String>, ServiceError>`: search + read note contents touch `&Engine`; then
  `runtime.answer(prompt, sink)` runs a `tau serve` subprocess for seconds.
- `AgentEvent` (`crates/cairn-ports`) is `#[non_exhaustive]` and **not** serde/ts-rs.
- The default `AgentRuntime` is `NullRuntime`, which errors `"no agent runtime configured
  (set TAU_BIN to enable \`cairn ask\`)"`.
- `crates/cairn-daemon` **already depends on `cairn-infra`**; `crates/cairn-daemon/src/main.rs`
  is the composition root that builds `AppState::new(engine)`.

### Why not the handoff's literal "put agent events on the wire `Event` stream"

The broadcast `Event` channel is **lossy** (dropping a token delta corrupts the answer),
**uncorrelated** (two concurrent asks interleave), and `dispatch_command` would **hold the engine
lock** for the entire multi-second agent run, blocking every other command/query. Answer streaming
is request-scoped, ordered, and lossless — a different shape from the closed, broadcast domain-event
log. So it gets its own contract vocabulary and its own delivery path.

## Decisions

| # | Decision |
|---|----------|
| Scope | Land contract-native vocabulary + a lock-safe service seam; **daemon** is the demonstrable client. Tauri in-process delivery is a Wave-2 UI-repo follow-up reusing the same types. |
| Contract shape | **Dedicated** `AskRequest` + `AnswerEvent`. The existing `Command` / `Query` / `Event` enums stay pure (each maps to a single response). |
| Wire framing | **SSE** (`text/event-stream`) over an **authenticated POST**, consumed via `fetch` + `ReadableStream` — the industry-standard LLM-streaming shape (OpenAI/Anthropic/Vercel), minus the browser `EventSource` API (which cannot send the daemon's bearer token). |
| Concurrency | The agent run must **not** hold the engine `Mutex` and must **not** block the async reactor. |

## Design

### 1. Contract vocabulary (`crates/cairn-contract/src/lib.rs`)

Two new `#[ts(export)]` types. Existing enums untouched.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct AskRequest {
    pub query: String,
    pub top_k: Option<usize>,   // None => 5 (the CLI default)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerEvent {
    Sources { paths: Vec<String> },        // cited notes; emitted FIRST, from the search step
    TextDelta { text: String },
    ToolStarted { tool: String },
    ToolCompleted { tool: String, ok: bool },
    TurnCompleted,
    Completed,
    Failed { message: String },
}
```

Three deliberate choices:

- **Struct variants, not tuple.** `AgentEvent::TextDelta(String)` is a newtype variant;
  `#[serde(tag = "type")]` (internally tagged) **cannot** serialize newtype variants, so the wire
  form must be `TextDelta { text }`. Forced, not stylistic.
- **`AnswerEvent` is closed** (no `#[non_exhaustive]`), unlike the port's `AgentEvent`. The port
  enum stays open because it shadows tau's external vocabulary; the *wire* enum is cairn's own
  contract — closed like `Event`, so the UI matches exhaustively.
- **`Sources` is wire-only.** It carries `augmented_answer`'s `Vec<String>` cited-paths return
  (the CLI prints these as "sources:"), surfaced as a leading frame so the chat panel can show
  citations before the first token.

### 2. Service seam + lock-safety (`crates/cairn-service/src/lib.rs`)

Split `augmented_answer`'s two concerns so a caller can drop the engine lock before the agent run:

```rust
// Engine-touching half — runs UNDER the lock (search top-k + read note contents).
pub fn gather_answer_context(engine: &Engine, query: &str, top_k: usize)
    -> Result<(String /*prompt*/, Vec<String> /*cited*/), ServiceError>;

// port event -> wire event. None == skip (the #[non_exhaustive] wildcard arm,
// mirroring the CLI's "ignore unknown kinds").
pub fn agent_event_to_wire(e: AgentEvent) -> Option<AnswerEvent>;
```

`augmented_answer` is **kept**, rewritten as a thin composition of `gather_answer_context` +
`runtime.answer`, so the **CLI call site does not change** (single-threaded, no lock concern).
The daemon calls the two pieces separately: gather under the lock, release, then run the agent
lock-free.

### 3. Runtime injection (composition root)

`AppState` gains `runtime: Arc<dyn AgentRuntime + Send + Sync>` — the **port**, so the daemon *lib*
stays concrete-free. The composition root `crates/cairn-daemon/src/main.rs` builds the concrete
`cairn_infra::TauServeRuntime` from `TauConfig::from_env()` (exactly as the CLI does), defaulting to
`NullRuntime` when `TAU_BIN` is unset, and injects it via an extended `AppState::new` (or a
`with_runtime` builder). `cairn-infra` is already a daemon dependency — no `Cargo.toml` change.
Test doubles inject a stub runtime the same way (this is what makes the endpoint testable).

`AgentRuntime::answer(&self, …)` takes `&self`, so an `Arc<dyn AgentRuntime + Send + Sync>` is
shareable across request tasks. (Implementation note: confirm `TauServeRuntime: Send + Sync` — it
holds only `TauConfig` (strings/paths) — and add the `Send + Sync` bound at the trait-object site.)

### 4. Daemon endpoint (`crates/cairn-daemon/src/lib.rs`)

`POST /ask`, body `AskRequest`, behind the **existing** bearer-token + origin middleware. Handler:

1. Lock engine → `gather_answer_context(&engine, &req.query, top_k.unwrap_or(5))` → **unlock**.
   (Brief: search + reads.)
2. `tokio::sync::mpsc::channel`; send `AnswerEvent::Sources { paths: cited }` first.
3. `tokio::task::spawn_blocking`: build `AnswerStreamSink { tx }` (impl `AgentSink::emit` — maps
   `AgentEvent` → `AnswerEvent` via `agent_event_to_wire`; `tx.blocking_send` for natural
   backpressure and losslessness). Call `runtime.answer(&prompt, &mut sink)`. On `Err` → send one
   `Failed { message }` frame. Drop `tx` → stream closes.
4. Return axum `Sse<ReceiverStream<…>>` → `text/event-stream`, one `data: {AnswerEvent json}\n\n`
   per frame.

This keeps the agent run **off the async reactor** (`spawn_blocking`) and **off the engine mutex**
(released after step 1) — the daemon stays responsive to other `/command` / `/query` traffic during
an answer. `blocking_send` is lossless and back-pressured, unlike the broadcast `/events` channel.

### 5. Error handling

- **Pre-stream** (gather fails — bad query, IO): HTTP non-200 with `ContractError`, consistent with
  `/command` and `/query`. The SSE stream only begins once the agent is running.
- **`NullRuntime`** (TAU_BIN unset): `runtime.answer` returns `Err` → single
  `Failed { "no agent runtime configured (set TAU_BIN…)" }` frame, then close. The chat panel
  renders a failed answer rather than a transport error.
- **In-run failure** (subprocess dies mid-answer): the port contract already delivers
  `AgentEvent::Failed` → `AnswerEvent::Failed` frame. (Port doc: `Err` = "failed before any event";
  `Failed` event = "started then failed".)

### 6. Testing (TDD — part of done)

- **Contract**: extend `crates/cairn-contract/tests/codegen.rs` to assert `AskRequest` /
  `AnswerEvent` declare + export; binding files land in `bindings/`.
- **Service**: unit-test `gather_answer_context` (cited paths returned; prompt contains note
  context) and `agent_event_to_wire` (every variant + wildcard → `None`). Reuse the existing
  fake-runtime + `VecSink` rig.
- **Daemon**: integration test injecting a `StubRuntime` that emits a scripted `AgentEvent`
  sequence; assert the SSE body carries `Sources` first, then deltas in order, ending `completed`.
  Plus: auth gate (401 without token), origin gate, and `NullRuntime` → `Failed` frame.

### 7. Contract re-sync + Track 04 coordination

- After engine merge: run the contract codegen → new `AskRequest.ts` / `AnswerEvent.ts`, then
  re-sync into the UI repo `web/src/contract/` via the sync script (committed raw; in
  `web/.prettierignore`). This UI-repo follow-up satisfies this track's "Done".
- **Track 04's mock must use these exact wire tags/fields** so Wave-2 wiring is a drop-in:
  tags `sources` / `text_delta` / `tool_started` / `tool_completed` / `turn_completed` /
  `completed` / `failed`; fields `paths`, `text`, `tool`, `ok`, `message`.

## Out of scope (YAGNI for v1)

- Token-usage metrics.
- Multi-turn / conversation history (each `/ask` is independent).
- A server-side cancellation endpoint — the client cancels by dropping the connection; the
  `spawn_blocking` task observes the closed `mpsc` receiver on its next `blocking_send`.

## Done

A running daemon streams agent answers (`POST /ask` → SSE of `AnswerEvent`) to a connected client;
contract bindings regenerated and re-synced into the UI repo `web/src/contract/`.
