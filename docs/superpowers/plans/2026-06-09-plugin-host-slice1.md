# Plugin Host Slice 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An out-of-process plugin command host: the engine discovers a plugin manifest, spawns the process, handshakes over JSON-RPC/stdio, and lists + invokes its commands — surfaced through the contract.

**Architecture:** A new `cairn-plugin-protocol` crate (JSON-RPC 2.0 over NDJSON) used by a `ProcessPluginHost` adapter (`cairn-infra`) behind a `PluginHost` port (`cairn-ports`, default `NoopPluginHost`). `Engine` holds `Box<dyn PluginHost>` (no generic ripple); the contract gains `ListPlugins`/`InvokePluginCommand`. An example plugin bin crate proves it end-to-end.

**Tech Stack:** Rust, `serde`/`serde_json`, JSON-RPC over NDJSON on stdio, `std::process`, `toml` (manifest), ts-rs 11 `serde-json-impl`.

**Branch:** `feat/plugin-host` (the spec is committed there).

**Spec:** `docs/superpowers/specs/2026-06-09-plugin-host-slice1-design.md`.

**Verified:** ts-rs 11 + `serde-json-impl` maps `serde_json::Value` → a generated `JsonValue` TS type (emits `bindings/serde_json/JsonValue.ts`).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/cairn-plugin-protocol/` (new) | JSON-RPC/NDJSON wire types, framing, method shapes, `Manifest` | 1 |
| `Cargo.toml` (workspace `members`) | register the two new crates | 1, 7 |
| `crates/cairn-ports/src/lib.rs` (+ `Cargo.toml`) | `PluginHost` trait, `PluginInfo`/`PluginCommand`, `NoopPluginHost` | 2 |
| `crates/cairn-app/src/lib.rs` | `Engine.plugins` + `set_plugin_host`/`list_plugins`/`invoke_plugin_command` | 3 |
| `crates/cairn-infra/src/plugin_host.rs` (+ `lib.rs`, `Cargo.toml`) | `ProcessPluginHost` adapter | 4 |
| `crates/cairn-contract/src/lib.rs` (+ `Cargo.toml`, bindings) | `ListPlugins`/`InvokePluginCommand` + DTOs | 5 |
| `crates/cairn-service/src/lib.rs` | dispatch arms | 6 |
| `crates/cairn-plugin-example/` (new) | example plugin bin + end-to-end test | 7 |
| `crates/cairn-daemon/src/main.rs` | load plugins on startup | 7 |
| `docs/decisions/0008-plugin-host.md`, handoff | ADR + docs | 8 |

Each task ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`. **Cargo.lock:** Tasks adding deps (1,2,5) must commit the updated `Cargo.lock` (CI runs `--locked`).

**Commit convention:** use `git -c commit.gpgsign=false commit` if signing fails.

---

### Task 1: `cairn-plugin-protocol` crate

**Files:**
- Create: `crates/cairn-plugin-protocol/Cargo.toml`, `crates/cairn-plugin-protocol/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create the crate + register it**

`crates/cairn-plugin-protocol/Cargo.toml`:
```toml
[package]
name = "cairn-plugin-protocol"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }

[lints]
workspace = true
```
In the root `Cargo.toml`, add `"crates/cairn-plugin-protocol",` to `[workspace] members`.

- [ ] **Step 2: Write the crate with tests**

`crates/cairn-plugin-protocol/src/lib.rs`:
```rust
//! Wire-format types and NDJSON framing for the cairn plugin protocol
//! (JSON-RPC 2.0 over stdio, MCP-style). No transport or process logic here.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)

use std::io::{BufRead, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const JSONRPC_VERSION: &str = "2.0";
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_INVOKE: &str = "invokeCommand";

/// A JSON-RPC request (host -> plugin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A JSON-RPC response (plugin -> host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// Params of the `initialize` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub host_version: String,
}

/// Result of `initialize`: the plugin's identity + declared commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandDecl>,
}

/// A command the plugin declares it can handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title: String,
}

/// Params of the `invokeCommand` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeParams {
    pub command: String,
    pub args: serde_json::Value,
}

/// A plugin manifest (`<cairn>/.cairn/plugins/<id>/manifest.toml`). Parsed by
/// the host; this crate only defines the shape (no `toml` dependency).
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub engine: EngineSection,
}

/// The `[engine]` section of a manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineSection {
    /// Executable to spawn (absolute, or relative to the plugin's directory).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Declared capabilities — recorded only (not enforced in slice 1).
    #[serde(default)]
    pub capabilities: Vec<String>,
}

fn invalid_data(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Write one NDJSON message (a serializable value) + `\n` to `w`, flushing.
///
/// # Errors
/// IO or serialization failure.
pub fn write_message<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    serde_json::to_writer(&mut *w, msg).map_err(invalid_data)?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Read one NDJSON message from `r` (skipping blank lines), deserializing into
/// `T`. `Ok(None)` on clean EOF.
///
/// # Errors
/// IO failure, or malformed JSON (`InvalidData`).
pub fn read_message<R: BufRead, T: DeserializeOwned>(r: &mut R) -> std::io::Result<Option<T>> {
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return serde_json::from_str(trimmed)
            .map(Some)
            .map_err(invalid_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_response_roundtrip_over_ndjson() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.into(),
            id: 1,
            method: METHOD_INVOKE.into(),
            params: serde_json::json!({"command": "echo", "args": {"x": 1}}),
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &req).unwrap();
        assert!(buf.ends_with(b"\n"));
        let mut r = std::io::Cursor::new(buf);
        let got: Request = read_message(&mut r).unwrap().unwrap();
        assert_eq!(got.id, 1);
        assert_eq!(got.method, METHOD_INVOKE);
        assert_eq!(got.params["command"], "echo");
    }

    #[test]
    fn initialize_result_roundtrips() {
        let init = InitializeResult {
            name: "example".into(),
            version: "0.1.0".into(),
            commands: vec![CommandDecl { id: "echo".into(), title: "Echo".into() }],
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &init).unwrap();
        let mut r = std::io::Cursor::new(buf);
        let got: InitializeResult = read_message(&mut r).unwrap().unwrap();
        assert_eq!(got, init);
    }

    #[test]
    fn eof_is_none_blank_skipped_malformed_errors() {
        // EOF
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(read_message::<_, Request>(&mut empty).unwrap().is_none());
        // blank lines then EOF
        let mut blanks = std::io::Cursor::new(b"\n  \n".to_vec());
        assert!(read_message::<_, Request>(&mut blanks).unwrap().is_none());
        // malformed
        let mut bad = std::io::Cursor::new(b"{not json\n".to_vec());
        assert!(read_message::<_, Request>(&mut bad).is_err());
    }

    #[test]
    fn manifest_parses_from_toml() {
        let m: Manifest = toml::from_str(
            "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\n[engine]\ncommand = \"./x\"\n",
        )
        .unwrap();
        assert_eq!(m.id, "x");
        assert_eq!(m.engine.command, "./x");
        assert!(m.engine.args.is_empty());
        assert!(m.engine.capabilities.is_empty());
    }
}
```
The `manifest_parses_from_toml` test needs `toml` as a dev-dependency. Add to
`crates/cairn-plugin-protocol/Cargo.toml`:
```toml
[dev-dependencies]
toml = { workspace = true }
```

- [ ] **Step 3: Run + gate + commit**

- `cargo test -p cairn-plugin-protocol` → 4 tests pass.
- `cargo build --workspace` (refresh `Cargo.lock` for the new crate).
- `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → clean.
```bash
git add crates/cairn-plugin-protocol Cargo.toml Cargo.lock
git commit -m "feat(plugin-protocol): JSON-RPC/NDJSON wire types + framing + manifest"
```

---

### Task 2: `PluginHost` port + `NoopPluginHost`

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`, `crates/cairn-ports/Cargo.toml`

- [ ] **Step 1: Add serde_json to cairn-ports**

In `crates/cairn-ports/Cargo.toml` `[dependencies]`, add:
```toml
serde_json = { workspace = true }
```

- [ ] **Step 2: Add the port + seam**

In `crates/cairn-ports/src/lib.rs`, add (near the other ports/traits):
```rust
/// A loaded plugin and the commands it declared at handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Manifest id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Commands the plugin handles.
    pub commands: Vec<PluginCommand>,
}

/// A command a plugin can handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    /// Command id (used to invoke).
    pub id: String,
    /// Human title.
    pub title: String,
}

/// Hosts out-of-process plugins. Seam: [`NoopPluginHost`].
pub trait PluginHost: Send {
    /// The loaded plugins and their declared commands.
    fn plugins(&self) -> Vec<PluginInfo>;

    /// Invoke `command` on `plugin` with JSON `args`, returning its JSON result.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the plugin/command is unknown; [`PortError::Adapter`]
    /// on a transport or plugin-reported error.
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PortError>;
}

/// No-plugins seam — the engine's default host.
#[derive(Debug, Default)]
pub struct NoopPluginHost;

impl PluginHost for NoopPluginHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        Vec::new()
    }
    fn invoke(
        &mut self,
        plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
}
```

- [ ] **Step 3: Run + gate + commit**

- `cargo build -p cairn-ports && cargo test -p cairn-ports`.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-ports/Cargo.toml Cargo.lock
git commit -m "feat(ports): PluginHost trait + PluginInfo + NoopPluginHost seam"
```

---

### Task 3: Engine plugin methods

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/cairn-app/src/lib.rs` `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn default_plugin_host_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        assert!(eng.list_plugins().is_empty());
        let err = eng
            .invoke_plugin_command("nope", "x", &serde_json::Value::Null)
            .unwrap_err();
        assert!(matches!(err, PortError::NotFound(_)));
    }
```
Run `cargo test -p cairn-app default_plugin_host_is_noop` → FAILS (no `list_plugins`).

- [ ] **Step 2: Add the import, field, default, methods**

In `crates/cairn-app/src/lib.rs`:
- Add `PluginHost`, `PluginInfo`, `NoopPluginHost` to the `cairn_ports` import.
- Add the field to `Engine` (after `notes_cache`):
```rust
    plugins: Box<dyn PluginHost>,
```
- In `Engine::new`, after `notes_cache: RefCell::new(None),` add:
```rust
            plugins: Box::new(NoopPluginHost),
```
- Add methods inside the `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` block:
```rust
    /// Replace the plugin host (the composition root injects the real one).
    pub fn set_plugin_host(&mut self, host: Box<dyn PluginHost>) {
        self.plugins = host;
    }

    /// Loaded plugins and their declared commands.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.plugins()
    }

    /// Invoke a plugin command.
    ///
    /// # Errors
    /// Propagates [`PortError`] from the plugin host.
    pub fn invoke_plugin_command(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PortError> {
        self.plugins.invoke(plugin, command, args)
    }
```

- [ ] **Step 3: Run + gate + commit**

- `cargo test -p cairn-app default_plugin_host_is_noop` → PASS.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green. (`Engine: Send` still holds — `Box<dyn PluginHost>` is `Send` via the trait bound; the daemon builds.)
```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): Engine plugin host (Box<dyn PluginHost>, default Noop)"
```

---

### Task 4: `ProcessPluginHost` adapter

**Files:**
- Create: `crates/cairn-infra/src/plugin_host.rs`
- Modify: `crates/cairn-infra/src/lib.rs`, `crates/cairn-infra/Cargo.toml`

- [ ] **Step 1: Add deps**

In `crates/cairn-infra/Cargo.toml` `[dependencies]`, add:
```toml
cairn-plugin-protocol = { path = "../cairn-plugin-protocol" }
serde_json = { workspace = true }
toml = { workspace = true }
```

- [ ] **Step 2: Write the adapter**

Create `crates/cairn-infra/src/plugin_host.rs`:
```rust
//! `PluginHost` backed by child processes speaking JSON-RPC/NDJSON over stdio.

use std::io::{BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeParams, InitializeResult, InvokeParams,
    Manifest, Request, Response, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
};
use cairn_ports::{PluginCommand, PluginHost, PluginInfo, PortError};

fn adapt<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Adapter(e.to_string())
}

struct LoadedPlugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    info: PluginInfo,
    next_id: u64,
}

impl LoadedPlugin {
    /// Send one request and read its response.
    fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: self.next_id,
            method: method.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        let resp: Response = read_message(&mut self.stdout)
            .map_err(adapt)?
            .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
        if let Some(err) = resp.error {
            return Err(PortError::Adapter(format!("plugin error: {}", err.message)));
        }
        resp.result.ok_or_else(|| PortError::Adapter("plugin response had no result".into()))
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawns and talks to plugins under a plugins directory.
#[derive(Default)]
pub struct ProcessPluginHost {
    loaded: Vec<LoadedPlugin>,
}

impl ProcessPluginHost {
    /// Load every `<dir>/<id>/manifest.toml`: spawn the binary, handshake, and
    /// keep the process. A missing dir loads nothing; a plugin that fails to
    /// spawn/handshake is skipped (logged), not fatal.
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load(dir: &Path) -> Result<Self, PortError> {
        let mut loaded = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(adapt(e)),
        };
        for entry in entries {
            let plugin_dir = match entry {
                Ok(e) if e.path().is_dir() => e.path(),
                _ => continue,
            };
            match Self::spawn_plugin(&plugin_dir) {
                Ok(p) => loaded.push(p),
                Err(e) => eprintln!("plugin: skipping {}: {e}", plugin_dir.display()),
            }
        }
        Ok(Self { loaded })
    }

    fn spawn_plugin(plugin_dir: &Path) -> Result<LoadedPlugin, PortError> {
        let raw = std::fs::read_to_string(plugin_dir.join("manifest.toml")).map_err(adapt)?;
        let manifest: Manifest = toml::from_str(&raw).map_err(adapt)?;

        let cmd_path = {
            let p = Path::new(&manifest.engine.command);
            if p.is_absolute() { p.to_path_buf() } else { plugin_dir.join(p) }
        };
        let mut child = Command::new(&cmd_path)
            .args(&manifest.engine.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(adapt)?;
        let stdin = child.stdin.take().ok_or_else(|| adapt("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| adapt("no stdout"))?);

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
        };
        let init_params = serde_json::to_value(InitializeParams {
            host_version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .map_err(adapt)?;
        let result = plugin.call(METHOD_INITIALIZE, init_params)?;
        let init: InitializeResult = serde_json::from_value(result).map_err(adapt)?;
        plugin.info.commands = init
            .commands
            .into_iter()
            .map(|CommandDecl { id, title }| PluginCommand { id, title })
            .collect();
        // Prefer the manifest id; trust the handshake name/version/commands.
        plugin.info.name = init.name;
        plugin.info.version = init.version;
        Ok(plugin)
    }
}

impl PluginHost for ProcessPluginHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        self.loaded.iter().map(|p| p.info.clone()).collect()
    }

    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
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
        p.call(METHOD_INVOKE, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let host = ProcessPluginHost::load(&tmp.path().join("missing")).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn unspawnable_plugin_is_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let pdir = tmp.path().join("broken");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("manifest.toml"),
            "id=\"broken\"\nname=\"B\"\nversion=\"0\"\n[engine]\ncommand=\"/nonexistent/xyz\"\n",
        )
        .unwrap();
        // Load succeeds; the broken plugin is skipped.
        let host = ProcessPluginHost::load(tmp.path()).unwrap();
        assert!(host.plugins().is_empty());
    }
}
```

- [ ] **Step 3: Export it**

In `crates/cairn-infra/src/lib.rs`, add:
```rust
mod plugin_host;
pub use plugin_host::ProcessPluginHost;
```

- [ ] **Step 4: Run + gate + commit**

- `cargo test -p cairn-infra plugin_host` → 2 tests pass.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-infra/src/lib.rs crates/cairn-infra/Cargo.toml Cargo.lock
git commit -m "feat(infra): ProcessPluginHost (spawn + JSON-RPC handshake/invoke)"
```

---

### Task 5: Contract

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`, `crates/cairn-contract/Cargo.toml`, `crates/cairn-contract/bindings/`

- [ ] **Step 1: Enable ts-rs serde-json-impl**

In `crates/cairn-contract/Cargo.toml`, change the ts-rs line to:
```toml
ts-rs = { workspace = true, features = ["serde-json-impl"] }
```

- [ ] **Step 2: Add the DTOs + variants**

In `crates/cairn-contract/src/lib.rs`:
- Add the plugin DTOs (near `NoteSummary`):
```rust
/// A loaded plugin and its commands (response to `ListPlugins`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginSummary {
    /// Manifest id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Declared commands.
    pub commands: Vec<PluginCommandSummary>,
}

/// A command a plugin handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginCommandSummary {
    /// Command id.
    pub id: String,
    /// Human title.
    pub title: String,
}
```
- In `Command`, add a variant (after `Commit` or near the others):
```rust
    /// Invoke a command exposed by a loaded plugin.
    InvokePluginCommand {
        /// Plugin id.
        plugin: String,
        /// Command id.
        command: String,
        /// Arbitrary JSON arguments.
        args: serde_json::Value,
    },
```
- In `Query`, add (after `NotesByTag`):
```rust
    /// List loaded plugins and their commands.
    ListPlugins,
```
- In `CommandResponse`: **remove `Eq` from its derive line** (now holds `serde_json::Value`),
  keeping `Debug, Clone, PartialEq, Serialize, Deserialize, TS`. Add the variant:
```rust
    /// Result of a plugin command (arbitrary JSON).
    PluginResult {
        /// The command's JSON output.
        result: serde_json::Value,
    },
```
- In `QueryResponse` (already lacks `Eq`), add the variant:
```rust
    /// Loaded plugins (response to `ListPlugins`).
    Plugins {
        /// One per loaded plugin.
        plugins: Vec<PluginSummary>,
    },
```

- [ ] **Step 3: Add a round-trip test**

In the `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn plugin_command_and_response_roundtrip() {
        let cmd = Command::InvokePluginCommand {
            plugin: "p".into(),
            command: "echo".into(),
            args: serde_json::json!({"x": 1}),
        };
        let j = serde_json::to_string(&cmd).unwrap();
        assert!(j.contains("\"type\":\"invoke_plugin_command\""));
        assert_eq!(serde_json::from_str::<Command>(&j).unwrap(), cmd);

        let resp = QueryResponse::Plugins {
            plugins: vec![PluginSummary {
                id: "p".into(),
                name: "P".into(),
                version: "0.1.0".into(),
                commands: vec![PluginCommandSummary { id: "echo".into(), title: "Echo".into() }],
            }],
        };
        let j = serde_json::to_string(&resp).unwrap();
        assert!(j.contains("\"type\":\"plugins\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), resp);
    }
```

- [ ] **Step 4: Regenerate bindings + verify**

- `cargo test -p cairn-contract` → pass (regenerates TS). Note: `serde_json::Value` emits
  `bindings/serde_json/JsonValue.ts` and references `JsonValue`.
- Verify: `grep -rn "invoke_plugin_command\|plugins\|PluginSummary\|JsonValue" crates/cairn-contract/bindings/Command.ts crates/cairn-contract/bindings/QueryResponse.ts crates/cairn-contract/bindings/CommandResponse.ts` shows the new arms; `crates/cairn-contract/bindings/PluginSummary.ts` and `bindings/serde_json/JsonValue.ts` exist.

- [ ] **Step 5: Gate + commit**

- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/Cargo.toml crates/cairn-contract/bindings/ Cargo.lock
git commit -m "feat(contract): ListPlugins + InvokePluginCommand + plugin DTOs"
```

---

### Task 6: Dispatcher

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`

- [ ] **Step 1: Add dispatch arms**

In `crates/cairn-service/src/lib.rs`, add `PluginSummary` and `PluginCommandSummary` to the
`cairn_contract` import list. Add to `dispatch_query`'s match:
```rust
        Query::ListPlugins => {
            let plugins = engine
                .list_plugins()
                .into_iter()
                .map(|p| PluginSummary {
                    id: p.id,
                    name: p.name,
                    version: p.version,
                    commands: p
                        .commands
                        .into_iter()
                        .map(|c| PluginCommandSummary { id: c.id, title: c.title })
                        .collect(),
                })
                .collect();
            Ok(QueryResponse::Plugins { plugins })
        }
```
Add to `dispatch_command`'s match:
```rust
        Command::InvokePluginCommand { plugin, command, args } => {
            let result = engine.invoke_plugin_command(plugin, command, args)?;
            Ok(CommandResponse::PluginResult { result })
        }
```

- [ ] **Step 2: Add service tests**

In the `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn list_plugins_empty_and_invoke_unknown_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        match dispatch_query(&eng, &Query::ListPlugins).unwrap() {
            QueryResponse::Plugins { plugins } => assert!(plugins.is_empty()),
            other => panic!("expected Plugins, got {other:?}"),
        }
        let mut sink: Vec<AppEvent> = Vec::new();
        let err = dispatch_command(
            &mut eng,
            &Command::InvokePluginCommand {
                plugin: "nope".into(),
                command: "x".into(),
                args: serde_json::Value::Null,
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }
```

- [ ] **Step 3: Run + gate + commit**

- `cargo test -p cairn-service` → pass.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): dispatch ListPlugins + InvokePluginCommand"
```

---

### Task 7: Example plugin + end-to-end test + daemon wiring

**Files:**
- Create: `crates/cairn-plugin-example/Cargo.toml`, `crates/cairn-plugin-example/src/main.rs`, `crates/cairn-plugin-example/tests/host.rs`
- Modify: `Cargo.toml` (workspace members), `crates/cairn-daemon/src/main.rs`

- [ ] **Step 1: Create the example plugin crate**

`crates/cairn-plugin-example/Cargo.toml`:
```toml
[package]
name = "cairn-plugin-example"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-plugin-protocol = { path = "../cairn-plugin-protocol" }
serde_json = { workspace = true }

[dev-dependencies]
cairn-infra = { path = "../cairn-infra" }
cairn-ports = { path = "../cairn-ports" }
tempfile = { workspace = true }

[lints]
workspace = true
```
Register `"crates/cairn-plugin-example",` in the root `Cargo.toml` `members`.

`crates/cairn-plugin-example/src/main.rs`:
```rust
//! A minimal example cairn plugin: registers an `echo` command that returns its
//! args unchanged. Speaks JSON-RPC/NDJSON over stdio; exits on stdin EOF.

use std::io::{self, BufReader};

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, Request, Response,
    RpcError, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
};

fn main() {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();

    while let Some(req) = read_message::<_, Request>(&mut reader).unwrap_or(None) {
        let resp = handle(&req);
        if write_message(&mut stdout, &resp).is_err() {
            break;
        }
    }
}

fn handle(req: &Request) -> Response {
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
                commands: vec![CommandDecl { id: "echo".to_string(), title: "Echo".to_string() }],
            };
            resp.result = Some(serde_json::to_value(init).unwrap());
        }
        METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
            Ok(p) if p.command == "echo" => resp.result = Some(p.args),
            Ok(p) => {
                resp.error = Some(RpcError { code: -32601, message: format!("unknown command {}", p.command) });
            }
            Err(e) => resp.error = Some(RpcError { code: -32602, message: e.to_string() }),
        },
        other => {
            resp.error = Some(RpcError { code: -32601, message: format!("unknown method {other}") });
        }
    }
    resp
}
```

- [ ] **Step 2: Write the end-to-end test**

`crates/cairn-plugin-example/tests/host.rs`:
```rust
use cairn_infra::ProcessPluginHost;
use cairn_ports::{PluginHost, PortError};

#[test]
fn host_loads_invokes_and_rejects_unknown() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!("id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n[engine]\ncommand=\"{bin}\"\n"),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();

    // Handshake surfaced the echo command.
    let plugins = host.plugins();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "example");
    assert!(plugins[0].commands.iter().any(|c| c.id == "echo"));

    // Invoke echo -> args returned unchanged.
    let out = host
        .invoke("example", "echo", &serde_json::json!({"x": 1, "y": "z"}))
        .unwrap();
    assert_eq!(out, serde_json::json!({"x": 1, "y": "z"}));

    // Unknown plugin / command -> NotFound.
    assert!(matches!(
        host.invoke("missing", "echo", &serde_json::Value::Null),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        host.invoke("example", "nope", &serde_json::Value::Null),
        Err(PortError::NotFound(_))
    ));
}
```

- [ ] **Step 3: Wire the daemon to load plugins on startup**

In `crates/cairn-daemon/src/main.rs`, after the engine is built + reconciled/reindexed and
BEFORE `let state = AppState::new(engine);`, add (the engine binding is `let engine` from the
if/else; make it `let mut engine` if needed):
```rust
    // Load engine plugins from <cairn>/.cairn/plugins (absent dir => none).
    match cairn_infra::ProcessPluginHost::load(&cli.cairn.join(".cairn").join("plugins")) {
        Ok(host) => engine.set_plugin_host(Box::new(host)),
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }
```
(If the `if persist { … } else { … }` binds `let engine`, change it to `let mut engine` so
`set_plugin_host` can run; the value is still moved into `AppState::new`.)

- [ ] **Step 4: Run + gate + commit**

- `cargo test -p cairn-plugin-example` → the e2e test passes (it builds + spawns the bin).
- `cargo test -p cairn-daemon` → still green.
- `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → green.
```bash
git add crates/cairn-plugin-example crates/cairn-daemon/src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: example plugin + e2e host test + daemon plugin loading"
```

---

### Task 8: ADR-0008 + handoff + final gate

**Files:**
- Create: `docs/decisions/0008-plugin-host.md`
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Write ADR-0008**

Read `docs/decisions/0007-note-cache.md` for the format, then create
`docs/decisions/0008-plugin-host.md` matching it, with:
- **Context:** the §7 plugin vision; need a proven out-of-process walking skeleton.
- **Decision:** own `cairn-plugin-protocol` (JSON-RPC 2.0 over NDJSON, MCP-style), no tau
  dependency, JSON-RPC not MessagePack (debuggable, standards-aligned); `ProcessPluginHost`
  behind a `PluginHost` port (default `NoopPluginHost`) injected via `Engine::set_plugin_host`
  (`Box<dyn>`, no generic ripple); `Query::ListPlugins` / `Command::InvokePluginCommand`.
  Capabilities are declared-only (not enforced) this slice.
- **Consequences:** slice-1 mechanics proven (manifest → spawn → handshake → invoke);
  daemon loads `<cairn>/.cairn/plugins/`; plugins exit on stdin EOF; the slice roadmap (SDK,
  host-callbacks + capability enforcement, vault events, content processors, sandbox,
  git distribution) is deferred.

- [ ] **Step 2: Update the handoff**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, add a capability note:
```
- **Plugins (engine, slice 1):** out-of-process plugins under
  `<cairn>/.cairn/plugins/<id>/manifest.toml` (JSON-RPC/stdio). Drive them via the contract:
  `{ type: "list_plugins" }` -> `{ type: "plugins", plugins: PluginSummary[] }`, and
  `{ type: "invoke_plugin_command", plugin, command, args }` -> `{ type: "plugin_result", result }`.
  Host-callbacks, capability enforcement, and sandbox are later slices.
```

- [ ] **Step 3: Final gate**

Run:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check licenses bans sources
```
Expected: all green (no new external deps beyond what's in the tree; `deny` ok). Do NOT run
plain `cargo deny check` (advisories sub-check can crash on old local cargo-deny).

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0008-plugin-host.md docs/handoffs/2026-06-01-ui-session-handoff.md
git commit -m "docs: ADR-0008 plugin host + handoff update"
```

---

## Notes for the implementer

- **Cargo.lock** must be committed in Tasks 1, 2, 4, 5, 7 (new crates/deps) — CI runs `--locked`.
- **The example plugin's e2e test lives in its own crate** so `env!("CARGO_BIN_EXE_cairn-plugin-example")` resolves to the built bin; it dev-depends on `cairn-infra` for `ProcessPluginHost`.
- **Synchronous request/response, one-in-flight-per-plugin** — no async, no concurrency in slice 1. The daemon already runs commands on `spawn_blocking`.
- **Plugins write only protocol JSON to stdout; diagnostics to stderr** (inherited by the host). A stray stdout line fails the current invoke with an `Adapter` error.
- **`Engine: Send`** must hold (daemon `Arc<Mutex<Engine>>`): `PluginHost: Send`, so `Box<dyn PluginHost>` is `Send`. The workspace compiling (daemon builds) confirms it.
- **Capabilities are parsed but NOT enforced** this slice — that's slice 3 (with host-callbacks).
