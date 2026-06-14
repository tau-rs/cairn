# Plugin Windows Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Confine trusted plugins on Windows in an AppContainer set up by a small external launcher (`cairn-sandbox-win`), so a plugin cannot write to the host, reach the network, or read the user's vault — and the host refuses to spawn where the jail cannot be applied.

**Architecture:** Mirror the macOS/Linux external-launcher pattern. `cairn-infra`'s new `WindowsAppContainerSandbox` (no `unsafe`) returns `Command::new(<launcher>).args([--plugin-dir, <dir>, --, <cmd>, <args…>])`. The new `cairn-sandbox-win` bin crate (the sole `unsafe` carve-out) does the Win32 AppContainer/job-object FFI, then execs the inner command inside the container.

**Tech Stack:** Rust (stable, MSRV 1.88), `windows-sys 0.61` (already in the lockfile), AppContainer + job objects.

---

## Environment & verification gates

This workspace runs on **macOS**; Windows targets cannot be compiled or run here.

- **Local gate** (runs on this host, after every task): `cargo build -p <crate>`, the cross-platform unit tests (`cargo nextest run -p cairn-infra` / `just test`), and `just lint` (clippy) / `just fmt`. The non-Windows stub of the launcher must compile cleanly.
- **Windows gate** (validated on CI `windows-latest`, and locally if you are on Windows): the `#[cfg(windows)]` behavioral integration tests in `cairn-sandbox-win`. Tasks 4–7 are *not* fully verifiable on macOS; their "Expected" notes describe the CI/Windows outcome.

**`windows-sys` caveat:** the FFI code below uses concrete `windows-sys 0.61` symbol paths and `features`. Exact module paths and feature-flag names occasionally drift between `windows-sys` minor versions. On Windows, run `cargo doc -p cairn-sandbox-win --open` (or check docs.rs/windows-sys/0.61) and fix any path/feature mismatch the compiler reports — the symbols, structs, and call sequence are correct; only their import paths may need adjustment.

**Branch:** the shared working dir means the branch can change between sessions — run `git branch --show-current` immediately before each commit and confirm it is `windows-sandbox-port`.

---

## File structure

- **Create** `crates/cairn-sandbox-win/Cargo.toml` — bin crate manifest; `unsafe_code = "allow"`; `windows-sys` as a `cfg(windows)` target dep.
- **Create** `crates/cairn-sandbox-win/src/main.rs` — arg parsing, mode dispatch (`--probe` | confine), exit-code plumbing; `#[cfg(windows)]` real / else stub.
- **Create** `crates/cairn-sandbox-win/src/win.rs` — `#[cfg(windows)]` FFI: AppContainer profile/SID, plugin-dir ACL grant, `SECURITY_CAPABILITIES` + `STARTUPINFOEX` + job object + `CreateProcess` + wait.
- **Create** `crates/cairn-sandbox-win/tests/confinement.rs` — `#[cfg(windows)]` behavioral integration tests.
- **Modify** `Cargo.toml` — add `crates/cairn-sandbox-win` to `[workspace] members`.
- **Modify** `crates/cairn-infra/src/sandbox.rs` — add `WindowsAppContainerSandbox`, `windows_launcher_args`, extend `platform_sandbox()`, update the module doc.

---

## Task 1: Scaffold the `cairn-sandbox-win` crate (stub + workspace wiring)

**Files:**
- Create: `crates/cairn-sandbox-win/Cargo.toml`
- Create: `crates/cairn-sandbox-win/src/main.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create the crate manifest**

`crates/cairn-sandbox-win/Cargo.toml`:

```toml
[package]
name = "cairn-sandbox-win"
version = "0.1.0"
edition.workspace = true
license.workspace = true

# This crate is the SOLE unsafe carve-out in the workspace: setting up a
# Windows AppContainer is unavoidably Win32 FFI (no MSRV-1.88, permissively
# licensed safe wrapper exists — see the design spec). All unsafe is localized
# here; cairn-infra and the Sandbox port stay unsafe-free. We therefore do NOT
# inherit the workspace `forbid(unsafe_code)` lint.
[lints.rust]
unsafe_code = "allow"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Security_Isolation",
    "Win32_Security_Authorization",
    "Win32_System_Threading",
    "Win32_System_JobObjects",
    "Win32_System_Memory",
] }
```

- [ ] **Step 2: Create the stub `main.rs`**

`crates/cairn-sandbox-win/src/main.rs`:

```rust
//! AppContainer launcher for trusted cairn plugins on Windows.
//!
//! Invoked by `cairn-infra`'s `WindowsAppContainerSandbox` exactly like
//! `bwrap`/`sandbox-exec`:
//!   cairn-sandbox-win --probe
//!   cairn-sandbox-win --plugin-dir <dir> -- <cmd> [<args>...]
//!
//! On non-Windows targets this is a stub that always fails, so the workspace
//! still builds everywhere while the real jail lives behind `cfg(windows)`.

#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("cairn-sandbox-win: AppContainer launcher is Windows-only");
    std::process::ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    win::run()
}
```

- [ ] **Step 3: Add the crate to the workspace members**

In the root `Cargo.toml`, add `"crates/cairn-sandbox-win",` to the `[workspace] members` array (alongside the other `crates/...` entries).

- [ ] **Step 4: Build the stub (local gate)**

Run: `cargo build -p cairn-sandbox-win`
Expected: PASS on macOS — the `cfg(not(windows))` stub compiles, no `windows-sys` pulled.

- [ ] **Step 5: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-sandbox-win/Cargo.toml crates/cairn-sandbox-win/src/main.rs Cargo.toml
git commit -m "feat(infra): scaffold cairn-sandbox-win launcher crate"
```

---

## Task 2: `windows_launcher_args` pure argv builder

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs` (add fn + test, near `bwrap_args`)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-infra/src/sandbox.rs`:

```rust
#[test]
fn windows_launcher_args_passes_plugin_dir_then_cmd_and_args() {
    let a = windows_launcher_args(
        Path::new(r"C:\cairn\.cairn\plugins\p"),
        Path::new(r"C:\cairn\.cairn\plugins\p\plugin.exe"),
        &["--flag".to_string(), "value".to_string()],
    );
    let s: Vec<String> = a.iter().map(|o| o.to_string_lossy().into_owned()).collect();
    assert_eq!(
        s,
        vec![
            "--plugin-dir",
            r"C:\cairn\.cairn\plugins\p",
            "--",
            r"C:\cairn\.cairn\plugins\p\plugin.exe",
            "--flag",
            "value",
        ]
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p cairn-infra windows_launcher_args_passes`
Expected: FAIL — `cannot find function windows_launcher_args`.

- [ ] **Step 3: Implement the builder**

Add near `bwrap_args` in `crates/cairn-infra/src/sandbox.rs`:

```rust
/// Build the argv (after the launcher program) that asks `cairn-sandbox-win`
/// to run `cmd` (with `args`) inside an AppContainer that can read `plugin_dir`
/// but not the vault. The testable analogue of `bwrap_args`.
///
/// `--` separates the launcher's own flags from the inner command, so an inner
/// argument starting with `-` is unambiguous. Paths are emitted as distinct
/// `OsString` argv entries, so no quoting is required and a non-UTF-8 path
/// survives intact. The vault is not named here: AppContainer denies it
/// structurally (deny-by-default — it is simply never granted).
pub(crate) fn windows_launcher_args(plugin_dir: &Path, cmd: &Path, args: &[String]) -> Vec<OsString> {
    let mut v = vec![
        OsString::from("--plugin-dir"),
        plugin_dir.as_os_str().to_os_string(),
        OsString::from("--"),
        cmd.as_os_str().to_os_string(),
    ];
    v.extend(args.iter().map(OsString::from));
    v
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo nextest run -p cairn-infra windows_launcher_args_passes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): add windows_launcher_args argv builder"
```

---

## Task 3: `WindowsAppContainerSandbox` + `wrap()` + `platform_sandbox()` arm

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-infra/src/sandbox.rs`:

```rust
#[test]
fn windows_sandbox_missing_launcher_is_unavailable() {
    let s = WindowsAppContainerSandbox::with_exec(PathBuf::from(
        r"C:\nonexistent\cairn-sandbox-win.exe",
    ));
    let err = s
        .wrap(Path::new("."), Path::new("."), Path::new("cmd.exe"), &[])
        .unwrap_err();
    assert!(matches!(err, SandboxError::Unavailable(_)));
}
```

(The nonexistent launcher makes `wrap()` return at the existence check, before
any canonicalize/probe — so this test is cross-platform.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p cairn-infra windows_sandbox_missing_launcher`
Expected: FAIL — `cannot find type WindowsAppContainerSandbox`.

- [ ] **Step 3: Implement the struct, probe, and `wrap()`**

Add to `crates/cairn-infra/src/sandbox.rs` (after `LinuxBwrapSandbox`). Note `std::sync::OnceLock` is already imported at the top of the file.

```rust
/// One-time probe confirming AppContainer can actually be set up on this host.
/// Runs `<launcher> --probe`, which creates (or confirms) the AppContainer
/// profile and exits 0 on success. A host where AppContainer is disabled then
/// yields a clean refusal from `wrap()` instead of a confusing spawn-time error
/// — the Windows analogue of the bwrap userns probe.
fn appcontainer_probe(exec: &Path) -> Result<(), String> {
    let out = Command::new(exec)
        .arg("--probe")
        .output()
        .map_err(|e| format!("spawn {}: {e}", exec.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "AppContainer unavailable: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Discover the launcher binary next to the running host executable.
fn default_launcher_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("cairn-sandbox-win.exe")))
        .unwrap_or_else(|| PathBuf::from("cairn-sandbox-win.exe"))
}

/// Windows AppContainer backend: runs the plugin under the `cairn-sandbox-win`
/// launcher, which confines it in an AppContainer (no host writes, no network,
/// vault unreadable). Mirrors how `MacSeatbeltSandbox` uses `sandbox-exec`.
pub struct WindowsAppContainerSandbox {
    /// Path to the `cairn-sandbox-win` launcher (overridable in tests).
    exec: PathBuf,
    /// Cached result of the one-time AppContainer probe.
    probe: OnceLock<Result<(), String>>,
}

impl Default for WindowsAppContainerSandbox {
    fn default() -> Self {
        Self {
            exec: default_launcher_path(),
            probe: OnceLock::new(),
        }
    }
}

impl WindowsAppContainerSandbox {
    /// Construct with an explicit launcher path (tests).
    pub fn with_exec(exec: PathBuf) -> Self {
        Self {
            exec,
            probe: OnceLock::new(),
        }
    }
}

impl Sandbox for WindowsAppContainerSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
    ) -> Result<Command, SandboxError> {
        if !self.exec.exists() {
            return Err(SandboxError::Unavailable(format!(
                "{} not found",
                self.exec.display()
            )));
        }
        if let Err(e) = self.probe.get_or_init(|| appcontainer_probe(&self.exec)) {
            return Err(SandboxError::Unavailable(e.clone()));
        }
        // The launcher grants the AppContainer read on the canonical plugin dir
        // and execs the canonical command. The vault is denied structurally
        // (never granted), so `_vault_root` needs no action here.
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let mut c = Command::new(&self.exec);
        c.args(windows_launcher_args(&dir, &cmd_abs, args));
        Ok(c)
    }
}
```

- [ ] **Step 4: Extend `platform_sandbox()` and update the module doc**

Replace the `platform_sandbox()` body so the Windows arm selects the new backend and `RefusingSandbox` covers only the remaining targets:

```rust
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSeatbeltSandbox::default())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxBwrapSandbox::default())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsAppContainerSandbox::default())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(RefusingSandbox)
    }
}
```

Update the file's top module doc comment (lines ~1-3) to mention Windows:

```rust
//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; Linux uses bubblewrap via `bwrap`; Windows uses an
//! AppContainer set up by the `cairn-sandbox-win` launcher; other platforms
//! refuse (no backend).
```

Also update the `RefusingSandbox` doc comment so it no longer claims Windows has no backend (change "the Windows backend is issue #62" to "no backend for this target").

- [ ] **Step 5: Run tests + clippy (local gate)**

Run: `cargo nextest run -p cairn-infra && just lint`
Expected: PASS — `windows_sandbox_missing_launcher_is_unavailable` passes; clippy clean. (On macOS, `WindowsAppContainerSandbox` compiles but `platform_sandbox()` still returns the Mac backend.)

- [ ] **Step 6: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): add WindowsAppContainerSandbox backend wiring"
```

---

## Task 4: Launcher `--probe` mode (AppContainer profile FFI)

> Windows-gated. On macOS this compiles only as the stub; the real code is
> behind `cfg(windows)`. Behavioral verification happens on CI `windows-latest`.
> Apply the `windows-sys` caveat from the top of this plan.

**Files:**
- Create: `crates/cairn-sandbox-win/src/win.rs`
- Create: `crates/cairn-sandbox-win/tests/confinement.rs`

- [ ] **Step 1: Create `win.rs` with arg parsing, the probe, and shared SID helpers**

`crates/cairn-sandbox-win/src/win.rs`:

```rust
//! Windows AppContainer launcher implementation. All unsafe FFI is here.

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, S_OK};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::PSID;

/// Fixed AppContainer profile name → stable package SID. Stable so the read
/// grant on a plugin dir is idempotent across runs.
const PROFILE_NAME: &str = "Cairn.PluginSandbox";

/// UTF-16, NUL-terminated copy of `s`, for the `PCWSTR` Win32 string params.
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    OsString::from(s)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Wide, NUL-terminated copy of an `OsStr` path.
fn wide_path(p: &std::ffi::OsStr) -> Vec<u16> {
    p.encode_wide().chain(std::iter::once(0)).collect()
}

/// Create-or-confirm the AppContainer profile and return its package SID.
/// `CreateAppContainerProfile` returns `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`
/// if the profile exists; in that case we derive the SID instead.
fn ensure_app_container_sid() -> Result<PSID, String> {
    let name = wide(PROFILE_NAME);
    let display = wide("Cairn Plugin Sandbox");
    let desc = wide("Confines trusted cairn plugins");
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: all pointers reference live local buffers for the duration of the
    // call; `sid` is an out-param populated on success.
    let hr = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display.as_ptr(),
            desc.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr == S_OK {
        return Ok(sid);
    }
    // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS) == 0x800700B7
    let already = (0x8007_0000u32 | (ERROR_ALREADY_EXISTS & 0xFFFF)) as i32;
    if hr == already {
        // SAFETY: out-param `sid` populated on success; `name` is live.
        let dhr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
        if dhr == S_OK {
            return Ok(sid);
        }
        return Err(format!("DeriveAppContainerSidFromAppContainerName failed: 0x{dhr:08X}"));
    }
    Err(format!("CreateAppContainerProfile failed: 0x{hr:08X}"))
}

/// Entry point dispatched from `main`.
pub fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().map(|a| a == "--probe").unwrap_or(false) {
        return match ensure_app_container_sid() {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("cairn-sandbox-win: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match confine_and_run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("cairn-sandbox-win: {e}");
            ExitCode::FAILURE
        }
    }
}
```

> `confine_and_run` is added in Task 5. To compile Task 4 in isolation on a
> Windows machine, temporarily stub it as
> `fn confine_and_run(_a: &[OsString]) -> Result<u8, String> { Err("unimplemented".into()) }`;
> Task 5 replaces the stub with the real implementation. (If executing
> sequentially with subagents, implement Task 5 before running Windows tests.)

- [ ] **Step 2: Add `mod win;` is already declared in `main.rs` (Task 1).** Confirm `crates/cairn-sandbox-win/src/main.rs` has `#[cfg(windows)] mod win;` and `win::run()` — added in Task 1.

- [ ] **Step 3: Write the probe integration test**

`crates/cairn-sandbox-win/tests/confinement.rs`:

```rust
//! Windows-only behavioral tests for the AppContainer launcher. Each spawns the
//! built launcher binary (`CARGO_BIN_EXE_cairn-sandbox-win`) and asserts the
//! confinement guarantee. Skipped unless AppContainer is available on the host.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn-sandbox-win"))
}

/// True if `--probe` succeeds (AppContainer usable on this host/runner).
fn appcontainer_usable() -> bool {
    Command::new(launcher())
        .arg("--probe")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn probe_reports_appcontainer_available() {
    // GitHub windows-latest supports AppContainer; assert the probe succeeds.
    assert!(appcontainer_usable(), "--probe must succeed on a runner with AppContainer");
}
```

- [ ] **Step 4: Build (local gate) + note Windows gate**

Run: `cargo build -p cairn-sandbox-win`
Expected (macOS): PASS — stub `main`, `win.rs` excluded by `cfg(windows)`.
Windows gate (CI): `probe_reports_appcontainer_available` passes.

- [ ] **Step 5: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-sandbox-win/src/win.rs crates/cairn-sandbox-win/tests/confinement.rs
git commit -m "feat(infra): cairn-sandbox-win --probe creates AppContainer profile"
```

---

## Task 5: Launcher confinement mode (grant ACE + spawn in AppContainer + job object)

> Windows-gated. Apply the `windows-sys` caveat. This is the core FFI: it grants
> the AppContainer read on the plugin dir, builds the security capabilities,
> spawns the inner command inside the container under a kill-on-close job, and
> propagates the exit code.

**Files:**
- Modify: `crates/cairn-sandbox-win/src/win.rs`

- [ ] **Step 1: Add the plugin-dir read grant**

Append to `win.rs`. Grants the package SID `GENERIC_READ | GENERIC_EXECUTE` on
the plugin dir, merged into the existing DACL (inherited by children so files
inside are readable). The vault is never passed here, so it stays deny-by-default.

```rust
use windows_sys::Win32::Foundation::{GENERIC_EXECUTE, GENERIC_READ, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows_sys::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use windows_sys::Win32::System::SystemServices::{
    CONTAINER_INHERIT_ACE, OBJECT_INHERIT_ACE,
};

fn grant_appcontainer_read(plugin_dir: &std::ffi::OsStr, sid: PSID) -> Result<(), String> {
    let mut path = wide_path(plugin_dir);
    let mut old_dacl: *mut ACL = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: out-params populated on success; `path` is live.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(format!("GetNamedSecurityInfoW failed: {rc}"));
    }

    let mut ea = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_READ | GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u32,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            ptstrName: sid as *mut u16, // for TRUSTEE_IS_SID, this field holds the PSID
        },
    };

    let mut new_dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `ea` and `old_dacl` are live; `new_dacl` is an out-param we free below.
    let rc = unsafe { SetEntriesInAclW(1, &mut ea, old_dacl, &mut new_dacl) };
    if rc != 0 {
        // SAFETY: `sd` came from GetNamedSecurityInfoW and must be LocalFree'd.
        unsafe { LocalFree(sd as _) };
        return Err(format!("SetEntriesInAclW failed: {rc}"));
    }

    // SAFETY: `new_dacl` is the merged DACL; `path` is live.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new_dacl,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: free the buffers allocated by the two calls above.
    unsafe {
        LocalFree(new_dacl as _);
        LocalFree(sd as _);
    }
    if rc != 0 {
        return Err(format!("SetNamedSecurityInfoW failed: {rc}"));
    }
    Ok(())
}
```

- [ ] **Step 2: Add the confined spawn**

Append to `win.rs`. Parses `--plugin-dir <dir> -- <cmd> <args...>`, grants the
read, builds `SECURITY_CAPABILITIES` (no capabilities → no network) into a
`STARTUPINFOEXW`, creates a kill-on-close job, spawns suspended, assigns to the
job, resumes, waits, and returns the child exit code.

```rust
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::SECURITY_CAPABILITIES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
};

/// Quote one argv element per the CRT/CommandLineToArgvW rules and append to
/// `out`. (`CreateProcessW` takes a single command line, not an argv vector.)
fn append_quoted(out: &mut String, arg: &str) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                backslashes = 0;
                out.push('"');
            }
            _ => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(c);
            }
        }
    }
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
}

fn confine_and_run(args: &[OsString]) -> Result<u8, String> {
    // Parse: --plugin-dir <dir> -- <cmd> <args...>
    if args.first().map(|a| a != "--plugin-dir").unwrap_or(true) {
        return Err("usage: --plugin-dir <dir> -- <cmd> [args...]".into());
    }
    let plugin_dir = args.get(1).ok_or("missing <dir> after --plugin-dir")?.clone();
    if args.get(2).map(|a| a != "--").unwrap_or(true) {
        return Err("expected `--` after --plugin-dir <dir>".into());
    }
    let cmd = args.get(3).ok_or("missing <cmd> after --")?.clone();
    let inner: Vec<&OsString> = args.iter().skip(4).collect();

    let sid = ensure_app_container_sid()?;
    grant_appcontainer_read(&plugin_dir, sid)?;

    // Build the command line: argv0 = cmd, then the inner args.
    let mut cmdline = String::new();
    append_quoted(&mut cmdline, &cmd.to_string_lossy());
    for a in &inner {
        cmdline.push(' ');
        append_quoted(&mut cmdline, &a.to_string_lossy());
    }
    let mut cmdline_w = wide(&cmdline);

    // SECURITY_CAPABILITIES with the package SID and NO capabilities → the
    // built-in WFP filter blocks all sockets (no network).
    let mut caps = SECURITY_CAPABILITIES {
        AppContainerSid: sid,
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    // STARTUPINFOEXW with a one-entry attribute list carrying the capabilities.
    let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;

    let mut attr_size: usize = 0;
    // SAFETY: first call with null list returns required size in `attr_size`.
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size) };
    let mut attr_buf = vec![0u8; attr_size];
    si.lpAttributeList = attr_buf.as_mut_ptr() as _;
    // SAFETY: `attr_buf` is sized per the probe call above.
    if unsafe { InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &mut attr_size) } == 0 {
        return Err("InitializeProcThreadAttributeList failed".into());
    }
    // SAFETY: `caps` outlives the CreateProcessW call below.
    let ok = unsafe {
        UpdateProcThreadAttribute(
            si.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut caps as *mut _ as _,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err("UpdateProcThreadAttribute failed".into());
    }

    // Kill-on-close job so the whole tree dies with the launcher.
    // SAFETY: job handle is closed before return.
    let job: HANDLE = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err("CreateJobObjectW failed".into());
    }
    let mut jli: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    jli.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `jli` is live and correctly sized.
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut jli as *mut _ as _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
    }

    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `cmdline_w` is a mutable, NUL-terminated wide buffer; `si` carries
    // a valid attribute list; handles inherited so stdio passes through.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmdline_w.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // bInheritHandles = TRUE → inherit stdio
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            std::ptr::null(),
            &mut si.StartupInfo,
            &mut pi,
        )
    };
    // SAFETY: attribute list no longer needed once the process is created.
    unsafe { DeleteProcThreadAttributeList(si.lpAttributeList) };
    if created == 0 {
        unsafe { CloseHandle(job) };
        return Err("CreateProcessW failed (is the command inside an AppContainer-readable path?)".into());
    }

    // Assign to the job, then resume.
    // SAFETY: both handles valid from CreateProcessW.
    unsafe {
        AssignProcessToJobObject(job, pi.hProcess);
        ResumeThread(pi.hThread);
    }

    // Wait and collect the exit code.
    // SAFETY: `pi.hProcess` valid until we CloseHandle it.
    let mut code: u32 = 1;
    unsafe {
        if WaitForSingleObject(pi.hProcess, INFINITE) == WAIT_OBJECT_0 {
            GetExitCodeProcess(pi.hProcess, &mut code);
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        CloseHandle(job);
    }
    Ok((code & 0xFF) as u8)
}
```

Remove the temporary `confine_and_run` stub from Task 4 if you added one.

- [ ] **Step 2b: Format + build (local gate)**

Run: `just fmt && cargo build -p cairn-sandbox-win`
Expected (macOS): PASS (stub path). Windows gate: compiles after resolving any
`windows-sys 0.61` path/feature mismatches the compiler flags (see caveat).

- [ ] **Step 3: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-sandbox-win/src/win.rs
git commit -m "feat(infra): cairn-sandbox-win confines plugin in AppContainer + job"
```

---

## Task 6: Windows behavioral tests — exec, write-deny, vault-deny

> Windows-gated. Added to `crates/cairn-sandbox-win/tests/confinement.rs`. Each
> test skips (returns early) if `appcontainer_usable()` is false.

**Files:**
- Modify: `crates/cairn-sandbox-win/tests/confinement.rs`

- [ ] **Step 1: Add the exec + stdout test**

```rust
#[test]
fn exec_and_pipe_stdout() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    let tmp = std::env::temp_dir();
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
    let out = Command::new(launcher())
        .arg("--plugin-dir").arg(&tmp)
        .arg("--").arg(&cmd).arg("/c").arg("echo hi")
        .output()
        .expect("spawn launcher");
    assert!(out.status.success(), "the plugin command must be allowed to exec");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hi"),
        "stdout must pipe through the jail: {:?}", out
    );
}
```

- [ ] **Step 2: Add the write-denied test**

```rust
#[test]
fn write_to_host_is_denied() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    let plugin_dir = std::env::temp_dir();
    // A host file OUTSIDE any AppContainer-writable location.
    let target = std::env::temp_dir().join("cairn_sbx_should_not_exist.txt");
    let _ = std::fs::remove_file(&target);
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
    let redirect = format!("echo x> {}", target.display());
    let status = Command::new(launcher())
        .arg("--plugin-dir").arg(&plugin_dir)
        .arg("--").arg(&cmd).arg("/c").arg(&redirect)
        .status()
        .expect("spawn launcher");
    assert!(!status.success(), "writing to the host must fail");
    assert!(!target.exists(), "the host file must not be created");
}
```

- [ ] **Step 3: Add the vault-deny / plugin-dir-allow test**

```rust
#[test]
fn denies_vault_read_but_allows_plugin_dir() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // Two sibling temp dirs: one acts as the plugin dir (granted), one as the
    // vault (never granted → unreadable). Using distinct dirs avoids granting
    // read on the shared temp root.
    let base = std::env::temp_dir().join(format!("cairn_sbx_{}", std::process::id()));
    let plugin_dir = base.join("plugin");
    let vault = base.join("vault");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(plugin_dir.join("own.txt"), b"OWN").unwrap();
    std::fs::write(vault.join("secret.md"), b"SECRET").unwrap();
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");

    let own = plugin_dir.join("own.txt");
    let allowed = Command::new(launcher())
        .arg("--plugin-dir").arg(&plugin_dir)
        .arg("--").arg(&cmd).arg("/c").arg("type").arg(&own)
        .output().expect("spawn");
    assert!(allowed.status.success(), "reading the plugin's own dir must be allowed");
    assert!(String::from_utf8_lossy(&allowed.stdout).contains("OWN"));

    let secret = vault.join("secret.md");
    let denied = Command::new(launcher())
        .arg("--plugin-dir").arg(&plugin_dir)
        .arg("--").arg(&cmd).arg("/c").arg("type").arg(&secret)
        .output().expect("spawn");
    assert!(!denied.status.success(), "reading the vault must be denied");

    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 4: Build + note Windows gate**

Run: `cargo build -p cairn-sandbox-win --tests`
Expected (macOS): the test file is `#![cfg(windows)]`, so it compiles to nothing
locally — `PASS` with no tests run. Windows gate (CI): the three tests pass.

- [ ] **Step 5: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-sandbox-win/tests/confinement.rs
git commit -m "test(infra): AppContainer exec/write-deny/vault-deny behavioral tests"
```

---

## Task 7: Windows behavioral test — network denied

> Windows-gated. AppContainer with no network capability blocks all sockets,
> including loopback (no `LoopbackExempt`), so a connect to a listener on the
> host must fail.

**Files:**
- Modify: `crates/cairn-sandbox-win/tests/confinement.rs`

- [ ] **Step 1: Add the network-denied test**

```rust
#[test]
fn network_is_denied() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // Listen on an ephemeral loopback port from the (unconfined) test process.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let plugin_dir = std::env::temp_dir();
    let cmd = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    // PowerShell connect attempt: exit 0 on success, 1 on failure.
    let script = format!(
        "try {{ (New-Object Net.Sockets.TcpClient).Connect('127.0.0.1',{port}); exit 0 }} catch {{ exit 1 }}"
    );
    let status = Command::new(launcher())
        .arg("--plugin-dir").arg(&plugin_dir)
        .arg("--").arg(&cmd)
        .arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(&script)
        .status()
        .expect("spawn launcher");
    assert!(!status.success(), "an AppContainer with no network capability must not connect");
}
```

- [ ] **Step 2: Build + note Windows gate**

Run: `cargo build -p cairn-sandbox-win --tests`
Expected (macOS): PASS, no tests run (cfg'd out). Windows gate (CI): the test passes.

- [ ] **Step 3: Commit**

```bash
git branch --show-current   # must print: windows-sandbox-port
git add crates/cairn-sandbox-win/tests/confinement.rs
git commit -m "test(infra): AppContainer denies network access"
```

---

## Task 8: Full workspace verification + PR

**Files:** none (verification only)

- [ ] **Step 1: Full local gate**

Run: `just fmt && just lint && just test`
Expected: PASS on macOS — fmt clean, clippy clean, all cross-platform tests
(including `windows_launcher_args_*` and `windows_sandbox_missing_launcher_*`)
green; the `#[cfg(windows)]` tests are skipped locally.

- [ ] **Step 2: Confirm cargo-deny still passes (new direct windows-sys dep)**

Run: `cargo deny check --all-features`
Expected: PASS — `windows-sys 0.61` is already in the tree and is
`MIT OR Apache-2.0`.

- [ ] **Step 3: Push and open the PR (enqueues via merge queue)**

```bash
git branch --show-current   # must print: windows-sandbox-port
git push -u origin windows-sandbox-port
gh pr create --base main --title "feat(infra): Windows plugin sandbox via AppContainer (#62)" \
  --body "$(cat <<'EOF'
Implements the `Sandbox` port on Windows (issue #62), completing the
cross-platform plugin jail after macOS Seatbelt (#60) and Linux bwrap (#61/#65).

Confines trusted plugins in an **AppContainer** set up by a new external
launcher crate `cairn-sandbox-win` (the sole `unsafe` carve-out), invoked
exactly like `sandbox-exec`/`bwrap` — so the `Sandbox` port and `cairn-infra`
stay `unsafe`-free.

- **no host writes**, **no network** (no AppContainer capabilities → WFP block),
  **vault unreadable** (deny-by-default; the vault is never granted) — identical
  to macOS/Linux.
- **Deliberate deviation:** reads are deny-by-default (system DLLs + the plugin's
  own granted dir), i.e. *stricter* than the macOS/Linux broad-read policy.
  Broad user-file reads on Windows would require an on-disk deny-ACE on the
  user's vault, which is explicitly refused. See the design spec.

Design: `docs/superpowers/specs/2026-06-14-plugin-windows-sandbox-design.md`
Plan: `docs/superpowers/plans/2026-06-14-plugin-windows-sandbox.md`

Behavioral confinement tests run on the `windows-latest` CI leg; no new CI
install step is needed (the launcher builds as a workspace member).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Enable auto-merge to enqueue**

```bash
gh pr merge --auto --squash
```

Then watch CI (especially the `test / windows` leg) and address any
`windows-sys` path/feature fixes the Windows compiler surfaces.

---

## Self-review notes

- **Spec coverage:** AppContainer launcher (Tasks 1,4,5) ✓; `windows_launcher_args` seam (Task 2) ✓; `WindowsAppContainerSandbox` + probe + `platform_sandbox()` arm + `RefusingSandbox` shrink + module doc (Task 3) ✓; stable profile name + idempotent grant (Tasks 4,5) ✓; no-caps SECURITY_CAPABILITIES → no network (Task 5,7) ✓; kill-on-close job (Task 5) ✓; deny-by-default vault (Tasks 5,6) ✓; deviation documented (spec + PR body) ✓; tests write/exec/vault/network (Tasks 6,7) ✓; no CI change (Task 8 notes it) ✓; `unsafe` carve-out localized + lints opt-out (Task 1) ✓; `windows-sys` already in lockfile (Task 8 deny check) ✓.
- **Type consistency:** `windows_launcher_args(plugin_dir, cmd, args)`, `WindowsAppContainerSandbox::{default,with_exec,wrap}`, `appcontainer_probe`, `default_launcher_path`, `ensure_app_container_sid`, `grant_appcontainer_read`, `confine_and_run`, `append_quoted`, `run` — names used consistently across tasks.
- **Known risk:** the FFI cannot be compiled on this macOS host; Task 5's `windows-sys 0.61` symbol paths/feature flags may need compiler-guided adjustment on Windows (flagged in the caveat). The call sequence and structs are correct; only import paths are at risk.
```

