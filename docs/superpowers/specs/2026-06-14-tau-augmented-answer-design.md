# Tau augmented answer — note-grounded streaming agent responses

**Issue:** first slice of cairn↔tau integration — replacing the `NullRuntime`
seam the README has promised since the walking skeleton.

**Builds on:** the [transport ADR](../../decisions/0002-transport.md) (daemon
already serves HTTP + WebSocket with an `/events` channel and an `EventSink`
event model), the existing `AgentRuntime` port (`crates/cairn-ports/src/lib.rs:265`)
and its `NullRuntime` seam (`crates/cairn-infra/src/seams.rs:41`), and the
existing engine search/read use-cases.

**Tau side (pinned facts, not built here):** tau exposes **serve mode** —
JSON-RPC 2.0 over NDJSON on stdio, one of its two stable embedding surfaces.
`meta.handshake` → `runtime.run_streaming {agent, prompt}` → a stream of
`runtime.event` notifications (`kind`: `TextDelta`, `ToolCallStarted`,
`ToolCallCompleted`, `TurnCompleted`, `RunCompleted`, `FatalError`) →
optional `runtime.cancel {id}`. `RunEvent` is `#[non_exhaustive]` upstream:
unknown `kind`s MUST render generically, never panic. (Reference client:
`tau-rs/tau-web-ui`.)

## Problem

Cairn is "built for first-class integration with tau," but the seam is a stub:
`AgentRuntime::run_action(action, context) -> String` returns one blob and
`NullRuntime` only errors. Nothing calls it. Two things are wrong for the first
real use case — **ask a question, get a note-grounded answer**:

1. **No streaming.** `run_action` returns a single `String`. Tau's whole value
   in serve mode is the incremental `TextDelta` stream; a blocking blob discards
   it and makes the answer feel dead.
2. **Wrong locus is tempting.** Routing this through the plugin host would hit a
   structural wall: the plugin protocol invoke is one request → one `Response`
   (`crates/cairn-plugin-protocol/src/lib.rs:51`, `Incoming` at `:248`) with no
   partial frame, a 30s per-message read timeout
   (`crates/cairn-infra/src/plugin_host.rs:43`) that a multi-turn agent run trips,
   and a grandchild-process lifecycle the host explicitly warns about
   (`plugin_host.rs:377`). A long-lived streaming agent is not a plugin command.

## Scope (smallest viable increment)

Ship **`cairn ask <query>`**: retrieve relevant notes, stream a tau agent's
answer to stdout token-by-token, grounded in those notes. Prove the seam
end-to-end, in-repo, without coordinating the web UI.

In scope:
- A daemon-supervised, long-lived `tau serve` **sidecar** (one process).
- A serve-mode NDJSON JSON-RPC **client**.
- The `AgentRuntime` port **reshaped to stream** via cairn's existing `EventSink`.
- A retrieval step reusing existing `search` + `read`.
- The `cairn ask` CLI command, streaming to stdout.

Explicitly **deferred** (see Non-goals):
- **centaur and all dataflow pipelines** — immature (CSV-only, pre-UI-bridge).
- Web-import and learning-card pipelines (depend on centaur).
- Pipeline editor UI and the pipeline-as-note format.
- cairn-as-MCP-server (autonomous agents holding note tools).
- The **web chat panel** in `cairn-web-ui` — **v1.1**, reuses the same `/events`
  WS path this slice establishes.

## Design

Core stays pure: `cairn-domain` and `cairn-service` never import tau. The tau
binary is reached only from the daemon-process (infra/transport layer) and the
adapter that lives behind the `AgentRuntime` port in `cairn-infra`.

```
 cairn ask ──► engine use-case "augmented answer"
                 │  1. search(query) → top-K hits         (existing)
                 │  2. read_note(hits) → context          (existing)
                 │  3. AgentRuntime::answer(prompt, sink)  (reshaped port)
                 ▼
        TauServeRuntime (cairn-infra)
                 │  serve-mode client over the sidecar's stdio
                 ▼
        tau serve  (one long-lived process, daemon-supervised)
                 │  runtime.run_streaming → runtime.event*
                 ▼
        each event → EventSink  ──► stdout (CLI)  /  /events WS (daemon, later UI)
```

### The reshaped `AgentRuntime` port (`cairn-ports`)

Replace the blob-returning `run_action` with a streaming call that pushes
typed events into a sink the caller supplies. The port names no tau type; events
are cairn's own vocabulary.

```rust
/// One increment of an agent run, in cairn's vocabulary (not tau's wire enum).
#[non_exhaustive]
pub enum AgentEvent {
    TextDelta(String),
    ToolStarted { tool: String },
    ToolCompleted { tool: String, ok: bool },
    TurnCompleted,                    // usage optional, omitted in v1
    Completed,
    Failed { message: String },
}

pub trait AgentRuntime {
    /// Run an agent over `prompt`, pushing each increment to `sink` until the
    /// run completes or fails. Returns when the run terminates.
    ///
    /// # Errors
    /// [`PortError`] if no runtime is configured or the transport fails before
    /// any event is delivered.
    fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError>;
}

pub trait AgentSink { fn emit(&mut self, event: AgentEvent); }
```

`AgentEvent` is `#[non_exhaustive]` so an unknown tau `kind` maps to a tolerated
generic increment rather than a panic. `NullRuntime` is updated to emit a single
`Failed { message: "no agent runtime configured" }` (or return `PortError`),
keeping the engine composable with no tau present.

> Open call to resolve in the plan: whether `AgentSink` is a fresh trait or the
> existing `EventSink` extended with an agent-event variant. Default: a small
> dedicated `AgentSink` to avoid coupling the wire-event enum to agent runs.

### `TauServeRuntime` adapter (`cairn-infra`)

The real `AgentRuntime`. Holds a handle to the supervised sidecar and speaks
serve mode:

- On first use (or at supervisor start): `meta.handshake {protocol_version: 1}`;
  fail fast on `-32000` version mismatch.
- `answer`: send `runtime.run_streaming {agent, prompt}` with the configured
  agent id; read `runtime.event` frames whose `params.id` matches the request;
  translate each `kind` → `AgentEvent` and `sink.emit`; stop on `RunCompleted`
  (→ `Completed`) or `FatalError` (→ `Failed`). Unknown `kind` → a generic
  tolerated event.
- Cancellation (Ctrl-C in the CLI) → `runtime.cancel {id}`.

### The sidecar supervisor (`cairn-daemon`, with a reusable core in `cairn-infra`)

One long-lived `tau serve`, owned and supervised by the daemon process the same
way the plugin host is:

- **Spawn:** `Command::new(<tau_bin>) serve --project <root>` with piped stdio;
  read the readiness line on stderr (`ready_on_stderr`) before first use.
- **Health:** periodic `meta.ping`; on failure or child exit, restart with
  bounded backoff.
- **Shutdown:** drop stdin → wait → kill-after-grace. Because tau owns its own
  process family and sandbox, cairn does not nest sandboxes around it.
- **Absent/misconfigured tau:** supervisor stays down; the `AgentRuntime` stays
  `NullRuntime`; `cairn ask` reports a clear "tau not configured" error. Default
  build composes and runs with no tau, exactly as today.

The CLI path (`cairn ask`) runs the engine in-process and may spawn its own
short-lived `tau serve`, or require a running daemon — to be settled in the plan;
default: the CLI spawns its own sidecar for a self-contained one-shot, mirroring
how `cairn-cli` already runs the engine in-process.

### Retrieval (engine use-case, `cairn-service`)

A new "augmented answer" use-case composes existing pieces, no new I/O:
`search(query)` → take top-K → `read_note` each → assemble a prompt
(system preamble + concatenated context with note paths as citations + the
question). Pure orchestration over ports already present; emits the assembled
citation list so the CLI can show which notes grounded the answer.

### CLI `cairn ask <query>` (`cairn-cli`)

A new `Command::Ask { query }` alongside `Init/Write/Read/Search/Watch`. Drives
the use-case with a stdout-backed `AgentSink` (mirroring the existing
`WatchSink: EventSink`), printing `TextDelta`s as they arrive and a trailing
list of cited notes. Ctrl-C cancels the run.

### Config

A `[tau]` section in `cairn.toml`: `bin` (path to the tau binary), `agent`
(agent id to invoke), `project` (tau project dir; defaults to the cairn root),
and an `enabled`/presence gate. Absent ⇒ `NullRuntime` (default-off, mirroring
the plugin `trusted` default-deny posture).

## Testing

- **Port/adapter:** a fake serve-mode peer (NDJSON in-memory pipe, à la the
  plugin host's test fixtures) feeding a scripted `runtime.event` sequence —
  assert correct `AgentEvent` translation, unknown-`kind` tolerance, and
  `FatalError` → `Failed`.
- **Supervisor:** spawn a stub binary that speaks the handshake + a canned run;
  assert ready-line detection, ping/restart on exit, clean shutdown (no orphan).
- **Retrieval use-case:** in-memory engine with seeded notes; assert top-K
  selection and prompt/citation assembly. Deterministic, no tau.
- **CLI:** `cairn ask` against the stub binary; assert streamed stdout ordering
  and the cited-notes footer.
- **Default-off:** with no `[tau]` config, `cairn ask` errors cleanly and the
  rest of the engine is unaffected.
- A live end-to-end test against a real `tau` self-skips when `TAU_BIN` is unset
  (the centaur pattern), so CI stays hermetic.

## Non-goals (deferred)

- **centaur / dataflow pipelines** — not mature enough for v1 (CSV-only,
  pre-UI-bridge). All batch use cases (web import, learning cards, synthesis)
  wait on it.
- **Pipeline editor UI** and the **pipeline-as-note** format.
- **cairn-as-MCP-server** — letting tau agents read/search/write notes as tools
  autonomously. v1 injects retrieved context into the prompt instead.
- **Web chat panel** (`cairn-web-ui`) — v1.1; reuses this slice's `/events` WS.
- **Token-usage accounting / multi-agent orchestration** — later.

## Follow-up issues (opened on merge)

- Web chat panel in `cairn-web-ui` consuming the `/events` WS (v1.1).
- Promote retrieval to cairn-as-MCP-server so agents fetch notes as tools.
- Reassess centaur maturity for the first batch pipeline (web import).
