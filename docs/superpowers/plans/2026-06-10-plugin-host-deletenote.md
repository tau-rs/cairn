# Plugin Host `host/deleteNote` + Capability Constants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `host/deleteNote` plugin callback (delete a note, emitting `NoteDeleted`, gated on `fs:write`) and replace the capability string literals in the host gate with shared `CAP_FS_READ`/`CAP_FS_WRITE` constants.

**Architecture:** `host/deleteNote` mirrors `host/writeNote` across every layer (protocol DTO → `PluginCallbacks` port → `EngineCallbacks` routing to the existing `Engine::delete_note` → host dispatch → SDK `Host` method → example command). The cap-name constants live in the protocol crate and are used by the host's `required_cap` gate. `PluginHost`'s trait signature is unchanged (the sink rides inside the handler).

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), JSON-RPC 2.0 over NDJSON/stdio, serde/serde_json, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-host-deletenote-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-protocol/src/lib.rs` | `CAP_FS_READ`/`CAP_FS_WRITE`, `METHOD_DELETE_NOTE`, `DeleteNoteParams` | 1 |
| `crates/cairn-ports/src/lib.rs` | `PluginCallbacks::delete_note` | 2 |
| `crates/cairn-app/src/lib.rs` | `EngineCallbacks::delete_note` + engine event test | 2 |
| `crates/cairn-plugin-example/tests/host.rs` | `MapCallbacks::delete_note` (T2); e2e delete tests (T3) | 2, 3 |
| `crates/cairn-infra/src/plugin_host.rs` | `required_cap` const swap + delete mapping; `service_callback` delete arm | 3 |
| `crates/cairn-plugin-sdk/src/lib.rs` | `Host::delete_note` + unit test | 3 |
| `crates/cairn-plugin-example/src/main.rs` | `deleteNote` command | 3 |

**Unchanged:** `PluginHost` trait signature, `cairn-contract`, `cairn-cli`, daemon. `Engine::invoke_plugin_command` already has the `sink` param (from slice 3b), so there is **no** service-layer ripple this slice.

**Plan-level refinement vs spec:** the spec suggested the example test manifests reference `CAP_FS_READ`/`CAP_FS_WRITE`. The new delete e2e tests keep **bare literal** capability strings (`"\"fs:write\""`), matching the existing write/read tests — a manifest fixture should show the actual wire string a plugin author writes, and using the const there would require adding `cairn-plugin-protocol` as a dev-dependency for marginal benefit. The const cleanup is applied where it matters: the authoritative `required_cap` gate.

---

## Task 1: Protocol — capability constants + `deleteNote` types

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`

Purely additive; the crate compiles and all existing tests pass throughout. TDD.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
    #[test]
    fn delete_note_params_roundtrip_and_caps() {
        let dp = DeleteNoteParams { path: "a.md".into() };
        let v = serde_json::to_value(&dp).unwrap();
        assert_eq!(serde_json::from_value::<DeleteNoteParams>(v).unwrap(), dp);
        assert_eq!(METHOD_DELETE_NOTE, "host/deleteNote");
        assert_eq!(CAP_FS_READ, "fs:read");
        assert_eq!(CAP_FS_WRITE, "fs:write");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol delete_note_params`
Expected: COMPILE failure — `DeleteNoteParams`, `METHOD_DELETE_NOTE`, `CAP_FS_READ`, `CAP_FS_WRITE` don't exist.

- [ ] **Step 3: Add the protocol items**

In `crates/cairn-plugin-protocol/src/lib.rs`, after the `METHOD_LIST_NOTES` const add:

```rust
/// Plugin -> host: delete a note. Requires the `fs:write` capability.
pub const METHOD_DELETE_NOTE: &str = "host/deleteNote";

/// Capability: read the cairn (read/search/list note content + metadata).
pub const CAP_FS_READ: &str = "fs:read";
/// Capability: mutate the cairn (create/overwrite/delete notes).
pub const CAP_FS_WRITE: &str = "fs:write";
```

After the `WriteNoteParams` struct add:

```rust
/// Params of the `host/deleteNote` callback. Success result is an empty object `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteNoteParams {
    pub path: String,
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS — new test + all existing.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(protocol): host/deleteNote types + CAP_FS_READ/CAP_FS_WRITE consts"
```

---

## Task 2: `delete_note` port + engine routing (keep tree green)

Adds `delete_note` to the `PluginCallbacks` trait, which breaks the two impls (`EngineCallbacks` in cairn-app, `MapCallbacks` in the example tests) until updated — so this task updates both and adds the engine-level event test. The host dispatch + SDK + example command come in Task 3; here a plugin can't yet trigger `deleteNote` (the host's `required_cap` doesn't know it), but no test needs that until Task 3.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-app/src/lib.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Write the failing engine event test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-app/src/lib.rs`, add a stub host that deletes, plus the test:

```rust
    /// A stub host whose invoke deletes a note via the callbacks handler —
    /// exercises delete event emission through invoke_plugin_command.
    struct CallbackDeleter;
    impl PluginHost for CallbackDeleter {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo { id: "d".into(), name: "d".into(), version: "0".into(), commands: Vec::new() }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            callbacks.delete_note(path)?;
            Ok(serde_json::json!({ "deleted": true }))
        }
    }

    #[test]
    fn delete_callback_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events: Vec<Event> = Vec::new();
        eng.write_note(&NotePath::new("x.md").unwrap(), "body", &mut events).unwrap();
        events.clear();
        eng.set_plugin_host(Box::new(CallbackDeleter));
        let out = eng
            .invoke_plugin_command("d", "del", &serde_json::json!({ "path": "x.md" }), &mut events)
            .unwrap();
        assert_eq!(out, serde_json::json!({ "deleted": true }));
        assert!(events.contains(&Event::NoteDeleted(NotePath::new("x.md").unwrap())));
        assert!(eng.read_note(&NotePath::new("x.md").unwrap()).is_err());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-app delete_callback_emits_event`
Expected: COMPILE failure — `PluginCallbacks` has no `delete_note`.

- [ ] **Step 3: Add `delete_note` to the `PluginCallbacks` trait**

In `crates/cairn-ports/src/lib.rs`, extend the `PluginCallbacks` trait (after the existing `list_notes` method):

```rust
    /// Delete a note. Gated on the `fs:write` capability. Emits a delete event
    /// through the host's sink.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note does not exist; [`PortError`] on a
    /// storage failure.
    fn delete_note(&mut self, path: &str) -> Result<(), PortError>;
```

- [ ] **Step 4: Implement `EngineCallbacks::delete_note`**

In `crates/cairn-app/src/lib.rs`, in the `impl ... PluginCallbacks for EngineCallbacks` block, add after `write_note`:

```rust
    fn delete_note(&mut self, path: &str) -> Result<(), PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        // Routes through the engine delete path: removes the note + caches and
        // emits NoteDeleted through the sink.
        self.engine.delete_note(&np, self.sink)
    }
```

(`Engine::delete_note(&mut self, &NotePath, &mut dyn EventSink)` already exists — it calls `store.delete` then `apply_change(FsChange::Removed)`, emitting `NoteDeleted`.)

- [ ] **Step 5: Keep `MapCallbacks` compiling — add `delete_note`**

In `crates/cairn-plugin-example/tests/host.rs`, in the `impl PluginCallbacks for MapCallbacks` block, add after `write_note`:

```rust
    fn delete_note(&mut self, path: &str) -> Result<(), PortError> {
        self.0.remove(path);
        Ok(())
    }
```

- [ ] **Step 6: Run the workspace suite**

Run: `cargo test --workspace`
Expected: PASS — `delete_callback_emits_event` (proves delete routing + event), every other crate green (the example host tests still compile + pass with the extended `MapCallbacks`).

- [ ] **Step 7: Lint + fmt**

Run: `cargo fmt` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin): PluginCallbacks::delete_note + engine routing"
```

---

## Task 3: Host dispatch + caps-const swap + SDK + example + e2e

Wire `deleteNote` through the host (capability-gated) and the SDK, add the example command, and the real-subprocess e2e tests. Also swap the host gate's capability literals to the new constants. TDD order: add the example command (fixture) + write the failing e2e tests, then implement the host dispatch + SDK.

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Modify: `crates/cairn-plugin-sdk/src/lib.rs`

- [ ] **Step 1: Add the `deleteNote` command to the example**

In `crates/cairn-plugin-example/src/main.rs`, add a command after the `find` registration and before `plugin.run();` (the `deleteNote` args are `{path}`, so reuse the existing `PathArgs`):

```rust
    plugin.command("deleteNote", "Delete note", |a: PathArgs, host: &mut Host| {
        host.delete_note(&a.path)?;
        Ok(json!({ "deleted": true }))
    });
```

- [ ] **Step 2: Add the `Host::delete_note` SDK unit test (failing)**

In `crates/cairn-plugin-sdk/src/lib.rs`, in the `#[cfg(test)] mod host_tests` block, add:

```rust
    #[test]
    fn delete_note_sends_request() {
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: Some(serde_json::json!({})),
            error: None,
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        {
            let mut host = Host { reader: &mut reader, stdout: &mut out, next_cb_id: &mut cb_id };
            host.delete_note("gone.md").unwrap();
        }
        let first_line = out.split(|&b| b == b'\n').next().unwrap();
        let written: Request = serde_json::from_slice(first_line).unwrap();
        assert_eq!(written.method, METHOD_DELETE_NOTE);
        assert_eq!(written.params["path"], "gone.md");
    }
```

- [ ] **Step 3: Write the failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`, add two tests (the `write_manifest` helper + `MapCallbacks` already exist; `MapCallbacks::delete_note` was added in Task 2):

```rust
#[test]
fn delete_note_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:write\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("n.md".to_string(), "body".to_string())]));
    let out = host
        .invoke("example", "deleteNote", &serde_json::json!({"path": "n.md"}), &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"deleted": true}));
    assert!(!cb.0.contains_key("n.md"), "the note should be removed");
}

#[test]
fn delete_denied_without_fs_write() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\""); // read but NOT write
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("n.md".to_string(), "body".to_string())]));
    let err = host
        .invoke("example", "deleteNote", &serde_json::json!({"path": "n.md"}), &mut cb)
        .unwrap_err();
    assert!(matches!(err, PortError::Adapter(_)), "expected Adapter, got {err:?}");
    assert!(cb.0.contains_key("n.md"), "denied delete must not mutate");
}
```

- [ ] **Step 4: Run the new tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host delete && cargo test -p cairn-plugin-sdk delete_note_sends_request`
Expected: the example `delete_note_via_callback` fails (the host denies `host/deleteNote` as an unknown method — no `required_cap` mapping / dispatch arm yet) and the SDK test fails to COMPILE (`Host::delete_note` doesn't exist). (`delete_denied_without_fs_write` may pass coincidentally.)

- [ ] **Step 5: Add `Host::delete_note` in the SDK**

In `crates/cairn-plugin-sdk/src/lib.rs`, extend the protocol `use` block to add `DeleteNoteParams` and `METHOD_DELETE_NOTE` (keep all existing imports — the block becomes):

```rust
use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, DeleteNoteParams, InitializeResult, InvokeParams,
    ListNotesResult, ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchParams,
    SearchResultDto, WriteNoteParams, JSONRPC_VERSION, METHOD_DELETE_NOTE, METHOD_INITIALIZE,
    METHOD_INVOKE, METHOD_LIST_NOTES, METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
```

In the `impl Host<'_>` block, add after `write_note`:

```rust
    /// Delete a note (`host/deleteNote`, requires `fs:write`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn delete_note(&mut self, path: &str) -> Result<(), PluginError> {
        let params = serde_json::to_value(DeleteNoteParams { path: path.to_string() })?;
        // host/deleteNote returns an empty `{}` body on success; nothing to extract.
        self.call(METHOD_DELETE_NOTE, params)?;
        Ok(())
    }
```

- [ ] **Step 6: Implement host dispatch + the caps-const swap**

In `crates/cairn-infra/src/plugin_host.rs`, extend the protocol `use` block to add `CAP_FS_READ, CAP_FS_WRITE, DeleteNoteParams, METHOD_DELETE_NOTE` (keep existing). Then replace `required_cap` with (literals → consts, plus the delete mapping):

```rust
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some(CAP_FS_READ),
        METHOD_WRITE_NOTE => Some(CAP_FS_WRITE),
        METHOD_DELETE_NOTE => Some(CAP_FS_WRITE),
        METHOD_SEARCH => Some(CAP_FS_READ),
        METHOD_LIST_NOTES => Some(CAP_FS_READ),
        _ => None,
    }
}
```

In `service_callback`'s `match cb.method.as_str()`, add a `METHOD_DELETE_NOTE` arm after the `METHOD_WRITE_NOTE` arm (mirrors write):

```rust
                METHOD_DELETE_NOTE => {
                    match serde_json::from_value::<DeleteNoteParams>(cb.params.clone()) {
                        Ok(p) => match callbacks.delete_note(&p.path) {
                            Ok(()) => resp.result = Some(serde_json::json!({})),
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
```

- [ ] **Step 7: Run the new tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host && cargo test -p cairn-plugin-sdk`
Expected: PASS — `delete_note_via_callback` → `{"deleted": true}` + entry removed; `delete_denied_without_fs_write` → `Adapter` + entry kept; `delete_note_sends_request` (SDK); all existing host + SDK tests still pass.

- [ ] **Step 8: Full workspace suite + lint + fmt + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green, no warnings, fmt clean, lock consistent (no new deps).

- [ ] **Step 9: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-sdk/src/lib.rs \
        crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin): host/deleteNote dispatch + SDK + example; cap-name consts in gate"
```

---

## Notes for the implementer

- **`deleteNote` mirrors `writeNote`** at every layer — when in doubt, copy the write path and swap write→delete, `WriteNoteParams`→`DeleteNoteParams`, `write_note`→`delete_note`, and the success body stays `{}` (the example returns `{"deleted": true}` from its own handler, distinct from the callback's `{}`).
- **`Engine::delete_note` already exists** — do NOT add a new engine method; `EngineCallbacks::delete_note` just adapts the `&str` path and delegates.
- **`PluginHost` trait signature is unchanged**, and `Engine::invoke_plugin_command` already takes the `sink` (slice 3b) — no service/CLI/daemon changes.
- **Capability constants** are the cleanup's real value: `required_cap` is the single enforcement gate. Test-manifest capability strings stay literal on purpose (a fixture should show the actual wire string).
- **fmt:** run `cargo fmt` before committing each task (subagents don't auto-format; CI's rustfmt check is strict).
- **Don't touch** `cairn-contract`, `cairn-cli`, the daemon, or `Engine::invoke_plugin_command`'s signature.
```
