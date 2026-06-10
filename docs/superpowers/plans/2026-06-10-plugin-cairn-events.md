# Plugin Host Slice 4: Cairn Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The host pushes `NoteChanged`/`NoteDeleted` events to plugins declaring the `events` capability, delivered as a `cairn/event` request reusing the invoke dispatch loop (handlers may make capability-gated callbacks).

**Architecture:** Inverts the plugin direction. A new ports-level `PluginEvent` + `PluginHost::dispatch_event` (default no-op); the infra host maps `PluginEvent` → the wire `CairnEvent` and delivers it via the same dispatch loop as `invokeCommand`. The engine's `dispatch_plugin_event` uses the same `mem::replace` re-entrancy as invoke. The daemon, after each command/watch-change (under the existing Mutex, so no plugin is mid-invoke), forwards collected note events to plugins; non-recursive; synchronous. The SDK adds `Plugin::on_event`.

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), JSON-RPC 2.0 over NDJSON/stdio, serde/serde_json, tokio broadcast (daemon), nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-cairn-events-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-protocol/src/lib.rs` | `METHOD_CAIRN_EVENT`, `CAP_EVENTS`, `CairnEventKind`, `CairnEvent` | 1 |
| `crates/cairn-ports/src/lib.rs` | `PluginEvent` enum; `PluginHost::dispatch_event` (default no-op) | 2 |
| `crates/cairn-app/src/lib.rs` | `Engine::dispatch_plugin_event` + engine test | 2 |
| `crates/cairn-plugin-sdk/src/lib.rs` | `Plugin::on_event` + `cairn/event` run-loop arm + unit test | 3 |
| `crates/cairn-infra/src/plugin_host.rs` | factor the dispatch loop; `deliver_event`; `ProcessPluginHost::dispatch_event`; `to_cairn_event` | 4 |
| `crates/cairn-plugin-example/src/main.rs` | `on_event` handler (writes a marker via callback) | 4 |
| `crates/cairn-plugin-example/tests/host.rs` | e2e: delivered+writes, skipped-without-cap | 4 |
| `crates/cairn-daemon/src/lib.rs` | `EventTap` sink; forward note events post-operation; `to_plugin_event` | 5 |
| `crates/cairn-daemon/tests/events.rs` | integration: write command forwards `NoteChanged` to a stub host | 5 |

**Unchanged:** `cairn-contract`, `cairn-cli`, `PluginHost::invoke`, `Engine::invoke_plugin_command`, the WS broadcast path.

---

## Task 1: Protocol — cairn-event types

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`

Purely additive. TDD.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn cairn_event_roundtrips() {
        let ev = CairnEvent { kind: CairnEventKind::NoteChanged, path: "a.md".into() };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "noteChanged"); // camelCase rename
        assert_eq!(serde_json::from_value::<CairnEvent>(v).unwrap(), ev);

        let del = CairnEvent { kind: CairnEventKind::NoteDeleted, path: "b.md".into() };
        assert_eq!(serde_json::to_value(&del).unwrap()["kind"], "noteDeleted");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-plugin-protocol cairn_event`
Expected: COMPILE failure — `CairnEvent`/`CairnEventKind` don't exist.

- [ ] **Step 3: Add the protocol items**

After the `METHOD_DELETE_NOTE` const add:

```rust
/// Host -> plugin: a cairn change event. Delivered to plugins declaring `events`.
pub const METHOD_CAIRN_EVENT: &str = "cairn/event";
```

Near the other `CAP_*` consts add:

```rust
/// Capability: receive pushed cairn events.
pub const CAP_EVENTS: &str = "events";
```

After the `DeleteNoteParams` struct add:

```rust
/// The kind of a cairn change pushed to plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CairnEventKind {
    NoteChanged,
    NoteDeleted,
}

/// Params of the `cairn/event` request (host -> plugin). Ack result is `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CairnEvent {
    pub kind: CairnEventKind,
    pub path: String,
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(protocol): cairn/event types + CAP_EVENTS"
```

---

## Task 2: Ports `PluginEvent` + `PluginHost::dispatch_event` + engine routing

Adds a wire-agnostic `PluginEvent` and a default-no-op `dispatch_event` on `PluginHost` (so existing impls/stubs keep compiling), plus the engine's `dispatch_plugin_event` and its test.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Write the failing engine test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-app/src/lib.rs`, add a stub host whose `dispatch_event` writes via the callbacks, plus the test:

```rust
    /// A stub host whose dispatch_event writes a marker note via the callbacks —
    /// exercises Engine::dispatch_plugin_event + handler callbacks.
    struct EventWriter;
    impl PluginHost for EventWriter {
        fn plugins(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        fn invoke(
            &mut self,
            plugin: &str,
            _command: &str,
            _args: &serde_json::Value,
            _callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            Err(PortError::NotFound(format!("plugin {plugin}")))
        }
        fn dispatch_event(
            &mut self,
            _event: &cairn_ports::PluginEvent,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) {
            let _ = callbacks.write_note("seen.md", "seen");
        }
    }

    #[test]
    fn dispatch_event_runs_handler_with_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(EventWriter));
        let mut events: Vec<Event> = Vec::new();
        eng.dispatch_plugin_event(
            &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
            &mut events,
        );
        // The handler wrote seen.md via the callback (which routes through the engine).
        assert_eq!(eng.read_note(&NotePath::new("seen.md").unwrap()).unwrap(), "seen");
        assert!(events.contains(&Event::NoteChanged(NotePath::new("seen.md").unwrap())));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-app dispatch_event_runs_handler`
Expected: COMPILE failure — `PluginEvent` and `PluginHost::dispatch_event`/`Engine::dispatch_plugin_event` don't exist.

- [ ] **Step 3: Add `PluginEvent` + the trait method in ports**

In `crates/cairn-ports/src/lib.rs`, add the `PluginEvent` enum just before `pub trait PluginHost` (NotePath is already imported):

```rust
/// A cairn change the host may push to subscribed plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    /// A note was created or updated.
    NoteChanged(NotePath),
    /// A note was deleted.
    NoteDeleted(NotePath),
}
```

In the `pub trait PluginHost` block, add a default-no-op method (after `invoke`):

```rust
    /// Deliver a cairn event to every loaded plugin that declared the `events`
    /// capability, servicing any host-callbacks each makes while handling it.
    /// Best-effort. Default: no-op (a host that doesn't support events ignores them).
    fn dispatch_event(&mut self, _event: &PluginEvent, _callbacks: &mut dyn PluginCallbacks) {}
```

(`NoopPluginHost` inherits the default — no change needed there.)

- [ ] **Step 4: Add `Engine::dispatch_plugin_event` in the app**

In `crates/cairn-app/src/lib.rs`, add `PluginEvent` to the `cairn_ports` import (alongside `PluginCallbacks`, `PluginHost`, etc.). Then add the method to the `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` block (next to `invoke_plugin_command`):

```rust
    /// Deliver a cairn event to subscribed plugins (best-effort). Event-handler
    /// callbacks route through the engine, and any events they emit go to `sink`.
    pub fn dispatch_plugin_event(&mut self, event: &PluginEvent, sink: &mut dyn EventSink) {
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.dispatch_event(event, &mut cb);
        }
        self.plugins = host;
    }
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p cairn-app dispatch_event_runs_handler`
Expected: PASS.

- [ ] **Step 6: Full suite + lint + fmt**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt`.
Expected: all green (the default `dispatch_event` keeps `NoopPluginHost` + all existing `PluginHost` stubs compiling).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs
git commit -m "feat(plugin): PluginEvent port + Engine::dispatch_plugin_event"
```

---

## Task 3: SDK `Plugin::on_event`

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Write the failing unit test**

In the `#[cfg(test)] mod run_tests` block of `crates/cairn-plugin-sdk/src/lib.rs`, add:

```rust
    #[test]
    fn on_event_acks_and_handles() {
        use cairn_plugin_protocol::{CairnEvent, CairnEventKind, METHOD_CAIRN_EVENT};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.on_event(move |ev: CairnEvent, _host| {
            assert_eq!(ev.path, "x.md");
            ran2.store(true, Ordering::SeqCst);
            Ok(())
        });
        let ev = CairnEvent { kind: CairnEventKind::NoteChanged, path: "x.md".into() };
        let out = drive(plugin, &request_line(1, METHOD_CAIRN_EVENT, serde_json::to_value(ev).unwrap()));
        assert!(ran.load(Ordering::SeqCst), "handler should have run");
        assert_eq!(out[0].result.clone().unwrap(), serde_json::json!({}));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-plugin-sdk on_event_acks`
Expected: COMPILE failure — `Plugin::on_event` doesn't exist (and `cairn/event` isn't handled).

- [ ] **Step 3: Add `on_event` + the run-loop arm**

In `crates/cairn-plugin-sdk/src/lib.rs`, extend the top protocol `use` block to add `CairnEvent` and `METHOD_CAIRN_EVENT` (keep all existing names). Re-export `CairnEvent`: change the `pub use cairn_plugin_protocol::{NoteSummaryDto, SearchHitDto};` line to:

```rust
pub use cairn_plugin_protocol::{CairnEvent, NoteSummaryDto, SearchHitDto};
```

Add a type alias next to the existing `ErasedHandler` alias (an alias keeps the
field within clippy's `type_complexity` limit):

```rust
/// The erased event handler stored on the `Plugin`. Returns `()` (events are
/// acked, not result-bearing).
type ErasedEventHandler = Box<dyn FnMut(CairnEvent, &mut Host<'_>) -> Result<(), PluginError>>;
```

Add an `event_handler` field to `Plugin`:

```rust
pub struct Plugin {
    name: String,
    version: String,
    commands: Vec<RegisteredCommand>,
    event_handler: Option<ErasedEventHandler>,
}
```

Update `Plugin::new` to initialize `event_handler: None` (keep the existing field initializers).

Add the registration method to `impl Plugin`:

```rust
    /// Register a handler for pushed cairn events (`cairn/event`). The handler
    /// gets capability-gated `Host` access to react (read/write the cairn).
    pub fn on_event<F>(&mut self, handler: F)
    where
        F: FnMut(CairnEvent, &mut Host<'_>) -> Result<(), PluginError> + 'static,
    {
        self.event_handler = Some(Box::new(handler));
    }
```

In `Plugin::handle`'s `match req.method.as_str()`, add a `METHOD_CAIRN_EVENT` arm (before the catch-all `other =>`):

```rust
            METHOD_CAIRN_EVENT => match serde_json::from_value::<CairnEvent>(req.params.clone()) {
                Ok(ev) => {
                    if let Some(handler) = self.event_handler.as_mut() {
                        let mut host = Host { reader, stdout, next_cb_id };
                        match handler(ev, &mut host) {
                            Ok(()) => resp.result = Some(serde_json::json!({})),
                            Err(e) => resp.error = Some(RpcError { code: e.code, message: e.message }),
                        }
                    } else {
                        resp.result = Some(serde_json::json!({})); // no handler: ack
                    }
                }
                Err(e) => {
                    resp.error = Some(RpcError { code: -32602, message: e.to_string() });
                }
            },
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cairn-plugin-sdk`
Expected: PASS — `on_event_acks_and_handles` + all existing SDK tests + the doctest.

- [ ] **Step 5: Lint + fmt**

Run: `cargo fmt` then `cargo clippy -p cairn-plugin-sdk --all-targets -- -D warnings`.
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "feat(sdk): Plugin::on_event for cairn/event delivery"
```

---

## Task 4: Infra delivery + example handler + e2e

Factor the dispatch loop, add `deliver_event` + `ProcessPluginHost::dispatch_event`, the example's event handler, and the real-subprocess e2e.

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Add the example `on_event` handler**

In `crates/cairn-plugin-example/src/main.rs`, before `plugin.run();`, add:

```rust
    // On any cairn change, record the changed path into a marker note.
    plugin.on_event(|ev, host: &mut Host| {
        host.write_note("seen.md", &ev.path)?;
        Ok(())
    });
```

Update the imports if needed — `on_event`'s closure receives a `CairnEvent`; type inference handles the `ev` param, but if the compiler asks, annotate `|ev: cairn_plugin_sdk::CairnEvent, host: &mut Host|` (the SDK re-exports `CairnEvent`, so `use cairn_plugin_sdk::CairnEvent;` and annotate `|ev: CairnEvent, ...|`). Prefer adding `CairnEvent` to the existing `use cairn_plugin_sdk::{...};` import and annotating the closure.

- [ ] **Step 2: Write the failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`, add (the `write_manifest` helper + `MapCallbacks` exist; `PluginEvent` comes from `cairn_ports`, `NotePath` from `cairn_domain` — both already imported at the top of the file):

```rust
#[test]
fn event_delivered_and_handler_writes() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"events\", \"fs:write\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    host.dispatch_event(
        &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
        &mut cb,
    );
    // The example's on_event handler wrote seen.md = the changed path, via host.write_note.
    assert_eq!(cb.0.get("seen.md").map(String::as_str), Some("x.md"));
}

#[test]
fn event_skipped_without_events_cap() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:write\""); // fs:write but NOT events
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    host.dispatch_event(
        &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
        &mut cb,
    );
    assert!(cb.0.is_empty(), "no events cap -> no delivery");
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host event_`
Expected: FAIL to COMPILE — `ProcessPluginHost` has no `dispatch_event` (the default trait method exists, but it's a no-op, so `event_delivered_and_handler_writes` would FAIL the assertion — `seen.md` absent — once it compiles). Implement Step 4 to make it pass.

- [ ] **Step 4: Factor the dispatch loop + add delivery in the infra host**

In `crates/cairn-infra/src/plugin_host.rs`, extend the `cairn_plugin_protocol` import to add `CairnEvent, CairnEventKind, CAP_EVENTS, METHOD_CAIRN_EVENT` and the `cairn_ports` import to add `PluginEvent` (keep all existing names in both).

Factor the shared write+dispatch loop out of `invoke_command`. Replace the existing `invoke_command` method with a shared helper + a thin wrapper:

```rust
    /// Send one request and run the dispatch loop, servicing host-callbacks until
    /// the matching-id response arrives. Shared by invoke and event delivery.
    fn call_with_callbacks(
        &mut self,
        method: &str,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let req_id = self.next_id;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: req_id,
            method: method.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        loop {
            let msg: Incoming = read_message(&mut self.stdout)
                .map_err(adapt)?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
            match msg {
                Incoming::Response(resp) => {
                    if resp.id != req_id {
                        continue; // stray id; one-in-flight invariant, ignore
                    }
                    if let Some(err) = resp.error {
                        return Err(PortError::Adapter(format!("plugin error: {}", err.message)));
                    }
                    return resp
                        .result
                        .ok_or_else(|| PortError::Adapter("plugin response had no result".into()));
                }
                Incoming::Request(cb) => {
                    let response = self.service_callback(&cb, callbacks);
                    write_message(&mut self.stdin, &response).map_err(adapt)?;
                }
            }
        }
    }

    /// Invoke a command, servicing any host-callbacks until the plugin responds.
    fn invoke_command(
        &mut self,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.call_with_callbacks(METHOD_INVOKE, params, callbacks)
    }

    /// Deliver one cairn event, servicing any host-callbacks the handler makes.
    fn deliver_event(
        &mut self,
        event: &CairnEvent,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<(), PortError> {
        let params = serde_json::to_value(event).map_err(adapt)?;
        self.call_with_callbacks(METHOD_CAIRN_EVENT, params, callbacks)?;
        Ok(())
    }
```

Add a free function mapping the ports event to the wire event (near `required_cap`):

```rust
fn to_cairn_event(event: &PluginEvent) -> CairnEvent {
    match event {
        PluginEvent::NoteChanged(p) => CairnEvent {
            kind: CairnEventKind::NoteChanged,
            path: p.as_str().to_string(),
        },
        PluginEvent::NoteDeleted(p) => CairnEvent {
            kind: CairnEventKind::NoteDeleted,
            path: p.as_str().to_string(),
        },
    }
}
```

Add `dispatch_event` to the `impl PluginHost for ProcessPluginHost` block (after `invoke`):

```rust
    fn dispatch_event(&mut self, event: &PluginEvent, callbacks: &mut dyn PluginCallbacks) {
        let cairn_event = to_cairn_event(event);
        for p in self.loaded.iter_mut() {
            if p.capabilities.iter().any(|c| c == CAP_EVENTS) {
                if let Err(e) = p.deliver_event(&cairn_event, callbacks) {
                    eprintln!("plugin {}: event delivery failed: {e}", p.info.id);
                }
            }
        }
    }
```

- [ ] **Step 5: Run the e2e tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — `event_delivered_and_handler_writes` (`seen.md` == `"x.md"`), `event_skipped_without_events_cap` (map empty), and all existing host tests.

- [ ] **Step 6: Full suite + lint + fmt**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt`.
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/src/main.rs \
        crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin): host event delivery (cairn/event) + example handler + e2e"
```

---

## Task 5: Daemon wiring — forward note events to plugins

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs`
- Create: `crates/cairn-daemon/tests/events.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/cairn-daemon/tests/events.rs`:

```rust
use cairn_app::Engine;
use cairn_contract::Command;
use cairn_daemon::AppState;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::{PluginCallbacks, PluginEvent, PluginHost, PluginInfo, PortError};
use std::sync::{Arc, Mutex};

/// A stub host that records every dispatched cairn event.
struct RecordingHost(Arc<Mutex<Vec<PluginEvent>>>);
impl PluginHost for RecordingHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        Vec::new()
    }
    fn invoke(
        &mut self,
        plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
    fn dispatch_event(&mut self, event: &PluginEvent, _callbacks: &mut dyn PluginCallbacks) {
        self.0.lock().unwrap().push(event.clone());
    }
}

#[test]
fn write_command_forwards_note_event_to_plugins() {
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let recorded = Arc::new(Mutex::new(Vec::new()));
    engine.set_plugin_host(Box::new(RecordingHost(recorded.clone())));
    let state = AppState::new(engine);

    state
        .run_command_blocking(&Command::WriteNote {
            path: "a.md".into(),
            contents: "hi".into(),
        })
        .unwrap();

    let events = recorded.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PluginEvent::NoteChanged(p) if p.as_str() == "a.md")),
        "expected NoteChanged(a.md) forwarded, got {events:?}"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-daemon --test events`
Expected: FAIL — the assertion fails (the daemon doesn't forward events to plugins yet; `RecordingHost` records nothing). (If `cairn-ports` or `serde_json` is missing from `crates/cairn-daemon/Cargo.toml` `[dev-dependencies]`, add them: `cairn-ports = { path = "../cairn-ports" }` and `serde_json = { workspace = true }`. `cairn-ports` is already a regular dependency, so it is available — verify; add to dev-deps only if the test fails to resolve it.)

- [ ] **Step 3: Add the `EventTap` sink + `to_plugin_event` + forwarding**

In `crates/cairn-daemon/src/lib.rs`, add `use cairn_ports::PluginEvent;` to the imports (the crate already uses `cairn_ports::FsChange`, so the dependency exists). After the `BroadcastSink` definition add:

```rust
/// An `EventSink` that broadcasts engine events to the WS channel AND collects
/// them so the daemon can forward note events to plugins after the operation.
struct EventTap {
    tx: broadcast::Sender<WireEvent>,
    collected: Vec<AppEvent>,
}
impl EventSink for EventTap {
    fn emit(&mut self, event: AppEvent) {
        self.collected.push(event.clone());
        let _ = self.tx.send(app_event_to_wire(event));
    }
}

/// Map an engine event to the plugin-facing event, or `None` if plugins don't
/// receive it (only note mutations are forwarded).
fn to_plugin_event(event: &AppEvent) -> Option<PluginEvent> {
    match event {
        AppEvent::NoteChanged(p) => Some(PluginEvent::NoteChanged(p.clone())),
        AppEvent::NoteDeleted(p) => Some(PluginEvent::NoteDeleted(p.clone())),
        AppEvent::Committed(_) | AppEvent::Reindexed(_) => None,
    }
}
```

Replace `run_command_blocking` and `apply_change_blocking` to collect via `EventTap` and forward note events afterward (still under the held lock; a fresh `BroadcastSink` for handler-generated events makes them broadcast-only → non-recursive):

```rust
    pub fn run_command_blocking(&self, command: &Command) -> Result<CommandResponse, ServiceError> {
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
        let mut tap = EventTap { tx: self.events.clone(), collected: Vec::new() };
        let result = dispatch_command(&mut guard, command, &mut tap);
        let collected = tap.collected;
        if result.is_ok() {
            for pe in collected.iter().filter_map(to_plugin_event) {
                let mut fwd = BroadcastSink(self.events.clone());
                guard.dispatch_plugin_event(&pe, &mut fwd);
            }
        }
        result
    }
```

```rust
    pub fn apply_change_blocking(&self, change: &cairn_ports::FsChange) {
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
        let mut tap = EventTap { tx: self.events.clone(), collected: Vec::new() };
        if let Err(e) = guard.apply_change(change, &mut tap) {
            eprintln!("watch: apply_change failed: {e}");
            return;
        }
        let collected = tap.collected;
        for pe in collected.iter().filter_map(to_plugin_event) {
            let mut fwd = BroadcastSink(self.events.clone());
            guard.dispatch_plugin_event(&pe, &mut fwd);
        }
    }
```

(Keep the existing doc comments on both methods.)

- [ ] **Step 4: Run the integration test to verify it passes**

Run: `cargo test -p cairn-daemon --test events`
Expected: PASS — `RecordingHost` recorded `NoteChanged(a.md)`.

- [ ] **Step 5: Full suite + lint + fmt + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green, no warnings, fmt clean, lock consistent (only a possible `cairn-daemon` dev-dep change if `serde_json`/`cairn-ports` had to be added — commit `Cargo.lock` if so).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/events.rs
# include Cargo.toml + Cargo.lock only if a dev-dep was added in Step 2
git commit -m "feat(daemon): forward note events to plugins after each command/change"
```

---

## Notes for the implementer

- **The default `dispatch_event` (no-op) is what keeps the tree green** when the trait grows — `NoopPluginHost` and every existing `PluginHost` test stub inherit it; only `ProcessPluginHost` (Task 4) and the test stubs that need it (`EventWriter`, `RecordingHost`) override it.
- **Timing safety:** the daemon forwards events *after* the engine operation returns, under the same Mutex, so no plugin is mid-invoke and there's no stdio interleaving. Keep the forwarding loop where shown (post-`dispatch_command`/`apply_change`), never inside them.
- **Non-recursion:** handler-generated events use a plain `BroadcastSink` (broadcast-only), so they reach WS but are not re-forwarded to plugins. Do not thread an `EventTap` into `dispatch_plugin_event`.
- **`deliver_event` reuses `call_with_callbacks`** — do not duplicate the loop. `invoke_command` becomes a one-line wrapper over it; verify the existing invoke/callback tests still pass after the refactor.
- **fmt:** run `cargo fmt` before committing each task (CI's rustfmt check is strict).
- **Don't touch** `cairn-contract`, `cairn-cli`, or the `PluginHost::invoke` / `Engine::invoke_plugin_command` signatures.
```
