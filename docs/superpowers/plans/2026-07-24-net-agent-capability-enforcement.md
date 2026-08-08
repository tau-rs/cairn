# net / agent capability enforcement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `agent` plugin capability enforced through the host-callback gate (Domain 1), backed by the existing `AgentRuntime` seam; confirm `net` stays sandbox-enforced (Domain 2) and prove `agent` is host-only.

**Architecture:** `agent` is a Domain-1 host-mediated callback: a plugin sends `host/agent`, the host gates it on the declared `Capability::Agent` (→ `CALLBACK_DENIED` without it), then runs the engine-owned `AgentRuntime`, buffering the streamed answer into one string. The engine owns an optional runtime (default `None`), so no signature in the `dispatch_command`/`invoke_plugin_command` chain changes. `net` is untouched — it is already enforced by the capability-derived sandbox profile.

**Tech Stack:** Rust (workspace, `forbid(unsafe_code)`), serde, JSON-RPC/NDJSON plugin protocol, `thiserror` at boundaries.

## Global Constraints

- **Base branch:** C1's typed capability vocabulary **landed on `origin/main` as #138** (`91003cd`). This branch is reset onto it. The merged `Capability` enum is `{VaultRead, VaultWrite, VaultEvents, ContentProcess, Net, Exec, FsRead}` (each with `wire()`/`summary()`/`enforced_today()`); `required_cap(method: &str) -> Option<Capability>`; `sandbox_caps(caps: &[Capability])`; manifest `capabilities: Vec<Capability>` (strings deserialize via `#[serde(rename)]`, unknown → fail-closed). C2 adds one `Capability::Agent` variant.
- **`forbid(unsafe_code)`** is workspace-wide (`[lints] workspace = true`). No `unsafe`.
- **Boundaries:** `thiserror` at crate boundaries, `anyhow` internally. Host-callback failures surface as `PortError::Adapter`.
- **`AgentEvent` is `#[non_exhaustive]`** — every `match` over it MUST carry a wildcard arm.
- **DoD:** `cargo test`, `cargo clippy --locked --all-targets`, and `cargo fmt --check` all green; deny + allow paths both covered.
- **Commits:** conventional, imperative, scoped. End every commit message body with:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

---

## File Structure

- `crates/cairn-plugin-protocol/src/lib.rs` — new `Capability::Agent` variant, `METHOD_AGENT`, `AgentParams`, `AgentResult`.
- `crates/cairn-ports/src/lib.rs` — `PluginCallbacks::run_agent`; update the `NoCb` test double.
- `crates/cairn-app/src/lib.rs` — `Engine` gains `runtime: Option<Arc<dyn AgentRuntime + Send + Sync>>` + `set_runtime`; `EngineCallbacks` gains the real `run_agent`.
- `crates/cairn-infra/src/plugin_host.rs` — gate `METHOD_AGENT` in `required_cap` + `service_callback`; deny `run_agent` in `ReadOnlyCallbacks`; update the `Cb` test double; `sandbox_caps` host-only assertion.
- `crates/cairn-daemon/src/main.rs` — wire the daemon's runtime into the engine.
- `crates/cairn-plugin-sdk/src/lib.rs` — `Host::agent`.
- `crates/cairn-plugin-example/src/main.rs` — an `ask` command.
- `crates/cairn-plugin-example/tests/host.rs` — `MapCallbacks::run_agent` double + deny/allow e2e tests.

---

## Task 0: Reset onto #138 main (enum base) — DONE

**Files:** none (git only). Already completed: C1 landed as #138; this branch was
reset onto `origin/main` (enum base), verified `required_cap(...) -> Option<Capability>`
and the `Capability` enum are present. Prior string-const work preserved on
`c2-strings-backup`. No further action; proceed to Task 1.

---

## Task 1: Protocol — `agent` capability, method, and DTOs

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub const METHOD_AGENT: &str = "host/agent"`; `Capability::Agent`; `pub struct AgentParams { pub prompt: String }`; `pub struct AgentResult { pub answer: String }`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:
```rust
#[test]
fn agent_capability_and_dtos_roundtrip() {
    // Wire string + metadata.
    assert_eq!(Capability::Agent.wire(), "agent");
    assert!(Capability::Agent.enforced_today(), "agent gates the live host-RPC channel");
    assert_eq!(
        serde_json::from_str::<Capability>("\"agent\"").unwrap(),
        Capability::Agent
    );

    // Method const.
    assert_eq!(METHOD_AGENT, "host/agent");

    // DTOs.
    let p = AgentParams { prompt: "summarize my notes".into() };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(serde_json::from_value::<AgentParams>(v).unwrap(), p);

    let r = AgentResult { answer: "done".into() };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(serde_json::from_value::<AgentResult>(v).unwrap(), r);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol agent_capability_and_dtos_roundtrip`
Expected: FAIL — `METHOD_AGENT` / `Capability::Agent` / `AgentParams` not found.

- [ ] **Step 3: Add the variant, method const, and DTOs**

In the `Capability` enum, after the `FsRead` variant:
```rust
    /// Run an AI agent query via the host (Domain 1 — host-mediated).
    #[serde(rename = "agent")]
    Agent,
```
In `Capability::wire`, add the arm:
```rust
            Capability::Agent => "agent",
```
In `Capability::summary`, add the arm:
```rust
            Capability::Agent => "run an AI agent query on your behalf",
```
In `Capability::enforced_today`, ADD `| Capability::Agent` to the END of the
EXISTING `matches!` list — do not retype the list (on #138 it already contains
`VaultRead | VaultWrite | VaultEvents | ContentProcess | Net`). Result:
```rust
        matches!(
            self,
            Capability::VaultRead
                | Capability::VaultWrite
                | Capability::VaultEvents
                | Capability::ContentProcess
                | Capability::Net
                | Capability::Agent
        )
```
Add the method const next to the other `METHOD_*` consts:
```rust
/// Plugin -> host: run an AI agent query. Requires the `agent` capability.
pub const METHOD_AGENT: &str = "host/agent";
```
Add the DTOs next to the other callback param/result structs:
```rust
/// Params of the `host/agent` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentParams {
    pub prompt: String,
}

/// Result of the `host/agent` callback: the completed agent answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResult {
    pub answer: String,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS (new test + existing `capability_roundtrips_via_wire_string`).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(protocol): add agent capability, host/agent method, and DTOs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Engine runtime seam + real `run_agent`

Adds `PluginCallbacks::run_agent` (widening the trait, so every impl is updated to keep the workspace compiling), gives `Engine` an optional agent runtime, implements the real buffering `run_agent` on `EngineCallbacks`, and wires the daemon.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (trait + `NoCb` double)
- Modify: `crates/cairn-app/src/lib.rs` (`Engine` field/setter + `EngineCallbacks::run_agent` + test host)
- Modify: `crates/cairn-infra/src/plugin_host.rs` (`ReadOnlyCallbacks` deny + `Cb` double)
- Modify: `crates/cairn-plugin-example/tests/host.rs` (`MapCallbacks` double)
- Modify: `crates/cairn-daemon/src/main.rs` (wire runtime)
- Test: `crates/cairn-app/src/lib.rs` tests

**Interfaces:**
- Consumes: `cairn_ports::{AgentRuntime, AgentSink, AgentEvent}` (existing).
- Produces: `PluginCallbacks::run_agent(&mut self, prompt: &str) -> Result<String, PortError>`; `Engine::set_runtime(&mut self, runtime: Arc<dyn AgentRuntime + Send + Sync>)`.

- [ ] **Step 1: Write the failing test**

In `crates/cairn-app/src/lib.rs` tests module, add a fake host that drives the agent callback plus the assertions. (`AgentRuntime`/`AgentSink`/`AgentEvent` are already imported in that module via `cairn_infra`/`cairn_ports`; add `use cairn_ports::{AgentRuntime, AgentSink, AgentEvent};` inside the test fn if not in scope.)
```rust
#[test]
fn plugin_agent_callback_runs_engine_runtime() {
    use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
    use std::sync::Arc;

    // A host that, on invoke, asks the engine to run the agent and echoes it back.
    struct AgentHost;
    impl PluginHost for AgentHost {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "p".into(), name: "P".into(), version: "0".into(),
                commands: vec![PluginCommand { id: "ask".into(), title: "Ask".into() }],
                contributions: vec![],
            }]
        }
        fn invoke(&mut self, _p: &str, _c: &str, _a: &serde_json::Value,
                  cb: &mut dyn PluginCallbacks) -> Result<serde_json::Value, PortError> {
            let answer = cb.run_agent("hello")?;
            Ok(serde_json::json!({ "answer": answer }))
        }
        fn dispatch_event(&mut self, _e: &PluginEvent, _cb: &mut dyn PluginCallbacks)
            -> Vec<EventDispatchError> { vec![] }
        fn process_content(&mut self, _p: &str, c: &str, _cb: &mut dyn PluginCallbacks)
            -> Result<String, PortError> { Ok(c.to_string()) }
    }

    struct TwoChunk;
    impl AgentRuntime for TwoChunk {
        fn answer(&self, _prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
            sink.emit(AgentEvent::TextDelta("Hel".into()));
            sink.emit(AgentEvent::TextDelta("lo".into()));
            sink.emit(AgentEvent::Completed);
            Ok(())
        }
    }

    let mut eng = test_engine(); // existing helper used by neighbouring tests
    eng.set_plugin_host(Box::new(AgentHost));
    let mut sink: Vec<Event> = Vec::new();

    // No runtime configured -> Err.
    let denied = eng.invoke_plugin_command("p", "ask", &serde_json::Value::Null, &mut sink);
    assert!(matches!(denied, Err(PortError::Adapter(_))), "no runtime => Adapter, got {denied:?}");

    // Runtime configured -> buffered answer.
    eng.set_runtime(Arc::new(TwoChunk));
    let out = eng
        .invoke_plugin_command("p", "ask", &serde_json::Value::Null, &mut sink)
        .unwrap();
    assert_eq!(out, serde_json::json!({ "answer": "Hello" }));
}
```
NOTE: use the same engine-construction helper the neighbouring plugin tests use (search the test module for how `eng` is built, e.g. a `test_engine()` or inline `Engine::new(...)`); match it exactly. `Event` / `PluginCommand` / `PluginInfo` / `EventDispatchError` / `PluginEvent` are already imported by the module.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-app plugin_agent_callback_runs_engine_runtime`
Expected: FAIL — `set_runtime` and `run_agent` do not exist; also the crate won't compile until the trait method exists.

- [ ] **Step 3: Widen the trait + add the `NoCb` double arm (`cairn-ports`)**

In `PluginCallbacks` (after `delete_note`):
```rust
    /// Run an AI agent query via the host and return the completed answer.
    /// Gated on the `agent` capability.
    ///
    /// # Errors
    /// [`PortError::Adapter`] if no agent runtime is configured or the run fails.
    fn run_agent(&mut self, prompt: &str) -> Result<String, PortError>;
```
In the `NoCb` test double in that file's tests, add:
```rust
        fn run_agent(&mut self, _prompt: &str) -> Result<String, PortError> {
            Ok(String::new())
        }
```

- [ ] **Step 4: Add the runtime field/setter + real impl (`cairn-app`)**

Add the import at the top (extend the existing `use cairn_ports::{...}`):
```rust
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink};
```
Add `use std::sync::Arc;` if not already present.
Add the field to `struct Engine`:
```rust
    runtime: Option<std::sync::Arc<dyn AgentRuntime + Send + Sync>>,
```
Initialize it in `Engine::new` (add to the struct literal):
```rust
            runtime: None,
```
Add the setter next to `set_plugin_host`:
```rust
    /// Inject the agent runtime backing the plugin `host/agent` callback.
    /// Absent by default; a plugin `agent` call then fails as "no runtime".
    pub fn set_runtime(&mut self, runtime: std::sync::Arc<dyn AgentRuntime + Send + Sync>) {
        self.runtime = Some(runtime);
    }
```
Add the `run_agent` impl to `impl PluginCallbacks for EngineCallbacks<'_>`:
```rust
    fn run_agent(&mut self, prompt: &str) -> Result<String, PortError> {
        let rt = self
            .engine
            .runtime
            .clone()
            .ok_or_else(|| PortError::Adapter("no agent runtime configured".into()))?;

        // Buffer the streamed run into one string; `host/agent` is request/response,
        // not streaming. A `Failed` event becomes an error; other kinds are ignored.
        struct Buf {
            text: String,
            failed: Option<String>,
        }
        impl AgentSink for Buf {
            fn emit(&mut self, event: AgentEvent) {
                match event {
                    AgentEvent::TextDelta(s) => self.text.push_str(&s),
                    AgentEvent::Failed { message } => self.failed = Some(message),
                    _ => {} // AgentEvent is #[non_exhaustive]
                }
            }
        }
        let mut buf = Buf { text: String::new(), failed: None };
        rt.answer(prompt, &mut buf)?;
        if let Some(message) = buf.failed {
            return Err(PortError::Adapter(message.into()));
        }
        Ok(buf.text)
    }
```

- [ ] **Step 5: Add `run_agent` to the remaining impls (`cairn-infra`, example test)**

In `crates/cairn-infra/src/plugin_host.rs`, `impl PluginCallbacks for ReadOnlyCallbacks<'_>` (deny, like write/delete during content processing):
```rust
    fn run_agent(&mut self, _prompt: &str) -> Result<String, PortError> {
        Err(PortError::Adapter(
            "agent not permitted during content processing".into(),
        ))
    }
```
In the same file's `Cb` test double (`read_only_callbacks_forward_reads_deny_writes`):
```rust
            fn run_agent(&mut self, _: &str) -> Result<String, PortError> {
                Ok(String::new())
            }
```
In `crates/cairn-plugin-example/tests/host.rs`, `impl PluginCallbacks for MapCallbacks` (canned answer; the allow/deny e2e in Task 5 asserts on it):
```rust
    fn run_agent(&mut self, prompt: &str) -> Result<String, PortError> {
        Ok(format!("answer: {prompt}"))
    }
```

- [ ] **Step 6: Wire the daemon runtime into the engine (`cairn-daemon/src/main.rs`)**

After the `runtime` binding is constructed (the `let runtime: Arc<dyn ... AgentRuntime ...> = match ...` block) and before `engine` is moved into `AppState`, add:
```rust
    engine.set_runtime(runtime.clone());
```
(The `Arc` is cloned so both the engine — for `host/agent` — and `AppState` — for `/ask` — hold it.)

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p cairn-app plugin_agent_callback_runs_engine_runtime` then `cargo test --workspace`
Expected: PASS across the workspace (all `PluginCallbacks` impls now compile).

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs \
        crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs \
        crates/cairn-daemon/src/main.rs
git commit -m "feat(engine): run_agent host callback backed by an engine-owned AgentRuntime

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Gate `host/agent` in the host + host-only sandbox assertion

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `METHOD_AGENT`, `Capability::Agent`, `AgentParams`, `AgentResult` (Task 1); `PluginCallbacks::run_agent` (Task 2).

- [ ] **Step 1: Write the failing test**

Add to the infra `tests` module:
```rust
#[test]
fn required_cap_gates_agent() {
    use cairn_plugin_protocol::{Capability, METHOD_AGENT};
    assert_eq!(super::required_cap(METHOD_AGENT), Some(Capability::Agent));
}

#[test]
fn agent_is_host_only_not_a_sandbox_cap() {
    use cairn_plugin_protocol::Capability;
    use cairn_ports::SandboxCapabilities;
    // `agent` opens no network in the jail: the host makes the tau call.
    assert_eq!(
        super::sandbox_caps(&[Capability::Agent]),
        SandboxCapabilities { net: false }
    );
    // net still opens it.
    assert_eq!(
        super::sandbox_caps(&[Capability::Net]),
        SandboxCapabilities { net: true }
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra required_cap_gates_agent`
Expected: FAIL — `required_cap(METHOD_AGENT)` returns `None`.

- [ ] **Step 3: Gate the method**

Extend the protocol `use` import to include `AgentParams, AgentResult, METHOD_AGENT`.
In `required_cap`, add the arm before `_ => None`:
```rust
        METHOD_AGENT => Some(Capability::Agent),
```
In `service_callback`, add a dispatch arm alongside the others (inside the `Some(_) => match cb.method.as_str()` block):
```rust
                METHOD_AGENT => match serde_json::from_value::<AgentParams>(cb.params.clone()) {
                    Ok(p) => match callbacks.run_agent(&p.prompt) {
                        Ok(answer) => {
                            resp.result = serde_json::to_value(AgentResult { answer }).ok();
                        }
                        Err(e) => {
                            resp.error = Some(RpcError {
                                code: CALLBACK_FAILED,
                                message: e.to_string(),
                            });
                        }
                    },
                    Err(e) => {
                        resp.error = Some(RpcError {
                            code: CALLBACK_FAILED,
                            message: e.to_string(),
                        });
                    }
                },
```
`sandbox_caps` needs no change — `Capability::Agent` is absent from it, so it maps to `net: false` (the test's point).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra`
Expected: PASS (new gate tests + existing `sandbox_caps_*`).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(plugin-host): gate host/agent on the agent capability

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: SDK `Host::agent`

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `METHOD_AGENT`, `AgentParams`, `AgentResult` (Task 1).
- Produces: `Host::agent(&mut self, prompt: &str) -> Result<String, PluginError>`.

- [ ] **Step 1: Write the failing test**

Follow the existing SDK callback-test pattern (search the tests module for `read_note` / `written.method` to copy the harness that captures the emitted request and feeds a canned response). Add:
```rust
#[test]
fn host_agent_sends_request_and_parses_answer() {
    use cairn_plugin_protocol::{AgentResult, METHOD_AGENT};
    // Canned host response: {"answer":"hi"}.
    let response = serde_json::to_value(AgentResult { answer: "hi".into() }).unwrap();
    // `call_host_expecting` is the existing helper used by the read_note test;
    // reuse whatever that test uses to drive one Host::* call and capture the request.
    let (written, out) = call_host_expecting(response, |h| h.agent("summarize"));
    assert_eq!(written.method, METHOD_AGENT);
    assert_eq!(written.params["prompt"], "summarize");
    assert_eq!(out.unwrap(), "hi");
}
```
If no reusable helper exists, mirror the exact scaffolding of the nearest `Host::read_note` unit test in that module instead.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-plugin-sdk host_agent_sends_request_and_parses_answer`
Expected: FAIL — `Host::agent` not found.

- [ ] **Step 3: Implement `Host::agent`**

Extend the protocol `use` to include `AgentParams, AgentResult, METHOD_AGENT`. Add to `impl Host<'_>` (mirrors `read_note`):
```rust
    /// Run an AI agent query via the host (`host/agent`, requires `agent`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn agent(&mut self, prompt: &str) -> Result<String, PluginError> {
        let params = serde_json::to_value(AgentParams {
            prompt: prompt.to_string(),
        })?;
        let result = self.call(METHOD_AGENT, params)?;
        let out: AgentResult = serde_json::from_value(result)?;
        Ok(out.answer)
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-plugin-sdk`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): Host::agent helper for the host/agent callback

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Example plugin `ask` command + deny/allow e2e

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

**Interfaces:**
- Consumes: `Host::agent` (Task 4); `MapCallbacks::run_agent` (added in Task 2, returns `format!("answer: {prompt}")`).

- [ ] **Step 1: Write the failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`:
```rust
#[test]
fn ask_via_agent_callback_when_cap_declared() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"agent\"");
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new());
    let out = host
        .invoke("example", "ask", &serde_json::json!({"prompt": "hi"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({ "answer": "answer: hi" }));
}

#[test]
fn ask_denied_without_agent_cap() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no capabilities
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new());
    let err = host
        .invoke("example", "ask", &serde_json::json!({"prompt": "hi"}), &mut cb)
        .unwrap_err();
    assert!(matches!(err, PortError::Adapter(_)), "expected Adapter, got {err:?}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-plugin-example ask_`
Expected: FAIL — the `ask` command is unknown (`PortError::NotFound`), not yet the Adapter/answer we assert.

- [ ] **Step 3: Add the `ask` command to the example plugin**

In `crates/cairn-plugin-example/src/main.rs`, add near the other `plugin.command(...)` calls (reuse the existing `QueryArgs`-style pattern; define a small struct or inline-deserialize `prompt`):
```rust
    #[derive(Deserialize)]
    struct AskArgs {
        prompt: String,
    }
    plugin.command("ask", "Ask agent", |a: AskArgs, host: &mut Host| {
        let answer = host.agent(&a.prompt)?;
        Ok(json!({ "answer": answer }))
    });
```
(If local structs inside `main` are awkward with the SDK's generic handler, hoist `AskArgs` to module scope beside `PathArgs`/`QueryArgs`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-plugin-example`
Expected: PASS — allow returns `{"answer":"answer: hi"}`; deny returns `PortError::Adapter` (CALLBACK_DENIED).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "test(plugin-example): ask command exercising the agent callback deny/allow

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Full verification

**Files:** none (may fix lint/fmt inline).

- [ ] **Step 1: fmt**

Run: `cargo fmt --all` then `cargo fmt --check`
Expected: clean.

- [ ] **Step 2: clippy (locked, all targets)**

Run: `cargo clippy --locked --all-targets`
Expected: no warnings. Fix any inline (e.g. a needless `clone`, or a wildcard-match lint on `AgentEvent`). Commit fixes if any:
```bash
git commit -am "chore: clippy/fmt cleanup for agent capability

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: Full test run**

Run: `cargo test --workspace`
Expected: PASS. Confirm both new e2e cases (`ask_via_agent_callback_when_cap_declared`, `ask_denied_without_agent_cap`) and the engine test (`plugin_agent_callback_runs_engine_runtime`) are in the output.

---

## Self-Review (completed by plan author)

**Spec coverage:**
- `agent` cap + `host/agent` + DTOs → Task 1. ✓
- `run_agent` port + engine wiring (engine owns runtime, buffering sink, daemon wire) → Task 2. ✓
- `required_cap`/`service_callback` gate → Task 3. ✓
- `agent` host-only (no sandbox network) → Task 3 (`agent_is_host_only_not_a_sandbox_cap`). ✓
- `net` already enforced (no new logic) → asserted in Task 3's net half; behavioral net tests pre-exist in `sandbox.rs`. ✓
- SDK + example plugin e2e deny/allow → Tasks 4–5. ✓
- Buffering / `Failed` → `Err` → Task 2 test + impl. ✓
- clippy(locked)/fmt/tests green → Task 6. ✓

**Placeholder scan:** none — every code step carries real content; test-harness reuse points (SDK helper, engine constructor) name the exact existing symbols to copy.

**Type consistency:** `run_agent(&mut self, &str) -> Result<String, PortError>`, `set_runtime(Arc<dyn AgentRuntime + Send + Sync>)`, `Capability::Agent`, `METHOD_AGENT`, `AgentParams{prompt}`, `AgentResult{answer}` used identically across Tasks 1–5. `MapCallbacks::run_agent` canned value (`"answer: {prompt}"`, Task 2) matches the e2e assertion (`"answer: hi"`, Task 5). ✓
