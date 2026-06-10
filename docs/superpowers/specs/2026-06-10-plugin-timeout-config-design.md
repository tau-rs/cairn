# Daemon config: plugin read timeout (`cairn.toml`)

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation
**Builds on:** the plugin read-timeout slice (`ProcessPluginHost::load_with_timeout` + `DEFAULT_PLUGIN_TIMEOUT`, PR #32) and the daemon `cairn.toml` config ([ADR-0004](../../decisions/0004-daemon-cors.md))

## Goal

Let an operator tune the plugin read timeout via `cairn.toml` instead of the
hardcoded 30s default. The host already exposes the
`ProcessPluginHost::load_with_timeout(dir, Duration)` seam; this wires a config
value into it. Daemon-only — plugins are loaded only by the daemon (the CLI uses
`NoopPluginHost`).

```toml
[plugins]
timeout_secs = 60
```

## Decisions (resolved during brainstorming)

- **Config-file only, no CLI flag.** A plugin timeout is a deploy-config value that
  fits the file; unlike CORS origins (set ad-hoc in dev, hence `--cors-origin`), a
  `--plugin-timeout-secs` flag is YAGNI here. The `load_with_timeout` seam stays
  ready if a flag is ever wanted.
- **`Option<u64>`, default in one place.** `timeout_secs` is optional; unset means
  "use the host default" (`cairn_infra::DEFAULT_PLUGIN_TIMEOUT` = 30s). This avoids
  duplicating the `30` literal in the config crate.
- **`0` is rejected → default + warning.** A 0-second timeout would kill every
  plugin on its first read (no plugin can respond in 0s) — an obvious footgun. The
  daemon logs a warning and falls back to the default.

## Components

### 1. `crates/cairn-daemon/src/config.rs`

Add a `[plugins]` section to `Config`, mirroring the existing `IndexConfig`/
`CorsConfig` pattern:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

/// Plugin host settings.
#[derive(Debug, Default, Deserialize)]
pub struct PluginsConfig {
    /// Per-message plugin read timeout, in seconds. Unset → the host default
    /// (`cairn_infra::DEFAULT_PLUGIN_TIMEOUT`, 30s). A configured `0` is invalid
    /// (it would kill every plugin immediately) and is ignored with a warning.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
```

`PluginsConfig` derives `Default` (so an absent `[plugins]` section yields
`timeout_secs: None`). No `Default` impl is needed (the field defaults to `None`).

### 2. `crates/cairn-daemon/src/main.rs`

Resolve the timeout from the loaded `Config`, guard against `0`, and use
`load_with_timeout` (so the effective value is known and can be logged). Replace
the current plugin-load block:

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
    let plugins_dir = cli.cairn.join(".cairn").join("plugins");
    match cairn_infra::ProcessPluginHost::load_with_timeout(&plugins_dir, plugin_timeout) {
        Ok(host) => engine.set_plugin_host(Box::new(host)),
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }
    println!("plugins: read timeout {plugin_timeout:?}");
```

Add `use std::time::Duration;` to `main.rs` if not already imported. The existing
absent-dir/broken-plugin graceful handling is unchanged.

(`config` is the already-loaded `Config` in `run()`; `cairn_infra::DEFAULT_PLUGIN_TIMEOUT`
is `pub` from the timeout slice.)

## Testing

`config.rs` unit tests, mirroring the existing `[index]` tests:

```rust
#[test]
fn plugins_timeout_parses() {
    let c: Config = toml::from_str("[plugins]\ntimeout_secs = 60").unwrap();
    assert_eq!(c.plugins.timeout_secs, Some(60));
}

#[test]
fn plugins_timeout_defaults_none() {
    assert_eq!(toml::from_str::<Config>("").unwrap().plugins.timeout_secs, None);
    assert_eq!(
        toml::from_str::<Config>("[plugins]\n").unwrap().plugins.timeout_secs,
        None
    );
}
```

The `Some(0) → default` guard and the startup log are small daemon glue in `run()`
(a function not covered by a harness); they are self-evident and verified by reading
— a full daemon integration test for the warning branch is overkill for logged glue.
The existing daemon integration tests still pass (they don't set `[plugins]`, so they
get the default timeout, same behaviour as before).

## Out of scope

- A `--plugin-timeout-secs` CLI flag (config-file only).
- Per-plugin timeouts (one daemon-wide value).
- Threading the timeout into the CLI (the CLI doesn't load plugins).

## Unchanged

`cairn-infra` (the `load_with_timeout` seam already exists), the plugin host
internals, `cairn-cli`, and all other crates. This is a daemon config + wiring
change only.
