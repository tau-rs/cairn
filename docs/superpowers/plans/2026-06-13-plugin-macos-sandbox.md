# Plugin macOS Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Jail trusted plugins in an OS sandbox on macOS so a spawned plugin's direct filesystem-write, network, and exec are denied by the kernel; refuse to spawn anywhere a working sandbox can't be applied.

**Architecture:** A new `Sandbox` port in `cairn-ports` returns a ready-to-spawn `Command` or `SandboxError::Unavailable`. `cairn-infra` provides `MacSeatbeltSandbox` (wraps `/usr/bin/sandbox-exec -p <SBPL profile>`), a `RefusingSandbox` for platforms with no backend, and a `platform_sandbox()` factory. `ProcessPluginHost::load*`/`spawn_plugin` thread a `&dyn Sandbox`; an `Unavailable` is handled exactly like the existing hash-drift refusal (warn + skip). Static fixed-jail profile (model A): vault reachable only through the already-gated host-RPC channel.

**Tech Stack:** Rust, macOS Seatbelt (`sandbox-exec` / SBPL), `thiserror`, `tempfile` (dev). No `unsafe`, no new runtime dependencies.

---

## File Structure

- **Create** `crates/cairn-infra/src/sandbox.rs` — `Sandbox` adapters (`MacSeatbeltSandbox`, `RefusingSandbox`), the pure `seatbelt_profile()` builder + `sbpl_quote()`, and `platform_sandbox()`. One responsibility: turning a plugin command into a sandboxed `Command`.
- **Modify** `crates/cairn-ports/src/lib.rs` — add the `Sandbox` trait + `SandboxError`.
- **Modify** `crates/cairn-infra/src/lib.rs` — `pub mod sandbox;` + re-exports.
- **Modify** `crates/cairn-infra/src/plugin_host.rs` — thread `&dyn Sandbox` through `load` / `load_with_timeout` / `spawn_plugin`; add a `PermissiveSandbox` test double; new refusal test.
- **Modify** `crates/cairn-daemon/src/main.rs` — pass `platform_sandbox()` into `load_with_timeout`.

---

### Task 1: `Sandbox` port + `SandboxError`

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (add near the other port traits, after `PluginHost`)

This task defines a pure interface with no behavior, so there is no behavioral test — the compile gate (Step 2) is the check. Later tasks' adapters exercise it.

- [ ] **Step 1: Add the trait and error**

At the top of `crates/cairn-ports/src/lib.rs`, ensure these std imports exist (add any missing):

```rust
use std::path::Path;
use std::process::Command;
```

Then add at the end of the file:

```rust
/// Confines a plugin's spawned child process at the OS level. An adapter that
/// cannot confine on the current platform/host **refuses** (returns
/// `Unavailable`), so the host never falls back to spawning an unjailed plugin.
pub trait Sandbox {
    /// Build a [`Command`] that runs `cmd` (with `args`) under an OS sandbox
    /// permitting read of `plugin_dir` and the runtime libraries needed to
    /// exec, while denying direct file-write, network, and further `exec`.
    ///
    /// The returned `Command` has **no stdio configured** — the caller wires
    /// stdin/stdout/stderr after wrapping.
    ///
    /// # Errors
    /// [`SandboxError::Unavailable`] when this platform/host cannot sandbox
    /// (no backend, or the sandbox tool is absent / the paths can't be
    /// resolved). The caller treats this as a refusal to spawn.
    fn wrap(&self, plugin_dir: &Path, cmd: &Path, args: &[String])
        -> Result<Command, SandboxError>;
}

/// Why a [`Sandbox`] could not confine a child.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// No working OS sandbox on this platform/host; the plugin must not spawn.
    #[error("no OS sandbox available: {0}")]
    Unavailable(String),
}
```

- [ ] **Step 2: Compile-check**

Run: `cargo build -p cairn-ports`
Expected: builds clean (no warnings from the new code).

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-ports/src/lib.rs
git commit -m "feat(ports): add Sandbox port for OS-level plugin confinement (#40)"
```

---

### Task 2: Pure `seatbelt_profile()` builder

**Files:**
- Create: `crates/cairn-infra/src/sandbox.rs`
- Modify: `crates/cairn-infra/src/lib.rs`

- [ ] **Step 1: Create the module with the profile builder and a failing test**

Create `crates/cairn-infra/src/sandbox.rs`:

```rust
//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; other platforms refuse (no backend yet — see issue #40).

use std::path::Path;

/// Quote a path as an SBPL string literal, escaping `\` and `"`.
fn sbpl_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The static "fixed jail" (model A) SBPL profile: read-only access to the
/// plugin's own dir + the runtime libraries needed to exec; deny all direct
/// file-write, network, and any `exec` other than the plugin command itself.
/// The vault is reachable only through the gated host-callback channel.
pub(crate) fn seatbelt_profile(plugin_dir: &Path, cmd: &Path) -> String {
    let dir = sbpl_quote(plugin_dir);
    let cmd = sbpl_quote(cmd);
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process-fork)\n\
         (allow file-read*\n\
         \t(subpath \"/usr/lib\")\n\
         \t(subpath \"/System\")\n\
         \t(subpath \"/Library/Frameworks\")\n\
         \t(literal {cmd})\n\
         \t(subpath {dir}))\n\
         (deny file-write*)\n\
         (deny network*)\n\
         (deny process-exec*)\n\
         (allow process-exec (literal {cmd}))\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn profile_denies_write_network_and_interpolates_paths() {
        let p = seatbelt_profile(&PathBuf::from("/cairn/.cairn/plugins/p"), &PathBuf::from("/cairn/.cairn/plugins/p/bin"));
        assert!(p.contains("(deny default)"));
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("(deny process-exec*)"));
        assert!(p.contains("(subpath \"/cairn/.cairn/plugins/p\")"));
        assert!(p.contains("(literal \"/cairn/.cairn/plugins/p/bin\")"));
        assert!(p.contains("(allow process-exec (literal \"/cairn/.cairn/plugins/p/bin\")"));
    }

    #[test]
    fn sbpl_quote_escapes_quotes_and_backslashes() {
        assert_eq!(sbpl_quote(Path::new(r#"/a/"b"\c"#)), r#""/a/\"b\"\\c""#);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/cairn-infra/src/lib.rs`, add after the existing `mod plugin_host;` line:

```rust
pub mod sandbox;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p cairn-infra sandbox::tests`
Expected: PASS (both `profile_denies_...` and `sbpl_quote_...`).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): static Seatbelt SBPL profile builder (#40)"
```

---

### Task 3: `MacSeatbeltSandbox`, `RefusingSandbox`, `platform_sandbox()`

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (re-exports)

- [ ] **Step 1: Add the adapters + factory with failing tests**

Append to `crates/cairn-infra/src/sandbox.rs` (before the `#[cfg(test)] mod tests`):

```rust
use std::path::PathBuf;
use std::process::Command;

use cairn_ports::{Sandbox, SandboxError};

/// macOS Seatbelt backend: runs the plugin under `sandbox-exec -p <profile>`.
pub struct MacSeatbeltSandbox {
    /// Path to the `sandbox-exec` binary (overridable in tests).
    exec: PathBuf,
}

impl Default for MacSeatbeltSandbox {
    fn default() -> Self {
        Self { exec: PathBuf::from("/usr/bin/sandbox-exec") }
    }
}

impl MacSeatbeltSandbox {
    /// Construct with an explicit `sandbox-exec` path (tests).
    pub fn with_exec(exec: PathBuf) -> Self {
        Self { exec }
    }
}

impl Sandbox for MacSeatbeltSandbox {
    fn wrap(&self, plugin_dir: &Path, cmd: &Path, args: &[String]) -> Result<Command, SandboxError> {
        if !self.exec.exists() {
            return Err(SandboxError::Unavailable(format!(
                "{} not found",
                self.exec.display()
            )));
        }
        // Seatbelt `subpath`/`literal` match canonical absolute paths.
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let profile = seatbelt_profile(&dir, &cmd_abs);
        let mut c = Command::new(&self.exec);
        c.arg("-p").arg(profile).arg("--").arg(&cmd_abs).args(args);
        Ok(c)
    }
}

/// Backend for platforms with no OS sandbox yet: always refuses, so the host
/// never spawns an unjailed plugin (Linux/Windows backends are issue-#40
/// follow-ups).
pub struct RefusingSandbox;

impl Sandbox for RefusingSandbox {
    fn wrap(&self, _plugin_dir: &Path, _cmd: &Path, _args: &[String]) -> Result<Command, SandboxError> {
        Err(SandboxError::Unavailable(format!(
            "no sandbox backend for target_os={}",
            std::env::consts::OS
        )))
    }
}

/// The sandbox for the current platform: Seatbelt on macOS, refusing elsewhere.
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSeatbeltSandbox::default())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(RefusingSandbox)
    }
}
```

Add these tests inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn refusing_sandbox_is_always_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let err = RefusingSandbox
            .wrap(tmp.path(), Path::new("/bin/echo"), &[])
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_sandbox_exec_is_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let s = MacSeatbeltSandbox::with_exec(PathBuf::from("/nonexistent/sandbox-exec"));
        let err = s.wrap(tmp.path(), Path::new("/bin/echo"), &[]).unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }
```

The test module already has `use super::*;`; also add at the top of the test module:

```rust
    use cairn_ports::{Sandbox, SandboxError};
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p cairn-infra sandbox::tests`
Expected: PASS. On macOS, `missing_sandbox_exec_is_unavailable` also runs; on Linux/Windows it is `cfg`-skipped.

- [ ] **Step 3: Re-export from the crate root**

In `crates/cairn-infra/src/lib.rs`, add to the re-export block:

```rust
pub use sandbox::{platform_sandbox, RefusingSandbox};
#[cfg(target_os = "macos")]
pub use sandbox::MacSeatbeltSandbox;
```

- [ ] **Step 4: Build to confirm re-exports**

Run: `cargo build -p cairn-infra`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): macOS Seatbelt + refusing sandbox backends (#40)"
```

---

### Task 4: Thread `&dyn Sandbox` through `ProcessPluginHost`

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs:398-512` (signatures + spawn) and the test module

- [ ] **Step 1: Add the import**

At the top of `crates/cairn-infra/src/plugin_host.rs`, add:

```rust
use cairn_ports::Sandbox;
```

- [ ] **Step 2: Add `sandbox` params to `load` and `load_with_timeout`**

Replace the `load` body (`plugin_host.rs:398-400`):

```rust
    pub fn load(dir: &Path, trusted: &TrustedPlugins, sandbox: &dyn Sandbox) -> Result<Self, PortError> {
        Self::load_with_timeout(dir, DEFAULT_PLUGIN_TIMEOUT, trusted, sandbox)
    }
```

Change the `load_with_timeout` signature (`plugin_host.rs:407-411`) to add the param:

```rust
    pub fn load_with_timeout(
        dir: &Path,
        timeout: Duration,
        trusted: &TrustedPlugins,
        sandbox: &dyn Sandbox,
    ) -> Result<Self, PortError> {
```

In its body, change the spawn call (`plugin_host.rs:469`) to pass the sandbox:

```rust
            match Self::spawn_plugin(&plugin_dir, timeout, sandbox) {
                Ok(p) => loaded.push(p),
                Err(e) => tracing::warn!("plugin: refusing {}: {e}", plugin_dir.display()),
            }
```

- [ ] **Step 3: Use the sandbox in `spawn_plugin`**

Change the `spawn_plugin` signature (`plugin_host.rs:477`):

```rust
    fn spawn_plugin(
        plugin_dir: &Path,
        timeout: Duration,
        sandbox: &dyn Sandbox,
    ) -> Result<LoadedPlugin, PortError> {
```

Replace the direct spawn (`plugin_host.rs:506-512`) — wrap via the sandbox, then add stdio:

```rust
        let mut command = sandbox
            .wrap(plugin_dir, &cmd_path, &manifest.engine.args)
            .map_err(adapt)?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(adapt)?;
```

- [ ] **Step 4: Add the test double and update existing tests**

In the `#[cfg(test)] mod tests` block of `plugin_host.rs`, add a permissive double near the top (after `use super::*;`):

```rust
    use crate::sandbox::RefusingSandbox;
    use cairn_ports::{Sandbox, SandboxError};
    use std::process::Command;

    /// Test double: spawns the command verbatim (no OS jail) so the spawn path
    /// is exercised on every platform without Seatbelt.
    struct PermissiveSandbox;
    impl Sandbox for PermissiveSandbox {
        fn wrap(&self, _dir: &Path, cmd: &Path, args: &[String]) -> Result<Command, SandboxError> {
            let mut c = Command::new(cmd);
            c.args(args);
            Ok(c)
        }
    }
```

Update the three existing `load` call sites to pass `&PermissiveSandbox`:

- `load_absent_dir_is_empty`:
  ```rust
  let host = ProcessPluginHost::load(&tmp.path().join("missing"), &trusted, &PermissiveSandbox).unwrap();
  ```
- `unspawnable_plugin_is_skipped_not_fatal`:
  ```rust
  let host = ProcessPluginHost::load(tmp.path(), &trusted, &PermissiveSandbox).unwrap();
  ```
- `untrusted_plugin_is_not_loaded`:
  ```rust
  let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none(), &PermissiveSandbox).unwrap();
  ```
- `untrusted_manifest_is_not_parsed`:
  ```rust
  let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none(), &PermissiveSandbox).unwrap();
  ```

- [ ] **Step 5: Add the refusal test (choice B: no jail ⇒ no spawn)**

Add to the same test module:

```rust
    #[test]
    fn unavailable_sandbox_refuses_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "p", "p");
        let trusted = TrustedPlugins::from_ids(["p".to_string()]);
        // RefusingSandbox => the plugin is refused, never spawned.
        let host = ProcessPluginHost::load(tmp.path(), &trusted, &RefusingSandbox).unwrap();
        assert!(host.plugins().is_empty());
    }
```

- [ ] **Step 6: Run the plugin-host tests**

Run: `cargo test -p cairn-infra plugin_host::tests`
Expected: PASS (all existing tests + `unavailable_sandbox_refuses_spawn`).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(infra): require a Sandbox to spawn plugins; refuse if unavailable (#40)"
```

---

### Task 5: Wire `platform_sandbox()` into the daemon

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs:100-101`

- [ ] **Step 1: Pass the platform sandbox to the loader**

Replace the `load_with_timeout` call (`main.rs:100`). Bind the box first so it outlives the call:

```rust
    let sandbox = cairn_infra::platform_sandbox();
    match cairn_infra::ProcessPluginHost::load_with_timeout(
        &plugins_dir,
        plugin_timeout,
        &trusted,
        sandbox.as_ref(),
    ) {
```

(The rest of the `match` block — `Ok(host) => ...`, `Err(e) => ...` — is unchanged.)

- [ ] **Step 2: Build the daemon**

Run: `cargo build -p cairn-daemon`
Expected: builds clean.

- [ ] **Step 3: Run the whole workspace test suite**

Run: `just test`
Expected: PASS across the workspace (Linux dev box: macOS-`cfg` tests are skipped; the refusal path is covered by `unavailable_sandbox_refuses_spawn`).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): sandbox trusted plugins via platform_sandbox (#40)"
```

---

### Task 6: Real-jail macOS integration test

Proves the spec's "the jail is real, not nominal" requirement by exercising
`MacSeatbeltSandbox` against the live kernel: a sandboxed command that tries to
write outside its dir must fail, while a benign command must still run (exec +
stdout pipe work). Runs on the `macos-latest` leg of `ci.yml`'s `test` job (every
PR) and `heavy.yml`'s `os-matrix`; `cfg`-skipped elsewhere.

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs` (test module)

- [ ] **Step 1: Add the integration tests**

Add inside the `#[cfg(test)] mod tests` block of `sandbox.rs`:

```rust
    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_denies_write_outside_plugin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let escaped = tmp.path().join("escaped.txt");

        // /usr/bin/touch stands in for the plugin command.
        let mut cmd = MacSeatbeltSandbox::default()
            .wrap(
                &plugin_dir,
                Path::new("/usr/bin/touch"),
                &[escaped.to_string_lossy().into_owned()],
            )
            .expect("sandbox-exec present on macOS");
        let status = cmd.status().expect("spawn under sandbox");

        assert!(!status.success(), "write outside the jail must be denied");
        assert!(!escaped.exists(), "the file must not have been created");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_allows_plugin_to_exec_and_pipe_stdout() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("p");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let output = MacSeatbeltSandbox::default()
            .wrap(&plugin_dir, Path::new("/bin/echo"), &["hi".to_string()])
            .expect("sandbox-exec present on macOS")
            .output()
            .expect("spawn under sandbox");

        assert!(output.status.success(), "the plugin command must be allowed to exec");
        assert_eq!(output.stdout, b"hi\n", "stdout must pipe through the jail");
    }
```

- [ ] **Step 2: Run the tests**

On a macOS host: `cargo test -p cairn-infra sandbox::tests`
Expected: PASS, including both `seatbelt_*` tests.
On a non-macOS host: same command PASSES with the two `seatbelt_*` tests `cfg`-skipped.

> **Mechanism note (the one risk):** if `seatbelt_allows_plugin_to_exec_and_pipe_stdout` fails because the target binary can't exec under `(deny process-exec*)`, the `(allow process-exec (literal {cmd}))` line in `seatbelt_profile` is what permits it — confirm that line is present and ordered **after** the deny (last-match-wins in SBPL). If `seatbelt_denies_write_outside_plugin_dir` *passes* (write denied) the jail is engaged; that is the load-bearing assertion.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs
git commit -m "test(infra): real Seatbelt jail integration tests on macOS (#40)"
```

---

### Task 7: Open follow-up issues (post-implementation)

Not a code change — record the deferred work so the increment's boundaries are visible. Run after the branch is up.

- [ ] **Step 1: Open the follow-up issues**

```bash
gh issue create --title "Plugin sandbox: Linux backend (Landlock + seccomp)" \
  --body "Follow-up to #40 (macOS Seatbelt landed). Implement the \`Sandbox\` port on Linux via Landlock (filesystem) + seccomp (network/exec). Evaluate \`birdcage\`/\`extrasafe\` vs a hand-rolled \`pre_exec\` (the latter needs an unsafe carve-out). Until then \`platform_sandbox()\` returns \`RefusingSandbox\` and trusted plugins do not spawn on Linux."

gh issue create --title "Plugin sandbox: Windows backend (AppContainer/job object)" \
  --body "Follow-up to #40. Implement the \`Sandbox\` port on Windows (AppContainer or job-object jail). Research spike first. Until then trusted plugins do not spawn on Windows."

gh issue create --title "Plugin sandbox: capability-derived profiles (model B)" \
  --body "Follow-up to #40. Replace the fixed model-A jail with a profile derived from the manifest's declared capabilities (e.g. a \`net\` capability opens outbound network). Requires a \`net\`/\`exec\` capability vocabulary that does not exist yet."
```

- [ ] **Step 2: Tick the #40 checkbox**

Edit issue #40 to check "OS-level sandbox for the spawned child (seccomp / landlock / sandbox-exec)" and note macOS is done, Linux/Windows tracked by the new issues.

---

## Done criteria

- `just test` green on the workspace (Linux dev + CI).
- `ci.yml` `test` job green on its `macos-latest` leg, including the two `seatbelt_*` integration tests.
- A trusted, pinned plugin spawns on macOS under Seatbelt; on Linux/Windows it is refused with a clear `tracing::warn!`.
- No `unsafe`, no new runtime dependencies.
