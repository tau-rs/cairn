# Plugin Host Slice 3b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `host/writeNote` (event-emitting, `fs:write`), `host/search`, and `host/listNotes` (`fs:read`) callbacks to the slice-3a plugin host.

**Architecture:** Extends slice 3a's full-duplex dispatch loop with three more capability-gated callbacks. The new mechanic is threading the `EventSink` through the callback boundary: `EngineCallbacks` gains a sink field and `Engine::invoke_plugin_command` gains a `sink` param, so a plugin's `write_note` routes through `Engine::write_note` and emits `NoteChanged`/`Reindexed`. `PluginHost`'s trait signature is unchanged (the sink rides inside the handler).

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), JSON-RPC 2.0 over NDJSON/stdio, serde/serde_json, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-host-slice3b-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-protocol/src/lib.rs` | New method consts + `WriteNoteParams`, `SearchParams`, `SearchHitDto`, `SearchResultDto`, `NoteSummaryDto`, `ListNotesResult` | 1 |
| `crates/cairn-ports/src/lib.rs` | `PluginCallbacks` gains `write_note`/`search`/`list_notes` | 2 |
| `crates/cairn-app/src/lib.rs` | `EngineCallbacks` + sink field + 3 impls; `invoke_plugin_command` gains `sink`; engine-level write-emits-event test | 2 |
| `crates/cairn-service/src/lib.rs` | `dispatch_command` passes its `sink` to `invoke_plugin_command` | 2 |
| `crates/cairn-plugin-example/tests/host.rs` | `MapCallbacks` implements the 3 new methods (T2); e2e tests (T3) | 2, 3 |
| `crates/cairn-infra/src/plugin_host.rs` | `required_cap` + `service_callback` dispatch for the 3 methods | 3 |
| `crates/cairn-plugin-example/src/main.rs` | `writeNote`/`noteCount`/`find` commands | 3 |

**Unchanged:** `cairn-contract`, `cairn-cli`, daemon. `PluginHost` trait signature unchanged.

---

## Task 1: Protocol DTOs

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`

Purely additive; the crate compiles and all existing tests pass throughout. TDD.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
    #[test]
    fn slice3b_dtos_roundtrip() {
        let wp = WriteNoteParams { path: "a.md".into(), contents: "body".into() };
        let v = serde_json::to_value(&wp).unwrap();
        assert_eq!(serde_json::from_value::<WriteNoteParams>(v).unwrap(), wp);

        let sp = SearchParams { query: "hello".into() };
        assert_eq!(serde_json::from_value::<SearchParams>(serde_json::to_value(&sp).unwrap()).unwrap(), sp);

        let sr = SearchResultDto {
            hits: vec![SearchHitDto { path: "a.md".into(), score: 1.5, snippet: "hi".into() }],
        };
        let back: SearchResultDto = serde_json::from_value(serde_json::to_value(&sr).unwrap()).unwrap();
        assert_eq!(back.hits.len(), 1);
        assert_eq!(back.hits[0].path, "a.md");

        let ln = ListNotesResult {
            notes: vec![NoteSummaryDto { path: "a.md".into(), title: "A".into() }],
        };
        let back: ListNotesResult = serde_json::from_value(serde_json::to_value(&ln).unwrap()).unwrap();
        assert_eq!(back.notes, ln.notes);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol slice3b_dtos`
Expected: COMPILE failure — the new types/consts don't exist.

- [ ] **Step 3: Add the protocol items**

In `crates/cairn-plugin-protocol/src/lib.rs`, after the `METHOD_READ_NOTE` const add:

```rust
/// Plugin -> host: create/overwrite a note. Requires the `fs:write` capability.
pub const METHOD_WRITE_NOTE: &str = "host/writeNote";
/// Plugin -> host: ranked full-text search. Requires the `fs:read` capability.
pub const METHOD_SEARCH: &str = "host/search";
/// Plugin -> host: list all notes (path + title). Requires the `fs:read` capability.
pub const METHOD_LIST_NOTES: &str = "host/listNotes";
```

After the `ReadNoteResult` struct add:

```rust
/// Params of the `host/writeNote` callback. Success result is an empty object `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteNoteParams {
    pub path: String,
    pub contents: String,
}

/// Params of the `host/search` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
}

/// One ranked search hit (host -> plugin). Plugin-protocol-local; intentionally
/// omits the contract's UI-only highlight ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub path: String,
    pub score: f32,
    pub snippet: String,
}

/// Result of the `host/search` callback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub hits: Vec<SearchHitDto>,
}

/// One note summary (host -> plugin): path + display title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteSummaryDto {
    pub path: String,
    pub title: String,
}

/// Result of the `host/listNotes` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummaryDto>,
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS — new test + all existing protocol tests.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(protocol): write/search/listNotes callback DTOs"
```

---

## Task 2: Callbacks trait + sink threading (signature change, host behavior unchanged)

Adds the three trait methods, threads the sink through the engine, wires the service pass-through, and keeps the tree green (extend `MapCallbacks` so it still satisfies the trait). The host's dispatch of the new methods comes in Task 3 — here a plugin sending `host/writeNote` would just be denied (`required_cap` doesn't know it yet), but no test exercises that until Task 3 adds the example commands.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-app/src/lib.rs`
- Modify: `crates/cairn-service/src/lib.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Write the failing engine-level test (write callback emits an event)**

In the `#[cfg(test)] mod tests` block of `crates/cairn-app/src/lib.rs`, add a stub host that calls `write_note`, plus the test:

```rust
    /// A stub host whose invoke writes a note via the callbacks handler —
    /// exercises sink threading through invoke_plugin_command.
    struct CallbackWriter;
    impl PluginHost for CallbackWriter {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo { id: "w".into(), name: "w".into(), version: "0".into(), commands: Vec::new() }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            let contents = args["contents"].as_str().unwrap_or_default();
            callbacks.write_note(path, contents)?;
            Ok(serde_json::json!({ "written": true }))
        }
    }

    #[test]
    fn write_callback_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(CallbackWriter));
        let mut sink: Vec<Event> = Vec::new();
        let out = eng
            .invoke_plugin_command(
                "w",
                "write",
                &serde_json::json!({ "path": "x.md", "contents": "body text" }),
                &mut sink,
            )
            .unwrap();
        assert_eq!(out, serde_json::json!({ "written": true }));
        // The write routed through Engine::write_note: emitted NoteChanged...
        assert!(sink.contains(&Event::NoteChanged(NotePath::new("x.md").unwrap())));
        // ...and actually persisted.
        assert_eq!(eng.read_note(&NotePath::new("x.md").unwrap()).unwrap(), "body text");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-app write_callback_emits_event`
Expected: COMPILE failure — `PluginCallbacks` has no `write_note`; `invoke_plugin_command` takes no `sink`.

- [ ] **Step 3: Add the three methods to the `PluginCallbacks` trait**

In `crates/cairn-ports/src/lib.rs`, extend the `PluginCallbacks` trait (after the existing `read_note`):

```rust
    /// Create or overwrite a note. Gated on the `fs:write` capability. Emits
    /// change events through the host's sink.
    ///
    /// # Errors
    /// [`PortError`] on an invalid path or a storage failure.
    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError>;

    /// Ranked full-text search. Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError`] on an index failure.
    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError>;

    /// List all notes (for path + title). Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError`] on a storage failure.
    fn list_notes(&mut self) -> Result<Vec<Note>, PortError>;
```

`SearchHit` and `Note` are already imported in this file (`use cairn_domain::{Note, NotePath};` and the `SearchHit` struct is defined here). No import change needed.

- [ ] **Step 4: Add the sink field + impls in the engine**

In `crates/cairn-app/src/lib.rs`, change the `EngineCallbacks` struct to carry a sink:

```rust
struct EngineCallbacks<'a, S, I, V> {
    engine: &'a mut Engine<S, I, V>,
    sink: &'a mut dyn EventSink,
}
```

Extend its `PluginCallbacks` impl with the three methods (keep the existing `read_note`):

```rust
    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        // Routes through the engine write path: persists, updates the note cache,
        // and emits NoteChanged/Reindexed through the sink.
        self.engine.write_note(&np, contents, self.sink)
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.engine.search(query)
    }

    fn list_notes(&mut self) -> Result<Vec<Note>, PortError> {
        self.engine.list_notes()
    }
```

Add `SearchHit` to the `cairn_ports` import if not already present, and confirm `Note` is imported from `cairn_domain` (it is). The current `cairn_ports` import is:

```rust
use cairn_ports::{
    FileStamp, FsChange, NoopPluginHost, PluginCallbacks, PluginHost, PluginInfo, PortError,
    SearchHit, SearchIndex, VaultStore, Vcs,
};
```

`SearchHit` is already there — no change needed (verify before editing). `Note` is imported via `use cairn_domain::{rewrite_link_target, Graph, Note, NotePath};`.

Change `invoke_plugin_command` to take and thread the sink:

```rust
    pub fn invoke_plugin_command(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        sink: &mut dyn EventSink,
    ) -> Result<serde_json::Value, PortError> {
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        let result = {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.invoke(plugin, command, args, &mut cb)
            // cb is dropped here, releasing the &mut self borrow
        };
        self.plugins = host;
        result
    }
```

Keep the existing doc comment above the method (the panic-safety note from 3a still applies).

- [ ] **Step 5: Update the two existing cairn-app test call sites**

The existing tests call `invoke_plugin_command` with 3 args; add a sink. In `invoke_services_read_callback` (the 3a re-entrancy test), change:

```rust
        let out = eng
            .invoke_plugin_command("cb", "readit", &serde_json::json!({ "path": "a.md" }))
            .unwrap();
```

to:

```rust
        let mut sink: Vec<Event> = Vec::new();
        let out = eng
            .invoke_plugin_command("cb", "readit", &serde_json::json!({ "path": "a.md" }), &mut sink)
            .unwrap();
```

In `default_plugin_host_is_noop`, change:

```rust
        let err = eng
            .invoke_plugin_command("nope", "x", &serde_json::Value::Null)
            .unwrap_err();
```

to:

```rust
        let mut sink: Vec<Event> = Vec::new();
        let err = eng
            .invoke_plugin_command("nope", "x", &serde_json::Value::Null, &mut sink)
            .unwrap_err();
```

- [ ] **Step 6: Thread the sink in the service dispatcher**

In `crates/cairn-service/src/lib.rs`, the `Command::InvokePluginCommand` arm currently calls `engine.invoke_plugin_command(plugin, command, args)`. Change it to pass the `sink` that `dispatch_command` already receives:

```rust
        Command::InvokePluginCommand {
            plugin,
            command,
            args,
        } => {
            let result = engine.invoke_plugin_command(plugin, command, args, sink)?;
            Ok(CommandResponse::PluginResult { result })
        }
```

- [ ] **Step 7: Extend `MapCallbacks` so it satisfies the grown trait**

In `crates/cairn-plugin-example/tests/host.rs`, `MapCallbacks` currently impls only `read_note`. Add the three methods so the file compiles. Update its imports to bring in the domain/ports types it now constructs — change the top imports to:

```rust
use cairn_infra::ProcessPluginHost;
use cairn_domain::{Note, NotePath};
use cairn_ports::{PluginCallbacks, PluginHost, PortError, SearchHit};
use std::collections::HashMap;
```

(Add `cairn-domain` to `crates/cairn-plugin-example/Cargo.toml` `[dev-dependencies]` if not present: `cairn-domain = { path = "../cairn-domain" }`. Check first — if absent, add it and commit `Cargo.lock`.)

Extend the impl:

```rust
impl PluginCallbacks for MapCallbacks {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("note {path}")))
    }

    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError> {
        self.0.insert(path.to_string(), contents.to_string());
        Ok(())
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        // Substring match over values; deterministic for tests.
        let mut hits = Vec::new();
        for (path, contents) in &self.0 {
            if contents.contains(query) {
                hits.push(SearchHit {
                    path: NotePath::new(path)
                        .map_err(|e| PortError::Adapter(e.to_string()))?,
                    score: 1.0,
                    snippet: contents.clone(),
                    highlights: Vec::new(),
                });
            }
        }
        Ok(hits)
    }

    fn list_notes(&mut self) -> Result<Vec<Note>, PortError> {
        let mut notes: Vec<Note> = self
            .0
            .iter()
            .map(|(path, contents)| {
                NotePath::new(path)
                    .map(|np| Note::parse(np, contents))
                    .map_err(|e| PortError::Adapter(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        notes.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(notes)
    }
}
```

- [ ] **Step 8: Run the full workspace suite**

Run: `cargo test --workspace`
Expected: PASS — `write_callback_emits_event` (proves sink threading), the updated 3a tests, the existing example host test, and every other crate green.

- [ ] **Step 9: Lint + lock**

Run: `cargo clippy --workspace --all-targets -- -D warnings` then `cargo build --workspace --locked`.
Expected: no warnings; lock consistent (if `cairn-domain` dev-dep was added, `Cargo.lock` changed → stage it).

- [ ] **Step 10: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs crates/cairn-service/src/lib.rs \
        crates/cairn-plugin-example/tests/host.rs crates/cairn-plugin-example/Cargo.toml Cargo.lock
git commit -m "feat(plugin): write/search/list callbacks port + EventSink threading"
```

(Only `git add Cargo.lock`/`Cargo.toml` if the `cairn-domain` dev-dep was actually added.)

---

## Task 3: Host dispatch + example commands + e2e

Implement the host-side dispatch + capability mapping for the three methods, add the example plugin commands, and the e2e tests. TDD order: add the example commands (fixtures), write the failing e2e tests, then implement the host dispatch.

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`
- Modify: `crates/cairn-infra/src/plugin_host.rs`

- [ ] **Step 1: Add the three commands to the example plugin**

In `crates/cairn-plugin-example/src/main.rs`, update the protocol import to add the new items (keep all existing ones):

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, ListNotesResult,
    ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchParams, SearchResultDto,
    WriteNoteParams, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
```

In the `initialize` arm, add three `CommandDecl`s to the `commands` vec (after `echo` and `noteLen`):

```rust
                    CommandDecl { id: "writeNote".to_string(), title: "Write note".to_string() },
                    CommandDecl { id: "noteCount".to_string(), title: "Note count".to_string() },
                    CommandDecl { id: "find".to_string(), title: "Find".to_string() },
```

In the `METHOD_INVOKE` match, add three command arms (alongside `echo` and `noteLen`):

```rust
            Ok(p) if p.command == "writeNote" => {
                let path = p.args.get("path").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let contents = p.args.get("contents").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match write_note_via_host(reader, stdout, cb_id, &path, &contents) {
                    Ok(()) => resp.result = Some(serde_json::json!({ "written": true })),
                    Err(err) => resp.error = Some(err),
                }
            }
            Ok(p) if p.command == "noteCount" => {
                match list_notes_via_host(reader, stdout, cb_id) {
                    Ok(notes) => resp.result = Some(serde_json::json!({ "count": notes.notes.len() })),
                    Err(err) => resp.error = Some(err),
                }
            }
            Ok(p) if p.command == "find" => {
                let query = p.args.get("query").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match search_via_host(reader, stdout, cb_id, &query) {
                    Ok(res) => resp.result = Some(serde_json::json!({ "hits": res.hits.len() })),
                    Err(err) => resp.error = Some(err),
                }
            }
```

Add three callback helpers (next to the existing `read_note_via_host`). They follow the same pattern: build the request, write, read the response, propagate `error`, else parse the typed result.

```rust
/// Send a `host/writeNote` callback; success carries an empty `{}` body.
fn write_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
    contents: &str,
) -> Result<(), RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_WRITE_NOTE,
        serde_json::to_value(WriteNoteParams { path: path.to_string(), contents: contents.to_string() }).unwrap(),
    )?;
    let _ = result; // success body is `{}`, ignored
    Ok(())
}

/// Send a `host/listNotes` callback.
fn list_notes_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
) -> Result<ListNotesResult, RpcError> {
    let result = call_host(reader, stdout, cb_id, METHOD_LIST_NOTES, serde_json::Value::Null)?;
    serde_json::from_value(result).map_err(|e| RpcError { code: -32603, message: e.to_string() })
}

/// Send a `host/search` callback.
fn search_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    query: &str,
) -> Result<SearchResultDto, RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_SEARCH,
        serde_json::to_value(SearchParams { query: query.to_string() }).unwrap(),
    )?;
    serde_json::from_value(result).map_err(|e| RpcError { code: -32603, message: e.to_string() })
}
```

Refactor the shared callback round-trip out of `read_note_via_host` into a helper `call_host` (write request → read response → propagate error → return the `result` Value), and rewrite `read_note_via_host` to use it:

```rust
/// Send a host-callback request and return its `result` Value (or the host's error).
fn call_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, RpcError> {
    *cb_id += 1;
    let req = Request {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: *cb_id,
        method: method.to_string(),
        params,
    };
    write_message(stdout, &req).map_err(|e| RpcError { code: -32603, message: format!("callback write failed: {e}") })?;
    let cb_resp: Response = read_message(reader)
        .map_err(|e| RpcError { code: -32603, message: format!("callback read failed: {e}") })?
        .ok_or_else(|| RpcError { code: -32603, message: "host closed before callback response".to_string() })?;
    if let Some(err) = cb_resp.error {
        return Err(err);
    }
    cb_resp.result.ok_or_else(|| RpcError { code: -32603, message: "empty callback response".to_string() })
}

/// Send a `host/readNote` callback to the host and block for its response.
fn read_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
) -> Result<String, RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_READ_NOTE,
        serde_json::to_value(ReadNoteParams { path: path.to_string() }).unwrap(),
    )?;
    let rn: ReadNoteResult = serde_json::from_value(result).map_err(|e| RpcError { code: -32603, message: e.to_string() })?;
    Ok(rn.contents)
}
```

- [ ] **Step 2: Write the failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`, add a small helper to write a manifest with given capabilities, then the tests. (You may instead inline the manifest as the existing tests do — but a helper reduces repetition.)

```rust
fn write_manifest(pdir: &std::path::Path, bin: &str, caps: &str) {
    std::fs::create_dir_all(pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[{caps}]\n"
        ),
    )
    .unwrap();
}

#[test]
fn write_note_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:write\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    let out = host
        .invoke("example", "writeNote", &serde_json::json!({"path": "n.md", "contents": "hi there"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"written": true}));
    assert_eq!(cb.0.get("n.md").map(String::as_str), Some("hi there"));
}

#[test]
fn write_denied_without_fs_write() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\""); // read but NOT write
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    let err = host
        .invoke("example", "writeNote", &serde_json::json!({"path": "n.md", "contents": "x"}), &mut cb)
        .unwrap_err();
    assert!(matches!(err, PortError::Adapter(_)), "expected Adapter, got {err:?}");
    assert!(cb.0.is_empty(), "denied write must not mutate");
}

#[test]
fn note_count_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([
        ("a.md".to_string(), "alpha".to_string()),
        ("b.md".to_string(), "beta".to_string()),
    ]));
    let out = host
        .invoke("example", "noteCount", &serde_json::Value::Null, &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"count": 2}));
}

#[test]
fn find_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([
        ("a.md".to_string(), "the quick fox".to_string()),
        ("b.md".to_string(), "lazy dog".to_string()),
    ]));
    let out = host
        .invoke("example", "find", &serde_json::json!({"query": "quick"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"hits": 1}));
}

#[test]
fn search_denied_without_fs_read() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no capabilities
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("a.md".to_string(), "x".to_string())]));
    let err = host
        .invoke("example", "find", &serde_json::json!({"query": "x"}), &mut cb)
        .unwrap_err();
    assert!(matches!(err, PortError::Adapter(_)), "expected Adapter, got {err:?}");
}
```

- [ ] **Step 3: Run the e2e tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: the new tests FAIL — the host's `required_cap` doesn't know the new methods, so every new callback is denied (`writeNote` happy, `noteCount`, `find` happy would error; the two denied tests might pass coincidentally). All five must pass after Step 4.

- [ ] **Step 4: Implement host dispatch for the three methods**

In `crates/cairn-infra/src/plugin_host.rs`, update the protocol import to add the new items (keep existing):

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, Incoming, InitializeParams, InitializeResult,
    InvokeParams, ListNotesResult, Manifest, NoteSummaryDto, ReadNoteParams, ReadNoteResult,
    Request, Response, RpcError, SearchHitDto, SearchParams, SearchResultDto, WriteNoteParams,
    CALLBACK_DENIED, CALLBACK_FAILED, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
    METHOD_LIST_NOTES, METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
```

Extend `required_cap`:

```rust
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some("fs:read"),
        METHOD_WRITE_NOTE => Some("fs:write"),
        METHOD_SEARCH => Some("fs:read"),
        METHOD_LIST_NOTES => Some("fs:read"),
        _ => None,
    }
}
```

In `service_callback`, the allowed-method `match cb.method.as_str()` currently has `METHOD_READ_NOTE => { ... }` and the defensive `_ => CALLBACK_DENIED` arm. Add three arms before the `_`:

```rust
                METHOD_WRITE_NOTE => match serde_json::from_value::<WriteNoteParams>(cb.params.clone()) {
                    Ok(p) => match callbacks.write_note(&p.path, &p.contents) {
                        Ok(()) => resp.result = Some(serde_json::json!({})),
                        Err(e) => {
                            resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() });
                        }
                    },
                    Err(e) => {
                        resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() });
                    }
                },
                METHOD_SEARCH => match serde_json::from_value::<SearchParams>(cb.params.clone()) {
                    Ok(p) => match callbacks.search(&p.query) {
                        Ok(hits) => {
                            let dto = SearchResultDto {
                                hits: hits
                                    .into_iter()
                                    .map(|h| SearchHitDto {
                                        path: h.path.as_str().to_string(),
                                        score: h.score,
                                        snippet: h.snippet,
                                    })
                                    .collect(),
                            };
                            resp.result = serde_json::to_value(dto).ok();
                        }
                        Err(e) => {
                            resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() });
                        }
                    },
                    Err(e) => {
                        resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() });
                    }
                },
                METHOD_LIST_NOTES => match callbacks.list_notes() {
                    Ok(notes) => {
                        let dto = ListNotesResult {
                            notes: notes
                                .into_iter()
                                .map(|n| NoteSummaryDto {
                                    path: n.path.as_str().to_string(),
                                    title: n.display_title(),
                                })
                                .collect(),
                        };
                        resp.result = serde_json::to_value(dto).ok();
                    }
                    Err(e) => {
                        resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() });
                    }
                },
```

(`METHOD_LIST_NOTES` ignores `cb.params`. `n.path.as_str()` and `n.display_title()` are existing `cairn-domain` methods; `cairn-infra` already depends on `cairn-domain`.)

- [ ] **Step 5: Run the e2e tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — `write_note_via_callback`, `write_denied_without_fs_write`, `note_count_via_callback`, `find_via_callback`, `search_denied_without_fs_read`, plus the existing `host_loads_invokes_and_rejects_unknown`, `note_len_*` tests.

- [ ] **Step 6: Run the full workspace suite + lint + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo build --workspace --locked`.
Expected: all green, no warnings, lock consistent.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-example/tests/host.rs \
        crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(plugin): host dispatch for write/search/list callbacks + e2e"
```

---

## Notes for the implementer

- **`PluginHost` trait signature is unchanged** — only `Engine::invoke_plugin_command` grows a `sink` param. Don't touch `cairn-contract`, `cairn-cli`, or the daemon.
- **`host/writeNote` success body is `{}`** — the plugin checks only for absence of `error`; don't make the plugin parse a result struct for writes.
- **`host/listNotes` takes no params** — the host ignores `cb.params`; the plugin sends `serde_json::Value::Null`.
- **Windows CI:** manifest `command` paths stay in TOML *literal* (single-quote) strings.
- **Cargo.lock:** the only possible dependency change is adding `cairn-domain` as a `cairn-plugin-example` dev-dependency (Task 2 Step 7) — if added, commit `Cargo.lock` with it (CI runs `--locked`).
- **`MapCallbacks::search`** is a deterministic substring match over values (a test double); the real `EngineCallbacks::search` delegates to Tantivy via `Engine::search`.
```
