# Plugin capability vocabulary + first-run approval — Design

> Closes the last open item of #40: *interactive first-run approval + surface
> declared capabilities to the user.* Also the **keystone for #63**
> (capability-derived sandbox profiles), which is blocked until a `net` / `exec`
> / read-scope capability vocabulary exists in the manifest.
>
> Builds on: the default-deny allowlist with content-hash pinning
> (`2026-06-11-cairn-plugin-trust-design.md`, #58) and the OS sandbox backends
> (`2026-06-14-plugin-linux-sandbox-design.md`, #60/#61).

## Problem

Two gaps remain in the plugin trust story:

1. **No declared-capability vocabulary for the sandbox.** Today's manifest
   capabilities (`fs:read`, `fs:write`, `events`) are free-form `Vec<String>`
   that gate *only* the host-callback RPC surface (`service_callback` in
   `crates/cairn-infra/src/plugin_host.rs`). There is no vocabulary for the
   powers the spawned **process** holds — outbound network, subprocess exec, how
   much of the real filesystem it may read. #63 cannot derive a sandbox profile
   from capabilities that do not exist.

2. **No first-run approval surface.** Trusting a plugin means hand-editing
   `cairn.toml` `[[plugins.trusted]]` with a directory name and content hash. The
   user is never shown what the plugin *declares it can do* before granting it
   arbitrary-code-execution as the daemon user.

This increment delivers the **vocabulary** (typed, fail-closed) and the
**approval surface** (a CLI that shows declared capabilities and emits the exact
trust entry). It does **not** change what the sandbox enforces — that is #63.

## Trust-model recap (unchanged)

An approved plugin is **fully-trusted code** running with the daemon user's
privileges, confined only by the OS sandbox. The allowlist gates *whether* a
plugin runs; capabilities describe *what it asks for*. Nothing here softens that:
the approval screen makes the grant explicit rather than implicit, and the
content hash binds the user's consent to the exact on-disk bytes they reviewed.

## Two enforcement domains

Capabilities split into two domains with different enforcement points. Naming
keeps them distinct so a reader never confuses "vault note access via RPC" with
"filesystem access in the jail".

- **Domain 1 — host-channel (vault).** Gates the JSON-RPC callbacks the plugin
  makes back to the host (`host/readNote`, `host/writeNote`, `host/search`,
  `host/listNotes`, `host/deleteNote`, and `cairn/event` delivery). Enforced
  **today** in `service_callback` / `dispatch_event`. These are *not* filesystem
  access: the sandbox always hides the vault on disk; notes are reachable only
  through this gated channel.
- **Domain 2 — OS sandbox / process.** Gates the jail around the child:
  outbound network, subprocess exec, and direct reads of the real filesystem
  beyond the process's runtime needs. The jail is **fixed today**; #63 makes it
  capability-derived. Until then these capabilities are **declared and surfaced
  but not enforced** (see "Enforcement lag").

## Design

### Capability vocabulary (`cairn-plugin-protocol`)

Replace the free-form `Vec<String>` with a typed, closed enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    // Domain 1 — host-channel (vault via gated RPC). Enforced today.
    #[serde(rename = "vault:read")]   VaultRead,
    #[serde(rename = "vault:write")]  VaultWrite,
    #[serde(rename = "vault:events")] VaultEvents,
    // Domain 2 — OS sandbox / process. Declared now, enforced by #63.
    #[serde(rename = "net")]          Net,
    #[serde(rename = "exec")]         Exec,
    #[serde(rename = "fs:read")]      FsRead, // broaden reads beyond the vault (coarse)
}
```

Each variant carries metadata used by **both** the approval surface and #63:

```rust
impl Capability {
    /// The manifest/wire string (e.g. "vault:read").
    pub fn wire(&self) -> &'static str;
    /// Plain-English line for the approval screen (e.g. "make outbound
    /// network connections").
    pub fn summary(&self) -> &'static str;
    /// True for the three vault:* capabilities (host-RPC gate, live today);
    /// false for net / exec / fs:read (enforced by #63). Drives the
    /// "enforced in a future release" label on the approval screen.
    pub fn enforced_today(&self) -> bool;
}
```

`std::fmt::Display` delegates to `wire()`.

**Fail-closed parsing, for free.** serde rejects an unknown variant string with a
deserialize error. An unknown capability (a typo, or a capability from a newer
manifest this host predates) therefore fails the whole manifest parse, which the
host's load loop already turns into a refusal-with-log (`spawn_plugin`'s
`toml::from_str(...).map_err(adapt)` → skipped with a `tracing::warn!`). No
silent under-granting. This matches the existing fail-fast stance on malformed
pins (`TrustedPlugins::from_entries`).

### Read-scope granularity (coarse)

`fs:read` is a **coarse binary switch**: "read real files outside the vault
broadly". Declaring it is #63's signal to keep today's broad read; omitting it is
#63's signal to tighten the default to minimal runtime + the plugin's own dir.
The vault is always masked regardless — vault reads go through Domain 1.

Path-scoped reads (`fs:read = ["~/Documents", …]`) are explicitly **out of
scope**; the enum variant can grow a payload later without disturbing the others.

### Enforcement lag (Domain 2)

Domain-2 capabilities are **declared and surfaced now, enforced by #63.** The
fixed jail is *stricter-or-equal* to every Domain-2 declaration in the dangerous
directions (it denies network and writes regardless of `net`; it already allows
broad reads regardless of `fs:read`). So:

- A plugin declaring `net` / `exec` / `fs:read` **spawns under today's fixed
  jail** — it is never granted more than it gets today. Safe by construction.
- The approval screen labels each Domain-2 capability **"enforced in a future
  release"** so the user is told the truth: the plugin *asks* for this, and a
  future cairn will grant it.
- The content-hash pin binds that consent: when #63 activates enforcement, the
  manifest the user approved is unchanged, so their reviewed-and-approved
  capability set is exactly what gets enforced. Any later manifest change drifts
  the hash and forces re-approval.

This increment's deliverable is **declaration + surfacing**; enforcement is #63.

### Host wiring (`cairn-infra`)

Mechanical migration, behavior identical:

- `EngineSection.capabilities: Vec<Capability>` (was `Vec<String>`).
- `LoadedPlugin.capabilities: Vec<Capability>`.
- `required_cap(method) -> Option<Capability>` returns enum variants
  (`VaultRead` / `VaultWrite`).
- `service_callback` compares `Capability` values; `dispatch_event` checks for
  `Capability::VaultEvents` instead of the `CAP_EVENTS` string.
- The `CAP_FS_READ` / `CAP_FS_WRITE` / `CAP_EVENTS` string consts are removed
  (their role is now the enum + `wire()`).

### Inspection (`cairn-infra`, read-only, no spawn)

A read-only sweep that the CLI renders. No process is spawned.

```rust
pub struct PluginInspection {
    pub dir_name: String,
    pub manifest: Option<InspectedManifest>, // None when unreadable
    pub computed_hash: Option<PinnedHash>,    // None when the dir can't be hashed
    pub status: TrustStatus,
}

pub struct InspectedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub command: String,
    pub capabilities: Vec<Capability>,
}

pub enum TrustStatus {
    Untrusted,        // not in [plugins].trusted
    TrustedUnpinned,  // trusted, no hash pin recorded
    Pinned,           // trusted + pin matches the computed hash
    Drift,            // trusted + pin recorded but contents changed
    Unreadable,       // manifest missing/malformed, or dir un-hashable
}

pub fn inspect_plugins(dir: &Path, trusted: &TrustedPlugins)
    -> Result<Vec<PluginInspection>, PortError>;
```

- `Err` only on an unexpected IO error reading the plugins directory; an absent
  directory yields an empty `Vec` (mirrors `ProcessPluginHost::load`).
- A malformed/missing manifest or an un-hashable dir (symlink, etc.) yields that
  entry with `status = Unreadable` — it never aborts the sweep.
- Capability parse: an unknown capability string makes the manifest `Unreadable`
  (same fail-closed rule as spawn).

**Deliberate distinction.** `inspect_plugins` *parses untrusted manifests* —
that is the entire point of reviewing a plugin before trusting it. This is a
user-initiated, read-only parse (no spawn, no code execution), so it does **not**
weaken the daemon's automatic-load invariant that untrusted manifest bytes are
never parsed during spawn. The two paths have different threat models: automatic
spawn (defense-in-depth: never touch attacker bytes) vs. explicit human review
(the user asked to see it).

### Shared trusted-set parser (targeted refactor)

The CLI needs the current trusted set (from `<cairn>/cairn.toml`
`[plugins].trusted`) to compute `TrustStatus` and to tell the user whether a
plugin is already trusted. That parser currently lives only in
`cairn-daemon/src/config.rs` (`TrustedEntry` / `PinnedEntry` / `normalize`),
which `cairn-cli` does **not** depend on. Duplicating it would be a security
footgun — the two copies could diverge on the `deny_unknown_fields` rule that
stops a typo'd `hash` key from silently dropping a pin.

So this increment **moves `TrustedEntry` / `PinnedEntry` / `normalize` into
`cairn-infra`** (next to `TrustedPlugins`; `cairn-infra` already has `serde` +
`toml`), and adds:

```rust
impl TrustedPlugins {
    /// Build from `<cairn>/cairn.toml` `[plugins].trusted`. Absent file or
    /// absent section ⇒ `none()` (default-deny). Malformed pin ⇒ `Err`.
    pub fn from_cairn_toml(cairn_root: &Path) -> Result<Self, PortError>;
}
```

`cairn-daemon` re-imports `TrustedEntry` from `cairn-infra` (its `PluginsConfig`
keeps using it), so there is exactly **one** parser for the trusted list. The CLI
calls `TrustedPlugins::from_cairn_toml(&cli.cairn)`. This is a focused
improvement that the goal requires, not a drive-by refactor.

### CLI (`cairn-cli`)

A new `plugin` subcommand group. `cairn-cli` already depends on `cairn-infra`.

```
cairn plugin list
cairn plugin trust <dir>
```

- **`list`** — render `inspect_plugins(...)` as a table: name, version, trust
  status, declared capabilities. Builds the trusted set via
  `TrustedPlugins::from_cairn_toml(&cli.cairn)`.
- **`trust <dir>`** — the approval flow:
  1. Look up `<dir>` in the inspection results; error if absent or `Unreadable`.
  2. Render the approval screen (below).
  3. Read `y/N` from stdin. **Non-TTY / piped / EOF input → treated as "no"**
     (refuse) so a scripted pipe never silently approves a plugin.
  4. On `y`, **print the exact `[[plugins.trusted]]` snippet** (dir + computed
     hash) for the user to paste into `cairn.toml`. The command **never writes
     the config file** (Option C — chosen to avoid clobbering a hand-edited
     `cairn.toml` and to add no new dependency).

Approval screen:

```
$ cairn plugin trust fetch-bot

  Plugin:   fetch-bot  (Fetch Bot  v1.0.0)
  Command:  ./fetch-bot          (runs as a sandboxed child of the daemon)
  Content:  sha256:9f2a…c7

  Capabilities this plugin declares:
    • vault:read   read and search your notes
    • net          make outbound network connections   (enforced in a future release)

  Approve and trust this exact version? [y/N]: y

  Add this to your cairn.toml to trust fetch-bot:

      [[plugins.trusted]]
      dir = "fetch-bot"
      hash = "sha256:9f2a…c7"
```

A plugin declaring no capabilities shows a single line stating it requests none.
The "(enforced in a future release)" suffix is appended exactly for capabilities
whose `enforced_today()` is false.

## Architecture / boundaries

- **`cairn-plugin-protocol`** — owns `Capability` (the shared vocabulary). No new
  dependency.
- **`cairn-infra`** — owns `inspect_plugins` + the `PluginInspection` /
  `TrustStatus` types, alongside `ProcessPluginHost` and `TrustedPlugins`. This
  is infrastructure (filesystem read, hashing, manifest parse), not domain
  logic, so it is a plain function — no new port.
- **`cairn-cli`** — a driving adapter: renders the inspection, prompts, emits the
  snippet. Owns no trust logic.
- Hexagonal: dependencies point inward; the CLI depends on infra/protocol, not
  vice versa. `thiserror` at boundaries (reuse `PortError`), no `unsafe`.

## Data flow

```
cairn.toml [plugins].trusted ─► TrustedPlugins ─┐
                                                 ▼
.cairn/plugins/<dir>/manifest.toml ─► inspect_plugins() ─► Vec<PluginInspection>
   (read-only parse, hash, no spawn)                   │
                                                        ▼
                                          cairn plugin list   → table
                                          cairn plugin trust  → approval screen
                                                                 → y/N (stdin)
                                                                 → print snippet
```

## Error handling

- Reuse `PortError` (`thiserror` already at the boundary). `inspect_plugins`
  returns `Err` only on an unexpected plugins-dir IO error; per-plugin problems
  become `TrustStatus::Unreadable`, not errors.
- `cairn plugin trust <dir>` errors (non-zero exit, message to stderr) when
  `<dir>` is absent or `Unreadable`. Declining at the prompt is a clean,
  non-error exit.
- Unknown capability anywhere → manifest treated as unreadable / refused; never a
  panic, never a silent ignore.

## Testing (TDD)

`cairn-plugin-protocol`:
- Each `Capability` round-trips through its wire string (serde).
- An unknown capability string fails to deserialize.
- `wire()` / `summary()` / `enforced_today()` return the expected values; the
  three `vault:*` are `enforced_today() == true`, the three Domain-2 are `false`.

`cairn-infra`:
- `inspect_plugins` status matrix on temp dirs: untrusted; trusted-unpinned;
  pinned-matching → `Pinned`; pinned-mismatched → `Drift`; malformed manifest →
  `Unreadable`; absent dir → empty `Vec`.
- A trusted plugin with a `net` capability inspects with that capability present
  and `enforced_today() == false` (the surfacing contract).
- The existing host tests migrate to the typed `Capability` / `vault:*` names and
  stay green (callback gating behavior unchanged).

`cairn-cli` (`assert_cmd`):
- `plugin list` shows a known plugin's status and capabilities.
- `plugin trust <dir>` with `y` on stdin prints the `[[plugins.trusted]]` snippet
  with the correct dir and hash.
- `plugin trust <dir>` with empty/`n`/non-TTY stdin refuses (no snippet, clean
  exit).
- `plugin trust <unknown>` errors.

## Scope

**In scope:** the typed `Capability` vocabulary; fail-closed parsing; the host
migration to the enum; `inspect_plugins`; `cairn plugin list` / `cairn plugin
trust`; surfacing declared capabilities with the enforcement-status label.

**Out of scope:**
- The sandbox actually honoring `net` / `exec` / `fs:read` (#63).
- Auto-writing `cairn.toml` (Option A, `toml_edit`) — this increment prints the
  snippet (Option C).
- Path-scoped reads (a future refinement of `fs:read`).

## Files

- **Modify** `crates/cairn-plugin-protocol/src/lib.rs` — add `Capability` + its
  methods; change `EngineSection.capabilities` to `Vec<Capability>`; remove the
  `CAP_*` consts.
- **Modify** `crates/cairn-infra/src/plugin_host.rs` — typed capabilities in
  `LoadedPlugin` / `required_cap` / `service_callback` / `dispatch_event`; add
  `inspect_plugins` + `PluginInspection` / `InspectedManifest` / `TrustStatus`;
  add `TrustedPlugins::from_cairn_toml`; host the moved `TrustedEntry` /
  `PinnedEntry` / `normalize` (next to `TrustedPlugins`).
- **Modify** `crates/cairn-daemon/src/config.rs` — re-import `TrustedEntry` from
  `cairn-infra` instead of defining it locally (one parser).
- **Modify** `crates/cairn-cli/src/main.rs` — add the `plugin` subcommand group
  (`list`, `trust`) and the approval/prompt/snippet rendering.
- **Modify** `crates/cairn-plugin-example/` manifest + any fixtures using the old
  `fs:read` / `events` strings → `vault:*`.
- **Modify** affected tests across the three crates.
```

