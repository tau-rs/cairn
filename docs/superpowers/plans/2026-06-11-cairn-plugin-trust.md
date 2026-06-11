# Plugin Trust Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate plugin spawning behind a default-deny allowlist keyed on the plugin directory name, so a present-on-disk plugin runs only when the user has explicitly trusted it in `cairn.toml`.

**Architecture:** Introduce a typed `TrustedPlugins` set in `cairn-infra`. `ProcessPluginHost::load`/`load_with_timeout` require it and skip any directory not in the set *before parsing its manifest*; a trusted directory whose `manifest.id` differs from the directory name is also skipped. The daemon reads `[plugins] trusted = [...]` from `cairn.toml` and passes the set in. Empty set ⇒ nothing spawns.

**Tech Stack:** Rust, `serde`/`toml`, `std::process::Command`, existing cairn workspace crates (`cairn-infra`, `cairn-daemon`, `cairn-plugin-example` integration tests).

---

## File Structure

- `crates/cairn-infra/src/plugin_host.rs` — add `TrustedPlugins`, gate the load loop, update `load`/`load_with_timeout` signatures, unit tests.
- `crates/cairn-infra/src/lib.rs` — re-export `TrustedPlugins`.
- `crates/cairn-daemon/src/config.rs` — add `trusted: Vec<String>` to `PluginsConfig` + parse tests.
- `crates/cairn-daemon/src/main.rs` — build `TrustedPlugins` from config, pass to `load_with_timeout`, log pointer.
- `crates/cairn-plugin-example/tests/host.rs` — local `load`/`load_with_timeout` helpers that trust `"example"`; new trust-gate integration tests.

---

## Task 1: `TrustedPlugins` type + gate in the load loop (cairn-infra)

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs` (add type near top; change `load`/`load_with_timeout` ~lines 346-373; add unit tests in the existing `#[cfg(test)] mod tests`)
- Modify: `crates/cairn-infra/src/lib.rs:15`

This task changes the public signatures of `load`/`load_with_timeout`, which will break callers in Tasks 2–4; that is expected and those tasks fix them. Implement cairn-infra fully here (it compiles on its own; the example integration tests in Task 4 are a separate test target).

- [ ] **Step 1: Add the failing unit tests**

In `crates/cairn-infra/src/plugin_host.rs`, replace the existing two-test `mod tests` block (currently `load_absent_dir_is_empty` and `unspawnable_plugin_is_skipped_not_fatal`) with the version below. It keeps both existing tests (updated to pass a trust set) and adds three new gate tests. The `write_manifest` helper writes a manifest whose `command` points at a path that cannot spawn — that is fine, because these tests assert on which directories are *attempted*, distinguishing "skipped before parse" from "trusted but failed to spawn" via the captured stderr is unnecessary: instead they assert via `host.plugins()` for the empty cases and via a real spawn only where needed. Since cairn-infra has no example binary, the "approved IS spawned" behaviour is covered in Task 4 (integration). Here we assert the *negative* gate decisions, which need no working binary.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Write `<dir>/<name>/manifest.toml` declaring `id` and a non-spawnable
    /// command. Returns the plugins root (`<dir>`).
    fn write_plugin(root: &Path, dir_name: &str, manifest_id: &str) {
        let pdir = root.join(dir_name);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("manifest.toml"),
            format!(
                "id=\"{manifest_id}\"\nname=\"N\"\nversion=\"0\"\n\
                 [engine]\ncommand=\"/nonexistent/xyz\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn load_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let trusted = TrustedPlugins::from_ids(["anything".to_string()]);
        let host =
            ProcessPluginHost::load(&tmp.path().join("missing"), &trusted).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn unspawnable_plugin_is_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "broken", "broken");
        let trusted = TrustedPlugins::from_ids(["broken".to_string()]);
        // Trusted but the command can't spawn: load succeeds, plugin absent.
        let host = ProcessPluginHost::load(tmp.path(), &trusted).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn untrusted_plugin_is_not_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "rogue", "rogue");
        // Empty trust set => default-deny.
        let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none()).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn untrusted_manifest_is_not_parsed() {
        // A directory not in the trust set must be skipped *before* its manifest
        // is read. A malformed manifest there must therefore NOT cause an error.
        let tmp = tempfile::tempdir().unwrap();
        let pdir = tmp.path().join("rogue");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("manifest.toml"), "this is not valid toml {{{").unwrap();
        let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none()).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn trusted_set_membership() {
        let trusted = TrustedPlugins::from_ids(["a".to_string(), "b".to_string()]);
        assert!(trusted.contains("a"));
        assert!(trusted.contains("b"));
        assert!(!trusted.contains("c"));
        assert!(!TrustedPlugins::none().contains("a"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-infra --lib plugin_host`
Expected: FAIL — compile errors (`TrustedPlugins` not found; `load` takes 1 arg not 2).

- [ ] **Step 3: Add the `TrustedPlugins` type**

In `crates/cairn-infra/src/plugin_host.rs`, add `use std::collections::HashSet;` to the imports at the top, then add this type just above `struct LoadedPlugin` (around line 55):

```rust
/// The set of plugin **directory names** the user has explicitly trusted. A
/// plugin under `<cairn>/.cairn/plugins/<dir>` is spawned only if `<dir>` is in
/// this set — the directory name is the trust anchor because the user controls
/// it, unlike the manifest's self-declared `id`. An empty set trusts nothing.
#[derive(Debug, Default, Clone)]
pub struct TrustedPlugins(HashSet<String>);

impl TrustedPlugins {
    /// A set that trusts no plugin (default-deny).
    pub fn none() -> Self {
        Self::default()
    }

    /// Build from an iterator of trusted directory names.
    pub fn from_ids<I: IntoIterator<Item = String>>(ids: I) -> Self {
        Self(ids.into_iter().collect())
    }

    /// Is this plugin directory name trusted?
    pub fn contains(&self, dir_name: &str) -> bool {
        self.0.contains(dir_name)
    }
}
```

(A plain `from_ids` constructor is used rather than a blanket `impl<I:
IntoIterator> From<I>`, which would collide with std's reflexive `impl<T> From<T>
for T` under coherence.)

- [ ] **Step 4: Thread `trusted` through `load`/`load_with_timeout` and gate the loop**

In `crates/cairn-infra/src/plugin_host.rs`, change `load` (currently ~lines 346-348):

```rust
    pub fn load(dir: &Path, trusted: &TrustedPlugins) -> Result<Self, PortError> {
        Self::load_with_timeout(dir, DEFAULT_PLUGIN_TIMEOUT, trusted)
    }
```

Change `load_with_timeout` (currently ~lines 355-373) to take `trusted` and gate each entry. Replace the loop body so the trust check happens before `spawn_plugin`:

```rust
    pub fn load_with_timeout(
        dir: &Path,
        timeout: Duration,
        trusted: &TrustedPlugins,
    ) -> Result<Self, PortError> {
        let mut loaded = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(adapt(e)),
        };
        for entry in entries {
            let plugin_dir = match entry {
                Ok(e) if e.path().is_dir() => e.path(),
                _ => continue,
            };
            // Trust gate: the directory name (not the manifest's self-declared
            // id) is the trust anchor. Untrusted dirs are skipped before their
            // manifest is even read, so attacker-controlled TOML is never parsed.
            let dir_name = plugin_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !trusted.contains(dir_name) {
                eprintln!(
                    "plugin: skipping {dir_name} (not in [plugins] trusted; \
                     add \"{dir_name}\" to cairn.toml to enable)"
                );
                continue;
            }
            match Self::spawn_plugin(&plugin_dir, timeout) {
                Ok(p) => loaded.push(p),
                Err(e) => eprintln!("plugin: skipping {}: {e}", plugin_dir.display()),
            }
        }
        Ok(Self { loaded })
    }
```

- [ ] **Step 5: Enforce `manifest.id == dir_name` in `spawn_plugin`**

In `crates/cairn-infra/src/plugin_host.rs`, in `spawn_plugin` (currently starting ~line 375), after the manifest is parsed (`let manifest: Manifest = toml::from_str(&raw).map_err(adapt)?;`) insert the consistency check:

```rust
        let dir_name = plugin_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if manifest.id != dir_name {
            return Err(PortError::Adapter(format!(
                "manifest id \"{}\" does not match directory name \"{dir_name}\"",
                manifest.id
            )));
        }
```

(The `Err` is logged by the caller's existing `plugin: skipping {}: {e}` arm, giving the distinct id-mismatch line the spec calls for.)

- [ ] **Step 6: Re-export `TrustedPlugins`**

In `crates/cairn-infra/src/lib.rs:15`, change:

```rust
pub use plugin_host::{ProcessPluginHost, TrustedPlugins, DEFAULT_PLUGIN_TIMEOUT};
```

- [ ] **Step 7: Run the unit tests to verify they pass**

Run: `cargo test -p cairn-infra --lib plugin_host`
Expected: PASS (all five tests).

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): default-deny plugin trust gate keyed on directory name"
```

---

## Task 2: Config field `[plugins] trusted` (cairn-daemon)

**Files:**
- Modify: `crates/cairn-daemon/src/config.rs:24-31` (struct) and the `#[cfg(test)] mod tests` block

- [ ] **Step 1: Add the failing parse tests**

In `crates/cairn-daemon/src/config.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn plugins_trusted_parses() {
        let c: Config = toml::from_str("[plugins]\ntrusted = [\"a\", \"b\"]").unwrap();
        assert_eq!(c.plugins.trusted, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn plugins_trusted_defaults_empty() {
        assert!(toml::from_str::<Config>("").unwrap().plugins.trusted.is_empty());
        assert!(toml::from_str::<Config>("[plugins]\n")
            .unwrap()
            .plugins
            .trusted
            .is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-daemon --lib config::tests::plugins_trusted`
Expected: FAIL — `no field trusted on type PluginsConfig`.

- [ ] **Step 3: Add the field**

In `crates/cairn-daemon/src/config.rs`, add to `PluginsConfig` (after the `timeout_secs` field, before the closing brace at ~line 31):

```rust
    /// Plugin directory names the user trusts to spawn. Absent/empty ⇒ no plugin
    /// is spawned (default-deny). The name must match the plugin's directory
    /// under `<cairn>/.cairn/plugins/`.
    #[serde(default)]
    pub trusted: Vec<String>,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon --lib config::tests::plugins_trusted`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/config.rs
git commit -m "feat(daemon): parse [plugins] trusted allowlist from cairn.toml"
```

---

## Task 3: Wire the allowlist into the daemon (cairn-daemon)

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs:100-108`

No new test here (behaviour is covered by Task 1 unit tests + Task 4 integration; main.rs is a thin wiring layer). Verify via build.

- [ ] **Step 1: Pass the trust set to the host**

In `crates/cairn-daemon/src/main.rs`, replace the plugin-load block (currently lines 100-108):

```rust
    // Load engine plugins from <cairn>/.cairn/plugins (absent dir => none).
    // Default-deny: only directories listed in [plugins].trusted are spawned.
    let plugins_dir = cli.cairn.join(".cairn").join("plugins");
    let trusted = cairn_infra::TrustedPlugins::from_ids(config.plugins.trusted.clone());
    if config.plugins.trusted.is_empty() {
        println!(
            "plugins: none trusted (add [plugins].trusted = [\"<dir>\"] to {}/cairn.toml to enable)",
            cli.cairn.display()
        );
    }
    match cairn_infra::ProcessPluginHost::load_with_timeout(&plugins_dir, plugin_timeout, &trusted) {
        Ok(host) => {
            engine.set_plugin_host(Box::new(host));
            println!("plugins: read timeout {plugin_timeout:?}");
        }
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }
```

- [ ] **Step 2: Build to verify wiring**

Run: `cargo build -p cairn-daemon`
Expected: builds cleanly.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): gate plugin spawn on [plugins] trusted allowlist"
```

---

## Task 4: Integration tests — approved IS spawned, others are not (cairn-plugin-example)

**Files:**
- Modify: `crates/cairn-plugin-example/tests/host.rs` (add load helpers near the top; update the ~14 existing `ProcessPluginHost::load(...)` / `load_with_timeout(...)` call sites to use them; add new trust-gate tests)

The existing tests all install the plugin into a directory named `example` whose manifest declares `id = "example"`, so they satisfy the new `manifest.id == dir_name` check. They only need to start passing a trust set containing `"example"`. Centralize that in two helpers.

- [ ] **Step 1: Add load helpers**

In `crates/cairn-plugin-example/tests/host.rs`, update the import line and add two helpers after the `write_manifest` function (~line 16):

Change the `use cairn_infra::ProcessPluginHost;` line (line 2) to:

```rust
use cairn_infra::{ProcessPluginHost, TrustedPlugins};
```

Then add:

```rust
/// Load a host from `<tmp>/.cairn/plugins`, trusting the `example` plugin.
fn load_example(tmp: &std::path::Path) -> ProcessPluginHost {
    let dir = tmp.join(".cairn").join("plugins");
    ProcessPluginHost::load(&dir, &TrustedPlugins::from_ids(["example".to_string()])).unwrap()
}

/// Like `load_example` but with an explicit per-message timeout.
fn load_example_with_timeout(
    tmp: &std::path::Path,
    timeout: std::time::Duration,
) -> ProcessPluginHost {
    let dir = tmp.join(".cairn").join("plugins");
    ProcessPluginHost::load_with_timeout(&dir, timeout, &TrustedPlugins::from_ids(["example".to_string()]))
        .unwrap()
}
```

- [ ] **Step 2: Point existing tests at the helpers**

In every existing test in this file, replace the load call. There are two shapes:

Replace each occurrence of
```rust
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
```
with
```rust
    let mut host = load_example(tmp.path());
```

And in `invoke_times_out_and_kills_plugin`, replace
```rust
    let mut host = ProcessPluginHost::load_with_timeout(
        &tmp.path().join(".cairn").join("plugins"),
        Duration::from_millis(2_000),
    )
    .unwrap();
```
with
```rust
    let mut host = load_example_with_timeout(tmp.path(), Duration::from_millis(2_000));
```

- [ ] **Step 3: Add the trust-gate integration tests**

Append to `crates/cairn-plugin-example/tests/host.rs`:

```rust
#[test]
fn approved_plugin_is_spawned_unapproved_is_not() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    // Two valid plugins on disk, each with a working binary.
    write_manifest(&plugins.join("example"), bin, "");
    write_manifest(&plugins.join("rogue"), bin, "");
    // Only `example` is trusted; `rogue` must be skipped.
    let host =
        ProcessPluginHost::load(&plugins, &TrustedPlugins::from_ids(["example".to_string()])).unwrap();
    let ids: Vec<&str> = host.plugins().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["example"]);
}

#[test]
fn default_deny_spawns_nothing() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    write_manifest(&plugins.join("example"), bin, "");
    let host = ProcessPluginHost::load(&plugins, &TrustedPlugins::none()).unwrap();
    assert!(host.plugins().is_empty());
}

#[test]
fn trusted_dir_with_mismatched_manifest_id_is_rejected() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    // Directory `example` is trusted, but its manifest claims id `evil`.
    let pdir = plugins.join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!("id=\"evil\"\nname=\"E\"\nversion=\"0\"\n[engine]\ncommand='{bin}'\n"),
    )
    .unwrap();
    let host =
        ProcessPluginHost::load(&plugins, &TrustedPlugins::from_ids(["example".to_string()])).unwrap();
    assert!(host.plugins().is_empty(), "id-mismatched plugin must not load");
}
```

Note: `write_manifest` writes `id="example"`, so calling it for the `rogue`
directory produces a dir named `rogue` with manifest id `example` — which would
fail the id check. The `approved_plugin_is_spawned_unapproved_is_not` test relies
on `rogue` being skipped by the *trust gate* (before the id check), so this is
correct: `rogue` is skipped for being untrusted, never reaching the id check.

- [ ] **Step 4: Run the integration tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test host`
Expected: PASS (existing tests + 3 new trust-gate tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-example/tests/host.rs
git commit -m "test(plugin): approved plugin spawns, unapproved/id-mismatch do not"
```

---

## Task 5: Full verification

- [ ] **Step 1: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS — no regressions.

- [ ] **Step 2: Lint**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Format check**

Run: `cargo fmt --all -- --check`
Expected: clean (run `cargo fmt --all` if not, then re-commit).

---

## Self-Review notes

- **Spec coverage:** default-deny gate (Task 1, 3), dir-name trust key (Task 1), id-consistency check (Task 1 step 5), untrusted-not-parsed (Task 1 test), config wiring (Task 2, 3), failing-first approved/unapproved + default-deny + id-mismatch tests (Task 1, 4). All spec sections mapped.
- **Type consistency:** `TrustedPlugins::none()`, `TrustedPlugins::from_ids(iter)`, `.contains(&str)` used identically across Tasks 1–4. `load(dir, &TrustedPlugins)` and `load_with_timeout(dir, Duration, &TrustedPlugins)` signatures consistent everywhere.
- **No placeholders:** every step has concrete code/commands.
