# Capability-Derived Sandbox Profiles — Design

> Issue #63. Follow-up to #40 (capability vocabulary), #60 (macOS Seatbelt
> backend), #61/#65 (Linux bubblewrap backend). Replaces the **fixed** sandbox
> jail with one **derived from the plugin manifest's declared capabilities**.

## Goal

Make the OS sandbox profile a function of a plugin's declared capabilities
instead of a constant. Concretely for this slice: the jail denies outbound
network **unless** the plugin declares a `net` capability. No port-shape
rewrite, no new Rust dependencies, no `unsafe` (`unsafe_code = "forbid"`).

## Prerequisite correction (important)

Issue #63's BLOCKED-BY note assumed #40 introduced a `net`/`exec`/`read-scope`
capability vocabulary in the manifest. **It did not.** What #40 shipped is a
*disjoint* capability set that gates the **host-RPC callback surface**, not the
OS sandbox:

- `cairn-plugin-protocol/src/lib.rs`: `EngineSection.capabilities: Vec<String>`.
- Defined values: `fs:read`, `fs:write`, `events` — consumed by
  `plugin_host::service_callback` to allow/deny host callbacks
  (readNote/writeNote/search/listNotes/events).
- The manifest doc comment is explicit that this gating "is *not* a sandbox and
  does not constrain what the spawned plugin process does directly (network,
  filesystem, exec)."
- `net` / `exec` / `read-scope` exist nowhere; ADR-0008 lists them only as
  deferred future capabilities.

The two mechanisms are orthogonal today: `Sandbox::wrap` never sees the
manifest's capability list. **This design therefore introduces the first
sandbox-driving capability (`net`) itself** — the work the issue expected #40 to
have delivered — and threads the declared set into the sandbox port.

## Trust model (stated honestly)

Capabilities are **self-declared** in the plugin's own manifest, and the project
treats an approved plugin as **fully-trusted code** (#57). Capability-derived
profiles are therefore **least-privilege-by-default / defense-in-depth for
honest trusted plugins** — they shrink the blast radius of a buggy or
compromised-at-runtime plugin and make intent auditable. They are **not** a
boundary against a malicious plugin that simply declares `net`. The design does
not oversell this.

## Architecture

Dependencies point inward. The capability *vocabulary* string lives in
`cairn-plugin-protocol`; the *typed* sandbox capability set lives at the port
boundary in `cairn-ports` (no dependency on the protocol crate); the
*translation* from one to the other lives in `cairn-infra` (`plugin_host.rs`),
which already depends on both.

### 1. Capability vocabulary — `cairn-plugin-protocol`

Add beside `CAP_FS_READ` / `CAP_FS_WRITE` / `CAP_EVENTS`:

```rust
/// Capability: direct outbound network access from the plugin process.
/// Unlike `fs:read`/`fs:write`/`events` (which gate host-RPC callbacks), `net`
/// is consumed by the OS sandbox to open the network in the jail — it does not
/// gate any host-callback method.
pub const CAP_NET: &str = "net";
```

### 2. Typed capability set — `cairn-ports`

```rust
/// OS-sandbox-relevant capabilities a plugin declared, translated from its
/// manifest. Distinct from the host-RPC capabilities in `cairn-plugin-protocol`,
/// which never reach the sandbox. Defaults to the fully locked-down posture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SandboxCapabilities {
    /// Allow outbound network access in the jail. Default `false` (denied).
    pub net: bool,
}
```

`Copy` + `Default` so callers and the existing tests pass
`SandboxCapabilities::default()` for the status-quo locked-down jail. Extensible:
future slices add fields (`exec`, read-scope) without another port change.

### 3. Port signature — `cairn-ports`

`Sandbox::wrap` gains one parameter (and a doc update noting the network is
denied unless `caps.net`):

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

No new `SandboxError` variant — `Unavailable` still covers every failure.

### 4. Translation — `cairn-infra/src/plugin_host.rs`

Map the manifest's `Vec<String>` to the typed set immediately before `wrap`
(was the `sandbox.wrap(...)` call at `plugin_host.rs:536`):

```rust
fn sandbox_caps(caps: &[String]) -> SandboxCapabilities {
    SandboxCapabilities { net: caps.iter().any(|c| c == CAP_NET) }
}
// ...
let caps = sandbox_caps(&manifest.engine.capabilities);
let mut command = sandbox
    .wrap(vault_root, plugin_dir, &cmd_path, &manifest.engine.args, caps)
    .map_err(adapt)?;
```

The `"net"` magic string never leaves infra; both backends see only the typed
`caps`.

### 5. Pure profile builders — `cairn-infra/src/sandbox.rs`

Both gain a `caps` param and branch **only** on it; everything else is unchanged.

**`bwrap_args` (Linux).** Today the vector ends `… --unshare-all
--die-with-parent -- <cmd>`. `--unshare-all` drops the network namespace, so the
default jail has no network. When `caps.net`, append **`--share-net`** (bwrap
re-shares the host network namespace after `--unshare-all`). `/etc/resolv.conf`
is already readable through `--ro-bind / /`, so DNS resolves. Insertion point:
immediately after `--unshare-all`, before `--die-with-parent`.

**`seatbelt_profile` (macOS).** Today emits a single `(deny network*)`. When
`caps.net`, replace that line with the outbound incantation:

```
(allow network-outbound)
(allow system-socket)
(allow mach-lookup (global-name "com.apple.mDNSResponder"))
```

The `mach-lookup` to `mDNSResponder` is required for hostname resolution under
Seatbelt; without it a `net` plugin could open sockets but not resolve names,
making `net` useless. When `caps.net` is false, keep `(deny network*)`
(status quo).

### Platform asymmetry (documented, like the existing exec difference)

| Aspect            | macOS (Seatbelt)                         | Linux (bwrap)                         |
|-------------------|------------------------------------------|---------------------------------------|
| Network when `net` absent | denied (`deny network*`)         | denied (no net namespace)             |
| Network when `net` present | **outbound-scoped** (+ DNS)     | **whole namespace shared** (in + out) |

Restricting Linux to outbound-only would need seccomp/netfilter — out of scope.
The headline guarantee is identical on both platforms: **no network at all
unless `net` is declared.** Only the *shape* of the opened access differs.

## Confinement guarantee (updated table)

| Capability        | macOS (Seatbelt)              | Linux (bwrap)                          |
|-------------------|-------------------------------|----------------------------------------|
| Read filesystem   | broad, vault denied           | broad (`--ro-bind / /`), vault tmpfs'd |
| Read plugin dir   | allowed                       | allowed (re-bound)                     |
| Write filesystem  | denied                        | denied (everything read-only)          |
| Network (default) | **denied**                    | **denied** (`--unshare-all`)           |
| Network (`net`)   | **outbound + DNS allowed**    | **namespace shared** (`--share-net`)   |
| Further `exec`    | denied                        | allowed but equally jailed (inherits ns) |

## Error handling

Reuse `SandboxError::Unavailable(String)`. No new variants; `thiserror` already
defines it at the port boundary.

## Testing

Tests are part of done. The pure builders are unit-testable on any platform;
behavioral tests are `#[cfg(target_os = ...)]`-gated and skip where the jail or
userns is unavailable.

**Pure (any platform — the must-haves):**
- `bwrap_args(.., net:false)` contains **no** `--share-net`; `net:true` appends
  exactly `--share-net` immediately after `--unshare-all`.
- `seatbelt_profile(.., net:false)` contains `(deny network*)` and not
  `network-outbound`; `net:true` contains `(allow network-outbound)` and
  **not** `(deny network*)`.
- Existing pure builder/argv tests updated to pass
  `SandboxCapabilities::default()` (asserts the default jail is unchanged).

**Behavioral (cfg-gated per OS, skipped if jail/userns unavailable):**
- The parent binds a `127.0.0.1:0` `TcpListener`, reads the assigned port, and
  spawns a jailed probe `bash -c 'exec 3<>/dev/tcp/127.0.0.1/<port>'`:
  - with `SandboxCapabilities { net: true }` → connects, exit 0;
  - with the default (no net) → fails (Linux: fresh netns loopback is down;
    macOS: `deny network*`).
  This distinguishes net-on from net-off using **loopback only**, so it needs no
  real outbound connectivity in CI. Dependency: `/bin/bash` + the `/dev/tcp`
  redirect (present on GitHub macOS and ubuntu runners). If it proves fragile, a
  tiny fixture binary replaces the `bash` probe; start simple.
- Existing behavioral write/read/exec tests updated for the new `caps` arg
  (default), confirming the locked-down jail still denies writes and vault reads.

**CI:** the Linux job already installs `bubblewrap` (#65) and GitHub's ubuntu
runners permit unprivileged user namespaces, so the Linux behavioral net test
executes rather than skips. No CI changes needed.

## Files

- **Modify** `crates/cairn-plugin-protocol/src/lib.rs` — add `CAP_NET`.
- **Modify** `crates/cairn-ports/src/lib.rs` — add `SandboxCapabilities`; add
  `caps` param + doc to `Sandbox::wrap`.
- **Modify** `crates/cairn-infra/src/sandbox.rs` — thread `caps` through
  `seatbelt_profile`, `bwrap_args`, all `wrap` impls (incl. `RefusingSandbox`),
  add net branches, update module/struct docs, add tests.
- **Modify** `crates/cairn-infra/src/plugin_host.rs` — add `sandbox_caps`
  mapping and update the `wrap` call site.
- **Modify** plugin docs that enumerate capabilities (`cairn-plugin-sdk` doc
  comment and/or ADR-0008) — list `net` and note it drives the sandbox, not the
  host-RPC gate.

## Out of scope (explicit)

- **Read-tightening** of non-vault reads — deferred to its own design. The
  macOS dyld shared cache lives behind the Preboot cryptex and cannot be
  allowlisted (the existing `seatbelt_profile` relies on broad reads for exactly
  this reason), so least-privilege reads are a separate, riskier problem.
- `exec` capability (macOS already denies further exec; Linux can't without
  seccomp).
- Linux outbound-only network scoping (needs seccomp/netfilter).
- Windows backend (#62).
