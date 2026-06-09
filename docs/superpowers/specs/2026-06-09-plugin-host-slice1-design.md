# Plugin Host — Slice 1: Out-of-Process Command Host (Walking Skeleton)

**Date:** 2026-06-09
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main`.

---

## 1. Goal

Prove the out-of-process plugin architecture end-to-end with the thinnest useful slice: the
engine **discovers** a plugin manifest in a cairn, **spawns** the plugin process,
**handshakes** over JSON-RPC/stdio, learns the plugin's **commands**, and can **invoke** a
command (args → result). Surfaced through the contract so a UI/daemon/CLI can drive it.

This is the cairn-native walking skeleton (manifest → spawn → RPC → invoke), with everything
heavier (SDK, capabilities enforcement, sandbox, host-callbacks, events, content processors,
git distribution) as **proven seams deferred to later slices**.

---

## 2. Decisions (locked during brainstorming)

1. **Own the protocol; no tau dependency.** A standalone `cairn-plugin-protocol` —
   **JSON-RPC 2.0 over NDJSON** (one message per line on stdio, MCP-style). Industry-aligned
   (LSP/MCP), debuggable, future-proof for the agent-tool role; avoids coupling cairn's build
   to tau's in-flight plugin crates.
2. **`Box<dyn PluginHost>` injected**, not a 4th `Engine` generic — keeps `Engine::new`'s
   signature unchanged (no ripple across every `Engine<…>`/`CairnEngine` site).
3. **`serde_json::Value` for plugin args/result** in the port + contract (ts-rs → `any`) —
   honest for arbitrary plugin payloads; adds `serde_json` to `cairn-ports`.

---

## 3. The protocol (`cairn-plugin-protocol`, new lib crate)

JSON-RPC 2.0 framed as **NDJSON**: each message is one JSON object on its own line
(`\n`-terminated) on the plugin's stdin (host→plugin) and stdout (plugin→host). Deps:
`serde`, `serde_json`.

### 3.1 Wire types
```rust
#[derive(Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,       // always "2.0"
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,       // "2.0"
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}
```

### 3.2 Method shapes (the two slice-1 methods, host→plugin)
```rust
// method "initialize"
#[derive(Serialize, Deserialize)]
pub struct InitializeParams { pub host_version: String }
#[derive(Serialize, Deserialize)]
pub struct InitializeResult {
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandDecl>,
}
#[derive(Serialize, Deserialize)]
pub struct CommandDecl { pub id: String, pub title: String }

// method "invokeCommand"
#[derive(Serialize, Deserialize)]
pub struct InvokeParams { pub command: String, pub args: serde_json::Value }
// the invoke result is an arbitrary `serde_json::Value` (the command's output)
```
Constants: `pub const JSONRPC_VERSION: &str = "2.0";`, method names
`pub const METHOD_INITIALIZE: &str = "initialize";`, `METHOD_INVOKE: &str = "invokeCommand";`.

### 3.3 Framing
```rust
/// Write one NDJSON message (a serializable value) + newline to `w`.
pub fn write_message<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()>;
/// Read one NDJSON line and deserialize into `T`. Ok(None) on clean EOF.
pub fn read_message<R: BufRead, T: DeserializeOwned>(r: &mut R) -> std::io::Result<Option<T>>;
```
(Blocking; one message per `read_line`. Malformed JSON → an `io::Error`.) **Invariant:** a
plugin writes *only* protocol messages to stdout; all diagnostics go to stderr (which the
host inherits). A non-protocol line on stdout makes the host's `read_message` fail the
current invoke with an `Adapter` error.

### 3.4 Manifest type (parsed by the host)
```rust
#[derive(Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub engine: EngineSection,
}
#[derive(Deserialize)]
pub struct EngineSection {
    /// Executable to spawn (absolute, or relative to the plugin dir).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Declared capabilities — recorded only this slice (NOT enforced).
    #[serde(default)]
    pub capabilities: Vec<String>,
}
```
The protocol crate defines `Manifest` (serde only); the host adapter does the TOML parsing
(keeps `toml` out of the protocol crate and out of plugins).

---

## 4. The port (`cairn-ports`)

Add `serde_json` as a dependency. Define:
```rust
/// A loaded plugin and the commands it declared at handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub commands: Vec<PluginCommand>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand { pub id: String, pub title: String }

/// Hosts out-of-process plugins. Seam: `NoopPluginHost`.
pub trait PluginHost: Send {
    /// The loaded plugins and their commands.
    fn plugins(&self) -> Vec<PluginInfo>;
    /// Invoke `command` on `plugin` with JSON `args`, returning its JSON result.
    ///
    /// # Errors
    /// `NotFound` if the plugin/command is unknown; `Adapter` on a transport or
    /// plugin-reported error.
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PortError>;
}

/// No-plugins seam (the engine's default).
pub struct NoopPluginHost;
impl PluginHost for NoopPluginHost {
    fn plugins(&self) -> Vec<PluginInfo> { Vec::new() }
    fn invoke(&mut self, plugin: &str, _c: &str, _a: &serde_json::Value)
        -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
}
```

---

## 5. The adapter (`cairn-infra` — `ProcessPluginHost`)

Deps added to `cairn-infra`: `cairn-plugin-protocol` (path), `serde_json`, `toml`.

- `ProcessPluginHost::load(dir: &Path) -> Result<Self, PortError>`:
  - For each `<dir>/<id>/manifest.toml`: read + `toml::from_str::<Manifest>`; resolve
    `engine.command` (relative → joined to the plugin's dir); spawn it with
    `std::process::Command` (piped stdin + stdout, inherited stderr); send an `initialize`
    request (`host_version` = crate version) and read the `InitializeResult`; store a
    `LoadedPlugin { child, stdin: ChildStdin, stdout: BufReader<ChildStdout>, info: PluginInfo, next_id }`.
  - A missing/empty plugins dir → an empty host (`Ok` with no plugins).
  - A plugin that fails to spawn/handshake → skip it with a logged warning (don't fail the
    whole load); record nothing for it. (Robust startup.)
- `plugins()` → clone the stored `PluginInfo`s.
- `invoke(plugin, command, args)`: find the `LoadedPlugin` (else `NotFound`); confirm the
  command exists in its declared list (else `NotFound`); write an `invokeCommand` request
  (incrementing `next_id`), read the response; `result` → `Ok(value)`, `error` →
  `Err(Adapter(msg))`, EOF/transport failure → `Err(Adapter(...))`.
- `Drop`: kill each child.
- Synchronous request/response, one in-flight per plugin (sufficient for the skeleton; the
  daemon already runs commands on `spawn_blocking`).

`ProcessPluginHost: Send` (it holds `Child` + pipes + `Vec`, all `Send`).

---

## 6. Engine wiring (`cairn-app`)

- `Engine` gains a field `plugins: Box<dyn PluginHost>`, defaulted in `Engine::new` to
  `Box::new(cairn_ports::NoopPluginHost)` — **`Engine::new`'s signature is unchanged**.
- Methods:
  ```rust
  /// Replace the plugin host (composition root injects the real one).
  pub fn set_plugin_host(&mut self, host: Box<dyn PluginHost>) { self.plugins = host; }

  /// Loaded plugins and their commands.
  pub fn list_plugins(&self) -> Vec<PluginInfo> { self.plugins.plugins() }

  /// Invoke a plugin command.
  /// # Errors
  /// Propagates [`PortError`] from the host.
  pub fn invoke_plugin_command(
      &mut self, plugin: &str, command: &str, args: &serde_json::Value,
  ) -> Result<serde_json::Value, PortError> {
      self.plugins.invoke(plugin, command, args)
  }
  ```
- `Engine: Send` still holds (`Box<dyn PluginHost>` is `Send` via the trait's `: Send` bound).
- The composition root (daemon/Tauri) opts in: `engine.set_plugin_host(Box::new(
  ProcessPluginHost::load(&cairn.join(".cairn").join("plugins"))?))`. The CLI/tests stay on
  `NoopPluginHost`. (Wiring the daemon to load plugins on startup is included; the CLI is
  not — plugins target the long-running hosts.)

---

## 7. Contract (`cairn-contract`)

Enable ts-rs's `serde-json-impl` feature (maps `serde_json::Value` → TS `any`); add
`serde_json` as a normal dependency.

- `Query` gains `ListPlugins` (no fields) → new `QueryResponse::Plugins { plugins: Vec<PluginSummary> }`:
  ```rust
  pub struct PluginSummary {
      pub id: String, pub name: String, pub version: String,
      pub commands: Vec<PluginCommandSummary>,
  }
  pub struct PluginCommandSummary { pub id: String, pub title: String }
  ```
- `Command` gains:
  ```rust
  InvokePluginCommand { plugin: String, command: String, args: serde_json::Value }
  ```
  → new `CommandResponse::PluginResult { result: serde_json::Value }`.
- `CommandResponse` currently derives `Eq` — **drop it** (the new `PluginResult` variant holds
  `serde_json::Value`, which is not `Eq`); keep `PartialEq`. (`QueryResponse` already lacks
  `Eq` since the `SearchResults` variant.) `PluginSummary`/`PluginCommandSummary` are
  string-only and derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS`. Regenerate
  the TS bindings.

---

## 8. Dispatcher (`cairn-service`)

- `dispatch_query` `Query::ListPlugins`: `engine.list_plugins()` → map each `PluginInfo` to
  `PluginSummary` → `QueryResponse::Plugins`.
- `dispatch_command` `Command::InvokePluginCommand { plugin, command, args }`:
  `engine.invoke_plugin_command(&plugin, &command, &args)?` → `CommandResponse::PluginResult
  { result }`. (`PortError::NotFound` → `ServiceError::NotFound` → 404; `Adapter` →
  `Internal` → 500, via the existing `From<PortError>`.)

---

## 9. Example plugin + end-to-end test (`cairn-plugin-example`, new bin crate)

- A small binary (`crates/cairn-plugin-example`) depending on `cairn-plugin-protocol`: it
  loops reading `Request`s from stdin; on `initialize` it returns
  `InitializeResult { name: "example", version, commands: [{id:"echo", title:"Echo"}] }`; on
  `invokeCommand` with `command=="echo"` it returns the `args` value unchanged; unknown
  command → a JSON-RPC `error`. It **exits cleanly on stdin EOF** (so when the host process
  dies, the plugin's stdin closes and it terminates — no orphans), and writes nothing but
  protocol messages to stdout. Demonstrates the intended (pre-SDK) authoring shape.
- The end-to-end test lives in `crates/cairn-plugin-example/tests/host.rs` (dev-deps:
  `cairn-infra`, `cairn-ports`, `tempfile`): it gets its own bin path via
  `env!("CARGO_BIN_EXE_cairn-plugin-example")`, writes a temp cairn with
  `.cairn/plugins/example/manifest.toml` (`command` = that path), `ProcessPluginHost::load`s
  it, asserts `plugins()` lists the `echo` command, `invoke("example","echo", json!({"x":1}))`
  returns `{"x":1}`, and `invoke` of an unknown plugin/command returns `NotFound`.

---

## 10. Testing

- **protocol:** `write_message`→`read_message` round-trip for `Request`/`Response`; an
  `initialize`/`invokeCommand` params/result round-trip; `read_message` returns `Ok(None)`
  at EOF and an error on malformed JSON.
- **infra:** `Manifest` parses from a TOML sample; `ProcessPluginHost::load` of an empty/absent
  dir yields no plugins; a manifest pointing at a non-spawnable command is skipped (load still
  `Ok`).
- **end-to-end:** §9 (spawn the example plugin, handshake, list, invoke echo, unknown →
  NotFound).
- **contract:** `Command::InvokePluginCommand` + `QueryResponse::Plugins` serde round-trip
  (tags `invoke_plugin_command` / `plugins`) + the bindings export `PluginSummary` and a
  `plugins`/`plugin_result` arm.
- **service:** dispatch `ListPlugins` (Noop host → empty) and `InvokePluginCommand` against a
  Noop host → `NotFound` mapping.

---

## 11. Docs

- **ADR-0008** (`docs/decisions/0008-plugin-host.md`): out-of-process plugins over
  JSON-RPC/NDJSON; why own-it-not-tau and JSON-RPC-not-MessagePack; `Box<dyn PluginHost>`
  injection; the slice roadmap; capabilities declared-not-enforced; sandbox/SDK/host-callbacks
  deferred.
- **Handoff update:** a short "Plugins (engine, slice 1)" note — manifests under
  `<cairn>/.cairn/plugins/<id>/`, `list_plugins` / `invoke_plugin_command` over the contract,
  and that host-callbacks/capabilities/sandbox are later slices.

---

## 12. Out of scope (→ later slices)

Plugin SDK (slice 2); host callbacks — plugins reading/writing the cairn — and **capability
enforcement** (slice 3; this slice records declared capabilities but enforces nothing because
plugins can't call back yet); vault events (slice 4); content processors / port backends
(slice 5); OS sandbox (slice 6); git-URL distribution / build-from-source (slice 7); UI
plugins (UI session). Concurrency beyond one-in-flight-request-per-plugin; plugin hot-reload;
dependency ordering between plugins.
