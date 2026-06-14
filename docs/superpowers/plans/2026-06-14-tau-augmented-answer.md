# Tau Augmented Answer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `cairn ask <query>` — retrieve relevant notes and stream a tau agent's answer to stdout token-by-token, grounded in those notes.

**Architecture:** A one-shot `tau serve` subprocess speaks JSON-RPC 2.0 over NDJSON on stdio. The `AgentRuntime` port is reshaped from a blocking `run_action(...) -> String` into a streaming `answer(prompt, sink)` that pushes `AgentEvent`s into a caller-supplied `AgentSink`. A new `cairn-service::augmented_answer` orchestrates search → read → prompt → run, *outside* the `Engine` so `cairn-domain`/`cairn-app` stay pure (the `Engine` never learns the word "tau"). The CLI wires a stdout sink. Batch/centaur pipelines are explicitly out of scope (deferred per spec).

**Tech Stack:** Rust (sync, no tokio), `serde_json` for the wire, `std::process` for the subprocess, `thiserror`-backed `PortError` at the boundary. Spec: `docs/superpowers/specs/2026-06-14-tau-augmented-answer-design.md`.

---

## File Structure

- `crates/cairn-ports/src/lib.rs` — **modify** (~line 264-271): replace the `AgentRuntime` trait; add `AgentEvent` enum + `AgentSink` trait. The new contract.
- `crates/cairn-infra/src/seams.rs` — **modify** (~line 38-50, 52-70): update `NullRuntime` to the new trait; update its seam test.
- `crates/cairn-infra/src/tau/mod.rs` — **create**: module root, re-exports.
- `crates/cairn-infra/src/tau/wire.rs` — **create**: JSON-RPC serde types + `runtime.event` → `AgentEvent` mapping.
- `crates/cairn-infra/src/tau/client.rs` — **create**: `ServeClient<R: BufRead, W: Write>` — handshake + `run_streaming`. Generic, so the protocol is unit-tested with in-memory pipes, no process.
- `crates/cairn-infra/src/tau/config.rs` — **create**: `TauConfig` + env loader.
- `crates/cairn-infra/src/tau/process.rs` — **create**: `TauServe` — spawn `tau serve`, await readiness, own the `ServeClient`, kill on drop.
- `crates/cairn-infra/src/tau/runtime.rs` — **create**: `TauServeRuntime` implementing `AgentRuntime` (one-shot spawn per `answer`).
- `crates/cairn-infra/src/lib.rs` — **modify** (~line 3-25): declare `pub mod tau;` and re-export `TauServeRuntime`, `TauConfig`.
- `crates/cairn-service/src/lib.rs` — **modify**: add `augmented_answer` + `build_answer_prompt`.
- `crates/cairn-service/Cargo.toml` — **modify**: add `cairn-startup` to `[dev-dependencies]`.
- `crates/cairn-cli/src/main.rs` — **modify**: add `Command::Ask`, `AgentStdoutSink`, dispatch arm, reindex gate.
- `README.md` — **modify**: status line (tau seam now partially wired).

---

### Task 1: Reshape the `AgentRuntime` port

Replace the blocking blob contract with a streaming one. This is a cross-crate atomic change: the port (`cairn-ports`) and its only implementor `NullRuntime` (`cairn-infra`) move together, so the workspace stays compiling.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs:264-271`
- Modify: `crates/cairn-infra/src/seams.rs:4-6,38-50,52-70`

- [ ] **Step 1: Replace the trait + add the event/sink types in `cairn-ports`**

Replace the existing block at `crates/cairn-ports/src/lib.rs:264-271`:

```rust
/// One increment of an agent run, in cairn's own vocabulary — deliberately not
/// tau's wire enum, so the port names no external type. `#[non_exhaustive]`:
/// adapters map unknown upstream event kinds to nothing rather than panicking,
/// and downstream `match`es must carry a wildcard arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AgentEvent {
    /// A chunk of answer text.
    TextDelta(String),
    /// The agent began a tool call.
    ToolStarted { tool: String },
    /// A tool call finished; `ok` is false if the tool reported an error.
    ToolCompleted { tool: String, ok: bool },
    /// One agent turn completed (token usage omitted in v1).
    TurnCompleted,
    /// The run finished successfully.
    Completed,
    /// The run failed; `message` is human-readable.
    Failed { message: String },
}

/// Receives [`AgentEvent`]s as a run streams. The caller owns rendering
/// (stdout in the CLI, a WebSocket later).
pub trait AgentSink {
    /// Handle one streamed increment.
    fn emit(&mut self, event: AgentEvent);
}

/// Agent runtime (tau). Seam: `NullRuntime`.
pub trait AgentRuntime {
    /// Run an agent over `prompt`, pushing each increment to `sink` until the
    /// run completes or fails. Returns when the run terminates.
    ///
    /// # Errors
    /// Returns [`PortError`] if no runtime is configured or the transport fails
    /// before any event is delivered. A run that starts and then fails is
    /// reported via an [`AgentEvent::Failed`] on `sink`, not an `Err`.
    fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError>;
}
```

- [ ] **Step 2: Update `NullRuntime` and its seam test in `cairn-infra`**

In `crates/cairn-infra/src/seams.rs`, change the import line (4-6) to add the new types and drop nothing else:

```rust
use cairn_ports::{
    AgentEvent, AgentRuntime, AgentSink, CollabSession, Executor, FsChange, PortError,
    WatchHandle, Watcher,
};
```

Replace the `NullRuntime` impl (38-50):

```rust
/// Null agent runtime seam.
#[derive(Debug, Default)]
pub struct NullRuntime;
impl AgentRuntime for NullRuntime {
    fn answer(&self, _prompt: &str, _sink: &mut dyn AgentSink) -> Result<(), PortError> {
        Err(PortError::Adapter(
            "no agent runtime configured (set TAU_BIN to enable `cairn ask`)".into(),
        ))
    }
}
```

In the test module (52-70), replace the `NullRuntime` assertion line:

```rust
        // Collect into a Vec sink; NullRuntime errors before emitting anything.
        struct NoopSink;
        impl AgentSink for NoopSink {
            fn emit(&mut self, _e: AgentEvent) {}
        }
        assert!(NullRuntime.answer("summarize this", &mut NoopSink).is_err());
```

- [ ] **Step 3: Add a port-level test for the streaming contract**

Append to `crates/cairn-ports/src/lib.rs` (end of file), a test proving a runtime can stream into a sink:

```rust
#[cfg(test)]
mod agent_runtime_tests {
    use super::*;

    struct TwoChunkRuntime;
    impl AgentRuntime for TwoChunkRuntime {
        fn answer(&self, _prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
            sink.emit(AgentEvent::TextDelta("Hel".into()));
            sink.emit(AgentEvent::TextDelta("lo".into()));
            sink.emit(AgentEvent::Completed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    #[test]
    fn runtime_streams_events_into_sink() {
        let mut sink = VecSink::default();
        TwoChunkRuntime.answer("hi", &mut sink).unwrap();
        assert_eq!(
            sink.0,
            vec![
                AgentEvent::TextDelta("Hel".into()),
                AgentEvent::TextDelta("lo".into()),
                AgentEvent::Completed,
            ]
        );
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo test -p cairn-ports -p cairn-infra`
Expected: PASS (both the new port test and the updated seam test).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/seams.rs
git commit -m "feat(ports): reshape AgentRuntime into a streaming answer(prompt, sink)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Tau wire types + event mapping

**Files:**
- Create: `crates/cairn-infra/src/tau/mod.rs`
- Create: `crates/cairn-infra/src/tau/wire.rs`
- Modify: `crates/cairn-infra/src/lib.rs:3-4` (declare the module)

- [ ] **Step 1: Declare the module**

In `crates/cairn-infra/src/lib.rs`, add after `pub mod sandbox;` (keep alpha-ish order, anywhere in the `pub mod` block is fine):

```rust
pub mod tau;
```

- [ ] **Step 2: Create the module root**

Create `crates/cairn-infra/src/tau/mod.rs`:

```rust
//! Adapter for the tau agent runtime over its serve-mode protocol
//! (JSON-RPC 2.0 over NDJSON on stdio). See
//! `docs/superpowers/specs/2026-06-14-tau-augmented-answer-design.md`.

pub mod client;
pub mod config;
pub mod process;
pub mod runtime;
pub mod wire;

pub use config::TauConfig;
pub use runtime::TauServeRuntime;
```

> Note: `mod.rs` references modules created in later tasks. The crate will not
> compile until Task 3-6 land. That is expected; do not run `cargo build` on the
> whole crate until Task 6. Each of Tasks 2-6 commits a file; the green-bar
> checkpoint is Task 6 Step (build). Tests for `wire` in this task compile
> standalone via `cargo test -p cairn-infra --lib tau::wire` only after the
> sibling modules exist — so this task's verification is deferred to Task 6.
> If you prefer a green bar per task, create empty stub files for the four
> sibling modules now (`pub` nothing) and fill them in their tasks.

- [ ] **Step 3: Write the failing test for event mapping**

Create `crates/cairn-infra/src/tau/wire.rs` with the test first:

```rust
//! Serde types for tau serve-mode JSON-RPC, and the mapping from tau's
//! `runtime.event` kinds to cairn's `AgentEvent`.

use cairn_ports::AgentEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An outbound JSON-RPC request.
#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub jsonrpc: &'a str,
    pub id: u64,
    pub method: &'a str,
    pub params: Value,
}

/// An inbound line: a response (`id` + `result`/`error`) or a notification
/// (`method` + `params`). Untagged — fields are optional and decoded leniently.
#[derive(Debug, Default, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
}

/// Map a `runtime.event` `(kind, data)` to a cairn [`AgentEvent`].
/// Returns `None` for unknown kinds (tolerated, never panics — tau's event enum
/// is `#[non_exhaustive]` upstream).
pub fn map_event(kind: &str, data: &Value) -> Option<AgentEvent> {
    let str_field = |k: &str| data.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match kind {
        "TextDelta" => Some(AgentEvent::TextDelta(str_field("text"))),
        "ToolCallStarted" => Some(AgentEvent::ToolStarted { tool: str_field("tool") }),
        "ToolCallCompleted" => {
            let is_error = data
                .get("result")
                .and_then(|r| r.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(AgentEvent::ToolCompleted { tool: str_field("tool"), ok: !is_error })
        }
        "TurnCompleted" => Some(AgentEvent::TurnCompleted),
        "RunCompleted" => Some(AgentEvent::Completed),
        "FatalError" => Some(AgentEvent::Failed {
            message: data.get("message").and_then(Value::as_str).unwrap_or("agent run failed").to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_known_kinds() {
        assert_eq!(
            map_event("TextDelta", &json!({"text": "hi"})),
            Some(AgentEvent::TextDelta("hi".into()))
        );
        assert_eq!(map_event("RunCompleted", &json!({})), Some(AgentEvent::Completed));
        assert_eq!(
            map_event("ToolCallStarted", &json!({"tool": "fs-read"})),
            Some(AgentEvent::ToolStarted { tool: "fs-read".into() })
        );
        assert_eq!(
            map_event("ToolCallCompleted", &json!({"tool": "fs-read", "result": {"is_error": true}})),
            Some(AgentEvent::ToolCompleted { tool: "fs-read".into(), ok: false })
        );
        assert_eq!(
            map_event("FatalError", &json!({"message": "boom"})),
            Some(AgentEvent::Failed { message: "boom".into() })
        );
    }

    #[test]
    fn unknown_kind_is_tolerated() {
        assert_eq!(map_event("SomeFutureKind", &json!({})), None);
    }

    #[test]
    fn incoming_decodes_notification_and_response() {
        let note: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"runtime.event","params":{"id":4,"kind":"TextDelta","data":{"text":"x"}}}"#).unwrap();
        assert_eq!(note.method.as_deref(), Some("runtime.event"));
        assert_eq!(note.params.get("id").and_then(|v| v.as_u64()), Some(4));

        let resp: Incoming = serde_json::from_str(r#"{"jsonrpc":"2.0","id":4,"result":{}}"#).unwrap();
        assert_eq!(resp.id, Some(4));
        assert!(resp.result.is_some());
    }
}
```

- [ ] **Step 4: Commit (verification deferred to Task 6 build)**

```bash
git add crates/cairn-infra/src/lib.rs crates/cairn-infra/src/tau/mod.rs crates/cairn-infra/src/tau/wire.rs
git commit -m "feat(infra): tau serve-mode wire types + event mapping

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Serve-mode client (generic, in-memory tested)

The protocol logic, generic over reader/writer so it is fully unit-testable without a subprocess. This is the task with the real protocol coverage.

**Files:**
- Create: `crates/cairn-infra/src/tau/client.rs`

- [ ] **Step 1: Write the implementation + test**

Create `crates/cairn-infra/src/tau/client.rs`:

```rust
//! The serve-mode JSON-RPC client, generic over a line reader and writer.

use std::io::{BufRead, Write};

use cairn_ports::{AdapterError, AgentEvent, AgentSink, PortError};
use serde_json::json;

use crate::tau::wire::{map_event, Incoming, Request};

/// Speaks tau serve-mode over any line-oriented transport.
pub struct ServeClient<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    next_id: u64,
}

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

impl<R: BufRead, W: Write> ServeClient<R, W> {
    /// Wrap an already-connected transport (the subprocess's stdout/stdin, or
    /// an in-memory pipe in tests).
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer, next_id: 0 }
    }

    fn send(&mut self, method: &str, params: serde_json::Value) -> Result<u64, PortError> {
        self.next_id += 1;
        let id = self.next_id;
        let req = Request { jsonrpc: "2.0", id, method, params };
        let line = serde_json::to_string(&req).map_err(adapt)?;
        self.writer.write_all(line.as_bytes()).map_err(adapt)?;
        self.writer.write_all(b"\n").map_err(adapt)?;
        self.writer.flush().map_err(adapt)?;
        Ok(id)
    }

    /// Read the next non-blank line as an [`Incoming`]; `Ok(None)` on EOF.
    fn read_msg(&mut self) -> Result<Option<Incoming>, PortError> {
        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf).map_err(adapt)?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(Some(serde_json::from_str(trimmed).map_err(adapt)?));
        }
    }

    /// Perform the protocol handshake. Errors on version mismatch or EOF.
    pub fn handshake(&mut self) -> Result<(), PortError> {
        let id = self.send(
            "meta.handshake",
            json!({"client_name": "cairn", "client_version": "0.0.0", "protocol_version": 1}),
        )?;
        loop {
            match self.read_msg()? {
                None => {
                    return Err(PortError::Adapter(
                        "tau serve closed during handshake".into(),
                    ))
                }
                Some(msg) if msg.id == Some(id) => {
                    if let Some(err) = msg.error {
                        return Err(PortError::Adapter(
                            format!("tau handshake rejected: {err}").into(),
                        ));
                    }
                    return Ok(());
                }
                Some(_) => continue, // ignore stray notifications before the reply
            }
        }
    }

    /// Run `agent` over `prompt`, emitting each increment to `sink`. Terminates
    /// on `RunCompleted`/`FatalError` or the matching JSON-RPC response,
    /// whichever arrives first.
    pub fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError> {
        let id = self.send("runtime.run_streaming", json!({"agent": agent, "prompt": prompt}))?;
        loop {
            match self.read_msg()? {
                None => {
                    sink.emit(AgentEvent::Failed { message: "tau serve closed mid-run".into() });
                    return Ok(());
                }
                Some(msg) if msg.method.as_deref() == Some("runtime.event") => {
                    if msg.params.get("id").and_then(|v| v.as_u64()) != Some(id) {
                        continue;
                    }
                    let kind = msg.params.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    let data = msg.params.get("data").cloned().unwrap_or(serde_json::Value::Null);
                    if let Some(ev) = map_event(kind, &data) {
                        let done = matches!(ev, AgentEvent::Completed | AgentEvent::Failed { .. });
                        sink.emit(ev);
                        if done {
                            return Ok(());
                        }
                    }
                }
                Some(msg) if msg.id == Some(id) => {
                    if let Some(err) = msg.error {
                        sink.emit(AgentEvent::Failed { message: format!("{err}") });
                    }
                    return Ok(());
                }
                Some(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    fn client(script: &str) -> ServeClient<BufReader<Cursor<Vec<u8>>>, Vec<u8>> {
        ServeClient::new(BufReader::new(Cursor::new(script.as_bytes().to_vec())), Vec::new())
    }

    #[test]
    fn handshake_accepts_matching_reply() {
        let mut c = client("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        c.handshake().unwrap();
        // The request written must be a handshake with protocol_version 1.
        let sent = String::from_utf8(c.writer.clone()).unwrap();
        assert!(sent.contains("\"method\":\"meta.handshake\""));
        assert!(sent.contains("\"protocol_version\":1"));
    }

    #[test]
    fn handshake_errors_on_rejection() {
        let mut c = client("{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000}}\n");
        assert!(c.handshake().is_err());
    }

    #[test]
    fn run_streaming_emits_deltas_then_completed() {
        // Note: this client sends run_streaming as its FIRST request, so id == 1.
        let script = concat!(
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"TextDelta\",\"data\":{\"text\":\"He\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"TextDelta\",\"data\":{\"text\":\"llo\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"RunCompleted\",\"data\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
        );
        let mut c = client(script);
        let mut sink = VecSink::default();
        c.run_streaming("greeter", "hi", &mut sink).unwrap();
        assert_eq!(
            sink.0,
            vec![
                AgentEvent::TextDelta("He".into()),
                AgentEvent::TextDelta("llo".into()),
                AgentEvent::Completed,
            ]
        );
        let sent = String::from_utf8(c.writer.clone()).unwrap();
        assert!(sent.contains("\"method\":\"runtime.run_streaming\""));
        assert!(sent.contains("\"agent\":\"greeter\""));
    }

    #[test]
    fn run_streaming_ignores_events_for_other_ids() {
        let script = concat!(
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":99,\"kind\":\"TextDelta\",\"data\":{\"text\":\"nope\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"RunCompleted\",\"data\":{}}}\n",
        );
        let mut c = client(script);
        let mut sink = VecSink::default();
        c.run_streaming("a", "b", &mut sink).unwrap();
        assert_eq!(sink.0, vec![AgentEvent::Completed]);
    }

    #[test]
    fn eof_mid_run_yields_failed() {
        let mut c = client(""); // immediate EOF after the request
        let mut sink = VecSink::default();
        c.run_streaming("a", "b", &mut sink).unwrap();
        assert!(matches!(sink.0.as_slice(), [AgentEvent::Failed { .. }]));
    }
}
```

- [ ] **Step 2: Commit (verification deferred to Task 6 build)**

```bash
git add crates/cairn-infra/src/tau/client.rs
git commit -m "feat(infra): tau serve-mode client (handshake + run_streaming)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Tau config

**Files:**
- Create: `crates/cairn-infra/src/tau/config.rs`

- [ ] **Step 1: Write the implementation + test**

Create `crates/cairn-infra/src/tau/config.rs`:

```rust
//! Configuration for reaching tau. v1 reads it from the environment; the daemon
//! `[tau]` TOML section + sidecar supervision land with the web panel (v1.1).

use std::path::PathBuf;

/// How to launch and address tau.
#[derive(Debug, Clone)]
pub struct TauConfig {
    /// Path to the `tau` binary.
    pub bin: PathBuf,
    /// Agent id to invoke.
    pub agent: String,
    /// tau project directory; `None` lets tau use its default.
    pub project: Option<PathBuf>,
}

impl TauConfig {
    /// Build from a lookup function. `None` if `TAU_BIN` is unset (tau disabled).
    /// `TAU_AGENT` defaults to `"default"`; `TAU_PROJECT` is optional.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let bin = get("TAU_BIN")?;
        Some(Self {
            bin: PathBuf::from(bin),
            agent: get("TAU_AGENT").unwrap_or_else(|| "default".to_string()),
            project: get("TAU_PROJECT").map(PathBuf::from),
        })
    }

    /// Build from the process environment.
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn none_without_bin() {
        assert!(TauConfig::from_lookup(lookup(&[("TAU_AGENT", "x")])).is_none());
    }

    #[test]
    fn defaults_agent_when_only_bin_set() {
        let cfg = TauConfig::from_lookup(lookup(&[("TAU_BIN", "/usr/bin/tau")])).unwrap();
        assert_eq!(cfg.bin, PathBuf::from("/usr/bin/tau"));
        assert_eq!(cfg.agent, "default");
        assert!(cfg.project.is_none());
    }

    #[test]
    fn reads_all_fields() {
        let cfg = TauConfig::from_lookup(lookup(&[
            ("TAU_BIN", "/t"),
            ("TAU_AGENT", "answerer"),
            ("TAU_PROJECT", "/proj"),
        ]))
        .unwrap();
        assert_eq!(cfg.agent, "answerer");
        assert_eq!(cfg.project, Some(PathBuf::from("/proj")));
    }
}
```

- [ ] **Step 2: Commit (verification deferred to Task 6 build)**

```bash
git add crates/cairn-infra/src/tau/config.rs
git commit -m "feat(infra): TauConfig env loader

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `TauServe` process handle

Spawns the subprocess, waits for the stderr readiness line, drives the client, kills on drop. The thin spawn wrapper is covered by a self-skipping live test (the centaur pattern); the protocol it delegates to is already covered in Task 3.

**Files:**
- Create: `crates/cairn-infra/src/tau/process.rs`

- [ ] **Step 1: Write the implementation + self-skipping live test**

Create `crates/cairn-infra/src/tau/process.rs`:

```rust
//! Owns one `tau serve` subprocess and the client speaking to it.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use cairn_ports::{AdapterError, AgentSink, PortError};

use crate::tau::client::ServeClient;
use crate::tau::config::TauConfig;

/// A live `tau serve` process plus its serve-mode client. Killed on drop.
pub struct TauServe {
    child: Child,
    client: ServeClient<BufReader<ChildStdout>, ChildStdin>,
}

fn missing(what: &str) -> PortError {
    PortError::Adapter(AdapterError::message(format!("tau serve: {what}")))
}

impl TauServe {
    /// Spawn `tau serve`, wait for its readiness line on stderr, and handshake.
    pub fn spawn(cfg: &TauConfig) -> Result<Self, PortError> {
        let mut cmd = Command::new(&cfg.bin);
        cmd.arg("serve").arg("--ready-on-stderr");
        if let Some(project) = &cfg.project {
            cmd.arg("--project").arg(project);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child =
            cmd.spawn().map_err(|e| PortError::Adapter(AdapterError::new(e)))?;

        // Block until tau writes its readiness marker to stderr (the
        // `--ready-on-stderr` flag keeps it off the NDJSON stdout channel).
        let stderr = child.stderr.take().ok_or_else(|| missing("no stderr"))?;
        let mut err = BufReader::new(stderr);
        let mut line = String::new();
        if err.read_line(&mut line).map_err(|e| PortError::Adapter(AdapterError::new(e)))? == 0 {
            return Err(missing("exited before signalling ready"));
        }

        let stdin = child.stdin.take().ok_or_else(|| missing("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| missing("no stdout"))?;
        let mut client = ServeClient::new(BufReader::new(stdout), stdin);
        client.handshake()?;
        Ok(Self { child, client })
    }

    /// Run `agent` over `prompt`, streaming into `sink`.
    pub fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError> {
        self.client.run_streaming(agent, prompt, sink)
    }
}

impl Drop for TauServe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;

    #[test]
    fn spawn_fails_for_missing_binary() {
        let cfg = TauConfig { bin: "/nonexistent/tau-xyz".into(), agent: "a".into(), project: None };
        assert!(TauServe::spawn(&cfg).is_err());
    }

    #[test]
    fn live_run_streams_when_tau_present() {
        // Self-skips unless a real tau is configured (CI stays hermetic).
        let Some(cfg) = TauConfig::from_env() else {
            eprintln!("skip: TAU_BIN unset");
            return;
        };
        let mut serve = TauServe::spawn(&cfg).expect("spawn tau serve");
        #[derive(Default)]
        struct Collect(Vec<AgentEvent>);
        impl AgentSink for Collect {
            fn emit(&mut self, e: AgentEvent) {
                self.0.push(e);
            }
        }
        let mut sink = Collect::default();
        serve.run_streaming(&cfg.agent, "say hello", &mut sink).expect("run");
        assert!(sink.0.iter().any(|e| matches!(e, AgentEvent::Completed | AgentEvent::Failed { .. })));
    }
}
```

- [ ] **Step 2: Commit (verification deferred to Task 6 build)**

```bash
git add crates/cairn-infra/src/tau/process.rs
git commit -m "feat(infra): TauServe subprocess handle (spawn, readiness, drop-kill)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `TauServeRuntime` adapter + crate green bar

The `AgentRuntime` implementation, one-shot per `answer`. This task completes the `tau` module, so the whole crate compiles and all infra tests run.

**Files:**
- Create: `crates/cairn-infra/src/tau/runtime.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (re-exports)

- [ ] **Step 1: Write the runtime adapter + test**

Create `crates/cairn-infra/src/tau/runtime.rs`:

```rust
//! `AgentRuntime` over a one-shot `tau serve` subprocess.

use cairn_ports::{AgentRuntime, AgentSink, PortError};

use crate::tau::config::TauConfig;
use crate::tau::process::TauServe;

/// Runs each `answer` against a freshly-spawned `tau serve` (v1: one-shot, no
/// long-lived supervision — that lands with the daemon path in v1.1).
#[derive(Debug, Clone)]
pub struct TauServeRuntime {
    config: TauConfig,
}

impl TauServeRuntime {
    /// Build from a [`TauConfig`].
    pub fn new(config: TauConfig) -> Self {
        Self { config }
    }
}

impl AgentRuntime for TauServeRuntime {
    fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
        let mut serve = TauServe::spawn(&self.config)?;
        serve.run_streaming(&self.config.agent, prompt, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;

    #[test]
    fn answer_errs_when_binary_missing() {
        let rt = TauServeRuntime::new(TauConfig {
            bin: "/nonexistent/tau-xyz".into(),
            agent: "a".into(),
            project: None,
        });
        struct Noop;
        impl AgentSink for Noop {
            fn emit(&mut self, _e: AgentEvent) {}
        }
        assert!(rt.answer("hi", &mut Noop).is_err());
    }
}
```

- [ ] **Step 2: Add crate re-exports**

In `crates/cairn-infra/src/lib.rs`, add to the re-export block (near the `pub use seams::...` line):

```rust
pub use tau::{TauConfig, TauServeRuntime};
```

- [ ] **Step 3: Build and test the whole crate**

Run: `cargo test -p cairn-infra`
Expected: PASS — `tau::wire`, `tau::client`, `tau::config`, `tau::process` (`spawn_fails_for_missing_binary`, live test self-skips), `tau::runtime` (`answer_errs_when_binary_missing`), plus the existing seam tests.

- [ ] **Step 4: Lint**

Run: `cargo clippy -p cairn-infra --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/tau/runtime.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): TauServeRuntime AgentRuntime adapter (one-shot serve)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `augmented_answer` orchestration in `cairn-service`

Compose existing search + read into a grounded prompt, then run the agent. Lives in the service layer (reusable by the daemon later), not the `Engine` (keeps core pure).

**Files:**
- Modify: `crates/cairn-service/src/lib.rs` (imports + new fns + test module)
- Modify: `crates/cairn-service/Cargo.toml` (`[dev-dependencies]`)

- [ ] **Step 1: Add the dev-dependency**

In `crates/cairn-service/Cargo.toml`, under `[dev-dependencies]` (alongside the existing `cairn-infra`, `serde_json`, `tempfile`):

```toml
cairn-startup = { path = "../cairn-startup" }
```

- [ ] **Step 2: Extend the port import**

In `crates/cairn-service/src/lib.rs:10`, change the `cairn_ports` import to add the agent types:

```rust
use cairn_ports::{AdapterError, AgentRuntime, AgentSink, FsChange, PortError, WatchHandle};
```

- [ ] **Step 3: Write the failing test**

Append to `crates/cairn-service/src/lib.rs` a new test module:

```rust
#[cfg(test)]
mod augmented_answer_tests {
    use super::*;
    use cairn_app::Event;
    use cairn_contract::Command;
    use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
    use std::cell::RefCell;

    struct RecordingRuntime {
        prompt: RefCell<String>,
    }
    impl AgentRuntime for RecordingRuntime {
        fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
            *self.prompt.borrow_mut() = prompt.to_string();
            sink.emit(AgentEvent::TextDelta("ok".into()));
            sink.emit(AgentEvent::Completed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    #[test]
    fn retrieves_context_builds_prompt_and_streams() {
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

        let rt = RecordingRuntime { prompt: RefCell::new(String::new()) };
        let mut sink = VecSink::default();
        let cited = augmented_answer(&engine, "ownership", &rt, &mut sink, 5).unwrap();

        assert_eq!(cited, vec!["a.md".to_string()]);
        assert!(rt.prompt.borrow().contains("ownership moves by default"));
        assert!(rt.prompt.borrow().contains("Question: ownership"));
        assert_eq!(
            sink.0,
            vec![AgentEvent::TextDelta("ok".into()), AgentEvent::Completed]
        );
    }

    #[test]
    fn prompt_without_context_omits_notes_section() {
        let p = build_answer_prompt("", "what is x");
        assert!(!p.contains("Notes:"));
        assert!(p.contains("Question: what is x"));
    }
}
```

- [ ] **Step 4: Run the test to confirm it fails**

Run: `cargo test -p cairn-service augmented_answer`
Expected: FAIL — `cannot find function augmented_answer` / `build_answer_prompt`.

- [ ] **Step 5: Implement the functions**

Append to `crates/cairn-service/src/lib.rs` (before the test modules):

```rust
/// Build a note-grounded answer to `query`: search the cairn, read the top
/// `top_k` hits into context, prompt the agent, and stream the answer into
/// `sink`. Returns the cited note paths (the retrieval set), in rank order.
///
/// # Errors
/// [`ServiceError`] if a search/read dispatch fails or the runtime fails before
/// streaming begins.
pub fn augmented_answer(
    engine: &Engine,
    query: &str,
    runtime: &dyn AgentRuntime,
    sink: &mut dyn AgentSink,
    top_k: usize,
) -> Result<Vec<String>, ServiceError> {
    let cited: Vec<String> =
        match dispatch_query(engine, &Query::Search { query: query.to_string() })? {
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
    runtime.answer(&prompt, sink)?;
    Ok(cited)
}

/// Assemble the agent prompt from retrieved `context` and the user `query`.
fn build_answer_prompt(context: &str, query: &str) -> String {
    if context.is_empty() {
        format!("Answer the question.\n\nQuestion: {query}")
    } else {
        format!(
            "Answer the question using the notes below. Cite note paths when relevant.\n\n\
             Notes:\n{context}\nQuestion: {query}"
        )
    }
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p cairn-service augmented_answer`
Expected: PASS (both tests).

- [ ] **Step 7: Lint**

Run: `cargo clippy -p cairn-service --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-service/src/lib.rs crates/cairn-service/Cargo.toml
git commit -m "feat(service): augmented_answer — note-grounded agent prompt + stream

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: CLI `cairn ask` command

**Files:**
- Modify: `crates/cairn-cli/src/main.rs`

- [ ] **Step 1: Add the `Ask` variant**

In `crates/cairn-cli/src/main.rs`, add to the `Command` enum (after `Search`, around line 89):

```rust
    /// Ask a question; answers grounded in your notes, streamed.
    Ask {
        /// The question.
        query: String,
    },
```

- [ ] **Step 2: Gate the startup reindex on `Ask`**

`Ask` runs a search, so it needs the index. Update `needs_startup_reindex` (line 138-140):

```rust
fn needs_startup_reindex(command: &Command) -> bool {
    matches!(
        command,
        Command::Search { .. } | Command::Watch { .. } | Command::Ask { .. }
    )
}
```

And add to the existing test `only_search_and_watch_need_the_startup_reindex` (after the `Watch` assertion):

```rust
        assert!(needs_startup_reindex(&Command::Ask { query: "x".into() }));
```

- [ ] **Step 3: Add the stdout sink**

Add near `WatchSink` (after its `impl`, around line 44):

```rust
/// Renders agent events for `cairn ask`: answer text to stdout (flushed per
/// chunk so it streams), tool/error chatter to stderr.
struct AgentStdoutSink;

impl cairn_ports::AgentSink for AgentStdoutSink {
    fn emit(&mut self, event: cairn_ports::AgentEvent) {
        use cairn_ports::AgentEvent::{
            Completed, Failed, TextDelta, ToolCompleted, ToolStarted, TurnCompleted,
        };
        match event {
            TextDelta(text) => {
                print!("{text}");
                let _ = std::io::stdout().flush();
            }
            ToolStarted { tool } => eprintln!("  [tool {tool}…]"),
            ToolCompleted { tool, ok } => {
                eprintln!("  [tool {tool} {}]", if ok { "ok" } else { "error" });
            }
            TurnCompleted => {}
            Completed => println!(),
            Failed { message } => eprintln!("\nagent error: {message}"),
            // tau's event vocabulary is #[non_exhaustive]; ignore unknown kinds.
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Add the dispatch arm**

In the `match cli.command` block (after the `Search` arm, before `Backlinks`), add:

```rust
        Command::Ask { query } => {
            let cfg = cairn_infra::TauConfig::from_env().ok_or_else(|| {
                "tau not configured: set TAU_BIN (and optionally TAU_AGENT, TAU_PROJECT)"
                    .to_string()
            })?;
            let runtime = cairn_infra::TauServeRuntime::new(cfg);
            let mut sink = AgentStdoutSink;
            let cited =
                cairn_service::augmented_answer(&engine, &query, &runtime, &mut sink, 5)
                    .map_err(|e| e.to_string())?;
            if !cited.is_empty() {
                eprintln!("sources:");
                for path in cited {
                    eprintln!("  - {path}");
                }
            }
        }
```

- [ ] **Step 5: Build and test the CLI**

Run: `cargo test -p cairn-cli`
Expected: PASS (the reindex test now includes `Ask`).

- [ ] **Step 6: Manual smoke (self-skipping if no tau)**

Run (only if you have a built tau + a configured agent):

```bash
TAU_BIN=/path/to/tau TAU_AGENT=default cargo run -p cairn-cli -- --cairn ./some-cairn ask "what did I note about ownership?"
```

Expected: answer text streams to stdout; a `sources:` list to stderr. Without `TAU_BIN`: `error: tau not configured: set TAU_BIN …`.

- [ ] **Step 7: Lint**

Run: `cargo clippy -p cairn-cli --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-cli/src/main.rs
git commit -m "feat(cli): add \`cairn ask\` — streamed, note-grounded answers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: README status + full workspace verification

**Files:**
- Modify: `README.md:13-17` (Status section)

- [ ] **Step 1: Update the status line**

In `README.md`, the Status paragraph currently lists `tau/AgentRuntime adapter` among future seams. Edit it to reflect that the interactive seam is now wired. Replace the sentence beginning "The web UI, engine-plugin host, tau/`AgentRuntime` adapter, …" with:

```markdown
The `tau`/`AgentRuntime` seam is now wired for interactive use — `cairn ask`
streams a note-grounded answer from a `tau serve` subprocess. The web UI,
daemon-supervised tau sidecar, dataflow pipelines, and CRDT collaboration remain
future sub-projects, each present today as a proven seam.
```

- [ ] **Step 2: Full workspace test**

Run: `cargo test --workspace`
Expected: PASS across all crates.

- [ ] **Step 3: Full workspace lint + format check**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: clean; no diff.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: note the tau interactive seam is wired (\`cairn ask\`)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Daemon-supervised long-lived sidecar → **partially**: v1 implements the one-shot `TauServe` (the spec's resolved CLI default); long-lived supervision (ping/restart) + daemon wiring is explicitly the v1.1 web-panel path (spec follow-ups). Tasks 5-6.
- Serve-mode NDJSON client → Tasks 2-3.
- `AgentRuntime` reshaped to stream → Task 1.
- Retrieval reusing search + read → Task 7.
- `cairn ask` streaming to stdout → Task 8.
- `[tau]` config, default-off → Task 4 (env-based for v1; daemon TOML `[tau]` deferred with the sidecar, noted in `config.rs` doc).
- Testing strategy (fake serve peer, unknown-kind tolerance, FatalError→Failed, default-off, self-skipping live) → Tasks 3, 6, 8.
- Non-goals (centaur/pipelines/MCP/web panel) → untouched, no tasks.

**Placeholder scan:** No `TBD`/`TODO`/"handle errors appropriately" — every code step is complete. The one cross-task caveat (Task 2 `mod.rs` referencing later modules) is called out explicitly with a green-bar option.

**Type consistency:** `AgentEvent`/`AgentSink`/`answer(prompt, sink)` are identical across Tasks 1, 3, 5, 6, 7, 8. `TauConfig { bin, agent, project }` identical in Tasks 4, 5, 6, 8. `augmented_answer(engine, query, runtime, sink, top_k) -> Vec<String>` matches between Task 7 definition and Task 8 call. `map_event` / `Incoming` / `Request` consistent between Tasks 2 and 3. `TauServe::spawn`/`run_streaming` consistent between Tasks 5 and 6.
