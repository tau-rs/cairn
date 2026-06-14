# Capability-Derived Sandbox Profiles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the OS sandbox jail deny outbound network unless the plugin manifest declares a new `net` capability — turning the fixed jail into a capability-derived profile.

**Architecture:** Add a `net` capability string to `cairn-plugin-protocol`; add a typed `SandboxCapabilities` set and a `caps` parameter to the `Sandbox::wrap` port in `cairn-ports`; translate the manifest's `Vec<String>` to the typed set in `cairn-infra` (`plugin_host.rs`); branch the two pure profile builders (`bwrap_args` adds `--share-net`, `seatbelt_profile` emits outbound + DNS rules) on `caps.net`. Read-tightening is explicitly out of scope.

**Tech Stack:** Rust (workspace, `unsafe_code = "forbid"`, `thiserror` at boundaries), macOS Seatbelt (`sandbox-exec` SBPL), Linux bubblewrap (`bwrap`).

**Spec:** `docs/superpowers/specs/2026-06-14-capability-derived-sandbox-profiles-design.md`

---

## File Map

- `crates/cairn-plugin-protocol/src/lib.rs` — **Modify**: add `CAP_NET` constant (near `CAP_EVENTS`, line 32).
- `crates/cairn-ports/src/lib.rs` — **Modify**: add `SandboxCapabilities` struct (near `SandboxError`, line 445); add `caps` param + doc to `Sandbox::wrap` (line 469-475).
- `crates/cairn-infra/src/sandbox.rs` — **Modify**: thread `caps` through `seatbelt_profile` (45), `bwrap_args` (80), all three `wrap` impls (`MacSeatbeltSandbox` 126, `LinuxBwrapSandbox` 217, `RefusingSandbox` 255); add net branches; update tests.
- `crates/cairn-infra/src/plugin_host.rs` — **Modify**: add `sandbox_caps` mapping fn, update `wrap` call site (537), update the `PermissiveSandbox` test double (671).
- `crates/cairn-plugin-example/tests/host.rs` — **Modify**: update the `PermissiveSandbox` test double (10).
- `crates/cairn-plugin-sdk/src/lib.rs` — **Modify**: doc comment listing capabilities — mention `net`.

A commit must compile and pass `cargo test`. Because adding a trait-method parameter breaks every impl and caller at once, Task 2 lands the signature change **and** every mechanical update together (builders accept `caps` but don't yet branch — no behavior change). Tasks 3–4 add the network behavior behind TDD.

---

### Task 1: Add the `net` capability constant

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs:32`
- Test: `crates/cairn-plugin-protocol/src/lib.rs` (inline `#[cfg(test)]`, or add if none — see Step 1)

- [ ] **Step 1: Write the failing test**

Add to (or create) the `#[cfg(test)] mod tests` block at the end of `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
#[test]
fn cap_net_is_the_net_string() {
    assert_eq!(super::CAP_NET, "net");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-plugin-protocol cap_net_is_the_net_string`
Expected: FAIL — `cannot find value CAP_NET in module super` (compile error).

- [ ] **Step 3: Add the constant**

After line 32 (`pub const CAP_EVENTS: &str = "events";`) insert:

```rust
/// Capability: direct outbound network access from the plugin process.
/// Unlike `fs:read`/`fs:write`/`events` (which gate host-RPC callbacks), `net`
/// is consumed by the OS sandbox to open the network in the jail — it gates no
/// host-callback method (see `cairn-infra` `sandbox.rs`).
pub const CAP_NET: &str = "net";
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-plugin-protocol cap_net_is_the_net_string`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: plugin-capability-sandbox-profiles (shared working dir)
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(plugin-protocol): add net capability constant (#63)"
```

---

### Task 2: Thread a typed capability set through the `Sandbox` port (no behavior change)

This task adds the `SandboxCapabilities` type and the `caps` parameter everywhere, keeping the workspace compiling and green. The builders accept `caps` but ignore it; network behavior arrives in Tasks 3–4. The one piece of new *logic* (the manifest → typed-set mapping) is unit-tested first.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (add struct ~445; signature 469-475)
- Modify: `crates/cairn-infra/src/sandbox.rs` (builders 45, 80; impls 126, 217, 255; all test call sites)
- Modify: `crates/cairn-infra/src/plugin_host.rs` (imports 12-23; `sandbox_caps` fn; call site 536-538; `PermissiveSandbox` 671-683)
- Modify: `crates/cairn-plugin-example/tests/host.rs` (imports 3; `PermissiveSandbox` 10-22)
- Test: `crates/cairn-infra/src/plugin_host.rs` (inline test for `sandbox_caps`)

- [ ] **Step 1: Write the failing test for the mapping function**

In the `#[cfg(test)] mod tests` block in `crates/cairn-infra/src/plugin_host.rs`, add:

```rust
#[test]
fn sandbox_caps_sets_net_only_when_declared() {
    use cairn_ports::SandboxCapabilities;
    assert_eq!(
        super::sandbox_caps(&["net".to_string()]),
        SandboxCapabilities { net: true }
    );
    assert_eq!(
        super::sandbox_caps(&["fs:read".to_string(), "events".to_string()]),
        SandboxCapabilities { net: false }
    );
    assert_eq!(super::sandbox_caps(&[]), SandboxCapabilities::default());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra sandbox_caps_sets_net_only_when_declared`
Expected: FAIL — `SandboxCapabilities` and `sandbox_caps` do not exist (compile error).

- [ ] **Step 3a: Add the `SandboxCapabilities` type to the port**

In `crates/cairn-ports/src/lib.rs`, immediately after the `SandboxError` enum (after line 445), add:

```rust
/// OS-sandbox-relevant capabilities a plugin declared, translated from its
/// manifest. Distinct from the host-RPC capabilities in `cairn-plugin-protocol`
/// (`fs:read`/`fs:write`/`events`), which never reach the sandbox. Defaults to
/// the fully locked-down posture (every field `false`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SandboxCapabilities {
    /// Allow outbound network access in the jail. Default `false` (denied).
    pub net: bool,
}
```

- [ ] **Step 3b: Add the `caps` parameter to `Sandbox::wrap`**

In `crates/cairn-ports/src/lib.rs`, change the `wrap` signature (lines 469-475) and update its doc. Replace the doc sentence about network and the signature:

Change the first doc paragraph's tail from
`…and denies direct file-write, network, and further \`exec\`.`
to
`…and denies direct file-write and further \`exec\`. Outbound network is denied unless \`caps.net\` is set.`

Then the signature:

```rust
    fn wrap(
        &self,
        vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
        caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError>;
```

- [ ] **Step 3c: Add the `caps` param to the two pure builders (ignored for now)**

In `crates/cairn-infra/src/sandbox.rs`, change `seatbelt_profile` (line 45) and `bwrap_args` (line 80) signatures to take `caps`, prefixed `_` since unused this task:

```rust
pub(crate) fn seatbelt_profile(
    vault_root: &Path,
    plugin_dir: &Path,
    cmd: &Path,
    _caps: SandboxCapabilities,
) -> String {
```

```rust
pub(crate) fn bwrap_args(
    vault_root: &Path,
    plugin_dir: &Path,
    cmd: &Path,
    _caps: SandboxCapabilities,
) -> Vec<OsString> {
```

Add the import at the top of `sandbox.rs` (the `use cairn_ports::{Sandbox, SandboxError};` line near line 10):

```rust
use cairn_ports::{Sandbox, SandboxCapabilities, SandboxError};
```

- [ ] **Step 3d: Update the three real `wrap` impls to accept and forward `caps`**

In `crates/cairn-infra/src/sandbox.rs`:

`MacSeatbeltSandbox::wrap` (line 126) — add the param and forward to the builder:

```rust
    fn wrap(
        &self,
        vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
        caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
```

and change the `seatbelt_profile(&vault_root_abs, &dir, &cmd_abs)` call (line 149) to `seatbelt_profile(&vault_root_abs, &dir, &cmd_abs, caps)`.

`LinuxBwrapSandbox::wrap` (line 217) — add the param:

```rust
    fn wrap(
        &self,
        vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
        caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
```

and change `bwrap_args(&vault_root_abs, &dir, &cmd_abs)` (line 244) to `bwrap_args(&vault_root_abs, &dir, &cmd_abs, caps)`.

`RefusingSandbox::wrap` (line 255) — add the param, prefixed `_`:

```rust
    fn wrap(
        &self,
        _vault_root: &Path,
        _plugin_dir: &Path,
        _cmd: &Path,
        _args: &[String],
        _caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
```

- [ ] **Step 3e: Add the `sandbox_caps` mapping fn and update the call site in `plugin_host.rs`**

In `crates/cairn-infra/src/plugin_host.rs`, add `CAP_NET` to the `cairn_plugin_protocol` import (line 16, alongside `CAP_EVENTS, CAP_FS_READ`) and `SandboxCapabilities` to the `cairn_ports` import (line 20-23, alongside `Sandbox`).

Add this free function near the top of the module (e.g. just above `fn spawn_plugin`, line 502):

```rust
/// Translate a manifest's self-declared capability strings into the typed
/// OS-sandbox capability set. Only sandbox-driving capabilities are mapped;
/// host-RPC capabilities (`fs:read`/`fs:write`/`events`) are irrelevant here.
fn sandbox_caps(caps: &[String]) -> SandboxCapabilities {
    SandboxCapabilities {
        net: caps.iter().any(|c| c == CAP_NET),
    }
}
```

Change the call site (lines 536-538) to:

```rust
        let caps = sandbox_caps(&manifest.engine.capabilities);
        let mut command = sandbox
            .wrap(vault_root, plugin_dir, &cmd_path, &manifest.engine.args, caps)
            .map_err(adapt)?;
```

- [ ] **Step 3f: Update the `PermissiveSandbox` test double in `plugin_host.rs`**

In `crates/cairn-infra/src/plugin_host.rs` (lines 671-683), add the param. The test module already imports from `cairn_ports`; add `SandboxCapabilities` to that test import or use a fully-qualified path. Update:

```rust
    impl Sandbox for PermissiveSandbox {
        fn wrap(
            &self,
            _vault_root: &Path,
            _dir: &Path,
            cmd: &Path,
            args: &[String],
            _caps: cairn_ports::SandboxCapabilities,
        ) -> Result<Command, SandboxError> {
            let mut c = Command::new(cmd);
            c.args(args);
            Ok(c)
        }
    }
```

- [ ] **Step 3g: Update the `PermissiveSandbox` test double in the example crate**

In `crates/cairn-plugin-example/tests/host.rs`, add `SandboxCapabilities` to the import on line 3 (`use cairn_ports::{… Sandbox, SandboxError, …}`) and update the impl (lines 10-22):

```rust
impl Sandbox for PermissiveSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        _dir: &Path,
        cmd: &Path,
        args: &[String],
        _caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
        let mut c = Command::new(cmd);
        c.args(args);
        Ok(c)
    }
}
```

- [ ] **Step 3h: Update every `.wrap(...)` and builder call site in `sandbox.rs` tests**

In `crates/cairn-infra/src/sandbox.rs`, the `#[cfg(test)] mod tests` block calls the builders and `wrap` with the old arity. Pass `SandboxCapabilities::default()` as the new trailing argument to each:

- `seatbelt_profile(...)` calls at lines ~294, ~319 → append `, SandboxCapabilities::default()`.
- `bwrap_args(...)` call at line ~340 → append `, SandboxCapabilities::default()`.
- Every `.wrap(...)` call (lines ~333, ~373, ~390-396, ~422, ~436-442, ~464-470, ~493-501, ~510-518, ~550-556, ~585-591, ~618-626, ~635-643) → append `, SandboxCapabilities::default()` as the final argument.

`SandboxCapabilities` is already in scope via the top-of-file `use cairn_ports::{Sandbox, SandboxCapabilities, SandboxError};` plus the test module's `use super::*;`.

- [ ] **Step 4: Run the full crate test suites to verify green (no behavior change)**

Run: `cargo test -p cairn-ports -p cairn-infra -p cairn-plugin-protocol -p cairn-plugin-example`
Expected: PASS — including `sandbox_caps_sets_net_only_when_declared`. Existing sandbox tests still pass because the builders ignore `caps`.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy -p cairn-ports -p cairn-infra --all-targets -- -D warnings
git branch   # confirm: plugin-capability-sandbox-profiles
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/sandbox.rs \
        crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(infra): thread declared capabilities into the Sandbox port (#63)"
```

---

### Task 3: Linux `bwrap_args` opens the network only when `net` is declared

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs` — `bwrap_args` (line 80) + tests
- Test: `crates/cairn-infra/src/sandbox.rs` (pure, any platform)

- [ ] **Step 1: Write the failing pure tests**

In the `#[cfg(test)] mod tests` block of `crates/cairn-infra/src/sandbox.rs`, add:

```rust
#[test]
fn bwrap_args_omits_share_net_without_net_cap() {
    let a = bwrap_args(
        Path::new("/cairn"),
        Path::new("/cairn/.cairn/plugins/p"),
        Path::new("/cairn/.cairn/plugins/p/bin"),
        SandboxCapabilities::default(),
    );
    let s: Vec<String> = a.iter().map(|o| o.to_string_lossy().into_owned()).collect();
    assert!(!s.iter().any(|x| x == "--share-net"), "default jail must have no network");
}

#[test]
fn bwrap_args_adds_share_net_after_unshare_all_with_net_cap() {
    let a = bwrap_args(
        Path::new("/cairn"),
        Path::new("/cairn/.cairn/plugins/p"),
        Path::new("/cairn/.cairn/plugins/p/bin"),
        SandboxCapabilities { net: true },
    );
    let s: Vec<String> = a.iter().map(|o| o.to_string_lossy().into_owned()).collect();
    let unshare = s.iter().position(|x| x == "--unshare-all").expect("--unshare-all present");
    assert_eq!(
        s.get(unshare + 1).map(String::as_str),
        Some("--share-net"),
        "--share-net must immediately follow --unshare-all"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra bwrap_args_adds_share_net_after_unshare_all_with_net_cap`
Expected: FAIL — `--share-net` is never emitted (assertion fails). (The `omits` test passes already.)

- [ ] **Step 3: Implement the net branch**

In `bwrap_args`, rename `_caps` to `caps` in the signature. Today the vector is built as a single `vec![ … ]` literal ending `--unshare-all`, `--die-with-parent`, `--`, `cmd`. Restructure so `--share-net` is conditionally inserted right after `--unshare-all`. Replace the `vec![ … ]` construction (lines 84-101) with:

```rust
    let mut v = vec![
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
    ];
    // `--unshare-all` drops the network namespace; re-share it only when the
    // plugin declared `net`. (bwrap cannot scope this to outbound-only; the
    // whole host namespace is shared — see the design's platform-asymmetry note.)
    if caps.net {
        v.push(OsString::from("--share-net"));
    }
    v.push(OsString::from("--die-with-parent"));
    v.push(OsString::from("--"));
    v.push(cmd);
    v
```

Also update the `bwrap_args` doc comment (lines 63-79): change the `--unshare-all` line note to mention that `--share-net` is appended when `net` is declared.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra bwrap_args`
Expected: PASS — all `bwrap_args*` tests, including the existing `bwrap_args_binds_root_masks_vault_reexposes_plugin_dir_and_disables_net` (which uses `default()` → no `--share-net`, still matches its expected vector).

- [ ] **Step 5: Commit**

```bash
git branch
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): open Linux jail network only on declared net cap (#63)"
```

---

### Task 4: macOS `seatbelt_profile` allows outbound network only when `net` is declared

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs` — `seatbelt_profile` (line 45) + tests
- Test: `crates/cairn-infra/src/sandbox.rs` (pure, any platform)

- [ ] **Step 1: Write the failing pure tests**

Add to the test module:

```rust
#[test]
fn seatbelt_denies_network_without_net_cap() {
    let p = seatbelt_profile(
        &PathBuf::from("/cairn"),
        &PathBuf::from("/cairn/.cairn/plugins/p"),
        &PathBuf::from("/cairn/.cairn/plugins/p/bin"),
        SandboxCapabilities::default(),
    );
    assert!(p.contains("(deny network*)"));
    assert!(!p.contains("network-outbound"));
}

#[test]
fn seatbelt_allows_outbound_network_with_net_cap() {
    let p = seatbelt_profile(
        &PathBuf::from("/cairn"),
        &PathBuf::from("/cairn/.cairn/plugins/p"),
        &PathBuf::from("/cairn/.cairn/plugins/p/bin"),
        SandboxCapabilities { net: true },
    );
    assert!(p.contains("(allow network-outbound)"));
    assert!(p.contains("com.apple.mDNSResponder"), "DNS resolution must be permitted");
    assert!(!p.contains("(deny network*)"), "the blanket network deny must be gone");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra seatbelt_allows_outbound_network_with_net_cap`
Expected: FAIL — profile still contains the hard-coded `(deny network*)` and no `network-outbound`.

- [ ] **Step 3: Implement the net branch**

In `seatbelt_profile`, rename `_caps` to `caps`. Replace the `(deny network*)\n` line inside the `format!` with an interpolated `{net}` segment computed from `caps.net`. Before the `format!`, add:

```rust
    let net = if caps.net {
        // Outbound only (no inbound bind). `system-socket` + the mDNSResponder
        // mach-lookup are required for DNS resolution under Seatbelt; without
        // them a `net` plugin could open sockets but never resolve a hostname.
        "(allow network-outbound)\n\
         (allow system-socket)\n\
         (allow mach-lookup (global-name \"com.apple.mDNSResponder\"))\n"
    } else {
        "(deny network*)\n"
    };
```

Then change the `(deny network*)\n` line in the `format!` literal to `{net}` (the `format!` already interpolates `vault`, `dir`, `cmd`; add `net`). The relevant lines (56-58) become:

```rust
         (deny file-write*)\n\
         {net}\
         (deny process-exec*)\n\
```

Update the `seatbelt_profile` doc comment (lines 38-44) to note that network is denied unless `net` is declared, in which case outbound + DNS are allowed.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra seatbelt`
Expected: PASS — the new two tests plus the existing `profile_denies_write_network_and_interpolates_paths` (which uses `default()` → still contains `(deny network*)`).

- [ ] **Step 5: Commit**

```bash
git branch
git add crates/cairn-infra/src/sandbox.rs
git commit -m "feat(infra): allow macOS jail outbound network only on declared net cap (#63)"
```

---

### Task 5: Behavioral net tests (cfg-gated per OS)

Proves the jail actually denies/allows network end-to-end using loopback only (no real outbound connectivity needed in CI). A parent `TcpListener` on `127.0.0.1:0` is the target; a jailed `bash -c 'exec 3<>/dev/tcp/127.0.0.1/<port>'` probe succeeds iff the jail has network.

**Files:**
- Modify: `crates/cairn-infra/src/sandbox.rs` — add cfg-gated tests in the test module
- Test: same file

- [ ] **Step 1: Write the Linux behavioral tests**

In the `#[cfg(target_os = "linux")]` test area (near line 526, after `linux_bwrap_usable`), add:

```rust
#[cfg(target_os = "linux")]
#[test]
fn bwrap_denies_network_without_net_cap() {
    if !linux_bwrap_usable() {
        eprintln!("skipping: bwrap/userns unavailable");
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let status = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/bash"),
            &["-c".to_string(), format!("exec 3<>/dev/tcp/127.0.0.1/{port}")],
            SandboxCapabilities::default(),
        )
        .expect("bwrap present")
        .status()
        .expect("spawn under bwrap");
    assert!(!status.success(), "no-net jail must not reach loopback (fresh netns, lo down)");
}

#[cfg(target_os = "linux")]
#[test]
fn bwrap_allows_network_with_net_cap() {
    if !linux_bwrap_usable() {
        eprintln!("skipping: bwrap/userns unavailable");
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Accept in a thread so the connect completes.
    let handle = std::thread::spawn(move || {
        let _ = listener.accept();
    });
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let status = LinuxBwrapSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/bash"),
            &["-c".to_string(), format!("exec 3<>/dev/tcp/127.0.0.1/{port}")],
            SandboxCapabilities { net: true },
        )
        .expect("bwrap present")
        .status()
        .expect("spawn under bwrap");
    let _ = handle.join();
    assert!(status.success(), "net jail must reach loopback via --share-net");
}
```

- [ ] **Step 2: Write the macOS behavioral tests**

In the `#[cfg(target_os = "macos")]` test area (near line 378), add:

```rust
#[cfg(target_os = "macos")]
#[test]
fn seatbelt_denies_network_without_net_cap() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let status = MacSeatbeltSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/bash"),
            &["-c".to_string(), format!("exec 3<>/dev/tcp/127.0.0.1/{port}")],
            SandboxCapabilities::default(),
        )
        .expect("sandbox-exec present")
        .status()
        .expect("spawn under sandbox");
    assert!(!status.success(), "no-net jail must deny the loopback connect");
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_allows_network_with_net_cap() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let _ = listener.accept();
    });
    let vault = tempfile::tempdir().unwrap();
    let plugin_dir = vault.path().join(".cairn/plugins/p");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let status = MacSeatbeltSandbox::default()
        .wrap(
            vault.path(),
            &plugin_dir,
            Path::new("/bin/bash"),
            &["-c".to_string(), format!("exec 3<>/dev/tcp/127.0.0.1/{port}")],
            SandboxCapabilities { net: true },
        )
        .expect("sandbox-exec present")
        .status()
        .expect("spawn under sandbox");
    let _ = handle.join();
    assert!(status.success(), "net jail must permit the outbound loopback connect");
}
```

- [ ] **Step 3: Run the behavioral tests on the host platform**

Run (macOS dev host): `cargo test -p cairn-infra seatbelt_denies_network_without_net_cap seatbelt_allows_network_with_net_cap -- --nocapture`
Expected: PASS. (Linux variants run in CI; if developing on Linux, run the `bwrap_*_net_cap` names instead.)

If a behavioral test misbehaves, debug via `superpowers:systematic-debugging` — likely causes: `/bin/bash` absent (use `which bash` path), or macOS DNS/socket rules insufficient (the loopback test needs only `network-outbound` + `system-socket`, no DNS — if the deny test unexpectedly *passes the connect*, re-check the `{net}` interpolation).

- [ ] **Step 4: Commit**

```bash
git branch
git add crates/cairn-infra/src/sandbox.rs
git commit -m "test(infra): behavioral net allow/deny tests for both jails (#63)"
```

---

### Task 6: Document the `net` capability

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs` (capability list doc comment)

- [ ] **Step 1: Locate the capability documentation**

Run: `grep -n "fs:read\|capabilities\|fs:write\|events" crates/cairn-plugin-sdk/src/lib.rs`
Read the surrounding doc comment that enumerates the host-RPC capabilities.

- [ ] **Step 2: Add the `net` entry**

In the SDK doc comment that lists `fs:read` / `fs:write` / `events`, add a line for `net`, making the distinction explicit, e.g.:

```rust
//! - `net` — open outbound network access in the OS sandbox. Unlike the
//!   three above, `net` gates no host-callback method; it is consumed by the
//!   sandbox to permit the plugin process to make outbound connections.
```

(Match the exact comment style/indentation already in the file.)

- [ ] **Step 3: Build docs to verify no broken intra-doc links**

Run: `cargo doc -p cairn-plugin-sdk --no-deps`
Expected: builds without warnings.

- [ ] **Step 4: Commit**

```bash
git branch
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "docs(plugin-sdk): document the net sandbox capability (#63)"
```

---

### Task 7: Full-workspace verification

- [ ] **Step 1: Run the whole suite + lints**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
Expected: all green. (Linux behavioral net tests run in CI on the ubuntu job, which already installs `bubblewrap`.)

- [ ] **Step 2: Confirm spec coverage**

Re-read `docs/superpowers/specs/2026-06-14-capability-derived-sandbox-profiles-design.md` against the diff. Every section maps to a task: vocabulary (T1), typed set + port (T2), Linux builder (T3), macOS builder (T4), behavioral tests (T5), docs (T6). Read-tightening remains out of scope — confirm no read rules changed.

- [ ] **Step 3: Push and open PR (only when asked)**

Per the merge-queue workflow, do not manually update the PR branch; use auto-merge to enqueue. Open against `main`:

```bash
git push -u origin plugin-capability-sandbox-profiles
gh pr create --base main --title "feat(infra): capability-derived sandbox profiles (#63)" --body "..."
```

---

## Self-Review

- **Spec coverage:** vocabulary → T1; `SandboxCapabilities` + port signature + mapping → T2; Linux `--share-net` → T3; macOS outbound+DNS → T4; pure + behavioral tests → T2/T3/T4/T5; docs → T6; out-of-scope read-tightening untouched → verified in T7. No gaps.
- **Type consistency:** `SandboxCapabilities { net: bool }` and `sandbox_caps(&[String]) -> SandboxCapabilities` are used identically across T2–T5. `CAP_NET = "net"` (T1) is the only string, mapped once in `sandbox_caps` (T2). Builders `seatbelt_profile`/`bwrap_args` keep their names, gaining a trailing `caps` arg consistently.
- **Compile-green invariant:** the arity change lands wholesale in T2 (all impls + call sites + both `PermissiveSandbox` doubles), so every commit compiles.
- **No placeholders:** every code step shows complete code; the only `grep`-to-locate step (T6) targets a doc comment whose exact text varies, with the inserted content fully specified.
