# Plugin OS sandbox — macOS Seatbelt jail before spawn

**Issue:** #40 — *Plugin trust: hardening follow-ups (post-S3)*, the "OS-level
sandbox for the spawned child" box.

**Builds on:** [the plugin trust gate](2026-06-11-cairn-plugin-trust-design.md)
and [content hashing](2026-06-12-plugin-content-hashing-design.md). Those make
*presence + drift* insufficient to run; this makes *the OS itself* able to
constrain a plugin once it does run.

## Problem

A trusted plugin is fully-trusted code: `ProcessPluginHost::spawn_plugin`
(`crates/cairn-infra/src/plugin_host.rs:506`) does `Command::new(cmd_path)` and
the child runs with the daemon user's full OS privileges — any file, any socket,
any further `exec`. The manifest `capabilities` field gates only the
host-callback RPC surface (`host/readNote`, `host/writeNote`); it does not
constrain what the process does directly. The trust gate decides *whether* a
plugin runs; nothing constrains *what it can do* once it does.

This spec adds an OS-level jail around the spawned child on macOS, so direct
filesystem and network access are denied by the kernel and the vault is
reachable **only** through the already-gated host-callback channel.

## Scope (smallest viable increment)

- **macOS only**, via `/usr/bin/sandbox-exec` (Seatbelt). Linux (Landlock +
  seccomp) and Windows (AppContainer/job object) are **out of scope**, tracked
  as follow-up issues opened on merge.
- **Fixed minimal jail (model A):** one static SBPL profile. Capabilities remain
  an RPC-only boundary; the sandbox is a blanket deny of direct fs-write,
  network, and further exec, plus a deny of direct reads of the vault (reads are
  otherwise allowed so the binary can link — see the profile section for why a
  curated read-allowlist failed on macOS 26). Deriving the profile from declared
  capabilities (model B) is a deferred follow-up, not built here.
- **Hard secure default (choice B):** a trusted plugin spawns **only** if a
  working sandbox is applied. On a platform with no backend (Linux/Windows) or
  if `sandbox-exec` is missing/fails, the plugin is **refused**, never spawned
  unsandboxed. No config knob, no per-plugin opt-out.

## Trust model, restated

The trust gate answers *whether* a plugin may run. This sandbox bounds *what it
can do directly*. They compose: a plugin must be (1) trusted-listed, (2)
content-pinned (no drift), and (3) successfully jailed by the OS — all three —
or it does not spawn. The sandbox is defense-in-depth around fully-trusted code,
not a license to run untrusted plugins.

## Design

### The `Sandbox` port

A new trait in `cairn-ports/src/lib.rs`, alongside `PluginHost`/`Watcher`/`Vcs`
(adapters live in `cairn-infra`, per the module's existing convention):

```rust
/// Wraps a plugin command so the OS confines the spawned child. An adapter that
/// cannot confine on this platform refuses, so the host never spawns unjailed.
pub trait Sandbox {
    /// Return a `Command` that runs `cmd` (with `args`) under an OS sandbox that
    /// allows reads broadly (so the binary can link) but denies direct reads of
    /// `vault_root` — re-allowing the plugin's own `plugin_dir` — and denies
    /// direct file-write, network, and further `exec`. Stdio is wired by the
    /// caller after wrapping.
    ///
    /// # Errors
    /// `SandboxError::Unavailable` when this platform/host cannot sandbox (no
    /// backend, or `sandbox-exec` absent) — the caller treats this as a refusal.
    fn wrap(&self, vault_root: &Path, plugin_dir: &Path, cmd: &Path, args: &[String])
        -> Result<Command, SandboxError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("no OS sandbox available: {0}")]
    Unavailable(String),
}
```

`wrap` returns a `Command` *without* stdio configured; `spawn_plugin` keeps
ownership of the `.stdin(piped).stdout(piped).stderr(inherit())` wiring exactly
as today. The sandbox only changes *which* program is launched and with what
leading args — it does not touch the pipe setup.

### Backends in `cairn-infra`

- **`MacSeatbeltSandbox`** (defined unconditionally — *not* cfg-gated, so
  `seatbelt_profile` and the SBPL-string unit tests stay live on every platform;
  only the lib.rs re-export and the live-spawn tests are `#[cfg(target_os =
  "macos")]`): builds `Command::new("/usr/bin/sandbox-exec")` with
  `-p <profile> -- <cmd> <args...>`. If `/usr/bin/sandbox-exec` is not present
  (or not executable), returns `Unavailable`.
- **`RefusingSandbox`**: `wrap` always returns `Unavailable("no backend for
  <os>")`. Used on non-macOS targets and in unit tests that assert refusal.
- **One factory:** `pub fn platform_sandbox() -> Box<dyn Sandbox>` returns
  `MacSeatbeltSandbox` on macOS, `RefusingSandbox` elsewhere, selected with
  `cfg!(target_os = "macos")`. The call site in `spawn_plugin` stays
  platform-agnostic.

### The SBPL profile (static, model-A jail)

Inline string, parameterized by `vault_root`, `plugin_dir`, and the resolved `cmd`:

```scheme
(version 1)
(deny default)
(allow process-fork)
(allow file-read*)
(deny file-read* (subpath "<vault-root>"))
(allow file-read* (subpath "<plugin-dir>"))
(deny file-write*)
(deny network*)
(deny process-exec*)
(allow process-exec (literal "<cmd-path>"))
```

> **Read posture — revised during implementation (the original failed on
> macOS 26).** This spec first proposed a curated read-*allow*list
> (`/usr/lib`, `/System`, `/Library/Frameworks`, the plugin dir, the command).
> Empirically that **bricks every plugin on macOS 26**: the dyld shared cache
> that every dynamically-linked binary must map at launch now lives behind the
> Preboot **cryptex** mount and is *not* covered even by `(subpath "/System")`,
> so the linker `SIGABRT`s before the plugin runs. Curating the exact read set
> is brittle (the cryptex path is UUID-versioned and moves across macOS
> releases / differs from the CI runner). The shipped profile instead allows
> reads **broadly**, then **denies the vault root** and **re-allows the
> plugin's own dir** (last-match-wins SBPL). Net security posture is unchanged
> for what matters: the user's notes are unreadable except through the gated
> host-RPC channel, and `file-write*` / `network*` / `process-exec*` stay
> denied — so even a read a plugin *can* perform off-vault cannot be
> exfiltrated. Validated end-to-end against the live macOS 26 kernel
> (see the integration tests). Reads of non-vault files (e.g. `~/.ssh`) are
> permitted but inert under the write/network/exec denial; tightening those is
> deferred to the model-B follow-up.

Notes:
- Rule **order is load-bearing**: `(allow file-read*)` → `(deny … vault)` →
  `(allow … plugin-dir)`. A note under the vault but outside the plugin dir
  matches the deny last → denied; a file under the plugin dir matches the final
  allow → readable.
- `vault_root`, `plugin_dir`, and `cmd` are all **canonicalized** before
  interpolation (Seatbelt matches resolved paths; on macOS `/tmp` → `/private/tmp`,
  etc.), so the deny and re-allow subpaths match the kernel's view.
- Stdin/stdout/stderr are inherited file descriptors; Seatbelt governs new
  `open`/`socket` calls, not fds the child already holds, so the host-RPC pipes
  keep working with no explicit allow — this is *why* denying direct vault reads
  does not break a plugin's legitimate note access (it reads notes over the
  pipe, not off disk).
- `(deny process-exec*)` followed by `(allow process-exec (literal "<cmd-path>"))`
  permits only the plugin binary to exec; confirmed by the integration test that
  the target launches under the jail.
- The profile is built by a small pure helper
  (`fn seatbelt_profile(vault_root: &Path, plugin_dir: &Path, cmd: &Path) -> String`)
  that is unit-tested for correct path interpolation and SBPL-safe quoting
  (escaping `\`, `"`, `\n`, `\r`) independent of any spawn.

### Wiring into `ProcessPluginHost`

`load`, `load_with_timeout`, and `spawn_plugin` gain a `sandbox: &dyn Sandbox`
parameter (threaded from the daemon/CLI construction site, which calls
`platform_sandbox()`). The public `load`/`load_with_timeout` signatures gain
only `sandbox`; the **vault root** the profile needs is derived *internally*
once in `load_with_timeout` (the plugins dir is always `<vault>/.cairn/plugins`,
so the vault root is its grandparent; a degenerate shallow layout falls back to
the plugins dir with a loud `tracing::warn!`) and threaded into `spawn_plugin`.
In `spawn_plugin`, after the manifest/id check:

```rust
let mut cmd = match sandbox.wrap(vault_root, plugin_dir, &cmd_path, &manifest.engine.args) {
    Ok(c) => c,
    Err(e) => return Err(adapt(/* SandboxError */ e)),  // -> refusal in load loop
};
let mut child = cmd
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .map_err(adapt)?;
```

In the `load` loop, a sandbox refusal is handled exactly like the existing
hash-drift refusal arm — `tracing::warn!("plugin: refusing {dir_name}: <reason>")`
and `continue`. A plugin is **never** spawned without a successful `wrap`.

### Config

**None.** Choice B has no sandbox-mode knob, so `[plugins]` in `cairn.toml` is
unchanged. The only new code path is constructing `platform_sandbox()` and
passing it to `load`.

## Testing

- **macOS jail integration test** — `#[cfg(target_os = "macos")]`, runs on the
  `macos-latest` leg of `ci.yml`'s `test` job (every PR) and `heavy.yml`'s
  `os-matrix`. A fixture plugin attempts, on `init`, to (a) write a file outside
  its own dir and (b) open a TCP socket; the test asserts **both fail** while a
  normal host-RPC round-trip still succeeds. Proves the jail is real, not
  nominal. Skipped on non-macOS legs by `cfg`.
- **Refusal unit tests** (all platforms): `RefusingSandbox` → `load` returns
  `Ok` with zero plugins and logs a refusal; `MacSeatbeltSandbox` pointed at a
  non-existent `sandbox-exec` path → `Unavailable` → same refusal. Proves the
  choice-B "no jail ⇒ no spawn" contract.
- **Profile-builder unit test**: `seatbelt_profile` interpolates `plugin_dir`
  and `cmd` correctly and quotes paths safely (spaces, no SBPL injection).
- **Existing trust/hash/timeout tests**: unchanged, updated to pass a permissive
  test sandbox (a `Sandbox` impl that returns `Command::new(cmd)` verbatim) so
  they still exercise the real spawn path on every platform without needing
  Seatbelt.

## Follow-up issues (opened on merge)

- **Linux backend** — Landlock (filesystem) + seccomp (network/exec). Evaluate
  `birdcage`/`extrasafe` vs a hand-rolled `pre_exec` (the latter needs an
  `unsafe` carve-out the crate avoids).
- **Windows backend** — AppContainer / job-object jail; research spike first.
- **Model B (capability-derived profiles)** — once a real plugin needs `net` or
  broader fs, derive the profile from declared capabilities instead of the fixed
  jail. Requires a `net`/`exec` capability vocabulary that does not exist yet.

## Non-goals

- Sandboxing on Linux/Windows (refused until their backends land).
- A configurable sandbox mode or per-plugin opt-out.
- Constraining the host-callback RPC surface — that is the existing capability
  gate, unchanged here.
