# Daemon Plugin-Timeout Config Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `cairn.toml` set the plugin read timeout via `[plugins] timeout_secs`, wired into `ProcessPluginHost::load_with_timeout`.

**Architecture:** A new `[plugins]` section on the daemon `Config` (mirroring `[index]`/`[cors]`), resolved in `main.rs` to a `Duration` (unset → the host's `DEFAULT_PLUGIN_TIMEOUT`; `0` → default + warning) and passed to `load_with_timeout`. Daemon-only.

**Tech Stack:** Rust (workspace, MSRV 1.88), serde/toml, clap, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-plugin-timeout-config-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-daemon/src/config.rs` | `PluginsConfig` + `Config.plugins` + config tests | 1 |
| `crates/cairn-daemon/src/main.rs` | resolve timeout (0-guard, default) + `load_with_timeout` + startup log | 2 |

**Unchanged:** `cairn-infra` (the `load_with_timeout` seam + `DEFAULT_PLUGIN_TIMEOUT` already exist), `cairn-cli`, all other crates.

---

## Task 1: `[plugins] timeout_secs` config field

**Files:**
- Modify: `crates/cairn-daemon/src/config.rs`

- [ ] **Step 1: Write the failing tests**

In the `#[cfg(test)] mod tests` block of `crates/cairn-daemon/src/config.rs`, add:

```rust
    #[test]
    fn plugins_timeout_parses() {
        let c: Config = toml::from_str("[plugins]\ntimeout_secs = 60").unwrap();
        assert_eq!(c.plugins.timeout_secs, Some(60));
    }

    #[test]
    fn plugins_timeout_defaults_none() {
        assert_eq!(
            toml::from_str::<Config>("").unwrap().plugins.timeout_secs,
            None
        );
        assert_eq!(
            toml::from_str::<Config>("[plugins]\n").unwrap().plugins.timeout_secs,
            None
        );
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-daemon --lib plugins_timeout`
Expected: COMPILE failure — `Config` has no `plugins` field.

- [ ] **Step 3: Add `PluginsConfig` + the `Config.plugins` field**

In `crates/cairn-daemon/src/config.rs`, add the field to the `Config` struct (after `index`):

```rust
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// CORS settings.
    #[serde(default)]
    pub cors: CorsConfig,
    /// On-disk index settings.
    #[serde(default)]
    pub index: IndexConfig,
    /// Plugin host settings.
    #[serde(default)]
    pub plugins: PluginsConfig,
}
```

Add the `PluginsConfig` struct (next to `CorsConfig`/`IndexConfig`):

```rust
/// Plugin host settings.
#[derive(Debug, Default, Deserialize)]
pub struct PluginsConfig {
    /// Per-message plugin read timeout, in seconds. Unset → the host default
    /// (`cairn_infra::DEFAULT_PLUGIN_TIMEOUT`, 30s). A configured `0` is invalid
    /// (it would kill every plugin immediately) and is ignored with a warning by
    /// the daemon.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
```

(`Option<u64>` defaults to `None` via `#[derive(Default)]` — no manual `Default` impl needed.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cairn-daemon --lib`
Expected: PASS — the two new tests + all existing config tests.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/config.rs
git commit -m "feat(daemon): [plugins] timeout_secs config field"
```

---

## Task 2: Wire the configured timeout into plugin loading

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs`

This is config-resolution glue (resolve the `Config` value, guard `0`, pass to
`load_with_timeout`, log the effective value). Verified by the daemon building +
existing daemon integration tests passing (they don't set `[plugins]`, so they get
the default — identical behaviour to before).

- [ ] **Step 1: Add the `Duration` import**

In `crates/cairn-daemon/src/main.rs`, add to the `std` imports near the top
(alongside `use std::path::{Path, PathBuf};` and `use std::process::ExitCode;`):

```rust
use std::time::Duration;
```

- [ ] **Step 2: Resolve the timeout and use `load_with_timeout`**

Replace the current plugin-load block:

```rust
    // Load engine plugins from <cairn>/.cairn/plugins (absent dir => none).
    match cairn_infra::ProcessPluginHost::load(&cli.cairn.join(".cairn").join("plugins")) {
        Ok(host) => engine.set_plugin_host(Box::new(host)),
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }
```

with:

```rust
    // Plugin read timeout: cairn.toml `[plugins] timeout_secs`, else the host default.
    let plugin_timeout = match config.plugins.timeout_secs {
        Some(0) => {
            eprintln!(
                "warning: [plugins] timeout_secs = 0 is invalid; using default {:?}",
                cairn_infra::DEFAULT_PLUGIN_TIMEOUT
            );
            cairn_infra::DEFAULT_PLUGIN_TIMEOUT
        }
        Some(s) => Duration::from_secs(s),
        None => cairn_infra::DEFAULT_PLUGIN_TIMEOUT,
    };
    // Load engine plugins from <cairn>/.cairn/plugins (absent dir => none).
    let plugins_dir = cli.cairn.join(".cairn").join("plugins");
    match cairn_infra::ProcessPluginHost::load_with_timeout(&plugins_dir, plugin_timeout) {
        Ok(host) => engine.set_plugin_host(Box::new(host)),
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }
    println!("plugins: read timeout {plugin_timeout:?}");
```

(`config` is the already-loaded `Config` in `run()`. `cairn_infra::DEFAULT_PLUGIN_TIMEOUT`
and `ProcessPluginHost::load_with_timeout` are both `pub` from the timeout slice.)

- [ ] **Step 3: Build the daemon**

Run: `cargo build -p cairn-daemon`
Expected: compiles.

- [ ] **Step 4: Full daemon suite + workspace + lint + fmt + lock**

Run: `cargo test -p cairn-daemon` then `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green (existing daemon integration tests still pass — they don't set `[plugins]`, so plugins load with the 30s default exactly as before), no warnings, fmt clean, lock consistent (no new deps — `cairn-daemon` already depends on `cairn-infra`).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): use [plugins] timeout_secs for the plugin read timeout"
```

---

## Notes for the implementer

- **Default lives in one place:** `cairn_infra::DEFAULT_PLUGIN_TIMEOUT` (the timeout slice). Do not hardcode `30` in the daemon — resolve `None`/`Some(0)` to that constant.
- **`Some(0)` is a guarded footgun:** a 0s timeout kills every plugin on its first read; the daemon warns and falls back to the default rather than passing it through.
- **No new test for the wiring** — it's config-resolution glue in `run()` (not harness-covered); the config *parsing* is tested in Task 1, and the existing daemon integration tests confirm the default path is unchanged. Do NOT add a flaky daemon-spawn test for the warning branch.
- **Daemon-only:** the CLI doesn't load plugins; don't touch `cairn-cli` or `cairn-infra`.
- **fmt:** run `cargo fmt` before committing (CI rustfmt is strict).
```
