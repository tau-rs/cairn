# Plugin Host Slice 3a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a plugin command call back to the host mid-invoke (`host/readNote`), gated by a manifest-declared, now-enforced `fs:read` capability.

**Architecture:** The host's invoke becomes a full-duplex dispatch loop: after sending `invokeCommand`, it reads messages that are *either* a host-callback request (service it, gated on capability, write a response back) *or* the final invoke response. Servicing a callback re-enters the engine via a `PluginCallbacks` handler; the engine resolves the borrow conflict (`self.plugins` borrowed while a callback needs `&mut self`) by moving the host out with `std::mem::replace` for the duration.

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), JSON-RPC 2.0 over NDJSON on stdio, serde/serde_json, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-09-plugin-host-slice3a-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-protocol/src/lib.rs` | Wire types: `host/readNote` method, `ReadNoteParams`/`ReadNoteResult`, `Incoming` enum, callback error codes | 1 |
| `crates/cairn-ports/src/lib.rs` | `PluginCallbacks` trait; `PluginHost::invoke` gains the handler arg; `NoopPluginHost` updated | 2 |
| `crates/cairn-app/src/lib.rs` | `EngineCallbacks` + `mem::replace` re-entrancy in `invoke_plugin_command`; engine re-entrancy unit test | 2 |
| `crates/cairn-infra/src/plugin_host.rs` | (T2) thread the param through, host unchanged; (T3) dispatch loop + capability enforcement | 2, 3 |
| `crates/cairn-plugin-example/src/main.rs` | `noteLen` command performing the `host/readNote` callback round-trip | 3 |
| `crates/cairn-plugin-example/tests/host.rs` | (T2) thread a test-double callbacks into existing calls; (T3) happy + denied e2e tests | 2, 3 |

**Unchanged:** `cairn-contract`, `cairn-service`, `cairn-cli`, `cairn-daemon` — `Engine::invoke_plugin_command`'s public signature (`plugin, command, args`) is preserved, so nothing downstream ripples.

---

## Task 1: Protocol additions

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`

These types are purely additive; the crate compiles and all existing tests pass throughout.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
    #[test]
    fn incoming_decodes_request_and_response_variants() {
        // A message carrying `method` is a host-callback Request.
        let req_json = r#"{"jsonrpc":"2.0","id":7,"method":"host/readNote","params":{"path":"a.md"}}"#;
        match serde_json::from_str::<Incoming>(req_json).unwrap() {
            Incoming::Request(r) => {
                assert_eq!(r.method, METHOD_READ_NOTE);
                assert_eq!(r.id, 7);
            }
            Incoming::Response(_) => panic!("expected Request variant"),
        }

        // A message carrying `result` (no `method`) is a Response.
        let resp_json = r#"{"jsonrpc":"2.0","id":7,"result":{"contents":"hi"}}"#;
        match serde_json::from_str::<Incoming>(resp_json).unwrap() {
            Incoming::Response(r) => {
                assert_eq!(r.id, 7);
                assert_eq!(r.result.unwrap()["contents"], "hi");
            }
            Incoming::Request(_) => panic!("expected Response variant"),
        }

        // A read-note result round-trips through its typed struct.
        let rn = ReadNoteResult { contents: "body".into() };
        let v = serde_json::to_value(&rn).unwrap();
        assert_eq!(serde_json::from_value::<ReadNoteResult>(v).unwrap().contents, "body");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol incoming_decodes -- --nocapture`
Expected: FAIL to compile — `cannot find type Incoming` / `ReadNoteResult` / const `METHOD_READ_NOTE`.

- [ ] **Step 3: Add the protocol items**

In `crates/cairn-plugin-protocol/src/lib.rs`, after the `METHOD_INVOKE` const (around line 12) add:

```rust
/// Plugin -> host: read a note's raw contents. Requires the `fs:read` capability.
pub const METHOD_READ_NOTE: &str = "host/readNote";

/// JSON-RPC error code: the host refused a callback (capability not declared, or
/// unknown host method).
pub const CALLBACK_DENIED: i64 = -32001;
/// JSON-RPC error code: a callback's host operation failed (e.g. note not found,
/// or malformed params).
pub const CALLBACK_FAILED: i64 = -32002;
```

After `InvokeParams` (around line 68) add:

```rust
/// Params of the `host/readNote` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadNoteParams {
    pub path: String,
}

/// Result of the `host/readNote` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadNoteResult {
    pub contents: String,
}

/// A message the host reads from a plugin *during* an invoke: either a callback
/// request from the plugin, or the response to the host's invoke. Distinguished
/// untagged by the presence of `method` (Request) vs `result`/`error` (Response).
/// The `Request` variant is listed first so serde tries it before `Response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Request(Request),
    Response(Response),
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS — the new test plus all existing protocol tests (`request_response_roundtrip_over_ndjson`, etc.).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(protocol): host/readNote callback types + Incoming message enum"
```

---

## Task 2: Callbacks port + engine re-entrancy (signature change, host behavior unchanged)

This task changes `PluginHost::invoke`'s arity, so it must keep the whole workspace compiling and green: update the port, the engine (with the `mem::replace` re-entrancy and its own unit test), thread an unused param through the infra host (still one-shot), and pass a test-double through the example's existing invoke calls. The real dispatch loop lands in Task 3.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-app/src/lib.rs`
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Write the failing engine re-entrancy test**

Add to the `#[cfg(test)] mod tests` block in `crates/cairn-app/src/lib.rs` (near `default_plugin_host_is_noop`):

```rust
    /// A stub host whose invoke calls back into the engine via the callbacks
    /// handler — exercises the mem::replace re-entrancy in invoke_plugin_command.
    struct CallbackEcho;
    impl PluginHost for CallbackEcho {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "cb".into(),
                name: "cb".into(),
                version: "0".into(),
                commands: Vec::new(),
            }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            let contents = callbacks.read_note(path)?;
            Ok(serde_json::json!({ "contents": contents }))
        }
    }

    #[test]
    fn invoke_services_read_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hello body", &mut events)
            .unwrap();
        eng.set_plugin_host(Box::new(CallbackEcho));
        let out = eng
            .invoke_plugin_command("cb", "readit", &serde_json::json!({ "path": "a.md" }))
            .unwrap();
        assert_eq!(out["contents"], "hello body");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-app invoke_services_read_callback`
Expected: FAIL to compile — `PluginCallbacks` does not exist; `PluginHost::invoke` does not take a 4th arg.

- [ ] **Step 3: Add the `PluginCallbacks` trait and update `PluginHost` in ports**

In `crates/cairn-ports/src/lib.rs`, immediately before `pub trait PluginHost: Send {` (around line 209) add:

```rust
/// Operations a plugin may request of the host *during* an invoke. The host gates
/// each on a declared capability before calling through to the implementation
/// (the engine).
pub trait PluginCallbacks {
    /// Read a note's raw contents by path. Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note does not exist; [`PortError::Adapter`]
    /// on a storage failure.
    fn read_note(&mut self, path: &str) -> Result<String, PortError>;
}
```

Change the `PluginHost::invoke` signature (add the `callbacks` parameter):

```rust
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError>;
```

Update `NoopPluginHost::invoke` to match (ignore the new arg — `-D warnings` requires the `_` prefix):

```rust
    fn invoke(
        &mut self,
        plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
```

- [ ] **Step 4: Add the re-entrancy in the engine**

In `crates/cairn-app/src/lib.rs`, replace the body of `invoke_plugin_command` (currently `self.plugins.invoke(plugin, command, args)` at ~line 484) with:

```rust
    pub fn invoke_plugin_command(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PortError> {
        // Move the real host into a local so `self.plugins` no longer aliases it;
        // the callbacks handler can then borrow the rest of `self` (the store) to
        // service host-callbacks the plugin sends mid-invoke.
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        let mut cb = EngineCallbacks { engine: self };
        let result = host.invoke(plugin, command, args, &mut cb);
        drop(cb); // release the &mut self borrow before restoring the host
        self.plugins = host;
        result
    }
```

Add the `EngineCallbacks` adapter just after the `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` block closes (after the `}` that ends that impl, before `type RestoredState = ...`):

```rust
/// Bridges plugin host-callbacks to engine operations. Held only for the duration
/// of a single `invoke_plugin_command`, while `self.plugins` is a `NoopPluginHost`.
struct EngineCallbacks<'a, S, I, V> {
    engine: &'a mut Engine<S, I, V>,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> PluginCallbacks for EngineCallbacks<'_, S, I, V> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        self.engine.read_note(&np)
    }
}
```

Add `PluginCallbacks` to the `cairn_ports` import (line 5-7). The import becomes:

```rust
use cairn_ports::{
    FileStamp, FsChange, NoopPluginHost, PluginCallbacks, PluginHost, PluginInfo, PortError,
    SearchHit, SearchIndex, VaultStore, Vcs,
};
```

(Keep any other names already in that import list — add `PluginCallbacks`, do not drop existing entries. Verify the exact current list before editing.)

- [ ] **Step 5: Thread the param through the infra host (behavior unchanged)**

In `crates/cairn-infra/src/plugin_host.rs`, update the `ProcessPluginHost::invoke` signature and add the import. The body still calls the one-shot `p.call(METHOD_INVOKE, params)` — the dispatch loop comes in Task 3. Change the import line (around line 11) to:

```rust
use cairn_ports::{PluginCallbacks, PluginCommand, PluginHost, PluginInfo, PortError};
```

Change the `invoke` signature (around line 147) to add the unused param:

```rust
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
```

Leave the rest of the method body exactly as-is (the `find`, command check, `InvokeParams`, and `p.call(METHOD_INVOKE, params)`).

- [ ] **Step 6: Keep the example e2e test compiling — add a test-double callbacks**

In `crates/cairn-plugin-example/tests/host.rs`, add a `MapCallbacks` double and thread it into the three existing `invoke` calls. Update the imports line (line 1-2) to:

```rust
use cairn_infra::ProcessPluginHost;
use cairn_ports::{PluginCallbacks, PluginHost, PortError};
use std::collections::HashMap;
```

Add this struct above the `#[test]` (after the imports):

```rust
/// A test double for host-callbacks: serves notes from an in-memory map.
struct MapCallbacks(HashMap<String, String>);
impl PluginCallbacks for MapCallbacks {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("note {path}")))
    }
}
```

In `host_loads_invokes_and_rejects_unknown`, after `let mut host = ...`, add:

```rust
    let mut cb = MapCallbacks(HashMap::new());
```

Then add `&mut cb` as the final argument to each of the three `invoke` calls:

```rust
    let out = host
        .invoke("example", "echo", &serde_json::json!({"x": 1, "y": "z"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"x": 1, "y": "z"}));

    assert!(matches!(
        host.invoke("missing", "echo", &serde_json::Value::Null, &mut cb),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        host.invoke("example", "nope", &serde_json::Value::Null, &mut cb),
        Err(PortError::NotFound(_))
    ));
```

- [ ] **Step 7: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS — `invoke_services_read_callback` passes (proving the re-entrancy), `default_plugin_host_is_noop` still passes, the example `host_loads_invokes_and_rejects_unknown` still passes, and every other crate is green.

- [ ] **Step 8: Lint**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS — no unused-variable warnings (the threaded params are `_`-prefixed).

- [ ] **Step 9: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs \
        crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin): PluginCallbacks port + engine mem::replace re-entrancy"
```

---

## Task 3: Host dispatch loop + capability enforcement + example callback

Implement the real bidirectional behavior. TDD order: add the `noteLen` command to the example plugin (the test fixture), write the two e2e tests, watch them fail (the host is still one-shot), then implement the host dispatch loop + capability gate.

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`
- Modify: `crates/cairn-infra/src/plugin_host.rs`

- [ ] **Step 1: Add the `noteLen` command to the example plugin**

In `crates/cairn-plugin-example/src/main.rs`, update the imports (lines 6-9) to:

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, ReadNoteParams,
    ReadNoteResult, Request, Response, RpcError, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
    METHOD_READ_NOTE,
};
use std::io::{BufRead, Write};
```

Change `main` so the invoke handler can read/write for the callback round-trip, and carry a callback-id counter:

```rust
fn main() {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();
    let mut cb_id: u64 = 1000; // ids for host-callback requests (distinct range)

    while let Some(req) = read_message::<_, Request>(&mut reader).unwrap_or(None) {
        let resp = handle(&req, &mut reader, &mut stdout, &mut cb_id);
        if write_message(&mut stdout, &resp).is_err() {
            break;
        }
    }
}
```

Change `handle`'s signature and add the `noteLen` arm + the declared command. Replace the whole `fn handle(...)` with:

```rust
fn handle<R: BufRead, W: Write>(
    req: &Request,
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
) -> Response {
    let mut resp = Response {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: req.id,
        result: None,
        error: None,
    };
    match req.method.as_str() {
        METHOD_INITIALIZE => {
            let init = InitializeResult {
                name: "example".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                commands: vec![
                    CommandDecl {
                        id: "echo".to_string(),
                        title: "Echo".to_string(),
                    },
                    CommandDecl {
                        id: "noteLen".to_string(),
                        title: "Note length".to_string(),
                    },
                ],
            };
            resp.result = Some(serde_json::to_value(init).unwrap());
        }
        METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
            Ok(p) if p.command == "echo" => resp.result = Some(p.args),
            Ok(p) if p.command == "noteLen" => {
                let path = p
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                match read_note_via_host(reader, stdout, cb_id, &path) {
                    Ok(contents) => {
                        resp.result = Some(serde_json::json!({ "len": contents.len() }));
                    }
                    Err(err) => resp.error = Some(err),
                }
            }
            Ok(p) => {
                resp.error = Some(RpcError {
                    code: -32601,
                    message: format!("unknown command {}", p.command),
                });
            }
            Err(e) => {
                resp.error = Some(RpcError {
                    code: -32602,
                    message: e.to_string(),
                });
            }
        },
        other => {
            resp.error = Some(RpcError {
                code: -32601,
                message: format!("unknown method {other}"),
            });
        }
    }
    resp
}

/// Send a `host/readNote` callback to the host and block for its response.
fn read_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
) -> Result<String, RpcError> {
    *cb_id += 1;
    let req = Request {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: *cb_id,
        method: METHOD_READ_NOTE.to_string(),
        params: serde_json::to_value(ReadNoteParams {
            path: path.to_string(),
        })
        .unwrap(),
    };
    write_message(stdout, &req).map_err(|e| RpcError {
        code: -32603,
        message: format!("callback write failed: {e}"),
    })?;
    let cb_resp: Response = read_message(reader)
        .map_err(|e| RpcError {
            code: -32603,
            message: format!("callback read failed: {e}"),
        })?
        .ok_or_else(|| RpcError {
            code: -32603,
            message: "host closed before callback response".to_string(),
        })?;
    if let Some(err) = cb_resp.error {
        return Err(err); // propagate the host's denial/failure
    }
    let result = cb_resp.result.ok_or_else(|| RpcError {
        code: -32603,
        message: "empty callback response".to_string(),
    })?;
    let rn: ReadNoteResult = serde_json::from_value(result).map_err(|e| RpcError {
        code: -32603,
        message: e.to_string(),
    })?;
    Ok(rn.contents)
}
```

- [ ] **Step 2: Write the two failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`, add both tests (the `MapCallbacks` double already exists from Task 2):

```rust
#[test]
fn note_len_reads_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    // Literal (single-quote) TOML string for the path; declare fs:read.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[\"fs:read\"]\n"
        ),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("note.md".to_string(), "hello body".to_string())]));

    let out = host
        .invoke("example", "noteLen", &serde_json::json!({"path": "note.md"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"len": 10}));
}

#[test]
fn note_len_denied_without_capability() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    // No capabilities declared -> the host must deny host/readNote.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\n"
        ),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("note.md".to_string(), "hello body".to_string())]));

    let err = host
        .invoke("example", "noteLen", &serde_json::json!({"path": "note.md"}), &mut cb)
        .unwrap_err();
    assert!(matches!(err, PortError::Adapter(_)), "expected Adapter, got {err:?}");
}
```

- [ ] **Step 3: Run the e2e tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host note_len`
Expected: FAIL — the host still uses the one-shot `call`, so it reads the plugin's `host/readNote` *request* as if it were the invoke *response* and errors (likely an `Adapter` "response had no result" / parse error), not the expected `{"len":10}`. (`note_len_denied_without_capability` may coincidentally error, but for the wrong reason; both must pass after Step 4.)

- [ ] **Step 4: Implement the dispatch loop + capability enforcement in the host**

In `crates/cairn-infra/src/plugin_host.rs`, update the protocol import (around line 7) to add the new items:

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, Incoming, InitializeParams, InitializeResult,
    InvokeParams, Manifest, ReadNoteParams, ReadNoteResult, Request, Response, RpcError,
    CALLBACK_DENIED, CALLBACK_FAILED, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
    METHOD_READ_NOTE,
};
```

Add a `capabilities` field to `LoadedPlugin` (struct around line 17):

```rust
struct LoadedPlugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    info: PluginInfo,
    next_id: u64,
    /// Capabilities the manifest declared; gates host-callbacks.
    capabilities: Vec<String>,
}
```

Populate it in `spawn_plugin` where the `LoadedPlugin { ... }` literal is built (around line 113), adding the field (read it from the manifest before `manifest` is partially moved — `capabilities` clone is fine alongside the existing `id`/`name`/`version` clones):

```rust
        let mut plugin = LoadedPlugin {
            child,
            stdin,
            stdout,
            info: PluginInfo {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                commands: Vec::new(),
            },
            next_id: 0,
            capabilities: manifest.engine.capabilities.clone(),
        };
```

Add the dispatch-loop method and the callback servicer to `impl LoadedPlugin` (alongside the existing one-shot `call`, which initialize still uses):

```rust
    /// Invoke a command, servicing any host-callbacks the plugin sends until it
    /// returns the response to our invoke request.
    fn invoke_command(
        &mut self,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let invoke_id = self.next_id;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: invoke_id,
            method: METHOD_INVOKE.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        loop {
            let msg: Incoming = read_message(&mut self.stdout)
                .map_err(adapt)?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
            match msg {
                Incoming::Response(resp) => {
                    if resp.id != invoke_id {
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

    /// Build the response to one host-callback request, gating on capability.
    fn service_callback(
        &self,
        cb: &Request,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Response {
        let mut resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: cb.id,
            result: None,
            error: None,
        };
        let required = required_cap(&cb.method);
        match required {
            None => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("unknown host method {}", cb.method),
                });
            }
            Some(cap) if !self.capabilities.iter().any(|c| c == cap) => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("capability {cap} not declared"),
                });
            }
            Some(_) => match cb.method.as_str() {
                METHOD_READ_NOTE => {
                    match serde_json::from_value::<ReadNoteParams>(cb.params.clone()) {
                        Ok(p) => match callbacks.read_note(&p.path) {
                            Ok(contents) => {
                                resp.result = serde_json::to_value(ReadNoteResult { contents }).ok();
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
                    }
                }
                _ => {
                    resp.error = Some(RpcError {
                        code: CALLBACK_DENIED,
                        message: format!("unknown host method {}", cb.method),
                    });
                }
            },
        }
        resp
    }
```

Add the free function `required_cap` near `fn adapt` (top of the file, after the `adapt` helper around line 15):

```rust
/// The capability a host-callback method requires, or `None` if the method is
/// unknown to the host.
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some("fs:read"),
        _ => None,
    }
}
```

Finally, switch `ProcessPluginHost::invoke` to use the loop. Replace `_callbacks` with `callbacks` and call `invoke_command` instead of `call`:

```rust
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        let p = self
            .loaded
            .iter_mut()
            .find(|p| p.info.id == plugin)
            .ok_or_else(|| PortError::NotFound(format!("plugin {plugin}")))?;
        if !p.info.commands.iter().any(|c| c.id == command) {
            return Err(PortError::NotFound(format!("command {command}")));
        }
        let params = serde_json::to_value(InvokeParams {
            command: command.to_string(),
            args: args.clone(),
        })
        .map_err(adapt)?;
        p.invoke_command(params, callbacks)
    }
```

- [ ] **Step 5: Run the e2e tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — `note_len_reads_via_callback` returns `{"len":10}` (host serviced the `fs:read` callback), `note_len_denied_without_capability` returns `PortError::Adapter` (host denied the callback, plugin surfaced the error), and `host_loads_invokes_and_rejects_unknown` still passes.

- [ ] **Step 6: Run the full workspace suite + lint**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS — every crate green, no warnings. (`LoadedPlugin::call` is still used by `spawn_plugin` for `initialize`, so no dead-code warning.)

- [ ] **Step 7: Confirm Cargo.lock is unchanged, then commit**

No new external dependencies were added (all new types are internal). Verify and commit:

```bash
cargo build --workspace --locked
git add crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-example/tests/host.rs \
        crates/cairn-infra/src/plugin_host.rs
git status --short   # if Cargo.lock changed, `git add Cargo.lock` too
git commit -m "feat(plugin): bidirectional host-callbacks + fs:read capability enforcement"
```

---

## Notes for the implementer

- **One in-flight request per plugin** is still the invariant; stray-id responses in the dispatch loop are ignored, not correlated.
- **`noteLen` returns byte length** (`contents.len()`); the test note `"hello body"` is ASCII, so byte length == char count == 10.
- **Windows CI:** the manifest `command` path goes in a TOML *literal* (single-quote) string — backslash paths break a *basic* (double-quote) string. This is already the pattern in `tests/host.rs`; keep it.
- **Panic safety:** if `host.invoke` panics, the engine's `self.plugins` is left as `NoopPluginHost` (not restored). Accepted, per spec — a panicking host is already a bug.
- **Do not touch** `cairn-contract`, `cairn-service`, `cairn-cli`, or `cairn-daemon`: the public `Engine::invoke_plugin_command` signature is unchanged.
```
