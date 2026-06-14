# Tier-3 plugin contract surface (engine forward-declaration)

**Date:** 2026-06-14
**Repo:** `tau-rs/cairn` (engine)
**Status:** Design approved
**Scope:** Additive contract types only. Host-side logic lives in `cairn-web-ui`.

## Problem

The `cairn-web-ui` Tier-3 feature (sandboxed-iframe plugins) needs three contract
types it does not yet have. The vendored TypeScript contract in `cairn-web-ui` is
generated from this engine's `cairn-contract` crate (ts-rs) and drift-checked. Until
those types exist here, the frontend Tier-3 host work is blocked. This is the single
cross-repo blocker — "needs 1 engine contract add."

## Scope decision: forward-declaration only

The engine adds the contract *types* and regenerates bindings. It does **not** wire a
data path:

- The internal `cairn_plugin_protocol::PluginWidget` (3 variants) is untouched, so the
  engine never constructs the new `Iframe` contract variant.
- The engine always emits `capabilities: None` (no source for capabilities yet).
- `map_widget`, the plugin SDK, and the example plugin are untouched.

The actual data path — plugins declaring capabilities, the engine emitting iframe
widgets — lands later in `cairn-web-ui`'s Wave-2 host work. The engine's only job now is
to make the bindings exist so the vendored contract can be re-synced.

## Changes

All contract changes are in `crates/cairn-contract/src/lib.rs`. ts-rs regenerates
`crates/cairn-contract/bindings/*.ts` during `cargo test`.

### 1. `PluginWidget` — append `Iframe` variant

The enum is `#[serde(tag = "kind", rename_all = "snake_case")]`, so the variant name
`Iframe` serializes to `"iframe"` automatically. Appended after `List`:

```rust
Iframe {
    html: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
},
```

`height` mirrors the existing optional-field style (`muted`, `icon`, `args`):
`skip_serializing_if` omits it from the wire when `None`; ts-rs still types it
`number | null`. Additive — no existing match on `cairn_contract::PluginWidget` breaks
(the only mapper, `cairn-service::map_widget`, matches on the *protocol* enum).

### 2. New `PluginCapability` enum

Mirrors `PluginSlot`'s inline-rename style (dotted wire strings). Derives `Eq` (it holds
no `serde_json::Value`, unlike `PluginWidget`):

```rust
/// A capability a Tier-3 (sandboxed-iframe) plugin may request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum PluginCapability {
    #[serde(rename = "activeNote.read")]
    ActiveNoteRead,
    #[serde(rename = "activeNote.write")]
    ActiveNoteWrite,
    #[serde(rename = "notes.read")]
    NotesRead,
    #[serde(rename = "notes.search")]
    NotesSearch,
    #[serde(rename = "command.invoke")]
    CommandInvoke,
}
```

Exact wire strings (the contract): `"activeNote.read"`, `"activeNote.write"`,
`"notes.read"`, `"notes.search"`, `"command.invoke"`.

### 3. `PluginSummary` — append `capabilities` field

```rust
/// Capabilities a Tier-3 plugin requests. None for plugins that declare none.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub capabilities: Option<Vec<PluginCapability>>,
```

`default` makes pre-existing payloads (no `capabilities` key) deserialize to `None`,
so Tier-2 consumers are unaffected. `skip_serializing_if` keeps the wire clean when
`None`.

### 4. Forced non-contract edit: `crates/cairn-service/src/lib.rs`

The single `PluginSummary` literal (~line 255) must gain `capabilities: None,` — serde
`default` covers deserialization, not Rust struct-literal completeness. This is the only
edit outside the contract crate.

### 5. Test — extend `plugin_value_arrays_match_enums`

In `crates/cairn-contract/src/lib.rs`, extend the existing test to assert:

- the 5 `PluginCapability` variants serialize to exactly the 5 dotted strings, in order;
- `PluginWidget::Iframe { html, height }` serializes with `kind == "iframe"`.

This is the drift guard on the exact wire strings (the ts-rs `export_bindings_*` macro
tests cover that bindings *generate*, but not their string values).

## Out of scope

- `cairn-plugin-protocol`, `cairn-plugin-sdk`, `cairn-plugin-example`, `map_widget`.
- Any host-side rendering, consent UI, or capability enforcement (all `cairn-web-ui`).
- Re-syncing the vendored contract in `cairn-web-ui` (done there, post-merge, via its
  own sync script).

## Verification

- `just ci` — `fmt`, `clippy -D warnings`, `nextest` (incl. `export_bindings_*` +
  the extended `plugin_value_arrays_match_enums`), `doc-test`, `cargo deny`,
  `locked-check`.
- `git status` shows exactly: `lib.rs` (contract) + `cairn-service/src/lib.rs` +
  `bindings/PluginWidget.ts` + `bindings/PluginSummary.ts` + new
  `bindings/PluginCapability.ts` + this spec. No other binding drift.

## Branch / PR

Engine `main` is push-protected — PRs only. Work on `feat/tier3-plugin-contract`
(isolated worktree off `origin/main`), PR with `--base main`.
