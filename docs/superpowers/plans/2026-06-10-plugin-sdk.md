# Plugin SDK Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `cairn-plugin-sdk` crate so a plugin author writes only command declarations + typed handlers; the SDK owns the stdio loop, `initialize` handshake, `invokeCommand` dispatch, and the host-callback round-trip.

**Architecture:** A new author-side crate over `cairn-plugin-protocol`. `Host` (non-generic, borrows `&mut dyn BufRead`/`&mut dyn Write`) wraps the callback round-trip with typed methods. `Plugin` holds typed commands erased into uniform `Box<dyn FnMut(Value, &mut Host) -> Result<Value, PluginError>>` closures and runs the loop. The example plugin is rewritten onto the SDK; its `tests/host.rs` e2e suite is the unchanged cross-check.

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), JSON-RPC 2.0 over NDJSON/stdio, serde/serde_json, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-sdk-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-sdk/Cargo.toml` | new crate manifest | 1 |
| `Cargo.toml` (workspace) | add `"crates/cairn-plugin-sdk"` to `members` | 1 |
| `crates/cairn-plugin-sdk/src/lib.rs` | `PluginError`, `Host` + callback methods, re-exports (T1); `Plugin`, `command`, `run`/`run_io` (T2) | 1, 2 |
| `crates/cairn-plugin-example/Cargo.toml` | swap protocol dep → sdk + serde | 3 |
| `crates/cairn-plugin-example/src/main.rs` | rewritten onto the SDK | 3 |

**Unchanged:** `cairn-plugin-protocol`, the host (`cairn-infra`/`cairn-app`/`cairn-ports`), `cairn-service`, `cairn-contract`, `cairn-cli`, daemon, and `crates/cairn-plugin-example/tests/host.rs` (the cross-check).

---

## Task 1: Crate scaffold + `PluginError` + `Host` callback handle

**Files:**
- Create: `crates/cairn-plugin-sdk/Cargo.toml`
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/cairn-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Create the crate manifest**

Create `crates/cairn-plugin-sdk/Cargo.toml`:

```toml
[package]
name = "cairn-plugin-sdk"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-plugin-protocol = { path = "../cairn-plugin-protocol" }
serde = { workspace = true }
serde_json = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Register the crate in the workspace**

In the root `Cargo.toml`, add `"crates/cairn-plugin-sdk",` to the `members` list (after `"crates/cairn-plugin-protocol",` to keep it grouped). The list currently ends with `"crates/cairn-plugin-example",` — insert the new entry anywhere in the array.

- [ ] **Step 3: Write the failing Host tests**

Create `crates/cairn-plugin-sdk/src/lib.rs` with ONLY this test module first (the `use super::*` will fail to resolve until Step 5):

```rust
#[cfg(test)]
mod host_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_note_sends_request_and_parses_response() {
        // Canned host response to our host/readNote callback.
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: Some(serde_json::to_value(ReadNoteResult { contents: "hello".to_string() }).unwrap()),
            error: None,
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        let contents = {
            let mut host = Host { reader: &mut reader, stdout: &mut out, next_cb_id: &mut cb_id };
            host.read_note("note.md").unwrap()
        };
        assert_eq!(contents, "hello");
        assert_eq!(cb_id, 1001);
        // The SDK wrote a host/readNote request with the right params.
        let first_line = out.split(|&b| b == b'\n').next().unwrap();
        let written: Request = serde_json::from_slice(first_line).unwrap();
        assert_eq!(written.method, METHOD_READ_NOTE);
        assert_eq!(written.params["path"], "note.md");
    }

    #[test]
    fn denied_callback_becomes_error_preserving_code() {
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: None,
            error: Some(RpcError { code: -32001, message: "capability fs:read not declared".to_string() }),
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        let mut host = Host { reader: &mut reader, stdout: &mut out, next_cb_id: &mut cb_id };
        let err = host.read_note("note.md").unwrap_err();
        assert_eq!(err.code, -32001);
        assert!(err.message.contains("fs:read"));
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p cairn-plugin-sdk`
Expected: COMPILE failure — `Host`, `PluginError`, and the protocol imports don't exist yet.

- [ ] **Step 5: Implement `PluginError` + `Host`**

Prepend to `crates/cairn-plugin-sdk/src/lib.rs` (above the test module):

```rust
//! cairn plugin SDK: write a plugin as command declarations + typed handlers;
//! the SDK owns the JSON-RPC/NDJSON stdio loop and the host-callback round-trip.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)

use std::io::{BufRead, Write};

use cairn_plugin_protocol::{
    read_message, write_message, ListNotesResult, ReadNoteParams, ReadNoteResult, Request, Response,
    RpcError, SearchParams, SearchResultDto, WriteNoteParams, JSONRPC_VERSION, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
use serde_json::Value;

pub use cairn_plugin_protocol::{NoteSummaryDto, SearchHitDto};

/// An error from a command handler or a host-callback. Maps to a JSON-RPC error
/// object on the wire.
#[derive(Debug, Clone)]
pub struct PluginError {
    pub code: i64,
    pub message: String,
}

impl PluginError {
    /// A handler error with the JSON-RPC "internal error" code (-32603).
    pub fn new(message: impl Into<String>) -> Self {
        Self { code: -32603, message: message.into() }
    }
}

impl From<&str> for PluginError {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
impl From<String> for PluginError {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}
impl From<RpcError> for PluginError {
    fn from(e: RpcError) -> Self {
        Self { code: e.code, message: e.message }
    }
}
impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(e.to_string())
    }
}

/// Handle passed to each command handler for calling back to the host. Each call
/// is gated host-side by the plugin's manifest-declared capabilities.
pub struct Host<'a> {
    reader: &'a mut dyn BufRead,
    stdout: &'a mut dyn Write,
    next_cb_id: &'a mut u64,
}

impl Host<'_> {
    /// Send one host-callback request and return its `result` Value (or the
    /// host's error, preserving its code+message).
    fn call(&mut self, method: &str, params: Value) -> Result<Value, PluginError> {
        *self.next_cb_id += 1;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: *self.next_cb_id,
            method: method.to_string(),
            params,
        };
        write_message(self.stdout, &req)
            .map_err(|e| PluginError::new(format!("callback write failed: {e}")))?;
        let resp: Response = read_message(self.reader)
            .map_err(|e| PluginError::new(format!("callback read failed: {e}")))?
            .ok_or_else(|| PluginError::new("host closed before callback response"))?;
        if let Some(err) = resp.error {
            return Err(PluginError::from(err));
        }
        resp.result.ok_or_else(|| PluginError::new("empty callback response"))
    }

    /// Read a note's raw contents (`host/readNote`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn read_note(&mut self, path: &str) -> Result<String, PluginError> {
        let params = serde_json::to_value(ReadNoteParams { path: path.to_string() })?;
        let result = self.call(METHOD_READ_NOTE, params)?;
        let rn: ReadNoteResult = serde_json::from_value(result)?;
        Ok(rn.contents)
    }

    /// Create or overwrite a note (`host/writeNote`, requires `fs:write`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PluginError> {
        let params = serde_json::to_value(WriteNoteParams {
            path: path.to_string(),
            contents: contents.to_string(),
        })?;
        self.call(METHOD_WRITE_NOTE, params)?;
        Ok(())
    }

    /// Ranked full-text search (`host/search`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchHitDto>, PluginError> {
        let params = serde_json::to_value(SearchParams { query: query.to_string() })?;
        let result = self.call(METHOD_SEARCH, params)?;
        let sr: SearchResultDto = serde_json::from_value(result)?;
        Ok(sr.hits)
    }

    /// List all notes (`host/listNotes`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn list_notes(&mut self) -> Result<Vec<NoteSummaryDto>, PluginError> {
        let result = self.call(METHOD_LIST_NOTES, Value::Null)?;
        let ln: ListNotesResult = serde_json::from_value(result)?;
        Ok(ln.notes)
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p cairn-plugin-sdk`
Expected: PASS — both host tests.

- [ ] **Step 7: Lint**

Run: `cargo clippy -p cairn-plugin-sdk --all-targets -- -D warnings`
Expected: clean. (The `serde` dependency is unused until Task 2 — an unused *dependency* is not a clippy warning, so this passes.)

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-plugin-sdk/Cargo.toml crates/cairn-plugin-sdk/src/lib.rs Cargo.toml Cargo.lock
git commit -m "feat(sdk): cairn-plugin-sdk crate — PluginError + Host callback handle"
```

---

## Task 2: `Plugin` builder + typed `command()` + run loop

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Write the failing run-loop tests**

Add a second test module to `crates/cairn-plugin-sdk/src/lib.rs`:

```rust
#[cfg(test)]
mod run_tests {
    use super::*;
    use cairn_plugin_protocol::{InitializeResult, METHOD_INITIALIZE, METHOD_INVOKE};
    use std::io::Cursor;

    fn request_line(id: u64, method: &str, params: Value) -> Vec<u8> {
        let mut buf = Vec::new();
        write_message(
            &mut buf,
            &Request { jsonrpc: JSONRPC_VERSION.to_string(), id, method: method.to_string(), params },
        )
        .unwrap();
        buf
    }

    fn drive(plugin: Plugin, input: &[u8]) -> Vec<Response> {
        let mut reader = Cursor::new(input.to_vec());
        let mut out: Vec<u8> = Vec::new();
        plugin.run_io(&mut reader, &mut out);
        out.split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice::<Response>(l).unwrap())
            .collect()
    }

    #[test]
    fn initialize_lists_commands_in_order() {
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.command("a", "A", |v: Value, _h| Ok(v));
        plugin.command("b", "B", |v: Value, _h| Ok(v));
        let out = drive(plugin, &request_line(1, METHOD_INITIALIZE, Value::Null));
        let init: InitializeResult = serde_json::from_value(out[0].result.clone().unwrap()).unwrap();
        assert_eq!(init.name, "ex");
        assert_eq!(init.version, "0.1.0");
        assert_eq!(
            init.commands.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn echo_roundtrips_and_unknown_command_is_minus_32601() {
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.command("echo", "Echo", |v: Value, _h| Ok(v));
        let mut input = request_line(1, METHOD_INVOKE, serde_json::json!({ "command": "echo", "args": { "x": 1 } }));
        input.extend(request_line(2, METHOD_INVOKE, serde_json::json!({ "command": "nope", "args": null })));
        let out = drive(plugin, &input);
        assert_eq!(out[0].result.clone().unwrap(), serde_json::json!({ "x": 1 }));
        assert_eq!(out[1].error.clone().unwrap().code, -32601);
    }

    #[test]
    fn bad_args_is_minus_32602() {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
        }
        let mut plugin = Plugin::new("ex", "0.1.0");
        // Reads `a.path` so the field isn't dead; on missing `path`, deserialize fails.
        plugin.command("needs", "Needs", |a: Args, _h| Ok(Value::String(a.path)));
        let out = drive(plugin, &request_line(1, METHOD_INVOKE, serde_json::json!({ "command": "needs", "args": {} })));
        assert_eq!(out[0].error.clone().unwrap().code, -32602);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-plugin-sdk run_tests`
Expected: COMPILE failure — `Plugin`, `Plugin::new`, `command`, `run_io` don't exist.

- [ ] **Step 3: Add the `Plugin` builder + run loop**

Add to the protocol `use` block at the top of `crates/cairn-plugin-sdk/src/lib.rs` the items the loop needs — change that `use cairn_plugin_protocol::{ ... };` block to also import `CommandDecl, InitializeResult, InvokeParams, METHOD_INITIALIZE, METHOD_INVOKE`:

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, ListNotesResult,
    ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchParams, SearchResultDto,
    WriteNoteParams, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
```

Then add the `Plugin` types + impl after the `Host` impl block (before the test modules):

```rust
/// A registered command: id, title, and a type-erased handler. The handler is
/// higher-ranked over the `Host` borrow so one stored closure accepts a Host of
/// any lifetime.
struct RegisteredCommand {
    id: String,
    title: String,
    handler: Box<dyn FnMut(Value, &mut Host<'_>) -> Result<Value, PluginError>>,
}

/// A plugin: a name/version and a set of typed commands. Build it, then
/// [`Plugin::run`].
pub struct Plugin {
    name: String,
    version: String,
    commands: Vec<RegisteredCommand>,
}

impl Plugin {
    /// Create an empty plugin.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self { name: name.into(), version: version.into(), commands: Vec::new() }
    }

    /// Register a typed command. `A` is deserialized from the invoke args; `O` is
    /// serialized into the result. Malformed args fail the invoke with JSON-RPC
    /// code -32602.
    pub fn command<A, O, F>(&mut self, id: impl Into<String>, title: impl Into<String>, mut handler: F)
    where
        A: serde::de::DeserializeOwned,
        O: serde::Serialize,
        F: FnMut(A, &mut Host<'_>) -> Result<O, PluginError> + 'static,
    {
        let boxed: Box<dyn FnMut(Value, &mut Host<'_>) -> Result<Value, PluginError>> =
            Box::new(move |raw: Value, host: &mut Host<'_>| {
                let args: A = serde_json::from_value(raw)
                    .map_err(|e| PluginError { code: -32602, message: e.to_string() })?;
                let out: O = handler(args, host)?;
                Ok(serde_json::to_value(out)?)
            });
        self.commands.push(RegisteredCommand { id: id.into(), title: title.into(), handler: boxed });
    }

    /// Run the stdio loop until stdin EOF, using real stdin/stdout.
    pub fn run(self) {
        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin.lock());
        let mut stdout = std::io::stdout();
        self.run_io(&mut reader, &mut stdout);
    }

    /// The loop, parameterized over IO for testing. Reads a `Request`, dispatches,
    /// writes a `Response`, until EOF or a read/write error.
    fn run_io(mut self, reader: &mut dyn BufRead, stdout: &mut dyn Write) {
        let mut next_cb_id: u64 = 1000;
        loop {
            let req: Request = match read_message(reader) {
                Ok(Some(r)) => r,
                _ => break, // EOF or malformed input → stop
            };
            let resp = self.handle(&req, reader, stdout, &mut next_cb_id);
            if write_message(stdout, &resp).is_err() {
                break;
            }
        }
    }

    fn handle(
        &mut self,
        req: &Request,
        reader: &mut dyn BufRead,
        stdout: &mut dyn Write,
        next_cb_id: &mut u64,
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
                    name: self.name.clone(),
                    version: self.version.clone(),
                    commands: self
                        .commands
                        .iter()
                        .map(|c| CommandDecl { id: c.id.clone(), title: c.title.clone() })
                        .collect(),
                };
                resp.result = Some(serde_json::to_value(init).unwrap_or(Value::Null));
            }
            METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
                Ok(p) => match self.commands.iter_mut().find(|c| c.id == p.command) {
                    Some(cmd) => {
                        let mut host = Host { reader, stdout, next_cb_id };
                        match (cmd.handler)(p.args, &mut host) {
                            Ok(value) => resp.result = Some(value),
                            Err(e) => resp.error = Some(RpcError { code: e.code, message: e.message }),
                        }
                    }
                    None => {
                        resp.error = Some(RpcError {
                            code: -32601,
                            message: format!("unknown command {}", p.command),
                        });
                    }
                },
                Err(e) => {
                    resp.error = Some(RpcError { code: -32602, message: e.to_string() });
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
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-plugin-sdk`
Expected: PASS — the three run-loop tests + the two host tests from Task 1.

- [ ] **Step 5: Lint**

Run: `cargo clippy -p cairn-plugin-sdk --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "feat(sdk): Plugin builder, typed command(), and run loop"
```

---

## Task 3: Rewrite the example plugin onto the SDK

**Files:**
- Modify: `crates/cairn-plugin-example/Cargo.toml`
- Modify: `crates/cairn-plugin-example/src/main.rs`

The `crates/cairn-plugin-example/tests/host.rs` e2e suite is the cross-check: it must pass **unchanged**, proving the SDK reproduces the hand-rolled wire behavior.

- [ ] **Step 1: Swap the example's dependencies**

In `crates/cairn-plugin-example/Cargo.toml`, change the `[dependencies]` section from:

```toml
[dependencies]
cairn-plugin-protocol = { path = "../cairn-plugin-protocol" }
serde_json = { workspace = true }
```

to:

```toml
[dependencies]
cairn-plugin-sdk = { path = "../cairn-plugin-sdk" }
serde = { workspace = true }
serde_json = { workspace = true }
```

Leave `[dev-dependencies]` and `[lints]` unchanged.

- [ ] **Step 2: Rewrite `main.rs` onto the SDK**

Replace the entire contents of `crates/cairn-plugin-example/src/main.rs` with:

```rust
//! Example cairn plugin built on `cairn-plugin-sdk`: declares commands + typed
//! handlers; the SDK owns the JSON-RPC/NDJSON loop and the host-callbacks.
//! `echo` returns its args; `noteLen`/`writeNote`/`noteCount`/`find` call back to
//! the host.

use cairn_plugin_sdk::{Host, Plugin};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    contents: String,
}

#[derive(Deserialize)]
struct QueryArgs {
    query: String,
}

fn main() {
    let mut plugin = Plugin::new("example", env!("CARGO_PKG_VERSION"));

    plugin.command("echo", "Echo", |args: Value, _host| Ok(args));

    plugin.command("noteLen", "Note length", |a: PathArgs, host: &mut Host| {
        let contents = host.read_note(&a.path)?;
        Ok(json!({ "len": contents.len() }))
    });

    plugin.command("writeNote", "Write note", |a: WriteArgs, host: &mut Host| {
        host.write_note(&a.path, &a.contents)?;
        Ok(json!({ "written": true }))
    });

    plugin.command("noteCount", "Note count", |_a: Value, host: &mut Host| {
        let notes = host.list_notes()?;
        Ok(json!({ "count": notes.len() }))
    });

    plugin.command("find", "Find", |a: QueryArgs, host: &mut Host| {
        let hits = host.search(&a.query)?;
        Ok(json!({ "hits": hits.len() }))
    });

    plugin.run();
}
```

(Note: the `echo` handler's `O` is `Value`; the others' `O` is also `Value` from `json!`. If type inference complains about the `host` parameter, annotate it `host: &mut Host<'_>` — the typed handlers above already annotate `&mut Host`.)

- [ ] **Step 3: Build the example**

Run: `cargo build -p cairn-plugin-example`
Expected: compiles. (If the borrow/lifetime on a closure fails to infer, annotate the closure's host param as `&mut Host<'_>`.)

- [ ] **Step 4: Run the e2e cross-check (must pass unchanged)**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — all 8 tests (`host_loads_invokes_and_rejects_unknown`, `note_len_reads_via_callback`, `note_len_denied_without_capability`, `write_note_via_callback`, `write_denied_without_fs_write`, `note_count_via_callback`, `find_via_callback`, `search_denied_without_fs_read`) pass with **no changes to `tests/host.rs`**. This proves byte-for-byte wire parity.

- [ ] **Step 5: Full workspace suite + lint + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green, no warnings, fmt clean, lock consistent (the dep swap changed `Cargo.lock` — it was already staged in Task 1 when the crate was added; re-stage if this task changed it further).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-plugin-example/Cargo.toml crates/cairn-plugin-example/src/main.rs Cargo.lock
git commit -m "refactor(example): rewrite example plugin onto cairn-plugin-sdk"
```

---

## Notes for the implementer

- **The cross-check is `tests/host.rs` — do NOT modify it.** Its passing is the proof the SDK matches the hand-rolled behavior. If it fails, the SDK has a wire-level discrepancy (error code, result shape, or callback framing) — fix the SDK, not the test.
- **Command order matters:** `initialize` lists commands in registration order; keep `echo, noteLen, writeNote, noteCount, find` so the declared-commands list matches what the host expects.
- **Callback ids start at 1000** (as the hand-rolled example did); `run_io` seeds `next_cb_id = 1000` and `Host::call` pre-increments, so the first callback is id 1001.
- **`run_io` is private** but reachable from the in-crate `#[cfg(test)]` modules — that's how the loop is unit-tested without a real process.
- **`fmt`:** run `cargo fmt` before committing each task (subagents don't auto-format; CI's rustfmt check is strict).
- **Don't touch** the host crates, `cairn-plugin-protocol`, or `tests/host.rs`.
```
