# Plugin Linux Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Jail trusted plugins on Linux under a `bwrap` (bubblewrap) sandbox — read-only filesystem, vault masked, no network — and refuse cleanly where no working jail can be applied.

**Architecture:** Add a `LinuxBwrapSandbox` adapter next to `MacSeatbeltSandbox` in `crates/cairn-infra/src/sandbox.rs`, implementing the existing `Sandbox` port (`wrap() -> Command`). It wraps the plugin command with an external `bwrap … -- <cmd>` launcher — the exact Linux analogue of the macOS `sandbox-exec` wrapper. No port changes, no new Rust dependencies, no `unsafe`. A one-time cached probe detects hosts where unprivileged user namespaces are disabled and refuses there.

**Tech Stack:** Rust, bubblewrap (`bwrap`, runtime dependency), `thiserror` (existing `SandboxError`), `tempfile` (dev). GitHub Actions CI.

---

## Background — read before starting

Read `crates/cairn-infra/src/sandbox.rs` end to end. The macOS backend is the
template you are mirroring:

- `MacSeatbeltSandbox { exec: PathBuf }` with `Default` (`/usr/bin/sandbox-exec`)
  and `with_exec()` for tests.
- A **pure** profile builder (`seatbelt_profile`) tested without spawning.
- `wrap()`: existence-check the launcher → canonicalize the three paths → build
  a `Command`. Returns `SandboxError::Unavailable` on any failure.
- `platform_sandbox()` factory chooses the backend by `cfg(target_os)`.
- The behavioral tests are `#[cfg(target_os = "macos")]` and spawn real
  processes under the jail.

The `Sandbox` port (`crates/cairn-ports/src/lib.rs`), `ProcessPluginHost`
(`plugin_host.rs`, already threads `&dyn Sandbox`), and the daemon (already
calls `platform_sandbox()`) need **no changes**.

### Two bubblewrap semantics that differ from Seatbelt (important for tests)

1. **Writes are isolated, not denied, inside the masked vault.** The vault is
   mounted `--tmpfs`, which is *writable in the namespace* but discarded and
   invisible to the host. So a write into the vault path *succeeds* in-namespace.
   The real security property — "the plugin cannot modify the host filesystem" —
   is enforced by `--ro-bind / /` making the host root read-only. Therefore the
   deny-write test must target a **read-only non-vault path** (write → `EROFS`),
   not a path under the vault.
2. **Reads of the vault are masked, not EPERM'd.** The `--tmpfs` over the vault
   is empty, so `cat <vault>/secret.md` fails with "No such file" — the secret
   is unreadable, which is the property we test.

### Path handling

`bwrap` arguments are passed as distinct `OsString` argv entries (via
`Command::args`), so — unlike the single-string SBPL profile — there is **no
quoting and no `to_string_lossy`**. Do **not** reuse `sbpl_quote`.

---

## File Structure

- **Modify** `crates/cairn-infra/src/sandbox.rs` — add the `OsString`/`OnceLock`
  imports, the pure `bwrap_args` builder, the `bwrap_probe` helper, the
  `LinuxBwrapSandbox` adapter, extend `platform_sandbox()`, update the module
  doc comment, and add tests. This is the only source file that changes.
- **Modify** `.github/workflows/ci.yml` — install `bubblewrap` on the Linux
  `test` matrix leg so the behavioral tests execute instead of skipping.

---

## Task 1: Pure `bwrap_args` builder

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs`

This builds the bubblewrap argument vector from three already-canonicalized
paths. Pure and platform-independent, so it is tested everywhere with a single
exact-equality assertion.

- [ ] **Step 1: Add the `OsString` import**

At the top of `crates/cairn-infra/src/sandbox.rs`, the imports are currently:

```rust
use std::path::{Path, PathBuf};
use std::process::Command;
```

Add the `OsString` import so the block reads:

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
```

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of the file:

```rust
#[test]
fn bwrap_args_binds_root_masks_vault_reexposes_plugin_dir_and_disables_net() {
    let a = bwrap_args(
        Path::new("/cairn"),
        Path::new("/cairn/.cairn/plugins/p"),
        Path::new("/cairn/.cairn/plugins/p/bin"),
    );
    let s: Vec<String> = a.iter().map(|o| o.to_string_lossy().into_owned()).collect();
    assert_eq!(
        s,
        vec![
            "--ro-bind", "/", "/",
            "--tmpfs", "/cairn",
            "--ro-bind", "/cairn/.cairn/plugins/p", "/cairn/.cairn/plugins/p",
            "--dev", "/dev",
            "--proc", "/proc",
            "--unshare-all",
            "--die-with-parent",
            "--",
            "/cairn/.cairn/plugins/p/bin",
        ]
    );
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cairn-infra bwrap_args_binds_root -- --nocapture`
Expected: FAIL — `cannot find function bwrap_args in this scope`.

- [ ] **Step 4: Implement the builder**

Add at module scope (place it just above `pub(crate) fn seatbelt_profile`):

```rust
/// Build the bubblewrap argument vector that jails a plugin command.
///
/// Mounts a broad read-only root (`--ro-bind / /`), masks the vault with an
/// empty tmpfs, re-exposes the plugin's own directory read-only on top, drops
/// the network and other namespaces (`--unshare-all`), and ties the jail's
/// lifetime to the host (`--die-with-parent`). The vector ends with `--` and
/// `cmd`; the caller appends the plugin's own arguments after it.
///
/// Ordering is significant: the vault `--tmpfs` must precede the plugin-dir
/// `--ro-bind` so the re-exposed plugin dir is layered on top of the mask.
///
/// All three paths are expected to be canonical absolute paths. They are
/// emitted as distinct `OsString` argv entries, so no quoting is required and a
/// non-UTF-8 path survives intact.
pub(crate) fn bwrap_args(vault_root: &Path, plugin_dir: &Path, cmd: &Path) -> Vec<OsString> {
    let vault = vault_root.as_os_str().to_os_string();
    let dir = plugin_dir.as_os_str().to_os_string();
    let cmd = cmd.as_os_str().to_os_string();
    vec![
        OsString::from("--ro-bind"),
        OsString::from("/"),
        OsString::from("/"),
        OsString::from("--tmpfs"),
        vault,
        OsString::from("--ro-bind"),
        dir.clone(),
        dir,
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--unshare-all"),
        OsString::from("--die-with-parent"),
        OsString::from("--"),
        cmd,
    ]
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p cairn-infra bwrap_args_binds_root`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): pure bwrap argv builder for the Linux sandbox (#61)"
```

---

## Task 2: `bwrap_probe` + `LinuxBwrapSandbox::wrap` + factory

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs`

Adds the adapter. `wrap()` mirrors the macOS structure but inserts a one-time
cached user-namespace probe between the existence check and canonicalization.
The "bwrap missing → Unavailable" test is platform-independent because the
`exists()` check runs before the probe.

- [ ] **Step 1: Add the `OnceLock` import**

Extend the import block from Task 1 so it reads:

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
```

- [ ] **Step 2: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn linux_sandbox_missing_bwrap_is_unavailable() {
    let s = LinuxBwrapSandbox::with_exec(PathBuf::from("/nonexistent/bwrap"));
    let err = s
        .wrap(Path::new("/"), Path::new("/"), Path::new("/bin/true"), &[])
        .unwrap_err();
    assert!(matches!(err, SandboxError::Unavailable(_)));
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cairn-infra linux_sandbox_missing_bwrap`
Expected: FAIL — `cannot find type LinuxBwrapSandbox in this scope`.

- [ ] **Step 4: Implement the probe and adapter**

Add at module scope, just above the `RefusingSandbox` definition:

```rust
/// One-time probe that confirms unprivileged user namespaces actually work on
/// this host. `bwrap` exists on many systems where userns is disabled by policy
/// (some hardened/older distros); without this probe such a host would surface a
/// confusing spawn-time error instead of a clean refusal. Runs a trivial jail
/// over `/bin/true` and reports the bwrap stderr on failure.
fn bwrap_probe(exec: &Path) -> Result<(), String> {
    let out = Command::new(exec)
        .args(["--ro-bind", "/", "/", "--unshare-all", "--", "/bin/true"])
        .output()
        .map_err(|e| format!("spawn {}: {e}", exec.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "user namespaces unavailable: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Linux bubblewrap backend: runs the plugin under `bwrap <flags> -- <cmd>`.
/// The external launcher applies the jail, mirroring how `MacSeatbeltSandbox`
/// uses `sandbox-exec`.
pub struct LinuxBwrapSandbox {
    /// Path to the `bwrap` binary (overridable in tests).
    exec: PathBuf,
    /// Cached result of the one-time userns probe (see [`bwrap_probe`]).
    probe: OnceLock<Result<(), String>>,
}

impl Default for LinuxBwrapSandbox {
    fn default() -> Self {
        Self {
            exec: PathBuf::from("/usr/bin/bwrap"),
            probe: OnceLock::new(),
        }
    }
}

impl LinuxBwrapSandbox {
    /// Construct with an explicit `bwrap` path (tests).
    pub fn with_exec(exec: PathBuf) -> Self {
        Self {
            exec,
            probe: OnceLock::new(),
        }
    }
}

impl Sandbox for LinuxBwrapSandbox {
    fn wrap(
        &self,
        vault_root: &Path,
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
        if let Err(e) = self.probe.get_or_init(|| bwrap_probe(&self.exec)) {
            return Err(SandboxError::Unavailable(e.clone()));
        }
        // bwrap binds/masks match canonical absolute paths.
        let vault_root_abs = vault_root
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize vault root: {e}")))?;
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let mut c = Command::new(&self.exec);
        c.args(bwrap_args(&vault_root_abs, &dir, &cmd_abs)).args(args);
        Ok(c)
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p cairn-infra linux_sandbox_missing_bwrap`
Expected: PASS.

- [ ] **Step 6: Extend the `platform_sandbox()` factory**

Replace the existing factory body:

```rust
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

with:

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
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(RefusingSandbox)
    }
}
```

- [ ] **Step 7: Update the module doc comment**

The file's first lines currently read:

```rust
//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; other platforms refuse (no backend yet — see issue #40).
```

Replace with:

```rust
//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; Linux uses bubblewrap via `bwrap`; other platforms refuse
//! (no backend yet — see issues #40, #62).
```

- [ ] **Step 8: Verify the crate builds and the suite passes**

Run: `cargo test -p cairn-infra`
Expected: PASS (existing tests + the two new ones). On a non-macOS, non-Linux
host the `RefusingSandbox` test still passes.

- [ ] **Step 9: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): Linux bwrap sandbox adapter with userns probe (#61)"
```

---

## Task 3: Linux behavioral tests

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs`

Real-process tests under the jail, gated `#[cfg(target_os = "linux")]` and
skipped when `bwrap`/userns is unavailable so they never fail on a host that
cannot sandbox. They mirror the macOS behavioral tests, adjusted for the two
bubblewrap semantics noted in the Background section.

- [ ] **Step 1: Add a skip-guard helper and the deny-write test**

Add to the `tests` module:

```rust
#[cfg(target_os = "linux")]
fn linux_bwrap_usable() -> bool {
    let exec = Path::new("/usr/bin/bwrap");
    exec.exists() && bwrap_probe(exec).is_ok()
}

#[cfg(target_os = "linux")]
#[test]
fn bwrap_denies_write_to_host_filesystem() {
    if !linux_bwrap_usable() {
        eprintln!("skipping: bwrap/userns unavailable");
        return;
    }
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    // A non-vault path: it lives under the read-only `--ro-bind / /` root, so a
    // write must fail with EROFS. (A path under the vault would land in the
    // writable-but-isolated tmpfs and succeed in-namespace — see plan notes.)
    let outside = tempfile::tempdir().unwrap();
    let escaped = outside.path().join("escaped.txt");

    let mut cmd = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/usr/bin/touch"),
            &[escaped.to_string_lossy().into_owned()],
        )
        .expect("bwrap present and userns usable");
    let status = cmd.status().expect("spawn under bwrap");

    assert!(!status.success(), "write to the read-only host fs must fail");
    assert!(!escaped.exists(), "the file must not have been created on the host");
}
```

- [ ] **Step 2: Run it to verify it passes (or skips)**

Run: `cargo test -p cairn-infra bwrap_denies_write_to_host_filesystem -- --nocapture`
Expected: PASS. On a Linux host with bubblewrap + userns it exercises the jail;
otherwise it prints "skipping" and passes. (On macOS the test is not compiled.)

- [ ] **Step 3: Add the allow-exec / pipe-stdout test**

Add to the `tests` module:

```rust
#[cfg(target_os = "linux")]
#[test]
fn bwrap_allows_plugin_to_exec_and_pipe_stdout() {
    if !linux_bwrap_usable() {
        eprintln!("skipping: bwrap/userns unavailable");
        return;
    }
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let output = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/echo"),
            &["hi".to_string()],
        )
        .expect("bwrap present and userns usable")
        .output()
        .expect("spawn under bwrap");

    assert!(output.status.success(), "the plugin command must be allowed to exec");
    assert_eq!(output.stdout, b"hi\n", "stdout must pipe through the jail");
}
```

- [ ] **Step 4: Run it**

Run: `cargo test -p cairn-infra bwrap_allows_plugin_to_exec_and_pipe_stdout`
Expected: PASS (or skip).

- [ ] **Step 5: Add the deny-vault-read / allow-plugin-dir test**

Add to the `tests` module:

```rust
#[cfg(target_os = "linux")]
#[test]
fn bwrap_denies_reading_vault_but_allows_plugin_dir() {
    if !linux_bwrap_usable() {
        eprintln!("skipping: bwrap/userns unavailable");
        return;
    }
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(vault.path().join("secret.md"), b"SECRET").unwrap();
    std::fs::write(plugin_dir.join("own.txt"), b"OWN").unwrap();

    // A sibling vault note: masked by the empty tmpfs, so the read fails.
    let secret = vault.path().join("secret.md");
    let denied = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/cat"),
            &[secret.to_string_lossy().into_owned()],
        )
        .expect("bwrap present")
        .output()
        .expect("spawn");
    assert!(!denied.status.success(), "reading a vault file must be denied");

    // The plugin's own dir is re-bound on top of the mask: read succeeds.
    let own = plugin_dir.join("own.txt");
    let allowed = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/cat"),
            &[own.to_string_lossy().into_owned()],
        )
        .expect("bwrap present")
        .output()
        .expect("spawn");
    assert!(allowed.status.success(), "reading the plugin's own dir must be allowed");
    assert_eq!(allowed.stdout, b"OWN");
}
```

- [ ] **Step 6: Run the full crate suite**

Run: `cargo test -p cairn-infra`
Expected: PASS — all sandbox tests, with the Linux behavioral ones either
exercising the jail or skipping.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-infra/src/sandbox.rs
git commit -m "test(infra): Linux bwrap sandbox behavioral tests (#61)"
```

---

## Task 4: Install bubblewrap in CI

**Files:**
- Modify: `.github/workflows/ci.yml`

Make the Linux leg of the `test` matrix install `bwrap` so the behavioral tests
run for real instead of skipping. GitHub's `ubuntu-latest` runners permit
unprivileged user namespaces, so the probe succeeds there.

- [ ] **Step 1: Add the install step**

In the `test` job, the steps currently end with:

```yaml
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: ${{ matrix.os }}
          with-nextest: true
          with-sccache: true
          with-mold: true
          with-just: true
      - run: just test
```

Insert an install step between `setup-rust` and `just test`:

```yaml
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: ${{ matrix.os }}
          with-nextest: true
          with-sccache: true
          with-mold: true
          with-just: true
      - name: Install bubblewrap (Linux sandbox backend)
        if: matrix.os == 'ubuntu-latest'
        run: sudo apt-get update && sudo apt-get install -y bubblewrap
      - run: just test
```

- [ ] **Step 2: Validate the workflow YAML**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('ok')"`
Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: install bubblewrap on the Linux test leg (#61)"
```

---

## Final verification

- [ ] **Run the whole workspace suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Clippy clean (the workspace forbids `unsafe_code`; this change adds none)**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Confirm no `unsafe` was introduced**

Run: `git diff origin/main... -- crates/cairn-infra/src/sandbox.rs | grep -n "unsafe" || echo "no unsafe — good"`
Expected: `no unsafe — good`.
