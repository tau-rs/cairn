# Plugin Content Processors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a plugin register a content processor that transforms note content on a new `RenderNote` read path, invoked host→plugin, side-effect-free.

**Architecture:** A new host→plugin method `content/process` reuses the existing `call_with_callbacks` machinery (like `cairn/event`). Rendering a note = raw read + a deterministic, fail-soft chain of matching processors. Raw `GetNote` is untouched; the recursion floor is structural (processor callbacks read raw content, which sits below the processor layer).

**Tech Stack:** Rust (workspace, MSRV 1.88), serde/serde_json, JSON-RPC/NDJSON over stdio, `thiserror` at boundaries, `tracing` for logs, `ts-rs` for wire-type TS export.

## Global Constraints

- MSRV 1.88; `forbid(unsafe_code)` workspace-wide (via `[lints] workspace = true`).
- Merge queue ENABLED: branch off `main` → PR → `gh pr merge --auto --squash`. No manual rebase / local-merge.
- Shared working dir: verify `git branch --show-current` before every commit.
- `cargo fmt --check` before every commit; a new dependency ⇒ `git add Cargo.lock`.
- TDD: write the failing test first. `thiserror` at boundaries, `anyhow` internally.
- DoD: `cargo test --workspace` + `cargo clippy --workspace --all-targets --locked` + `cargo fmt --check` all green.
- Protocol/SDK additions must be additive (`#[serde(default)]`) — Stream 6 shares these crates.
- Capabilities are NOT a security boundary (a loaded plugin is fully-trusted code). The `content:process` cap gates *whether the host invokes* the processor; the read-only callback restriction is about honesty + loop-prevention, not sandboxing.

**Task order (dependencies point inward):** 1 → 2 → 3 → 4 → 5 → 6 → 7. Tasks 3 and 4 both depend only on 1 (+ 2 for Task 3) and may be done in either order.

---

### Task 1: Protocol types + constants (`cairn-plugin-protocol`)

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub const METHOD_PROCESS_CONTENT: &str = "content/process";`
  - `pub const CAP_CONTENT_PROCESS: &str = "content:process";`
  - `pub struct ProcessContentParams { pub path: String, pub content: String }`
  - `pub struct ProcessContentResult { pub content: String }`
  - `pub struct ProcessorDecl { pub extensions: Vec<String> }`
  - `InitializeResult.processors: Vec<ProcessorDecl>` (serde-default)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
#[test]
fn content_process_constants_and_dtos() {
    assert_eq!(METHOD_PROCESS_CONTENT, "content/process");
    assert_eq!(CAP_CONTENT_PROCESS, "content:process");

    let p = ProcessContentParams { path: "a.md".into(), content: "hi".into() };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(serde_json::from_value::<ProcessContentParams>(v).unwrap(), p);

    let r = ProcessContentResult { content: "HI".into() };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(serde_json::from_value::<ProcessContentResult>(v).unwrap(), r);

    let d = ProcessorDecl { extensions: vec!["md".into()] };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(serde_json::from_value::<ProcessorDecl>(v).unwrap(), d);
}

#[test]
fn initialize_result_processors_default_when_absent() {
    // An initialize reply from a plugin built before `processors` existed must
    // still decode (field defaults to empty).
    let json = r#"{"name":"x","version":"0","commands":[]}"#;
    let init: InitializeResult = serde_json::from_str(json).unwrap();
    assert!(init.processors.is_empty());
    assert!(init.contributions.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol content_process_constants_and_dtos`
Expected: FAIL — `cannot find value METHOD_PROCESS_CONTENT` / `ProcessContentParams` not found.

- [ ] **Step 3: Add the constants**

After the `CAP_NET` constant block (near line 37) in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
/// Capability: register a content processor (host->plugin `content/process`).
/// Like `events`, this gates whether the host *invokes* the plugin, not a
/// plugin->host callback. Declared in the manifest for auditability.
pub const CAP_CONTENT_PROCESS: &str = "content:process";
```

After `METHOD_CAIRN_EVENT` (near line 25):

```rust
/// Host -> plugin: transform a note's content on the read/render path.
/// Delivered only to plugins declaring `content:process`.
pub const METHOD_PROCESS_CONTENT: &str = "content/process";
```

- [ ] **Step 4: Add the DTOs**

Near the other callback DTOs (e.g. after `CairnEvent`, around line 211):

```rust
/// Params of the `content/process` request (host -> plugin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContentParams {
    pub path: String,
    pub content: String,
}

/// Result of `content/process`: the transformed content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContentResult {
    pub content: String,
}

/// One content-processor declaration a plugin returns at `initialize`. A note is
/// routed to the processor when its path matches any listed extension; an empty
/// list matches all note types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessorDecl {
    #[serde(default)]
    pub extensions: Vec<String>,
}
```

- [ ] **Step 5: Extend `InitializeResult`**

In the `InitializeResult` struct (near line 82), add after the `contributions` field:

```rust
    #[serde(default)]
    pub processors: Vec<ProcessorDecl>,
```

- [ ] **Step 6: Fix the existing `InitializeResult` literal in tests**

The `initialize_result_roundtrips` test constructs `InitializeResult { … contributions: vec![] }`. Add `processors: vec![],` to that literal so it still compiles.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS (all, including the two new tests).

- [ ] **Step 8: Commit**

```bash
git branch --show-current   # must be plugin-content-processors
cargo fmt
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(plugin-protocol): content/process method, cap, and DTOs"
```

---

### Task 2: `PluginHost::process_content` default no-op (`cairn-ports`)

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (the `PluginHost` trait, near line 389)
- Test: same file, or rely on Task 5's engine test. Add a focused unit test here.

**Interfaces:**
- Consumes: `PluginCallbacks`, `PortError` (existing).
- Produces: `PluginHost::process_content(&mut self, path: &str, content: &str, callbacks: &mut dyn PluginCallbacks) -> Result<String, PortError>` with a default body that returns `content` unchanged.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module of `crates/cairn-ports/src/lib.rs` (create one if absent; follow the crate's existing test style). This asserts the default no-op passes content through:

```rust
#[test]
fn noop_host_process_content_is_identity() {
    struct NoCb;
    impl PluginCallbacks for NoCb {
        fn read_note(&mut self, _: &str) -> Result<String, PortError> { unreachable!() }
        fn write_note(&mut self, _: &str, _: &str) -> Result<(), PortError> { unreachable!() }
        fn search(&mut self, _: &str) -> Result<Vec<SearchHit>, PortError> { unreachable!() }
        fn list_notes(&mut self) -> Result<Vec<Note>, PortError> { unreachable!() }
        fn delete_note(&mut self, _: &str) -> Result<(), PortError> { unreachable!() }
    }
    let mut host = NoopPluginHost;
    let out = host.process_content("a.md", "raw", &mut NoCb).unwrap();
    assert_eq!(out, "raw");
}
```

(Ensure the test module imports what it needs: `use super::*;`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-ports noop_host_process_content_is_identity`
Expected: FAIL — `no method named process_content`.

- [ ] **Step 3: Add the trait method with a default body**

In the `PluginHost` trait (after `dispatch_event`, near line 417):

```rust
    /// Transform note content through this plugin's content processors on the
    /// read/render path (host -> plugin `content/process`), servicing any
    /// read-only host-callbacks each processor makes. Default: identity (a host
    /// with no processors returns `content` unchanged).
    ///
    /// # Errors
    /// [`PortError`] only on an unexpected host/transport failure; a failing
    /// individual processor is logged and skipped by the implementation
    /// (fail-soft), not surfaced here.
    fn process_content(
        &mut self,
        _path: &str,
        content: &str,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<String, PortError> {
        Ok(content.to_string())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-ports noop_host_process_content_is_identity`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-ports/src/lib.rs
git commit -m "feat(ports): PluginHost::process_content default identity method"
```

---

### Task 3: Host dispatch — selection, ordering, read-only callbacks, fail-soft (`cairn-infra`)

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `METHOD_PROCESS_CONTENT`, `CAP_CONTENT_PROCESS`, `ProcessContentParams`, `ProcessContentResult`, `ProcessorDecl` (Task 1); `PluginHost::process_content` (Task 2).
- Produces (all crate-private helpers + the trait impl):
  - `fn processor_matches(decls: &[ProcessorDecl], path: &str) -> bool`
  - `fn fold_content(content: String, order: &[String], invoke: impl FnMut(&str, &str) -> Result<String, PortError>) -> String`
  - `struct ReadOnlyCallbacks<'a>(&'a mut dyn PluginCallbacks)`
  - `LoadedPlugin.processors: Vec<ProcessorDecl>`
  - `LoadedPlugin::process_one(&mut self, path: &str, content: &str, cb: &mut dyn PluginCallbacks) -> Result<String, PortError>`
  - `impl PluginHost for ProcessPluginHost { fn process_content(...) }`

- [ ] **Step 1: Write the failing unit tests (pure helpers + decorator)**

Add to the `tests` module of `crates/cairn-infra/src/plugin_host.rs`:

```rust
#[test]
fn processor_matches_by_extension() {
    use cairn_plugin_protocol::ProcessorDecl;
    let md = [ProcessorDecl { extensions: vec!["md".into()] }];
    assert!(super::processor_matches(&md, "notes/a.md"));
    assert!(!super::processor_matches(&md, "notes/a.txt"));

    let all = [ProcessorDecl { extensions: vec![] }];
    assert!(super::processor_matches(&all, "anything.canvas"));

    let none: [ProcessorDecl; 0] = [];
    assert!(!super::processor_matches(&none, "a.md")); // no decls => not a candidate
}

#[test]
fn fold_content_chains_in_order_and_is_fail_soft() {
    // Two processors that append their id: proves order + "each sees prior output".
    let order = vec!["a".to_string(), "b".to_string()];
    let out = super::fold_content("raw".into(), &order, |id, c| Ok(format!("{c}{id}")));
    assert_eq!(out, "rawab");

    // A failing processor is skipped; the chain keeps the last-good content.
    let out = super::fold_content("raw".into(), &order, |id, c| {
        if id == "a" { Err(PortError::Adapter("boom".into())) } else { Ok(format!("{c}{id}")) }
    });
    assert_eq!(out, "rawb"); // "a" failed -> content stayed "raw" -> "b" appended
}

#[test]
fn read_only_callbacks_forward_reads_deny_writes() {
    use super::ReadOnlyCallbacks;
    // Minimal in-memory callbacks double.
    struct Cb { read_calls: u32 }
    impl cairn_ports::PluginCallbacks for Cb {
        fn read_note(&mut self, _: &str) -> Result<String, PortError> {
            self.read_calls += 1; Ok("body".into())
        }
        fn write_note(&mut self, _: &str, _: &str) -> Result<(), PortError> { Ok(()) }
        fn search(&mut self, _: &str) -> Result<Vec<cairn_ports::SearchHit>, PortError> { Ok(vec![]) }
        fn list_notes(&mut self) -> Result<Vec<cairn_domain::Note>, PortError> { Ok(vec![]) }
        fn delete_note(&mut self, _: &str) -> Result<(), PortError> { Ok(()) }
    }
    let mut inner = Cb { read_calls: 0 };
    let mut ro = ReadOnlyCallbacks(&mut inner);
    assert_eq!(ro.read_note("a.md").unwrap(), "body");
    assert!(ro.write_note("a.md", "x").is_err());
    assert!(ro.delete_note("a.md").is_err());
    assert_eq!(inner.read_calls, 1);
}
```

(Add `use cairn_ports::PluginCallbacks;` to the test module if not already imported.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra plugin_host`
Expected: FAIL — `processor_matches`, `fold_content`, `ReadOnlyCallbacks` not found.

- [ ] **Step 3: Add the pure helpers**

In `crates/cairn-infra/src/plugin_host.rs` (near `required_cap`, module scope), add the imports to the top `use cairn_plugin_protocol::{…}` block: `CAP_CONTENT_PROCESS, ProcessContentParams, ProcessContentResult, ProcessorDecl, METHOD_PROCESS_CONTENT`. Then:

```rust
/// Whether any of a plugin's processor declarations matches `path` by extension.
/// An empty `extensions` list matches every note; a plugin with no declarations
/// matches nothing.
fn processor_matches(decls: &[ProcessorDecl], path: &str) -> bool {
    decls.iter().any(|d| {
        d.extensions.is_empty()
            || d.extensions.iter().any(|e| path.ends_with(&format!(".{e}")))
    })
}

/// Fold `content` through processors invoked in `order` (a list of plugin ids,
/// pre-sorted). Fail-soft: a processor that errors is logged and skipped, and the
/// chain continues from the last good content.
fn fold_content(
    mut content: String,
    order: &[String],
    mut invoke: impl FnMut(&str, &str) -> Result<String, PortError>,
) -> String {
    for id in order {
        match invoke(id, &content) {
            Ok(next) => content = next,
            Err(e) => {
                tracing::warn!(plugin = %id, error = %e, "content processor failed; keeping prior content");
            }
        }
    }
    content
}
```

- [ ] **Step 4: Add the `ReadOnlyCallbacks` decorator**

Also at module scope in `plugin_host.rs`:

```rust
/// Wraps a callbacks handler for the duration of `content/process`: reads pass
/// through; writes/deletes are refused so a render stays side-effect-free (and
/// cannot loop back through the engine's event sink). Not a security boundary —
/// a plugin can still write to disk directly; this only closes the host-callback
/// path (see the design doc, D4).
struct ReadOnlyCallbacks<'a>(&'a mut dyn PluginCallbacks);

impl PluginCallbacks for ReadOnlyCallbacks<'_> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0.read_note(path)
    }
    fn search(&mut self, query: &str) -> Result<Vec<cairn_ports::SearchHit>, PortError> {
        self.0.search(query)
    }
    fn list_notes(&mut self) -> Result<Vec<cairn_domain::Note>, PortError> {
        self.0.list_notes()
    }
    fn write_note(&mut self, _path: &str, _contents: &str) -> Result<(), PortError> {
        Err(PortError::Adapter("write not permitted during content processing".into()))
    }
    fn delete_note(&mut self, _path: &str) -> Result<(), PortError> {
        Err(PortError::Adapter("delete not permitted during content processing".into()))
    }
}
```

(Confirm `cairn_ports::SearchHit` and `cairn_domain::Note` are the exact return types in `PluginCallbacks` — copy them from the trait definition in `cairn-ports/src/lib.rs:332`.)

- [ ] **Step 5: Run the helper/decorator tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_host`
Expected: PASS for `processor_matches_by_extension`, `fold_content_chains_in_order_and_is_fail_soft`, `read_only_callbacks_forward_reads_deny_writes`. (Other plugin_host tests still pass.)

- [ ] **Step 6: Add the `processors` field to `LoadedPlugin` and populate it**

In `struct LoadedPlugin` (near line 105), add:

```rust
    /// Content-processor declarations the plugin returned at `initialize`.
    processors: Vec<ProcessorDecl>,
```

In `spawn_plugin`, the `LoadedPlugin { … }` literal (near line 584) initializes with `processors: Vec::new(),`. After `init` is parsed and `plugin.info.contributions = init.contributions;` (near line 613), add:

```rust
        plugin.processors = init.processors;
```

- [ ] **Step 7: Add `LoadedPlugin::process_one`**

As a method on `impl LoadedPlugin` (near `deliver_event`):

```rust
    /// Send one `content/process` request and return the transformed content,
    /// servicing the plugin's (read-only) callbacks meanwhile.
    fn process_one(
        &mut self,
        path: &str,
        content: &str,
        cb: &mut dyn PluginCallbacks,
    ) -> Result<String, PortError> {
        let params = serde_json::to_value(ProcessContentParams {
            path: path.to_string(),
            content: content.to_string(),
        })
        .map_err(adapt)?;
        let result = self.call_with_callbacks(METHOD_PROCESS_CONTENT, params, cb)?;
        let out: ProcessContentResult = serde_json::from_value(result).map_err(adapt)?;
        Ok(out.content)
    }
```

- [ ] **Step 8: Implement `PluginHost::process_content` for `ProcessPluginHost`**

In `impl PluginHost for ProcessPluginHost` (near `dispatch_event`, line 655):

```rust
    fn process_content(
        &mut self,
        path: &str,
        content: &str,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<String, PortError> {
        // Candidate = declared the cap AND has a matcher hitting this path.
        let mut order: Vec<String> = self
            .loaded
            .iter()
            .filter(|p| {
                p.capabilities.iter().any(|c| c == CAP_CONTENT_PROCESS)
                    && processor_matches(&p.processors, path)
            })
            .map(|p| p.info.id.clone())
            .collect();
        order.sort(); // deterministic chain order by plugin id

        let loaded = &mut self.loaded;
        let out = fold_content(content.to_string(), &order, |id, c| {
            let p = loaded
                .iter_mut()
                .find(|p| p.info.id == id)
                .expect("id came from `order`, built from `loaded`");
            let mut ro = ReadOnlyCallbacks(&mut *callbacks);
            p.process_one(path, c, &mut ro)
        });
        Ok(out)
    }
```

- [ ] **Step 9: Run the full crate tests**

Run: `cargo test -p cairn-infra`
Expected: PASS. (Behavioral chaining over live child processes is covered by Task 7's e2e; here the helpers + decorator are unit-tested and the impl compiles against them.)

- [ ] **Step 10: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(infra): content processor dispatch — cap gate, id-ordered fail-soft chain, read-only callbacks"
```

---

### Task 4: SDK — `Plugin::processor` + `content/process` dispatch (`cairn-plugin-sdk`)

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs`
- Test: same file (`#[cfg(test)] mod run_tests`)

**Interfaces:**
- Consumes: `ProcessContentParams`, `ProcessContentResult`, `ProcessorDecl`, `METHOD_PROCESS_CONTENT` (Task 1).
- Produces: `Plugin::processor(&mut self, extensions, handler)`; a new `run_io` arm dispatching `METHOD_PROCESS_CONTENT`.

- [ ] **Step 1: Write the failing tests**

Add to `mod run_tests` in `crates/cairn-plugin-sdk/src/lib.rs`:

```rust
#[test]
fn processor_is_declared_at_initialize() {
    use cairn_plugin_protocol::METHOD_INITIALIZE;
    let mut plugin = Plugin::new("ex", "0.1.0");
    plugin.processor(["md"], |p: cairn_plugin_protocol::ProcessContentParams, _h| {
        Ok(cairn_plugin_protocol::ProcessContentResult { content: p.content })
    });
    let out = drive(plugin, &request_line(1, METHOD_INITIALIZE, Value::Null));
    let init: InitializeResult = serde_json::from_value(out[0].result.clone().unwrap()).unwrap();
    assert_eq!(init.processors.len(), 1);
    assert_eq!(init.processors[0].extensions, vec!["md".to_string()]);
}

#[test]
fn content_process_invokes_handler() {
    use cairn_plugin_protocol::{ProcessContentResult, METHOD_PROCESS_CONTENT};
    let mut plugin = Plugin::new("ex", "0.1.0");
    plugin.processor(["md"], |p: cairn_plugin_protocol::ProcessContentParams, _h| {
        Ok(ProcessContentResult { content: p.content.to_uppercase() })
    });
    let params = serde_json::json!({ "path": "a.md", "content": "hi" });
    let out = drive(plugin, &request_line(1, METHOD_PROCESS_CONTENT, params));
    let res: ProcessContentResult = serde_json::from_value(out[0].result.clone().unwrap()).unwrap();
    assert_eq!(res.content, "HI");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-plugin-sdk processor_is_declared_at_initialize content_process_invokes_handler`
Expected: FAIL — `no method named processor` / unknown method `content/process`.

- [ ] **Step 3: Add the erased handler type + `Plugin` fields**

In `crates/cairn-plugin-sdk/src/lib.rs`, near `ErasedEventHandler` (line 219):

```rust
/// The erased content-processor handler stored on the `Plugin`.
type ErasedProcessorHandler =
    Box<dyn FnMut(ProcessContentParams, &mut Host<'_>) -> Result<ProcessContentResult, PluginError>>;
```

Add imports to the `use cairn_plugin_protocol::{…}` block: `ProcessContentParams, ProcessContentResult, ProcessorDecl, METHOD_PROCESS_CONTENT`.

In `struct Plugin` (line 232), add fields:

```rust
    processor_handler: Option<ErasedProcessorHandler>,
    processor_decls: Vec<ProcessorDecl>,
```

In `Plugin::new` (line 242), initialize them:

```rust
            processor_handler: None,
            processor_decls: Vec::new(),
```

- [ ] **Step 4: Add `Plugin::processor`**

After `on_event` (line 259):

```rust
    /// Register the plugin's content processor. `extensions` are bare (no dot),
    /// e.g. `["md"]`; an empty list matches all note types. The handler receives
    /// the note path + current content and returns the transformed content, with
    /// capability-gated (read-only) `Host` access. One processor per plugin;
    /// calling this again replaces the previous one.
    pub fn processor<I, S, F>(&mut self, extensions: I, handler: F)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        F: FnMut(ProcessContentParams, &mut Host<'_>) -> Result<ProcessContentResult, PluginError> + 'static,
    {
        self.processor_decls = vec![ProcessorDecl {
            extensions: extensions.into_iter().map(Into::into).collect(),
        }];
        self.processor_handler = Some(Box::new(handler));
    }
```

- [ ] **Step 5: Emit `processors` in the `initialize` reply**

In `handle`, the `METHOD_INITIALIZE` arm builds `InitializeResult { … }` (line 329). Add:

```rust
                    processors: self.processor_decls.clone(),
```

- [ ] **Step 6: Dispatch `content/process` in `handle`**

Add a new match arm in `handle` (after the `METHOD_CAIRN_EVENT` arm, before the `other =>` arm):

```rust
            METHOD_PROCESS_CONTENT => {
                match serde_json::from_value::<ProcessContentParams>(req.params.clone()) {
                    Ok(params) => {
                        if let Some(handler) = self.processor_handler.as_mut() {
                            let mut host = Host { reader, stdout, next_cb_id };
                            match handler(params, &mut host) {
                                Ok(out) => {
                                    resp.result = Some(serde_json::to_value(out).unwrap_or(Value::Null));
                                }
                                Err(e) => {
                                    resp.error = Some(RpcError { code: e.code, message: e.message });
                                }
                            }
                        } else {
                            // No processor registered: return content unchanged.
                            resp.result = Some(
                                serde_json::to_value(ProcessContentResult { content: params.content })
                                    .unwrap_or(Value::Null),
                            );
                        }
                    }
                    Err(e) => {
                        resp.error = Some(RpcError { code: -32602, message: e.to_string() });
                    }
                }
            }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p cairn-plugin-sdk`
Expected: PASS (all, including the two new tests).

- [ ] **Step 8: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): Plugin::processor and content/process dispatch"
```

---

### Task 5: Engine — `render_note` (`cairn-app`)

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `PluginHost::process_content` (Task 2); existing `Engine::read_note`, `EngineCallbacks`, `NoopPluginHost`, panic-catch pattern from `invoke_plugin_command`.
- Produces: `Engine::render_note(&mut self, path: &NotePath) -> Result<String, PortError>`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `crates/cairn-app/src/lib.rs` (reuse the existing `engine(tmp.path())` helper and `set_plugin_host`):

```rust
/// A stub host whose process_content uppercases and appends the result of a
/// read-only callback — proves render invokes processors and services reads.
struct UpcaseHost;
impl PluginHost for UpcaseHost {
    fn plugins(&self) -> Vec<PluginInfo> { Vec::new() }
    fn invoke(
        &mut self, _p: &str, _c: &str, _a: &serde_json::Value,
        _cb: &mut dyn cairn_ports::PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> { unreachable!() }
    fn process_content(
        &mut self, _path: &str, content: &str,
        _cb: &mut dyn cairn_ports::PluginCallbacks,
    ) -> Result<String, PortError> {
        Ok(content.to_uppercase())
    }
}

#[test]
fn render_note_applies_processors() {
    let tmp = tempfile::tempdir().unwrap();
    let mut eng = engine(tmp.path());
    let mut events: Vec<Event> = Vec::new();
    eng.write_note(&NotePath::new("a.md").unwrap(), "hello", &mut events).unwrap();
    eng.set_plugin_host(Box::new(UpcaseHost));
    let out = eng.render_note(&NotePath::new("a.md").unwrap()).unwrap();
    assert_eq!(out, "HELLO");
    // Raw read is unchanged (recursion floor / raw vs rendered).
    assert_eq!(eng.read_note(&NotePath::new("a.md").unwrap()).unwrap(), "hello");
}

#[test]
fn render_note_is_identity_with_noop_host() {
    let tmp = tempfile::tempdir().unwrap();
    let mut eng = engine(tmp.path());
    let mut events: Vec<Event> = Vec::new();
    eng.write_note(&NotePath::new("a.md").unwrap(), "hello", &mut events).unwrap();
    let out = eng.render_note(&NotePath::new("a.md").unwrap()).unwrap();
    assert_eq!(out, "hello"); // default NoopPluginHost::process_content is identity
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-app render_note_applies_processors render_note_is_identity_with_noop_host`
Expected: FAIL — `no method named render_note`.

- [ ] **Step 3: Implement `render_note`**

Add to `impl Engine` (near `invoke_plugin_command`, after `dispatch_plugin_event`, around line 604):

```rust
    /// Render a note: read its raw content, then transform it through the loaded
    /// content processors (host -> plugin). Read-only — processors may make gated
    /// read callbacks but cannot write, so this emits no events and is
    /// side-effect-free. A panicking host is caught and surfaced as an error (as
    /// in `invoke_plugin_command`), and the host is restored.
    ///
    /// # Errors
    /// [`PortError`] if the note is missing, or [`PortError::Adapter`] if the host
    /// panicked. Individual processor failures are logged and skipped by the host
    /// (fail-soft), not surfaced here.
    pub fn render_note(&mut self, path: &NotePath) -> Result<String, PortError> {
        let raw = self.read_note(path)?; // raw read = the recursion floor
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        // Writes are denied during processing, so this sink is never touched.
        let mut discard: Vec<Event> = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks { engine: self, sink: &mut discard };
            host.process_content(path.as_str(), &raw, &mut cb)
        }));
        self.plugins = host;
        result.unwrap_or_else(|_| Err(PortError::Adapter("plugin host panicked".into())))
    }
```

(Confirm `NotePath::as_str` exists — it is used in `plugin_host.rs`/`app` elsewhere. If the path accessor differs, match the existing usage in `EngineCallbacks`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-app render_note_applies_processors render_note_is_identity_with_noop_host`
Expected: PASS.

- [ ] **Step 5: Run the whole crate to catch the stub-host trait gap**

Run: `cargo test -p cairn-app`
Expected: PASS. (Existing stub `PluginHost` impls in tests inherit the default `process_content`, so they need no change.)

- [ ] **Step 6: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): Engine::render_note — raw read + side-effect-free processor chain"
```

---

### Task 6: Contract + service + daemon — `RenderNote` wiring

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (add `Query::RenderNote`)
- Modify: `crates/cairn-service/src/lib.rs` (guard arm + `dispatch_query_mut`)
- Modify: `crates/cairn-daemon/src/lib.rs` (`run_query_blocking` + `query_kind`)
- Test: `crates/cairn-service/src/lib.rs` tests

**Interfaces:**
- Consumes: `Engine::render_note` (Task 5); existing `dispatch_query`, `QueryResponse::Note`, `parse_path`.
- Produces:
  - `Query::RenderNote { path: String }`
  - `pub fn dispatch_query_mut(engine: &mut Engine, query: &Query) -> Result<QueryResponse, ServiceError>`
  - `dispatch_query` gains a `Query::RenderNote` guard arm (kept `&Engine`).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module of `crates/cairn-service/src/lib.rs` (mirror the existing `dispatch_query` test setup that builds an `eng`; reuse whatever engine-builder the surrounding tests use, and set a stub processor host on it):

```rust
#[test]
fn render_note_dispatches_through_processors() {
    // Build an engine with a note and an uppercasing processor host.
    // (Use the same `eng` builder the other dispatch_query tests use; set the
    // plugin host to a stub whose process_content uppercases — see cairn-app
    // UpcaseHost for the shape, or a local equivalent.)
    let mut eng = /* existing test engine builder */;
    // write "a.md" = "hello" via dispatch_command(&mut eng, &Command::WriteNote{...}, &mut sink)
    // eng.set_plugin_host(Box::new(UpcaseHostLocal));

    let mut guard_engine = eng;
    match dispatch_query_mut(&mut guard_engine, &Query::RenderNote { path: "a.md".into() }).unwrap() {
        QueryResponse::Note { contents } => assert_eq!(contents, "HELLO"),
        other => panic!("expected Note, got {other:?}"),
    }
    // Raw GetNote stays unprocessed.
    match dispatch_query(&guard_engine, &Query::GetNote { path: "a.md".into() }).unwrap() {
        QueryResponse::Note { contents } => assert_eq!(contents, "hello"),
        other => panic!("expected Note, got {other:?}"),
    }
}
```

> Implementer note: define a local `UpcaseHostLocal` stub in the service test module (same shape as `cairn-app`'s `UpcaseHost` in Task 5) since cross-crate test types aren't shared. Follow the existing service tests for how they construct an `Engine` with a plugin host.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-service render_note_dispatches_through_processors`
Expected: FAIL — `no variant RenderNote` / `dispatch_query_mut` not found.

- [ ] **Step 3: Add the `Query::RenderNote` variant**

In `crates/cairn-contract/src/lib.rs`, the `Query` enum (line 61), after `GetNote`:

```rust
    /// Read a note's contents with content processors applied (render path).
    RenderNote {
        /// Relative note path.
        path: String,
    },
```

- [ ] **Step 4: Add the guard arm to `dispatch_query`**

In `crates/cairn-service/src/lib.rs`, `dispatch_query` (line 143) — add an arm so the match stays exhaustive while keeping the function `&Engine`:

```rust
        Query::RenderNote { .. } => Err(ServiceError::InvalidRequest(
            "render_note requires the mutating dispatch path (dispatch_query_mut)".to_string(),
        )),
```

(Use the exact `ServiceError` read-only-friendly variant the crate already has for bad requests — `InvalidRequest` per `parse_path`. Match its constructor signature.)

- [ ] **Step 5: Add `dispatch_query_mut`**

In `crates/cairn-service/src/lib.rs`, after `dispatch_query`:

```rust
/// Dispatch a read-only query that may need `&mut Engine` (currently only
/// `RenderNote`, which invokes content processors). Everything else delegates to
/// [`dispatch_query`] (which auto-reborrows `&mut Engine` as `&Engine`), so the
/// read-only dispatcher stays `&Engine` and its many callers are untouched.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
pub fn dispatch_query_mut(
    engine: &mut Engine,
    query: &Query,
) -> Result<QueryResponse, ServiceError> {
    match query {
        Query::RenderNote { path } => {
            let p = parse_path(path)?;
            let contents = engine.render_note(&p)?;
            Ok(QueryResponse::Note { contents })
        }
        other => dispatch_query(engine, other),
    }
}
```

- [ ] **Step 6: Route the daemon through `dispatch_query_mut`**

In `crates/cairn-daemon/src/lib.rs`:

- Update the import (line 32): add `dispatch_query_mut` to the `cairn_service::{…}` list.
- `run_query_blocking` (line 190):

```rust
    pub fn run_query_blocking(&self, query: &Query) -> Result<QueryResponse, ServiceError> {
        let mut guard = self.engine();
        dispatch_query_mut(&mut guard, query)
    }
```

- `query_kind` (line 263): add the arm

```rust
        Query::RenderNote { .. } => "render_note",
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p cairn-contract -p cairn-service -p cairn-daemon`
Expected: PASS. (`cargo test -p cairn-contract` also regenerates/validates the `ts-rs` bindings for the new variant.)

- [ ] **Step 8: Verify TS bindings regenerated (if committed to the repo)**

Run: `git status --short` — if a generated `.ts` binding file changed, include it in the commit. If the repo generates bindings only on demand, run the crate's bindings command (check `crates/cairn-contract` for a `bindings`/`export` test or script) and stage the output.

- [ ] **Step 9: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-contract/src/lib.rs crates/cairn-service/src/lib.rs crates/cairn-daemon/src/lib.rs
# include regenerated TS bindings if any:
git add -A
git commit -m "feat(contract,service,daemon): RenderNote query via dispatch_query_mut"
```

---

### Task 7: Example processor + end-to-end tests (`cairn-plugin-example`)

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs` (register a processor)
- Modify: `crates/cairn-plugin-example/tests/host.rs` (e2e)

**Interfaces:**
- Consumes: `Plugin::processor` (Task 4); `ProcessPluginHost::process_content` (Task 3); `content:process` cap (Task 1); the `write_manifest`/`load_example`/`MapCallbacks` harness already in `tests/host.rs`.
- Produces: an example content processor + e2e coverage (happy, denied, fail-soft, read-only chaining).

- [ ] **Step 1: Add a transclusion processor to the example**

In `crates/cairn-plugin-example/src/main.rs`, before `plugin.run();`, add. This is a demo: each line `@include <path>` is replaced with that note's contents (a read-only callback). A missing target makes the handler error — which the host treats fail-soft.

```rust
    // Content processor: expand `@include <path>` lines by reading the target
    // note through the (read-only) host callback. A missing target errors, which
    // the host handles fail-soft (the note renders raw).
    plugin.processor(
        ["md"],
        |p: cairn_plugin_sdk::ProcessContentParams, host: &mut Host| {
            let mut out = String::new();
            for line in p.content.lines() {
                if let Some(target) = line.strip_prefix("@include ") {
                    let included = host.read_note(target.trim())?;
                    out.push_str(&included);
                } else {
                    out.push_str(line);
                }
                out.push('\n');
            }
            Ok(cairn_plugin_sdk::ProcessContentResult { content: out })
        },
    );
```

- [ ] **Step 2: Re-export the processor DTOs from the SDK if needed**

`ProcessContentParams`/`ProcessContentResult` must be reachable from the example. In `crates/cairn-plugin-sdk/src/lib.rs`, the existing `pub use cairn_plugin_protocol::{CairnEvent, NoteSummaryDto, SearchHitDto};` (line 68) — extend it:

```rust
pub use cairn_plugin_protocol::{
    CairnEvent, NoteSummaryDto, ProcessContentParams, ProcessContentResult, SearchHitDto,
};
```

- [ ] **Step 3: Write the failing e2e tests**

In `crates/cairn-plugin-example/tests/host.rs`, add. Use `write_manifest(pdir, bin, caps)` to control the declared capabilities.

```rust
#[test]
fn render_expands_include_when_cap_declared() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"content:process\",\"fs:read\"");

    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::from([
        ("main.md".to_string(), "@include body.md".to_string()),
        ("body.md".to_string(), "the body".to_string()),
    ]));

    let out = host.process_content("main.md", "@include body.md", &mut cb).unwrap();
    assert_eq!(out.trim(), "the body");
}

#[test]
fn render_is_raw_when_cap_absent() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\""); // no content:process

    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new());

    // Not a candidate (cap missing) => content unchanged.
    let out = host.process_content("main.md", "@include body.md", &mut cb).unwrap();
    assert_eq!(out, "@include body.md");
}

#[test]
fn render_is_fail_soft_on_missing_include_target() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"content:process\",\"fs:read\"");

    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new()); // target absent => read_note errors

    // Processor errors (missing target) => host keeps last-good (raw) content.
    let out = host.process_content("main.md", "@include gone.md", &mut cb).unwrap();
    assert_eq!(out, "@include gone.md");
}
```

- [ ] **Step 4: Run the e2e tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host render_`
Expected: FAIL — `no method named process_content` on the host until the workspace is rebuilt with Tasks 1-3, and the example binary lacks a processor until Step 1. (If Tasks 1-5 are already committed, the failure is only from the new example behavior / assertions.)

- [ ] **Step 5: Run the e2e tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS (all, including the three new tests and the pre-existing ones).

- [ ] **Step 6: Commit**

```bash
git branch --show-current
cargo fmt
git add crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-sdk/src/lib.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin-example): transclusion content processor + e2e (happy, denied, fail-soft)"
```

---

### Task 8: Full-workspace verification (DoD gate)

**Files:** none (verification only).

- [ ] **Step 1: Full test suite**

Run: `cargo test --workspace`
Expected: PASS. (Note: `invoke_times_out_and_kills_plugin` may be flaky in some sandboxes — a known pre-existing issue, not caused by this change.)

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 3: Format check**

Run: `cargo fmt --check`
Expected: clean (no diff).

- [ ] **Step 4: Push + open PR (merge queue)**

```bash
git branch --show-current            # plugin-content-processors
git push -u origin plugin-content-processors
gh pr create --base main --title "feat: plugin content processors" \
  --body "Implements the design in docs/superpowers/specs/2026-07-19-plugin-content-processors-design.md"
gh pr merge --auto --squash
```

---

## Self-Review

**Spec coverage** (each design decision → task):
- D1 (RenderNote invocation, raw floor) → Task 5 (`render_note` reads raw then processes) + Task 6 (`Query::RenderNote`). Raw-unchanged asserted in Task 5 & Task 6 tests.
- D2 (cap in manifest, matcher at initialize) → Task 1 (`CAP_CONTENT_PROCESS`, `ProcessorDecl`, `InitializeResult.processors`), Task 3 (cap gate + `processor_matches`), Task 4 (`Plugin::processor` emits decls).
- D3 (no stage) → Task 1 types carry no `stage`.
- D4 (read-only callbacks) → Task 3 (`ReadOnlyCallbacks`, unit-tested) + Task 5 (render passes them, no sink used).
- D5 (fail-soft, last-good) → Task 3 (`fold_content`, unit-tested) + Task 7 (missing-include e2e).
- D6 (chain by plugin id) → Task 3 (`order.sort()` + `fold_content` order test).
- D7 (Query variant; avoid &mut cascade) → Task 6 (`dispatch_query_mut` wrapper + guard arm; `dispatch_query` stays `&Engine`).
- e2e happy + denied → Task 7.

**Placeholder scan:** Task 6 Step 1 test intentionally references "existing test engine builder" — the implementer must copy the concrete builder from the surrounding `cairn-service` tests (cross-crate test stubs aren't shared). Flagged inline as an implementer note, not a silent TODO. All code steps carry real code.

**Type consistency:** `process_content(&mut self, path: &str, content: &str, callbacks: &mut dyn PluginCallbacks) -> Result<String, PortError>` is identical in Task 2 (trait default), Task 3 (impl), Task 5 (call). `ProcessContentParams { path, content }` / `ProcessContentResult { content }` / `ProcessorDecl { extensions }` consistent across Tasks 1, 3, 4, 7. `dispatch_query_mut` signature matches between Task 6 definition and daemon call.

**Known deviation from the spec's D7 wording:** the spec said "widen query dispatch to `&mut Engine`." During planning this proved to cascade `&mut` into ~30 read-only call sites (CLI, `gather_answer_context` — which is deliberately `&Engine`). The plan keeps `dispatch_query(&Engine)` pristine and adds a thin `dispatch_query_mut` wrapper instead. The wire contract (RenderNote is a `Query`) is unchanged. The spec's D7 has been updated to match this refinement.
