# Plugin trust gate — explicit allowlist before spawn

**Audit finding:** S3 (High) — *Plugin trust boundary: self-declared capabilities
+ arbitrary executable, no approval/sandbox* (`audit/security.md:89`).

## Problem

`ProcessPluginHost::load_with_timeout` (`crates/cairn-infra/src/plugin_host.rs`)
iterates every directory under `<cairn>/.cairn/plugins/` and unconditionally
calls `spawn_plugin`, which reads that directory's `manifest.toml` and launches
`manifest.engine.command` as a child process with the user's full privileges.
There is no approval step and no sandbox. Placing a directory under
`.cairn/plugins/` is equivalent to running an arbitrary binary. The existing
capability gate (`service_callback`) only narrows the host-callback RPC surface;
it does not constrain what the spawned process does directly (network,
filesystem, exec).

The companion session `01-cairn-rce` closes the *write-a-manifest* path
(dot-leading note paths). This session addresses the trust model for plugins
that are legitimately present on disk: a present plugin must be **explicitly
approved**, not blindly trusted.

## Trust model: an approved plugin is fully-trusted code

State this plainly, because it is easy to misread the allowlist as a sandbox:

**An approved (trusted-listed) plugin is fully-trusted code. It runs as a child
of the daemon, with the daemon's full operating-system privileges — the same
user, filesystem, network, and process access the daemon itself has.** There is
**no OS-level sandbox** around the spawned child (seccomp/landlock/sandbox-exec
is a deferred follow-up, see below).

The allowlist gates **whether a plugin runs, not what it can do.** Adding a
directory name to `[plugins].trusted` is the act of granting that directory
arbitrary-code-execution as the daemon user. Two things follow:

- The manifest `capabilities` field constrains only the **host-callback RPC
  surface** (`host/readNote`, `host/writeNote`, etc. — see
  [the SDK design doc](2026-06-10-plugin-sdk-design.md) for the author-facing
  callback API and [ADR-0008](../../decisions/0008-plugin-host.md) for the host
  enforcement). It does **not** constrain what the spawned process does directly:
  a plugin can open sockets, read and write any file the daemon user can reach,
  and exec further binaries regardless of its declared capabilities. Capabilities
  are a convenience boundary on the callback channel, not a security boundary on
  the process.
- Trusting a plugin is therefore a decision to trust its author and its exact
  on-disk contents, by the same standard you would apply to any binary you run
  yourself. Trust is anchored on the **directory name** (controlled by whoever
  administers the cairn), not the self-declared manifest `id`.

The allowlist exists so that *presence on disk* is no longer *permission to run*;
it deliberately does not, and is not intended to, make an untrusted plugin safe
to run.

## Scope of this PR (smallest viable increment)

Add a **default-deny allowlist gate before spawn**. Out of scope (listed as
follow-ups in the PR body, not built here): manifest signing/hashing, OS-level
sandboxing of the child, and interactive first-run capability confirmation.

## Design

### Trust key: the plugin directory name

A plugin is identified for trust purposes by its **directory name** under
`.cairn/plugins/`, not by the `id` field inside its manifest. The directory name
is controlled by whoever administers the cairn (they placed/approved that
directory); the manifest `id` is authored by the plugin and therefore cannot be
the trust anchor — a plugin in any directory could otherwise impersonate a
trusted one by copying its `id`.

As a cheap consistency check, a trusted directory whose `manifest.id` does not
equal the directory name is **rejected** (not spawned). This keeps "directory
name" and "plugin id" the same value end to end, so users can think in terms of
ids, and it kills the confusing case of two directories claiming one id.

### The gate

In the load loop, for each `<cairn>/.cairn/plugins/<dir>`:

1. If `<dir>` is **not** in the trusted set → skip, log how to trust it, and
   **do not read or parse `manifest.toml`** (untrusted bytes never reach the
   TOML parser).
2. If trusted → parse the manifest, then assert `manifest.id == <dir>`. On
   mismatch → skip with a distinct log line.
3. Otherwise → spawn as today.

Empty trusted set ⇒ nothing spawns. This is the secure default and the whole
point of the change: "blindly trusted" → "explicitly approved".

### API shape

A small typed newtype carries the allowlist into the host, keeping the call site
self-documenting and giving a future home for richer trust data (e.g. hashes):

```rust
// crates/cairn-infra/src/plugin_host.rs (re-exported from cairn-infra)
pub struct TrustedPlugins(HashSet<String>);

impl TrustedPlugins {
    pub fn none() -> Self;                       // default-deny
    pub fn contains(&self, dir_name: &str) -> bool;
}
impl<I: IntoIterator<Item = String>> From<I> for TrustedPlugins { /* ... */ }
```

`load` and `load_with_timeout` each gain a `trusted: &TrustedPlugins`
parameter. There is no insecure overload — the only way to load is to pass an
explicit policy, so a caller cannot accidentally fall back to "spawn everything".

### Config wiring

`cairn.toml` `[plugins]` section (already exists for `timeout_secs`) gains:

```toml
[plugins]
trusted = ["my-plugin", "another-plugin"]
```

- `crates/cairn-daemon/src/config.rs`: add `trusted: Vec<String>` to
  `PluginsConfig` (`#[serde(default)]` → empty when absent ⇒ default-deny).
- `crates/cairn-daemon/src/main.rs`: build `TrustedPlugins` from
  `config.plugins.trusted` and pass it to `load_with_timeout`. When the plugins
  dir is non-empty but nothing is trusted, the existing per-dir skip logs make
  the situation visible; print a one-line pointer to the config key.

## Data flow

```
cairn.toml [plugins].trusted ──► PluginsConfig.trusted (Vec<String>)
                                        │  (cairn-daemon/main.rs)
                                        ▼
                              TrustedPlugins (HashSet)
                                        │
        ProcessPluginHost::load_with_timeout(dir, timeout, &trusted)
                                        │  per directory entry
                ┌───────────────────────┴───────────────────────┐
        dir ∈ trusted?                                    not trusted
                │ yes                                          │
        parse manifest.toml                              skip + log
                │                                     (no parse, no spawn)
        manifest.id == dir?
          │ yes        │ no
        spawn       skip + log
```

## Error handling

- Untrusted directory: `eprintln!` `plugin: skipping <dir> (not in [plugins]
  trusted; add "<dir>" to cairn.toml to enable)`. Not an error — load continues.
- Trusted dir, id mismatch: `eprintln!` `plugin: skipping <dir>: manifest id
  "<id>" does not match directory name`. Load continues.
- Spawn failure of a trusted plugin: unchanged (already skipped-with-log today).

## Testing (TDD)

Tests live in `crates/cairn-plugin-example/tests/host.rs` (real spawn against the
example binary) and unit tests in the two changed crates.

Primary failing-first tests:

1. **Unapproved plugin is not spawned, approved one is.** Two dirs — `example`
   (in trusted set) and `rogue` (not) — both with valid manifests/binary. After
   load, `host.plugins()` contains only `example`.
2. **Default-deny.** Trusted set empty + one valid plugin dir ⇒ `host.plugins()`
   is empty.
3. **Id-mismatch rejected.** Dir `example` is trusted but its manifest declares
   `id = "evil"` ⇒ not loaded.

Config tests in `cairn-daemon/src/config.rs`:

4. `[plugins] trusted = ["a","b"]` parses; absent ⇒ empty vec.

The existing ~14 host tests are updated to pass a trust set containing
`"example"` (via a small local `load`/`load_with_timeout` helper to keep the
churn centralized).

## Follow-ups (PR body, not this PR)

- Manifest signing / content hashing pinned in the trust list (detect a trusted
  directory whose contents changed).
- OS-level sandbox for the spawned child (seccomp/landlock/sandbox-exec).
- Interactive first-run approval + surfacing declared capabilities to the user.
- Documentation: state explicitly that an approved plugin is fully trusted code.
  *(Done — see "Trust model" above and the security note in
  `crates/cairn-plugin-sdk/src/lib.rs`.)*
