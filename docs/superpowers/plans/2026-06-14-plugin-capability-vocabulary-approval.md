# Plugin Capability Vocabulary + First-Run Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a typed, fail-closed plugin capability vocabulary (`vault:*` host-RPC caps + `net`/`exec`/`fs:read` sandbox caps) and a `cairn plugin list` / `cairn plugin trust` CLI that surfaces declared capabilities and prints the exact trust entry — unblocking #63.

**Architecture:** A closed `Capability` enum in `cairn-plugin-protocol` (serde deserializes manifest strings; unknown → refuse). `cairn-infra` migrates the host gate to the enum, adds a read-only `inspect_plugins` sweep, and gains a shared `TrustedPlugins::from_cairn_toml` (with the `[plugins].trusted` parser moved out of `cairn-daemon`). `cairn-cli` adds a `plugin` subcommand group that renders inspections, prompts, and emits a `[[plugins.trusted]]` snippet (never writes config). Hexagonal: CLI → infra → protocol; deps point inward; `thiserror`/`PortError` at boundaries; no `unsafe`.

**Tech Stack:** Rust (workspace `unsafe_code = "forbid"`), serde + toml, sha2 (existing `PinnedHash`), clap (CLI subcommands), assert_cmd + predicates (CLI tests).

**Spec:** `docs/superpowers/specs/2026-06-14-plugin-capability-vocabulary-approval-design.md`

**Working notes:**
- Run `git branch --show-current` before every commit; this working dir is shared across sessions. The branch must stay `plugin-trust-capability-vocabulary`.
- Build/test the whole workspace with `cargo test` from the repo root unless a narrower command is given.
- Conventional commits, imperative mood, scoped.

---

### Task 1: `Capability` enum in `cairn-plugin-protocol`

Add the typed vocabulary as a standalone type. Nothing consumes it yet, so this task compiles and tests green on its own (the existing `Vec<String>` field is untouched until Task 2).

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
    #[test]
    fn capability_roundtrips_via_wire_string() {
        for cap in [
            Capability::VaultRead,
            Capability::VaultWrite,
            Capability::VaultEvents,
            Capability::Net,
            Capability::Exec,
            Capability::FsRead,
        ] {
            let v = serde_json::to_value(cap).unwrap();
            assert_eq!(v, serde_json::Value::String(cap.wire().to_string()));
            let back: Capability = serde_json::from_value(v).unwrap();
            assert_eq!(back, cap);
        }
    }

    #[test]
    fn unknown_capability_is_rejected() {
        // typo, and an old name that no longer exists -> hard error (fail-closed)
        assert!(serde_json::from_value::<Capability>(serde_json::json!("net:outbund")).is_err());
        assert!(serde_json::from_value::<Capability>(serde_json::json!("fs:write")).is_err());
        assert!(serde_json::from_value::<Capability>(serde_json::json!("events")).is_err());
    }

    #[test]
    fn enforced_today_only_for_vault_caps() {
        assert!(Capability::VaultRead.enforced_today());
        assert!(Capability::VaultWrite.enforced_today());
        assert!(Capability::VaultEvents.enforced_today());
        assert!(!Capability::Net.enforced_today());
        assert!(!Capability::Exec.enforced_today());
        assert!(!Capability::FsRead.enforced_today());
    }

    #[test]
    fn capability_displays_as_wire_string() {
        assert_eq!(Capability::VaultRead.to_string(), "vault:read");
        assert_eq!(Capability::FsRead.to_string(), "fs:read");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-plugin-protocol`
Expected: FAIL — `cannot find type Capability in this scope`.

- [ ] **Step 3: Add the `Capability` type**

Insert after the `CAP_EVENTS` const block (around line 32, before the `CALLBACK_DENIED` const) in `crates/cairn-plugin-protocol/src/lib.rs`:

```rust
/// A capability a plugin declares in its manifest's `[engine].capabilities`.
///
/// Two enforcement domains:
/// - `vault:*` gate the **host-callback RPC** surface (`host/readNote`, …) and
///   are enforced today by the host (`cairn-infra` `service_callback`).
/// - `net` / `exec` / `fs:read` gate the **OS sandbox** around the spawned
///   child. They are declared and surfaced to the user now, and enforced by the
///   capability-derived sandbox profile (issue #63); until then the fixed jail
///   is stricter-or-equal, so declaring them never grants more than today.
///
/// The enum is **closed**: serde rejects any unknown string, so a typo or a
/// capability from a newer manifest fails the manifest parse (fail-closed) and
/// the host refuses the plugin rather than silently under-granting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    /// Read/search/list notes via the host channel.
    #[serde(rename = "vault:read")]
    VaultRead,
    /// Create/overwrite/delete notes via the host channel.
    #[serde(rename = "vault:write")]
    VaultWrite,
    /// Receive pushed cairn change events.
    #[serde(rename = "vault:events")]
    VaultEvents,
    /// Make outbound network connections (sandbox; enforced by #63).
    #[serde(rename = "net")]
    Net,
    /// Spawn subprocesses (sandbox; enforced by #63).
    #[serde(rename = "exec")]
    Exec,
    /// Read real files outside the vault, broadly (sandbox; enforced by #63).
    #[serde(rename = "fs:read")]
    FsRead,
}

impl Capability {
    /// The manifest/wire string for this capability (e.g. `"vault:read"`).
    pub fn wire(&self) -> &'static str {
        match self {
            Capability::VaultRead => "vault:read",
            Capability::VaultWrite => "vault:write",
            Capability::VaultEvents => "vault:events",
            Capability::Net => "net",
            Capability::Exec => "exec",
            Capability::FsRead => "fs:read",
        }
    }

    /// A plain-English line for the first-run approval screen.
    pub fn summary(&self) -> &'static str {
        match self {
            Capability::VaultRead => "read and search your notes",
            Capability::VaultWrite => "create, overwrite, and delete your notes",
            Capability::VaultEvents => "be notified when your notes change",
            Capability::Net => "make outbound network connections",
            Capability::Exec => "run other programs",
            Capability::FsRead => "read files on your computer outside your notes",
        }
    }

    /// Whether this capability is actually enforced by the current build. The
    /// three `vault:*` caps gate the live host-RPC channel (`true`); the three
    /// sandbox caps are declared now and enforced by #63 (`false`). Drives the
    /// "enforced in a future release" label on the approval screen.
    pub fn enforced_today(&self) -> bool {
        matches!(
            self,
            Capability::VaultRead | Capability::VaultWrite | Capability::VaultEvents
        )
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire())
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS (all new tests + existing tests green).

- [ ] **Step 5: Commit**

```bash
git branch --show-current   # must print: plugin-trust-capability-vocabulary
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(plugins): add typed Capability vocabulary (#40)"
```

---

### Task 2: Migrate the manifest + host gate to typed `Capability` (with `vault:*` rename)

Atomic breaking change: switch `EngineSection.capabilities` to `Vec<Capability>`, remove the old `CAP_*` string consts, rewire the host's callback gate, and update every test/manifest that used the old `fs:read`/`fs:write`/`events` strings. These must change together to compile.

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs` (EngineSection, remove `CAP_*`, doc)
- Modify: `crates/cairn-infra/src/plugin_host.rs` (imports, `required_cap`, `service_callback`, `dispatch_event`, `LoadedPlugin`)
- Modify: `crates/cairn-plugin-example/tests/host.rs` (manifest capability strings)

- [ ] **Step 1: Update the protocol manifest type + tests**

In `crates/cairn-plugin-protocol/src/lib.rs`:

1. Delete the three consts:
```rust
/// Capability: read the cairn (read/search/list note content + metadata).
pub const CAP_FS_READ: &str = "fs:read";
/// Capability: mutate the cairn (create/overwrite/delete notes).
pub const CAP_FS_WRITE: &str = "fs:write";
/// Capability: receive pushed cairn events.
pub const CAP_EVENTS: &str = "events";
```

2. Change the `EngineSection.capabilities` field type and doc to:
```rust
    /// Declared capabilities (typed; see [`Capability`]). The host gates every
    /// plugin->host callback on this list (see `cairn-infra`
    /// `plugin_host::service_callback`): a callback whose required capability is
    /// absent here is denied. An **unknown** capability string fails this parse
    /// (fail-closed), so the plugin is refused rather than silently
    /// under-granted. Note the boundary's limits (audit `security.md` S3):
    /// capabilities are *self-declared*, and the `vault:*` gate only narrows the
    /// host-callback RPC surface; the `net`/`exec`/`fs:read` sandbox caps are
    /// enforced by the capability-derived profile (#63).
    #[serde(default)]
    pub capabilities: Vec<Capability>,
```

3. Add a test to the protocol `tests` module proving typed parse:
```rust
    #[test]
    fn manifest_parses_typed_capabilities() {
        let m: Manifest = toml::from_str(
            "id=\"x\"\nname=\"X\"\nversion=\"0\"\n\
             [engine]\ncommand=\"./x\"\ncapabilities=[\"vault:read\", \"net\"]\n",
        )
        .unwrap();
        assert_eq!(m.engine.capabilities, vec![Capability::VaultRead, Capability::Net]);
    }

    #[test]
    fn manifest_rejects_unknown_capability() {
        let r: Result<Manifest, _> = toml::from_str(
            "id=\"x\"\nname=\"X\"\nversion=\"0\"\n\
             [engine]\ncommand=\"./x\"\ncapabilities=[\"fs:write\"]\n",
        );
        assert!(r.is_err(), "unknown capability must fail the manifest parse");
    }
```

- [ ] **Step 2: Run protocol tests to verify the new ones fail**

Run: `cargo test -p cairn-plugin-protocol`
Expected: FAIL — `manifest_parses_typed_capabilities` / `manifest_rejects_unknown_capability` not yet satisfied (and possibly a type error until Step 3 of this task is also done; that's fine — proceed).

- [ ] **Step 3: Rewire the host gate in `plugin_host.rs`**

In `crates/cairn-infra/src/plugin_host.rs`:

1. Fix the `cairn_plugin_protocol` import: remove `CAP_EVENTS, CAP_FS_READ, CAP_FS_WRITE` and add `Capability`. The import becomes (keep the other names as-is):
```rust
use cairn_plugin_protocol::{
    write_message, CairnEvent, CairnEventKind, Capability, CommandDecl, DeleteNoteParams, Incoming,
    InitializeParams, InitializeResult, InvokeParams, ListNotesResult, Manifest, NoteSummaryDto,
    ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchHitDto, SearchParams,
    SearchResultDto, WriteNoteParams, CALLBACK_DENIED, CALLBACK_FAILED, JSONRPC_VERSION,
    METHOD_CAIRN_EVENT, METHOD_DELETE_NOTE, METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
```

2. Change `required_cap` to return the enum:
```rust
/// The capability a host-callback method requires, or `None` if the method is
/// unknown to the host.
fn required_cap(method: &str) -> Option<Capability> {
    match method {
        METHOD_READ_NOTE => Some(Capability::VaultRead),
        METHOD_WRITE_NOTE => Some(Capability::VaultWrite),
        METHOD_DELETE_NOTE => Some(Capability::VaultWrite),
        METHOD_SEARCH => Some(Capability::VaultRead),
        METHOD_LIST_NOTES => Some(Capability::VaultRead),
        _ => None,
    }
}
```

3. Change the `LoadedPlugin.capabilities` field type:
```rust
    /// Capabilities the manifest declared; gates host-callbacks.
    capabilities: Vec<Capability>,
```

4. In `service_callback`, update the capability-absent guard arm (the `Some(cap) if …` arm) to compare by value (`Capability` is `Copy`) and fix the message:
```rust
            Some(cap) if !self.capabilities.contains(&cap) => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("capability {} not declared", cap.wire()),
                });
            }
```

5. In `dispatch_event`, change the events check:
```rust
            if p.capabilities.contains(&Capability::VaultEvents) {
```

(`spawn_plugin`'s `capabilities: manifest.engine.capabilities.clone()` needs no change — it is now `Vec<Capability>`.)

- [ ] **Step 4: Update the example host tests' manifest strings**

In `crates/cairn-plugin-example/tests/host.rs`, replace each old capability string with the `vault:*` name (the `write_manifest` helper takes a raw TOML fragment, so change the quoted strings):

- every `"\"fs:read\""` → `"\"vault:read\""` (lines ~179, ~262, ~285, ~302; and line ~174's comment "declare fs:read" → "declare vault:read")
- every `"\"fs:write\""` → `"\"vault:write\""` (lines ~242, ~324)

Run `grep -n 'fs:read\|fs:write\|"events"' crates/cairn-plugin-example/tests/host.rs` afterward and confirm zero matches. If any test declares `events`, change it to `vault:events`.

- [ ] **Step 5: Sweep for any other old-string usages**

Run:
```bash
grep -rn 'CAP_FS_READ\|CAP_FS_WRITE\|CAP_EVENTS\|"fs:read"\|"fs:write"\|"events"' crates/ docs/
```
Expected: no remaining references in `crates/` (a docs spec mention is fine). Fix any stragglers in `crates/` by hand (replace the const with the enum variant, or the string with `vault:*`).

- [ ] **Step 6: Run the full workspace tests**

Run: `cargo test`
Expected: PASS — protocol typed-capability tests pass, host callback-gating tests still pass under the renamed caps.

- [ ] **Step 7: Commit**

```bash
git branch --show-current
git add crates/cairn-plugin-protocol/src/lib.rs crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "refactor(plugins): type the capability gate and rename vault:* caps (#40)"
```

---

### Task 3: `inspect_plugins` read-only sweep in `cairn-infra`

A spawn-free inspection used by the CLI. Parses untrusted manifests on purpose (the review path); never spawns.

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs` (add types + `inspect_plugins` + tests)
- Modify: `crates/cairn-infra/src/lib.rs` (exports)

- [ ] **Step 1: Write the failing tests**

Add a test module section to `crates/cairn-infra/src/plugin_host.rs` inside the existing `#[cfg(test)] mod tests` block (reuse its `write_plugin` helper where possible, but these need explicit manifests with capabilities — define a local helper):

```rust
    fn write_manifest_with(root: &Path, dir_name: &str, body: &str) {
        let pdir = root.join(dir_name);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("manifest.toml"), body).unwrap();
    }

    #[test]
    fn inspect_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let got = inspect_plugins(&tmp.path().join("missing"), &TrustedPlugins::none()).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn inspect_reports_untrusted_with_capabilities() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(
            tmp.path(),
            "p",
            "id=\"p\"\nname=\"P\"\nversion=\"1\"\n\
             [engine]\ncommand=\"./p\"\ncapabilities=[\"vault:read\",\"net\"]\n",
        );
        let got = inspect_plugins(tmp.path(), &TrustedPlugins::none()).unwrap();
        assert_eq!(got.len(), 1);
        let p = &got[0];
        assert_eq!(p.dir_name, "p");
        assert_eq!(p.status, TrustStatus::Untrusted);
        let m = p.manifest.as_ref().unwrap();
        assert_eq!(m.capabilities, vec![Capability::VaultRead, Capability::Net]);
        assert!(p.computed_hash.is_some());
    }

    #[test]
    fn inspect_distinguishes_pinned_drift_unpinned() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(
            tmp.path(),
            "p",
            "id=\"p\"\nname=\"P\"\nversion=\"1\"\n[engine]\ncommand=\"./p\"\n",
        );
        let real = PinnedHash::of_dir(&tmp.path().join("p")).unwrap();

        // Pinned + matching hash -> Pinned.
        let pinned =
            TrustedPlugins::from_entries([("p".to_string(), Some(real.to_string()))]).unwrap();
        assert_eq!(inspect_plugins(tmp.path(), &pinned).unwrap()[0].status, TrustStatus::Pinned);

        // Pinned + wrong hash -> Drift.
        let wrong = format!("sha256:{}", "0".repeat(64));
        let drift = TrustedPlugins::from_entries([("p".to_string(), Some(wrong))]).unwrap();
        assert_eq!(inspect_plugins(tmp.path(), &drift).unwrap()[0].status, TrustStatus::Drift);

        // Trusted, no pin -> TrustedUnpinned.
        let unpinned = TrustedPlugins::from_ids(["p".to_string()]);
        assert_eq!(
            inspect_plugins(tmp.path(), &unpinned).unwrap()[0].status,
            TrustStatus::TrustedUnpinned
        );
    }

    #[test]
    fn inspect_marks_malformed_manifest_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(tmp.path(), "p", "this is not valid toml {{{");
        let got = inspect_plugins(tmp.path(), &TrustedPlugins::from_ids(["p".to_string()])).unwrap();
        assert_eq!(got[0].status, TrustStatus::Unreadable);
        assert!(got[0].manifest.is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-infra inspect_`
Expected: FAIL — `cannot find function inspect_plugins` / `TrustStatus`.

- [ ] **Step 3: Add the inspection types + function**

Add to `crates/cairn-infra/src/plugin_host.rs`, after the `TrustedPlugins` impl block (before `struct LoadedPlugin`):

```rust
/// How an on-disk plugin stands relative to the trusted set, as reported by
/// [`inspect_plugins`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    /// Not listed in `[plugins].trusted`.
    Untrusted,
    /// Trusted, but no content hash is pinned.
    TrustedUnpinned,
    /// Trusted and the pinned hash matches the directory's current contents.
    Pinned,
    /// Trusted with a pinned hash, but the contents have changed since.
    Drift,
    /// The manifest is missing/malformed or the directory cannot be hashed.
    Unreadable,
}

/// The manifest fields surfaced at approval time (read-only view; no spawn).
#[derive(Debug, Clone, PartialEq)]
pub struct InspectedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub command: String,
    pub capabilities: Vec<Capability>,
}

/// One plugin directory as seen by [`inspect_plugins`].
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInspection {
    pub dir_name: String,
    /// `None` when the manifest is missing/malformed.
    pub manifest: Option<InspectedManifest>,
    /// `None` when the directory cannot be hashed (e.g. contains a symlink).
    pub computed_hash: Option<PinnedHash>,
    pub status: TrustStatus,
}

/// Read-only inspection of every plugin directory under `dir`, for the CLI's
/// `plugin list` / `plugin trust` review flow. **No process is spawned.**
///
/// Unlike the daemon's automatic load path, this **does** parse untrusted
/// manifests — that is the point of letting a user review a plugin before
/// trusting it. It is a user-initiated, read-only parse (no code execution), so
/// it does not weaken the load path's "never parse untrusted bytes during spawn"
/// invariant.
///
/// # Errors
/// [`PortError::Adapter`] only on an unexpected IO error reading `dir`; an absent
/// `dir` yields an empty `Vec`. A per-plugin problem (bad manifest, un-hashable
/// dir) becomes [`TrustStatus::Unreadable`], never an error.
pub fn inspect_plugins(
    dir: &Path,
    trusted: &TrustedPlugins,
) -> Result<Vec<PluginInspection>, PortError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(adapt(e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let plugin_dir = match entry {
            Ok(e) if e.path().is_dir() => e.path(),
            _ => continue,
        };
        let Some(dir_name) = plugin_dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
        else {
            continue; // unnameable in a trust list
        };
        let dir_name = dir_name.to_string();
        let computed = PinnedHash::of_dir(&plugin_dir).ok();
        let manifest = read_inspected_manifest(&plugin_dir).ok();
        let status = match (trusted.get(&dir_name), &manifest, &computed) {
            // Cannot review what we cannot read/hash.
            (_, None, _) | (_, _, None) => TrustStatus::Unreadable,
            (None, _, _) => TrustStatus::Untrusted,
            (Some(None), _, _) => TrustStatus::TrustedUnpinned,
            (Some(Some(pin)), _, Some(c)) => {
                if pin == c {
                    TrustStatus::Pinned
                } else {
                    TrustStatus::Drift
                }
            }
        };
        out.push(PluginInspection {
            dir_name,
            manifest,
            computed_hash: computed,
            status,
        });
    }
    Ok(out)
}

/// Read + parse a plugin's `manifest.toml` into the surfaced subset. An unknown
/// capability fails here (fail-closed), surfacing as `Unreadable`.
fn read_inspected_manifest(plugin_dir: &Path) -> Result<InspectedManifest, PortError> {
    let raw = std::fs::read_to_string(plugin_dir.join("manifest.toml")).map_err(adapt)?;
    let m: Manifest = toml::from_str(&raw).map_err(adapt)?;
    Ok(InspectedManifest {
        id: m.id,
        name: m.name,
        version: m.version,
        command: m.engine.command,
        capabilities: m.engine.capabilities,
    })
}
```

- [ ] **Step 4: Export the new items**

In `crates/cairn-infra/src/lib.rs`, update the `plugin_host` re-export line and add a `Capability` re-export so the CLI need not depend on the protocol crate directly:

```rust
pub use cairn_plugin_protocol::Capability;
pub use plugin_host::{
    inspect_plugins, InspectedManifest, PluginInspection, ProcessPluginHost, TrustStatus,
    TrustedPlugins, DEFAULT_PLUGIN_TIMEOUT,
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p cairn-infra`
Expected: PASS (new `inspect_*` tests + existing host tests).

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(plugins): add read-only inspect_plugins sweep (#40)"
```

---

### Task 4: Shared `[plugins].trusted` parser + `TrustedPlugins::from_cairn_toml`

Move the trusted-list TOML parser into `cairn-infra` (one parser for daemon + CLI) and add a `from_cairn_toml` constructor.

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs` (add `TrustedEntry`/`PinnedEntry`/`normalize`, `from_cairn_toml`, tests)
- Modify: `crates/cairn-infra/src/lib.rs` (export `TrustedEntry`)
- Modify: `crates/cairn-daemon/src/config.rs` (re-import `TrustedEntry`, drop local defs)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/cairn-infra/src/plugin_host.rs`:

```rust
    #[test]
    fn from_cairn_toml_absent_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let t = TrustedPlugins::from_cairn_toml(tmp.path()).unwrap();
        assert!(t.get("anything").is_none());
    }

    #[test]
    fn from_cairn_toml_reads_legacy_and_table_forms() {
        let tmp = tempfile::tempdir().unwrap();
        let pin = format!("sha256:{}", "a".repeat(64));
        std::fs::write(
            tmp.path().join("cairn.toml"),
            format!(
                "[plugins]\ntrusted = [\"legacy\"]\n\
                 [[plugins.trusted]]\ndir = \"pinned\"\nhash = \"{pin}\"\n"
            ),
        )
        .unwrap();
        let t = TrustedPlugins::from_cairn_toml(tmp.path()).unwrap();
        assert!(matches!(t.get("legacy"), Some(None)));
        assert!(matches!(t.get("pinned"), Some(Some(_))));
        assert!(t.get("absent").is_none());
    }

    #[test]
    fn from_cairn_toml_rejects_bad_pin() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[[plugins.trusted]]\ndir = \"a\"\nhash = \"bogus\"\n",
        )
        .unwrap();
        assert!(TrustedPlugins::from_cairn_toml(tmp.path()).is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-infra from_cairn_toml`
Expected: FAIL — `no function from_cairn_toml`.

- [ ] **Step 3: Move the parser types into `plugin_host.rs`**

At the top of `crates/cairn-infra/src/plugin_host.rs`, add `serde::Deserialize` to imports:
```rust
use serde::Deserialize;
```

Add these types (place them just before `TrustedPlugins`):

```rust
/// One entry in `[plugins].trusted`. Untagged so both the bare string form
/// (`trusted = ["name"]`) and the table form (`[[plugins.trusted]] dir = …
/// hash = …`) parse. A bare string and a table without `hash` both mean
/// "trusted, unpinned".
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TrustedEntry {
    /// Trust by directory name, no pin.
    Name(String),
    /// Directory name plus an optional pinned content hash.
    Pinned(PinnedEntry),
}

/// The table form of a [`TrustedEntry`]. `deny_unknown_fields` is essential:
/// without it a typo'd `hsah = "…"` would silently drop the pin and the plugin
/// would run unpinned. (The deny lives on this inner struct, not the untagged
/// enum, where serde ignores it.)
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PinnedEntry {
    dir: String,
    #[serde(default)]
    hash: Option<String>,
}

impl TrustedEntry {
    /// Reduce to `(dir_name, optional_pin_string)` for [`TrustedPlugins`].
    pub fn normalize(&self) -> (String, Option<String>) {
        match self {
            TrustedEntry::Name(dir) => (dir.clone(), None),
            TrustedEntry::Pinned(p) => (p.dir.clone(), p.hash.clone()),
        }
    }
}

/// Minimal `cairn.toml` view: just enough to extract `[plugins].trusted`.
/// Unknown sections/keys are ignored (no top-level `deny_unknown_fields`).
#[derive(Debug, Default, Deserialize)]
struct TrustedListConfig {
    #[serde(default)]
    plugins: TrustedListPlugins,
}

#[derive(Debug, Default, Deserialize)]
struct TrustedListPlugins {
    #[serde(default)]
    trusted: Vec<TrustedEntry>,
}
```

Add the constructor to the `impl TrustedPlugins` block:

```rust
    /// Build from `<cairn_root>/cairn.toml` `[plugins].trusted`. An absent file
    /// or absent section yields [`Self::none`] (default-deny).
    ///
    /// # Errors
    /// [`PortError::Adapter`] on an IO error, malformed TOML, or a malformed pin
    /// (fail-fast: a typo'd pin must not degrade to "unpinned").
    pub fn from_cairn_toml(cairn_root: &Path) -> Result<Self, PortError> {
        let path = cairn_root.join("cairn.toml");
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::none()),
            Err(e) => return Err(adapt(e)),
        };
        let cfg: TrustedListConfig = toml::from_str(&raw).map_err(adapt)?;
        Self::from_entries(cfg.plugins.trusted.iter().map(|e| e.normalize()))
    }
```

- [ ] **Step 4: Export `TrustedEntry`**

In `crates/cairn-infra/src/lib.rs`, add `TrustedEntry` to the `plugin_host` re-export list:

```rust
pub use plugin_host::{
    inspect_plugins, InspectedManifest, PluginInspection, ProcessPluginHost, TrustStatus,
    TrustedEntry, TrustedPlugins, DEFAULT_PLUGIN_TIMEOUT,
};
```

- [ ] **Step 5: Re-import `TrustedEntry` in the daemon config**

In `crates/cairn-daemon/src/config.rs`:

1. Delete the local `TrustedEntry` enum, the `PinnedEntry` struct, and the `impl TrustedEntry { fn normalize … }` block (lines ~39–73).
2. Add the import near the top (after the existing `use serde::Deserialize;`):
```rust
use cairn_infra::TrustedEntry;
```
3. The `PluginsConfig.trusted: Vec<TrustedEntry>` field and the config tests (which call `.normalize()`) now use the re-imported type unchanged.

- [ ] **Step 6: Run the workspace tests**

Run: `cargo test`
Expected: PASS — infra `from_cairn_toml` tests pass; daemon config tests (`plugins_trusted_*`) still pass against the moved `TrustedEntry`.

- [ ] **Step 7: Commit**

```bash
git branch --show-current
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-infra/src/lib.rs crates/cairn-daemon/src/config.rs
git commit -m "refactor(plugins): share [plugins].trusted parser via from_cairn_toml (#40)"
```

---

### Task 5: `cairn plugin list` subcommand

Add the `plugin` subcommand group and implement `list`. Dispatch it before `build_engine` (it needs only the plugins dir + cairn.toml, not the engine).

**Files:**
- Modify: `crates/cairn-cli/src/main.rs` (Command enum, dispatch, rendering)
- Modify: `crates/cairn-cli/tests/cli.rs` (integration test)

- [ ] **Step 1: Write the failing integration test**

Add to `crates/cairn-cli/tests/cli.rs`:

```rust
/// Create `<dir>/.cairn/plugins/<name>/manifest.toml` with the given body.
fn write_plugin_manifest(dir: &std::path::Path, name: &str, body: &str) {
    let pdir = dir.join(".cairn").join("plugins").join(name);
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(pdir.join("manifest.toml"), body).unwrap();
}

#[test]
fn plugin_list_shows_status_and_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    write_plugin_manifest(
        dir,
        "fetch-bot",
        "id=\"fetch-bot\"\nname=\"Fetch Bot\"\nversion=\"1.0.0\"\n\
         [engine]\ncommand=\"./fetch-bot\"\ncapabilities=[\"vault:read\",\"net\"]\n",
    );
    cairn(dir)
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(
            contains("fetch-bot")
                .and(contains("untrusted"))
                .and(contains("vault:read"))
                .and(contains("net")),
        );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-cli plugin_list_shows_status_and_capabilities`
Expected: FAIL — clap rejects the unknown `plugin` subcommand.

- [ ] **Step 3: Add the subcommand to the `Command` enum**

In `crates/cairn-cli/src/main.rs`, add a variant to `enum Command` (after `Restore { … }`, before the closing `}`):

```rust
    /// Inspect and approve engine plugins.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
```

And add the subcommand enum just after the `Command` enum's closing brace:

```rust
#[derive(Subcommand)]
enum PluginAction {
    /// List discovered plugins, their trust status, and declared capabilities.
    List,
    /// Review a plugin and print the cairn.toml entry needed to trust it.
    Trust {
        /// The plugin's directory name under `.cairn/plugins/`.
        dir: String,
    },
}
```

- [ ] **Step 4: Dispatch `Plugin` before building the engine**

In `run()`, immediately after the `ensure_cairn(&root)…` block and **before** `let mut engine = build_engine(&root)…`, insert:

```rust
    // Plugin inspection/approval needs only the plugins dir + cairn.toml, not
    // the engine — handle it before the (potentially expensive) engine build.
    if let Command::Plugin { action } = &cli.command {
        return run_plugin(&root, action);
    }
```

- [ ] **Step 5: Implement `run_plugin` + list rendering**

Add these functions near the other free functions in `main.rs` (e.g. after `short_query_hint`). Add `use cairn_infra::{inspect_plugins, PluginInspection, TrustStatus, TrustedPlugins};` to the imports (the file already imports from `cairn_infra`):

```rust
fn run_plugin(root: &Path, action: &PluginAction) -> Result<(), String> {
    let plugins_dir = root.join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_cairn_toml(root).map_err(|e| e.to_string())?;
    let inspections = inspect_plugins(&plugins_dir, &trusted).map_err(|e| e.to_string())?;
    match action {
        PluginAction::List => {
            print_plugin_list(&inspections);
            Ok(())
        }
        PluginAction::Trust { dir } => run_plugin_trust(&inspections, dir),
    }
}

/// Human label for a trust status.
fn status_label(status: TrustStatus) -> &'static str {
    match status {
        TrustStatus::Untrusted => "untrusted",
        TrustStatus::TrustedUnpinned => "trusted (unpinned)",
        TrustStatus::Pinned => "trusted (pinned)",
        TrustStatus::Drift => "DRIFT — contents changed since pinned",
        TrustStatus::Unreadable => "unreadable manifest",
    }
}

fn print_plugin_list(inspections: &[PluginInspection]) {
    if inspections.is_empty() {
        println!("no plugins found under .cairn/plugins");
        return;
    }
    for insp in inspections {
        let (name, version) = match &insp.manifest {
            Some(m) => (m.name.as_str(), m.version.as_str()),
            None => ("?", "?"),
        };
        println!(
            "{}  v{}  [{}]  {}",
            insp.dir_name,
            version,
            status_label(insp.status),
            name
        );
        let caps = match &insp.manifest {
            Some(m) if !m.capabilities.is_empty() => m
                .capabilities
                .iter()
                .map(|c| c.wire())
                .collect::<Vec<_>>()
                .join(", "),
            Some(_) => "(none)".to_string(),
            None => "(unknown)".to_string(),
        };
        println!("  capabilities: {caps}");
    }
}
```

(`run_plugin_trust` is added in Task 6 — for now add a temporary stub so this compiles:)

```rust
fn run_plugin_trust(_inspections: &[PluginInspection], _dir: &str) -> Result<(), String> {
    Err("not yet implemented".to_string())
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p cairn-cli plugin_list_shows_status_and_capabilities`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git branch --show-current
git add crates/cairn-cli/src/main.rs crates/cairn-cli/tests/cli.rs
git commit -m "feat(cli): add cairn plugin list (#40)"
```

---

### Task 6: `cairn plugin trust` approval flow

Replace the stub: render the approval screen, prompt `y/N` (non-TTY/EOF → refuse), and on yes print the `[[plugins.trusted]]` snippet. Never writes config.

**Files:**
- Modify: `crates/cairn-cli/src/main.rs` (replace `run_plugin_trust`, add helpers)
- Modify: `crates/cairn-cli/tests/cli.rs` (integration tests)

- [ ] **Step 1: Write the failing integration tests**

Add to `crates/cairn-cli/tests/cli.rs`:

```rust
#[test]
fn plugin_trust_yes_prints_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    write_plugin_manifest(
        dir,
        "fetch-bot",
        "id=\"fetch-bot\"\nname=\"Fetch Bot\"\nversion=\"1.0.0\"\n\
         [engine]\ncommand=\"./fetch-bot\"\ncapabilities=[\"vault:read\",\"net\"]\n",
    );
    cairn(dir)
        .args(["plugin", "trust", "fetch-bot"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(
            contains("vault:read")
                .and(contains("enforced in a future release")) // net's label
                .and(contains("[[plugins.trusted]]"))
                .and(contains("dir = \"fetch-bot\""))
                .and(contains("hash = \"sha256:")),
        );
}

#[test]
fn plugin_trust_declined_prints_no_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    write_plugin_manifest(
        dir,
        "fetch-bot",
        "id=\"fetch-bot\"\nname=\"Fetch Bot\"\nversion=\"1.0.0\"\n\
         [engine]\ncommand=\"./fetch-bot\"\ncapabilities=[]\n",
    );
    // Empty stdin == EOF == non-interactive == refuse.
    cairn(dir)
        .args(["plugin", "trust", "fetch-bot"])
        .write_stdin("")
        .assert()
        .success()
        .stdout(contains("Not trusted").and(contains("[[plugins.trusted]]").not()));
}

#[test]
fn plugin_trust_unknown_dir_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["plugin", "trust", "ghost"])
        .write_stdin("y\n")
        .assert()
        .failure()
        .stderr(contains("ghost"));
}
```

(`contains(...).not()` needs `PredicateBooleanExt`, already imported at the top of `cli.rs`.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-cli plugin_trust`
Expected: FAIL — the stub returns "not yet implemented" (the yes/declined tests fail; the unknown-dir test may already pass via the stub error, but will be re-validated).

- [ ] **Step 3: Replace the stub with the real flow**

In `crates/cairn-cli/src/main.rs`, replace the `run_plugin_trust` stub with:

```rust
fn run_plugin_trust(inspections: &[PluginInspection], dir: &str) -> Result<(), String> {
    let insp = inspections
        .iter()
        .find(|i| i.dir_name == dir)
        .ok_or_else(|| format!("no plugin directory named {dir:?} under .cairn/plugins"))?;
    let manifest = insp
        .manifest
        .as_ref()
        .ok_or_else(|| format!("plugin {dir:?} has an unreadable manifest; cannot trust it"))?;
    let hash = insp
        .computed_hash
        .as_ref()
        .ok_or_else(|| format!("plugin {dir:?} could not be hashed; cannot trust it"))?;

    print_approval_screen(manifest, &hash.to_string());

    if !confirm_yes("  Approve and trust this exact version? [y/N]: ")? {
        println!("  Not trusted.");
        return Ok(());
    }

    println!();
    println!("  Add this to your cairn.toml to trust {dir}:");
    println!();
    println!("      [[plugins.trusted]]");
    println!("      dir = \"{dir}\"");
    println!("      hash = \"{hash}\"");
    Ok(())
}

/// Render the first-run approval screen for a plugin under review.
fn print_approval_screen(m: &cairn_infra::InspectedManifest, hash: &str) {
    println!();
    println!("  Plugin:   {}  ({}  v{})", m.id, m.name, m.version);
    println!(
        "  Command:  {}          (runs as a sandboxed child of the daemon)",
        m.command
    );
    println!("  Content:  {hash}");
    println!();
    if m.capabilities.is_empty() {
        println!("  This plugin declares no capabilities.");
    } else {
        println!("  Capabilities this plugin declares:");
        for cap in &m.capabilities {
            let suffix = if cap.enforced_today() {
                ""
            } else {
                "   (enforced in a future release)"
            };
            println!("    • {:<13} {}{}", cap.wire(), cap.summary(), suffix);
        }
    }
    println!();
}

/// Prompt on stdout and read a yes/no answer from stdin. Empty input, EOF
/// (non-interactive pipe), or anything other than y/yes is treated as **no** —
/// a scripted pipe must never silently approve a plugin.
fn confirm_yes(prompt: &str) -> Result<bool, String> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    let n = std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(false); // EOF: non-interactive
    }
    Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}
```

Add `InspectedManifest` to the `cairn_infra` import group (alongside `inspect_plugins, PluginInspection, TrustStatus, TrustedPlugins`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-cli plugin_trust`
Expected: PASS (yes prints snippet; declined prints "Not trusted" and no snippet; unknown dir errors to stderr).

- [ ] **Step 5: Run the full workspace test + clippy**

Run: `cargo test && cargo clippy --workspace --all-targets`
Expected: PASS, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git add crates/cairn-cli/src/main.rs crates/cairn-cli/tests/cli.rs
git commit -m "feat(cli): add cairn plugin trust approval flow (#40)"
```

---

### Task 7: Documentation touch-ups

Reflect the new vocabulary and approval flow where the codebase already documents plugin trust.

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs` (capability names in any doc/security note)
- Modify: any plugin docs under `docs/` that list `fs:read`/`fs:write`/`events`

- [ ] **Step 1: Find stale capability references in docs/SDK**

Run:
```bash
grep -rn 'fs:read\|fs:write\|"events"\|capabilities' crates/cairn-plugin-sdk/src/ docs/decisions/ docs/ 2>/dev/null | grep -v 'specs/2026-06-14-plugin-capability'
```

- [ ] **Step 2: Update each hit to the `vault:*` names and mention the new caps**

For each doc/comment that enumerates capabilities, replace `fs:read`→`vault:read`, `fs:write`→`vault:write`, `events`→`vault:events`, and add a one-line note that `net`/`exec`/`fs:read` are declared-and-surfaced now / enforced by #63. Keep edits minimal and in the existing voice. (If a file's capability mention is illustrative prose that still reads correctly, leave it; only fix concrete name lists.)

- [ ] **Step 3: Verify nothing else references the old strings in shipping code/docs**

Run:
```bash
grep -rn 'CAP_FS_READ\|CAP_FS_WRITE\|CAP_EVENTS' crates/
```
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git branch --show-current
git add -A
git commit -m "docs(plugins): update capability names + first-run approval (#40)"
```

---

## Self-Review

**Spec coverage:**
- Typed `Capability` enum, two domains, `wire`/`summary`/`enforced_today` → Task 1. ✓
- Fail-closed unknown-capability parsing → Task 1 (serde) + Task 2 (manifest test) + Task 3 (`Unreadable`). ✓
- `vault:*` rename + host gate migration + `CAP_*` removal → Task 2. ✓
- `fs:read` coarse switch (no payload) → Task 1 enum (unit variant). ✓
- Enforcement-lag (declare+surface, spawn under fixed jail, "enforced in a future release" label) → Task 1 `enforced_today` + Task 6 approval screen. ✓
- `inspect_plugins` + `PluginInspection`/`InspectedManifest`/`TrustStatus`, untrusted-parse distinction, per-entry `Unreadable` → Task 3. ✓
- Shared trusted-set parser move + `from_cairn_toml` → Task 4. ✓
- `cairn plugin list` → Task 5; `cairn plugin trust` with y/N (non-TTY→refuse) + snippet, no file write → Task 6. ✓
- Docs touch-up → Task 7. ✓
- Out of scope (sandbox enforcement #63, auto-write `cairn.toml`, path-scoped reads) → not built. ✓

**Placeholder scan:** Task 5 introduces a deliberate, named temporary stub for `run_plugin_trust` that Task 6 replaces — not a plan placeholder; every other step has complete code/commands.

**Type consistency:** `Capability::{VaultRead,VaultWrite,VaultEvents,Net,Exec,FsRead}`, `wire()`/`summary()`/`enforced_today()`, `inspect_plugins`, `PluginInspection{dir_name,manifest,computed_hash,status}`, `InspectedManifest{id,name,version,command,capabilities}`, `TrustStatus{Untrusted,TrustedUnpinned,Pinned,Drift,Unreadable}`, `TrustedPlugins::{none,from_ids,from_entries,get,from_cairn_toml}`, `TrustedEntry::normalize`, `run_plugin`/`run_plugin_trust`/`print_plugin_list`/`print_approval_screen`/`confirm_yes`/`status_label` — names used consistently across tasks.
