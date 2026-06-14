# Plugin Windows Sandbox — Design

> Follow-up to #40 / #62. macOS Seatbelt landed in #60, Linux bubblewrap in #61
> (`docs/superpowers/specs/2026-06-1{3,4}-plugin-{macos,linux}-sandbox-design.md`
> + `crates/cairn-infra/src/sandbox.rs`). This design adds the Windows backend.

## Goal

Jail trusted plugins in an OS sandbox on Windows so a spawned plugin's direct
filesystem-write, network access, and reads of the user's vault are denied by
the OS — and refuse to spawn anywhere a working jail cannot be applied.
Behaviorally equivalent to the macOS/Linux backends on **writes, network, and
vault-deny**; it diverges only on **read breadth** (see "Deliberate deviation"),
implemented through the existing `Sandbox` port with **no port changes**.

## Research spike: why this shape

A spike (AppContainer vs job object vs restricted token; whether a pure external
launcher is possible) established three facts that drive the whole design:

1. **No in-box launcher exists.** Windows ships nothing like `sandbox-exec` /
   `bwrap`. Windows Sandbox is a Hyper-V VM (unusable on CI runners, wrong
   granularity); `runas /trustlevel` does not drop integrity or confine fs/net;
   `icacls` is not a launcher. A faithful jail **requires Win32 FFI**, hence
   `unsafe`.

2. **`wrap() -> Command` cannot carry the jail through stable `std`.**
   `std::process::Command` exposes no token / integrity / AppContainer API.
   `creation_flags()` is stable, but `raw_attribute()` — the only path to the
   `STARTUPINFOEX` `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` that AppContainer
   needs — is unstable (rust-lang/rust#114854) and unusable on the pinned MSRV
   1.88. No MSRV-1.88, permissively-licensed safe wrapper crate exists (`rappct`
   needs 1.90; `runas-rs` is GPL). So the FFI must be ours, and it cannot live
   behind a `std::process::Command` built in `cairn-infra`.

3. **Windows has no mount/profile namespace.** You can have at most two of
   {broad reads, vault-deny, don't-touch-the-vault-on-disk}. A restricted
   token + low integrity gives broad-read + write-deny but **cannot** deny one
   subtree without writing a deny-ACE onto the vault's on-disk ACL (restricting
   SIDs are deny-by-intersection — an allowlist — so they kill broad reads
   instead). AppContainer is **deny-by-default**: the vault, lacking any ACE for
   the package SID, is unreadable *for free, touching nothing on the vault*, at
   the cost of broad reads.

We therefore confine via **AppContainer**, set up by a **small Windows-only
launcher binary** invoked exactly like `bwrap`/`sandbox-exec`.

## Mechanism: AppContainer via an external launcher (decision)

`WindowsAppContainerSandbox::wrap()` returns
`Command::new(<launcher>).args([--plugin-dir, <dir>, --, <cmd>, <args…>])`. The
launcher (`cairn-sandbox-win`) does the AppContainer setup via FFI, then
`CreateProcess`-execs the inner command inside the container. This mirrors the
macOS/Linux external-launcher pattern, keeps the `Sandbox` port and
`cairn-infra` **`unsafe`-free**, and isolates all FFI in one crate.

### Confinement recipe (launcher, `--plugin-dir <dir> -- <cmd> <args…>`)

1. Derive a **stable** AppContainer SID from a fixed name (`Cairn.PluginSandbox`)
   via `CreateAppContainerProfile` (idempotent — `ERROR_ALREADY_EXISTS` is fine)
   / `DeriveAppContainerSidFromAppContainerName`.
2. Grant that SID **read + execute** on the plugin dir
   (`SetNamedSecurityInfo`, idempotent). The vault is **never** granted → it
   stays unreadable by deny-by-default.
3. Build `SECURITY_CAPABILITIES` with the package SID and **no capabilities**
   (no `internetClient`/`internetClientServer`/`privateNetworkClientServer`) →
   Windows' built-in WFP block denies all sockets. Attach via `STARTUPINFOEX`
   `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`.
4. Create a **job object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
   `CreateProcess` the inner command **suspended**, `AssignProcessToJobObject`,
   then resume. Stdio is inherited so the host's pipes pass through.
5. Wait for the child and exit with the child's exit code, so the host observes
   the real status (the behavioral tests assert on it).

### `--probe` mode

`cairn-sandbox-win --probe` creates (or confirms) the AppContainer profile and
exits 0 on success, non-zero otherwise. This is the Windows analogue of the
bwrap userns probe: it turns a host where AppContainer is unavailable into a
clean refusal from `wrap()` instead of a confusing spawn-time error.

## Deliberate deviation: read breadth

The macOS/Linux backends allow **broad reads** (vault denied). AppContainer is
deny-by-default, so a Windows-jailed plugin can read **only**: system locations
ACL'd for `ALL_APPLICATION_PACKAGES` (System32 / KnownDLLs / WinSxS — enough to
load the exe and system DLLs, which is why UWP apps run) plus its own plugin dir
(granted in step 2). Arbitrary **user-file** reads are denied.

This is accepted as a documented, tracked deviation, for the same reasons the
Linux backend documented its exec-deny gap:

- Broad user-file reads are a **fidelity choice, not a technical necessity** on
  Windows — the binary and system DLLs load fine under AppContainer.
- The divergence is **stricter** (safer), not weaker; the three security-bearing
  guarantees — no writes, no network, vault unreadable — hold identically.
- Tightening non-vault reads is exactly the spirit of #63; doing the inverse
  (loosening AppContainer to broad reads) would require an on-disk deny-ACE on
  the user's git-backed vault, which this design explicitly refuses.

## Architecture

### `crates/cairn-infra/src/sandbox.rs` (no `unsafe`)

Compiled on all platforms, like `MacSeatbeltSandbox` / `LinuxBwrapSandbox`.

```rust
pub struct WindowsAppContainerSandbox {
    /// Path to the launcher binary (overridable in tests).
    exec: PathBuf,
    /// Cached result of the one-time `--probe` (see `wrap`).
    probe: std::sync::OnceLock<Result<(), String>>,
}
```

- `Default` → `exec` discovered next to `std::env::current_exe()`
  (`cairn-sandbox-win.exe` ships alongside the host binary).
- `with_exec(PathBuf)` for tests (mirrors the other backends).

Pure, cross-platform-testable seam (analogue of `bwrap_args`):

```rust
pub(crate) fn windows_launcher_args(plugin_dir: &Path, cmd: &Path, args: &[String])
    -> Vec<OsString>
```

emits `[--plugin-dir, <plugin_dir>, --, <cmd>, <args…>]` as distinct `OsString`
argv entries — no quoting, non-UTF-8 paths survive intact (the `--` separates
the launcher's own flags from the inner command, so an inner arg starting with
`-` is unambiguous).

`wrap()` mirrors the Linux adapter:

1. **Existence check** — `self.exec.exists()` false → `Unavailable`.
2. **One-time probe** — `OnceLock::get_or_init` runs `launcher --probe`; a
   non-zero exit / spawn failure caches and returns
   `Unavailable("AppContainer unavailable: …")`.
3. **Canonicalize** `plugin_dir` and `cmd` (each error → `Unavailable`).
   `vault_root` needs no action — deny is structural (never granted).
4. **Build** `Command::new(&self.exec).args(windows_launcher_args(...))`. Stdio
   left unconfigured — the caller wires it (unchanged contract).

`platform_sandbox()` gains the Windows arm; `RefusingSandbox` then covers only
non-macOS/non-Linux/non-Windows targets.

```rust
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]   { Box::new(MacSeatbeltSandbox::default()) }
    #[cfg(target_os = "linux")]   { Box::new(LinuxBwrapSandbox::default()) }
    #[cfg(target_os = "windows")] { Box::new(WindowsAppContainerSandbox::default()) }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                                  { Box::new(RefusingSandbox) }
}
```

### `crates/cairn-sandbox-win/` (new bin crate — the sole `unsafe` carve-out)

- Added to `[workspace] members`.
- Does **not** inherit the workspace `forbid(unsafe_code)`: it sets
  `[lints.rust] unsafe_code = "allow"` with a comment explaining that AppContainer
  setup is unavoidably Win32 FFI and all of it is localized here.
- `windows-sys` (already in the lockfile at 0.61.2; `MIT OR Apache-2.0`; MSRV
  1.85) under `[target.'cfg(windows)'.dependencies]`, with the feature flags for
  `CreateAppContainerProfile`, `DeriveAppContainerSidFromAppContainerName`,
  `SetNamedSecurityInfo`, `STARTUPINFOEX` / `UpdateProcThreadAttribute`,
  `CreateProcessW`, `CreateJobObjectW`, `AssignProcessToJobObject`,
  `ResumeThread`, `WaitForSingleObject`, `GetExitCodeProcess`.
- `main()` is `#[cfg(windows)]` real work; on other targets a stub that exits
  non-zero (so non-Windows builds compile cleanly and pull no `windows-sys`).
- Argv: `--probe` | `--plugin-dir <dir> -- <cmd> <args…>`.

The `Sandbox` port (`cairn-ports`), `ProcessPluginHost`, and the daemon are
**untouched** (they already thread `&dyn Sandbox` / call `platform_sandbox()`).

## Confinement guarantee

Equivalent — not byte-identical — to the other backends:

| Capability       | macOS (Seatbelt)    | Linux (bwrap)             | Windows (AppContainer)                   |
|------------------|---------------------|---------------------------|------------------------------------------|
| Read filesystem  | broad, vault denied | broad, vault tmpfs'd      | **deny-by-default**: system DLLs + plugin dir; vault & other user files denied |
| Read plugin dir  | allowed             | allowed (re-bound)        | allowed (granted R+X ACE)                |
| Write filesystem | denied              | denied (read-only)        | denied (writable area is the per-package profile only, not the host) |
| Network          | denied              | denied (`--unshare-all`)  | **denied** (no network capability → WFP block) |
| Further `exec`   | denied              | allowed but equally jailed| allowed but equally jailed (child inherits the AppContainer token) |
| Lifetime         | —                   | `--die-with-parent`       | job object `KILL_ON_JOB_CLOSE`           |

## Error handling

Reuse `SandboxError::Unavailable(String)` — no new variants, no port change.
`thiserror` already defines it at the port boundary.

## Testing

**Pure (any platform), in `cairn-infra`:**
- `windows_launcher_args` produces the expected flags + the two paths + inner
  args in order (no spawn).
- `Unavailable` when the launcher is missing (`with_exec` → nonexistent path).

**`#[cfg(windows)]` behavioral, in `cairn-sandbox-win` integration tests**
(clean launcher discovery via `env!("CARGO_BIN_EXE_cairn-sandbox-win")`; each
skips if `--probe` reports AppContainer unavailable):
- **Write denied** — sandboxed write to a host file fails; file not created.
- **Exec + stdout** — `cmd /c echo hi` → `hi` piped through, exit 0.
- **Network denied** — the test listens on `127.0.0.1:<port>`; a sandboxed
  client connect fails (AppContainer blocks loopback by default — no
  `LoopbackExempt`).
- **Vault read denied / plugin-dir read allowed** — `type <vault>\secret` fails
  (vault never granted); `type <plugin_dir>\own.txt` succeeds (granted R+X).

**CI:** `windows-latest` is already in the `test` 3-OS matrix and runs as
administrator with UAC disabled; `CreateAppContainerProfile`,
`SetNamedSecurityInfo`, and `STARTUPINFOEX` process creation need no elevation,
and the launcher is built by cargo as a workspace member — so **no extra install
step** is required (unlike the Linux `apt-get install bubblewrap`).

## Out of scope

- Capability-derived / tightened-read profiles (#63).
- Restoring macOS/Linux-style broad user-file reads on Windows (would require an
  on-disk deny-ACE on the vault — explicitly refused; see "Deliberate deviation").
- Blocking further `execve`-equivalent (accepted as token-inherited confinement,
  matching the Linux backend).

## Files

- **Add** `crates/cairn-sandbox-win/` (`Cargo.toml` + `src/main.rs`) — the
  AppContainer launcher; the sole `unsafe` carve-out.
- **Modify** `Cargo.toml` — add the crate to `[workspace] members`.
- **Modify** `crates/cairn-infra/src/sandbox.rs` — add
  `WindowsAppContainerSandbox`, `windows_launcher_args`, extend
  `platform_sandbox()`, update the module doc.
- **No change** to `.github/workflows/ci.yml` (launcher builds as a workspace
  member; no tool to install).
