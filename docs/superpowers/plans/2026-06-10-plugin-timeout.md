# Plugin Host Read Timeout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A plugin that stops responding is killed after a per-message timeout (default 30s) instead of blocking the host's read loop forever.

**Architecture:** Each plugin gets a background reader thread that owns its stdout and forwards NDJSON lines down an `mpsc` channel; the dispatch loop uses `recv_timeout` (std blocking pipe reads can't be interrupted, so a reader thread is the only portable fix). On timeout the host kills the child and returns an error. Entirely internal to `cairn-infra/src/plugin_host.rs`; the only public API addition is `load_with_timeout`.

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), std `mpsc`/threads, JSON-RPC over NDJSON/stdio, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-timeout-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-plugin-example/src/main.rs` | a `hang` command (test fixture: blocks forever) | 1 |
| `crates/cairn-infra/src/plugin_host.rs` | reader thread, `recv_message`, `timeout`, `load_with_timeout`, `Drop` join | 2 |
| `crates/cairn-plugin-example/tests/host.rs` | timeout e2e test | 2 |

**Unchanged:** `cairn-plugin-protocol` (still exports `read_message` for the SDK/example), `cairn-ports`, `cairn-app`, `cairn-sdk`, `cairn-service`, `cairn-cli`, the daemon (all use `ProcessPluginHost::load`, unchanged).

---

## Task 1: Add a `hang` command to the example plugin

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`

A test fixture: a command whose handler never returns, so the host has something to time out against. (The existing tests don't invoke it, so they stay green.)

- [ ] **Step 1: Add the command**

In `crates/cairn-plugin-example/src/main.rs`, add this registration just before `plugin.run();` (after the existing commands and the `on_event` handler):

```rust
    // Test fixture: never responds, so the host's read timeout can fire.
    plugin.command("hang", "Hang", |_args: Value, _host: &mut Host| {
        std::thread::sleep(std::time::Duration::from_secs(86_400));
        Ok(json!(null))
    });
```

- [ ] **Step 2: Build the example**

Run: `cargo build -p cairn-plugin-example`
Expected: compiles.

- [ ] **Step 3: Verify existing tests still pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — all existing host tests still green (`hang` is declared but never invoked).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-plugin-example/src/main.rs
git commit -m "test(example): add a hang command (read-timeout fixture)"
```

---

## Task 2: Reader-thread read timeout in the host

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Write the failing e2e test**

In `crates/cairn-plugin-example/tests/host.rs`, add at the end (the `write_manifest` helper + `MapCallbacks` already exist):

```rust
#[test]
fn invoke_times_out_and_kills_plugin() {
    use std::time::{Duration, Instant};
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no caps needed; `hang` makes no callbacks
    let mut host = ProcessPluginHost::load_with_timeout(
        &tmp.path().join(".cairn").join("plugins"),
        Duration::from_millis(200),
    )
    .unwrap();
    let mut cb = MapCallbacks(HashMap::new());

    let start = Instant::now();
    let err = host
        .invoke("example", "hang", &serde_json::Value::Null, &mut cb)
        .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(5), "hang should time out quickly");
    assert!(
        matches!(&err, PortError::Adapter(m) if m.contains("timed out")),
        "expected a timeout Adapter, got {err:?}"
    );

    // The plugin was killed, so a follow-up invoke fails fast (no re-hang).
    let err2 = host
        .invoke("example", "echo", &serde_json::json!({"x": 1}), &mut cb)
        .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(5), "follow-up should not hang");
    assert!(matches!(err2, PortError::Adapter(_)), "expected Adapter, got {err2:?}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-plugin-example --test host invoke_times_out`
Expected: COMPILE failure — `ProcessPluginHost::load_with_timeout` doesn't exist.

- [ ] **Step 3: Add imports + the timeout constant**

In `crates/cairn-infra/src/plugin_host.rs`, change the std imports at the top (currently `use std::io::BufReader;`, `use std::path::Path;`, `use std::process::{...};`) so `BufRead` is in scope (needed for `read_line`) and add `mpsc`/threads/`Duration`:

```rust
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;
```

After the `adapt` helper (near the top), add the default timeout constant:

```rust
/// Default per-message timeout for plugin reads: a plugin silent longer than this
/// is treated as hung and killed.
pub const DEFAULT_PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);
```

- [ ] **Step 4: Rework `LoadedPlugin`'s read side**

Change the `LoadedPlugin` struct (currently has `child, stdin, stdout: BufReader<ChildStdout>, info, next_id, capabilities`) to replace `stdout` with the channel + reader handle + timeout:

```rust
struct LoadedPlugin {
    child: Child,
    stdin: ChildStdin,
    /// Lines from the plugin's stdout, fed by a background reader thread so reads
    /// can be bounded by `timeout` (std pipe reads can't be interrupted directly).
    rx: Receiver<std::io::Result<String>>,
    reader: Option<JoinHandle<()>>,
    timeout: Duration,
    info: PluginInfo,
    next_id: u64,
    /// Capabilities the manifest declared; gates host-callbacks.
    capabilities: Vec<String>,
}
```

Add a `recv_message` method to `impl LoadedPlugin` (place it just above `fn call`):

```rust
    /// Receive + parse the next message, killing the plugin if it stalls past the
    /// timeout. `Ok(None)` on a clean EOF (the reader thread ended).
    fn recv_message<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, PortError> {
        match self.rx.recv_timeout(self.timeout) {
            Ok(Ok(line)) => serde_json::from_str(&line).map(Some).map_err(adapt),
            Ok(Err(e)) => Err(adapt(e)),
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                Err(PortError::Adapter(format!(
                    "plugin {} timed out after {:?}",
                    self.info.id, self.timeout
                )))
            }
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }
```

- [ ] **Step 5: Switch both read sites to `recv_message`**

In `fn call`, change:

```rust
        let resp: Response = read_message(&mut self.stdout)
            .map_err(adapt)?
            .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
```

to:

```rust
        let resp: Response = self
            .recv_message()?
            .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
```

In `fn call_with_callbacks`, change:

```rust
            let msg: Incoming = read_message(&mut self.stdout)
                .map_err(adapt)?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
```

to:

```rust
            let msg: Incoming = self
                .recv_message()?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
```

(The free `read_message` import from `cairn_plugin_protocol` is now unused in this file — remove `read_message` from that `use` list to avoid an unused-import warning. Keep `write_message`.)

- [ ] **Step 6: Spawn the reader thread in `spawn_plugin`**

Change `fn spawn_plugin(plugin_dir: &Path)` to `fn spawn_plugin(plugin_dir: &Path, timeout: Duration)`. Replace the stdout setup + the `LoadedPlugin { ... }` literal. Currently:

```rust
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
            capabilities: manifest.engine.capabilities.clone(),
        };
```

Replace with:

```rust
        let stdin = child.stdin.take().ok_or_else(|| adapt("no stdin"))?;
        let child_stdout = child.stdout.take().ok_or_else(|| adapt("no stdout"))?;
        let (tx, rx) = mpsc::channel::<std::io::Result<String>>();
        let reader = std::thread::spawn(move || {
            let mut stdout = BufReader::new(child_stdout);
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) => break, // EOF: drop tx -> channel disconnects
                    Ok(_) => {
                        if line.trim().is_empty() {
                            continue; // skip blank lines (matches old read_message)
                        }
                        if tx.send(Ok(line)).is_err() {
                            break; // consumer (LoadedPlugin) was dropped
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let mut plugin = LoadedPlugin {
            child,
            stdin,
            rx,
            reader: Some(reader),
            timeout,
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

- [ ] **Step 7: Add `load_with_timeout` + route `load` through it**

Replace the `pub fn load(dir: &Path) -> Result<Self, PortError>` signature line + body so `load` delegates and the loop lives in `load_with_timeout`. The current `load` body builds `loaded` by calling `Self::spawn_plugin(&plugin_dir)`. Change to:

```rust
    /// Load every `<dir>/<id>/manifest.toml` with the default read timeout.
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load(dir: &Path) -> Result<Self, PortError> {
        Self::load_with_timeout(dir, DEFAULT_PLUGIN_TIMEOUT)
    }

    /// Like [`Self::load`] but with an explicit per-message read `timeout` (used by
    /// tests, and the seam for future config).
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load_with_timeout(dir: &Path, timeout: Duration) -> Result<Self, PortError> {
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
            match Self::spawn_plugin(&plugin_dir, timeout) {
                Ok(p) => loaded.push(p),
                Err(e) => eprintln!("plugin: skipping {}: {e}", plugin_dir.display()),
            }
        }
        Ok(Self { loaded })
    }
```

(Keep the existing doc comment that was above `load` only if it still fits; the replacement above includes fresh doc comments. `ProcessPluginHost` stays `#[derive(Default)]` with just `loaded` — no struct change.)

- [ ] **Step 8: Join the reader thread on `Drop`**

Change `impl Drop for LoadedPlugin` from:

```rust
impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

to:

```rust
impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        // Kill first so the reader thread's read_line hits EOF and exits, then join.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}
```

- [ ] **Step 9: Run the timeout test**

Run: `cargo test -p cairn-plugin-example --test host invoke_times_out`
Expected: PASS — `hang` returns a "timed out" `Adapter` within a few seconds, and the follow-up `echo` errors fast (plugin killed).

- [ ] **Step 10: Run the full host suite (no false kills)**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS — every existing host test (echo/noteLen/write/delete/search/list/events/denials) still passes through the new reader-thread read path with the 30s default. Then `cargo test -p cairn-infra` — the infra unit tests (`load_absent_dir_is_empty`, `unspawnable_plugin_is_skipped_not_fatal`) still pass.

- [ ] **Step 11: Full workspace suite + lint + fmt + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green, no warnings, fmt clean, lock consistent (no new deps).

- [ ] **Step 12: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin): per-message read timeout (reader thread + recv_timeout)"
```

---

## Notes for the implementer

- **The reader thread is the whole point** — std `read_line` on a child pipe can't be interrupted, so a per-plugin thread + `recv_timeout` is the only portable timeout. Don't try to set a socket/fd timeout (not portable for pipes across the 3-OS matrix).
- **`recv_message` reproduces the old `read_message` contract** (blank-skip is now in the reader thread; `Ok(None)` ↔ channel disconnect ↔ EOF; parse error → `Adapter`) and adds timeout/kill. Both `call` and `call_with_callbacks` go through it, so initialize, invoke, and event delivery are all bounded.
- **Kill-then-join order in `Drop`** is required: killing the child makes the reader's blocking `read_line` return EOF so the thread can exit; joining before the kill would hang.
- **A killed plugin doesn't re-hang:** its channel disconnects, so the next `recv_message` returns `Ok(None)` → the existing "plugin closed its output" fast-error; and a write to its dead stdin errors fast too.
- **`hang` sleeps 24h** (not `loop {}`/`park()`): no busy-CPU, no spurious-wakeup risk, and the process is killed by the host well before it wakes.
- **fmt:** run `cargo fmt` before committing (CI's rustfmt check is strict).
- **Don't touch** the protocol crate's `read_message` (the SDK + example main still use it), or any other crate — the timeout is internal to `cairn-infra`.
```
