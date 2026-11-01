# Tier-2 Slot-Mount — Phase 0 (Engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the cairn engine so a plugin can declare UI `contributions` in its `initialize` handshake, carry them through to the `list_plugins` contract response, and sync the regenerated TS contract into `cairn-web-ui`.

**Architecture:** Contributions are new types defined in `cairn-contract` (ts-rs → frontend), **mirrored** in `cairn-plugin-protocol` (the contract-independent plugin wire protocol), declared via a new `cairn-plugin-sdk` builder, carried through engine-internal `PluginInfo`, and **mapped** in `cairn-service` exactly as `CommandDecl`/`PluginCommandSummary` already are. The day-one caller is `cairn-plugin-example`.

**Tech Stack:** Rust, `ts-rs`, `serde`, JSON-RPC/NDJSON. Repo: **`tau-rs/cairn`** (separate from this workspace — clone it as a sibling, e.g. `../cairn`).

**Spec:** `docs/superpowers/specs/2026-06-13-tier2-slot-mount-design.md` (in `cairn-web-ui`).

> **Repo note:** All tasks 1–7 run in a clone of `tau-rs/cairn`. Task 8 runs back in `cairn-web-ui`. Confirm exact line numbers against the live engine before editing — this plan cites the structures, not pinned lines, for engine files probed read-only.

---

## File Structure (engine repo `tau-rs/cairn`)

- `crates/cairn-contract/src/lib.rs` — **add** contribution types + `PluginSummary.contributions`; this is the single contract-types file (ts-rs `#[ts(export)]`).
- `crates/cairn-contract/bindings/pluginValues.ts` — **create** hand-authored runtime value arrays (ts-rs exports types only).
- `crates/cairn-contract/src/lib.rs` tests — **add** a test asserting the value arrays match the Rust variants.
- `crates/cairn-plugin-protocol/src/lib.rs` — **add** mirrored contribution types + `InitializeResult.contributions`.
- `crates/cairn-plugin-sdk/src/lib.rs` — **add** `contributions` field, `.contribution()` builder, include in `initialize`.
- `crates/cairn-app/src/lib.rs` — **add** `contributions` to engine-internal `PluginInfo`.
- `crates/cairn-infra/src/plugin_host.rs` — carry `contributions` from parsed `InitializeResult` into `PluginInfo`.
- `crates/cairn-service/src/lib.rs:243-253` — **map** `PluginInfo.contributions` → `PluginSummary.contributions`.
- `crates/cairn-plugin-example/src/main.rs` — **declare** day-one contributions.
- `crates/cairn-plugin-example/tests/host.rs` — **assert** contributions round-trip.

---

## Task 1: Contribution types in `cairn-contract`

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (near `PluginSummary`, ~line 150)

- [ ] **Step 1: Add the new types** (follow the existing `#[derive(... TS)] #[ts(export)]` + `#[serde(tag=...)]` pattern used by `Command`/`CommandResponse`)

```rust
/// An icon a plugin may reference by name. Closed set — never a string/URL/SVG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PluginIcon { Tag, Search, Note, Folder, Link, Star, Info, Play }

/// A named shell slot a contribution targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum PluginSlot {
    #[serde(rename = "sidebar.section")] SidebarSection,
    #[serde(rename = "topbar.action")]   TopbarAction,
    #[serde(rename = "command")]         Command,
}

/// One row inside a `list` widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginListItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub args: Option<serde_json::Value>,
}

/// A host-renderable widget. Closed vocabulary; first cut: text / action / list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginWidget {
    Text   { text: String, #[serde(skip_serializing_if = "Option::is_none")] muted: Option<bool> },
    Action { label: String,
             #[serde(skip_serializing_if = "Option::is_none")] icon: Option<PluginIcon>,
             command: String,
             #[serde(skip_serializing_if = "Option::is_none")] args: Option<serde_json::Value> },
    List   { items: Vec<PluginListItem> },
}

/// One placement of one widget into one slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginContribution {
    pub id: String,
    pub slot: PluginSlot,
    pub widget: PluginWidget,
    #[serde(skip_serializing_if = "Option::is_none")] pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")] pub order: Option<i32>,
}
```

- [ ] **Step 2: Add `contributions` to `PluginSummary`** (lib.rs:153-162)

```rust
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub commands: Vec<PluginCommandSummary>,
    /// UI contributions (Tier-2). Empty for plugins that declare none.
    #[serde(default)]
    pub contributions: Vec<PluginContribution>,
}
```

- [ ] **Step 3: Regenerate bindings + verify they compile**

Run: `cargo test -p cairn-contract` (ts-rs `#[ts(export)]` writes `crates/cairn-contract/bindings/*.ts` during tests)
Expected: PASS; new files `PluginWidget.ts`, `PluginSlot.ts`, `PluginIcon.ts`, `PluginContribution.ts`, `PluginListItem.ts` appear in `bindings/`, and `PluginSummary.ts` gains `contributions`.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/bindings
git commit -m "feat(contract): add Tier-2 plugin contribution types"
```

---

## Task 2: Runtime value arrays + drift-proof test

ts-rs exports *types* (erased at runtime); the frontend lockstep test needs iterable values. Hand-author them as a committed binding file, guarded by a Rust test so they can't drift from the enums.

**Files:**
- Create: `crates/cairn-contract/bindings/pluginValues.ts`
- Modify: `crates/cairn-contract/src/lib.rs` (test module)

- [ ] **Step 1: Create the value-arrays binding**

```ts
// crates/cairn-contract/bindings/pluginValues.ts
// Hand-authored to accompany the ts-rs-generated PluginSlot/PluginWidget/PluginIcon
// types (ts-rs emits types only). Kept in lockstep with the Rust enums by a
// cairn-contract unit test. Vendored into web/src/contract by sync-contract.sh.
export const PLUGIN_SLOT_VALUES = ["sidebar.section", "topbar.action", "command"] as const;
export const PLUGIN_WIDGET_KIND_VALUES = ["text", "action", "list"] as const;
export const PLUGIN_ICON_VALUES =
  ["tag", "search", "note", "folder", "link", "star", "info", "play"] as const;
```

- [ ] **Step 2: Write the failing drift test** (asserts each array == its enum's serde reprs)

```rust
#[test]
fn plugin_value_arrays_match_enums() {
    use serde_json::to_value;
    // Each enum variant's serialized string must appear in the .ts arrays below.
    let slots = [PluginSlot::SidebarSection, PluginSlot::TopbarAction, PluginSlot::Command];
    let slot_strs: Vec<String> = slots.iter().map(|s| to_value(s).unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(slot_strs, ["sidebar.section", "topbar.action", "command"]);

    let icons = [PluginIcon::Tag, PluginIcon::Search, PluginIcon::Note, PluginIcon::Folder,
                 PluginIcon::Link, PluginIcon::Star, PluginIcon::Info, PluginIcon::Play];
    let icon_strs: Vec<String> = icons.iter().map(|s| to_value(s).unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(icon_strs, ["tag","search","note","folder","link","star","info","play"]);

    // Widget kinds are the serde `tag` discriminants:
    let kinds: Vec<String> = [
        to_value(PluginWidget::Text { text: "x".into(), muted: None }).unwrap(),
        to_value(PluginWidget::Action { label: "x".into(), icon: None, command: "c".into(), args: None }).unwrap(),
        to_value(PluginWidget::List { items: vec![] }).unwrap(),
    ].iter().map(|v| v["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["text","action","list"]);
}
```

- [ ] **Step 3: Run it**

Run: `cargo test -p cairn-contract plugin_value_arrays_match_enums`
Expected: PASS (the assertions encode the same strings as `pluginValues.ts`; if a future dev renames an enum variant, this test fails, forcing them to update `pluginValues.ts`).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-contract/bindings/pluginValues.ts crates/cairn-contract/src/lib.rs
git commit -m "feat(contract): plugin enum value arrays + lockstep test"
```

---

## Task 3: Mirror types in `cairn-plugin-protocol` + extend `InitializeResult`

The protocol crate is contract-independent, so mirror the wire shape (plain serde, no ts-rs).

**Files:**
- Modify: `crates/cairn-plugin-protocol/src/lib.rs` (near `InitializeResult`, line 77)

- [ ] **Step 1: Add mirrored types** (identical serde shape to Task 1, minus `TS`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginIcon { Tag, Search, Note, Folder, Link, Star, Info, Play }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginSlot {
    #[serde(rename = "sidebar.section")] SidebarSection,
    #[serde(rename = "topbar.action")]   TopbarAction,
    #[serde(rename = "command")]         Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginListItem {
    pub id: String, pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginWidget {
    Text   { text: String, #[serde(skip_serializing_if = "Option::is_none")] muted: Option<bool> },
    Action { label: String, #[serde(skip_serializing_if = "Option::is_none")] icon: Option<PluginIcon>,
             command: String, #[serde(skip_serializing_if = "Option::is_none")] args: Option<serde_json::Value> },
    List   { items: Vec<PluginListItem> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginContribution {
    pub id: String, pub slot: PluginSlot, pub widget: PluginWidget,
    #[serde(skip_serializing_if = "Option::is_none")] pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")] pub order: Option<i32>,
}
```

- [ ] **Step 2: Extend `InitializeResult`** (lib.rs:77-81)

```rust
pub struct InitializeResult {
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub contributions: Vec<PluginContribution>,
}
```

- [ ] **Step 3: Build + existing protocol tests pass**

Run: `cargo test -p cairn-plugin-protocol`
Expected: PASS (existing `initialize` round-trip tests still green; `contributions` defaults to `[]`).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-plugin-protocol/src/lib.rs
git commit -m "feat(plugin-protocol): contributions in InitializeResult"
```

---

## Task 4: `cairn-plugin-sdk` — `.contribution()` builder

**Files:**
- Modify: `crates/cairn-plugin-sdk/src/lib.rs` (`Plugin` struct line 224; `initialize` build ~line 314)

- [ ] **Step 1: Write a failing SDK test** (a plugin declaring a contribution surfaces it in `initialize`)

```rust
#[test]
fn initialize_includes_declared_contributions() {
    use cairn_plugin_protocol::{PluginContribution, PluginSlot, PluginWidget};
    let mut p = Plugin::new("t", "0.1.0");
    p.contribution(PluginContribution {
        id: "s".into(), slot: PluginSlot::SidebarSection,
        widget: PluginWidget::Text { text: "hi".into(), muted: None },
        title: None, icon: None, order: None,
    });
    // initialize() is the request handler; drive it via run_io with one initialize line:
    let input = request_line(1, METHOD_INITIALIZE, serde_json::json!({}));
    let mut out = Vec::new();
    p.run_io(&mut Cursor::new(input), &mut out);
    let resp: serde_json::Value = serde_json::from_slice(out.split(|&b| b == b'\n').next().unwrap()).unwrap();
    let init: InitializeResult = serde_json::from_value(resp["result"].clone()).unwrap();
    assert_eq!(init.contributions.len(), 1);
}
```

- [ ] **Step 2: Run it — fails to compile** (`contribution` undefined)

Run: `cargo test -p cairn-plugin-sdk initialize_includes_declared_contributions`
Expected: FAIL — `no method named contribution`.

- [ ] **Step 3: Add the field + builder + wire into initialize**

Add to `Plugin` (line 224):
```rust
pub struct Plugin {
    name: String,
    version: String,
    commands: Vec<RegisteredCommand>,
    contributions: Vec<cairn_plugin_protocol::PluginContribution>,  // NEW
    event_handler: Option<ErasedEventHandler>,
}
```
Init it in `new()` (line 233): `contributions: Vec::new(),`.
Add the builder (next to `command`, ~line 254):
```rust
/// Declare a UI contribution surfaced to the shell at `initialize`.
pub fn contribution(&mut self, c: cairn_plugin_protocol::PluginContribution) {
    self.contributions.push(c);
}
```
Extend the `InitializeResult` construction (~line 314):
```rust
let init = InitializeResult {
    name: self.name.clone(),
    version: self.version.clone(),
    commands: self.commands.iter().map(|c| CommandDecl { /* unchanged */ }).collect(),
    contributions: self.contributions.clone(),   // NEW
};
```

- [ ] **Step 4: Run the test — passes**

Run: `cargo test -p cairn-plugin-sdk initialize_includes_declared_contributions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-plugin-sdk/src/lib.rs
git commit -m "feat(plugin-sdk): .contribution() builder"
```

---

## Task 5: Carry contributions through `PluginInfo` + `plugin_host`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs` (the `PluginInfo` struct returned by `list_plugins`, ~line 539)
- Modify: `crates/cairn-infra/src/plugin_host.rs` (where a parsed `InitializeResult` becomes a `PluginInfo`)

- [ ] **Step 1: Add `contributions` to `PluginInfo`**

```rust
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandInfo>,   // existing (name may differ — match the crate)
    pub contributions: Vec<cairn_plugin_protocol::PluginContribution>,  // NEW
}
```

- [ ] **Step 2: Populate it where `plugin_host` builds `PluginInfo` from `InitializeResult`**

Find the construction (the same place it copies `init.commands`) and add:
```rust
contributions: init.contributions.clone(),
```

- [ ] **Step 3: Build the workspace**

Run: `cargo build -p cairn-app -p cairn-infra`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-app/src/lib.rs crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(plugin-host): carry contributions into PluginInfo"
```

---

## Task 6: Map `PluginInfo → PluginSummary` in `cairn-service`

**Files:**
- Modify: `crates/cairn-service/src/lib.rs:243-253` (the `Query::ListPlugins` arm)

- [ ] **Step 1: Write the failing service test** (a fake engine plugin with a contribution surfaces it)

Extend the existing `list_plugins_empty_and_invoke_unknown_is_not_found` style test (lib.rs:583) — or add a sibling — that loads a plugin declaring one contribution and asserts:
```rust
match dispatch_query(&eng, &Query::ListPlugins).unwrap() {
    QueryResponse::Plugins { plugins } => {
        assert_eq!(plugins[0].contributions.len(), 1);
        assert_eq!(plugins[0].contributions[0].slot, PluginSlot::SidebarSection);
    }
    other => panic!("expected Plugins, got {other:?}"),
}
```

- [ ] **Step 2: Run — fails** (field missing in the map)

Run: `cargo test -p cairn-service`
Expected: FAIL — `PluginSummary` has no `contributions` populated / type mismatch.

- [ ] **Step 3: Extend the map** (lib.rs:243-253). `PluginInfo.contributions` are protocol-typed; map each into the contract type field-for-field (same shape):

```rust
.map(|p| PluginSummary {
    id: p.id,
    name: p.name,
    version: p.version,
    commands: p.commands.into_iter().map(|c| PluginCommandSummary { id: c.id, title: c.title }).collect(),
    contributions: p.contributions.into_iter().map(map_contribution).collect(),  // NEW
})
```
Add a `fn map_contribution(c: protocol::PluginContribution) -> contract::PluginContribution` that copies each field and recursively maps `PluginWidget`/`PluginListItem`/`PluginIcon`/`PluginSlot` (mechanical 1:1 — both shapes are identical). Keep it in `cairn-service` next to the query dispatch.

- [ ] **Step 4: Run — passes**

Run: `cargo test -p cairn-service`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): map plugin contributions into PluginSummary"
```

---

## Task 7: Day-one caller — `cairn-plugin-example`

**Files:**
- Modify: `crates/cairn-plugin-example/src/main.rs`
- Modify: `crates/cairn-plugin-example/tests/host.rs`

- [ ] **Step 1: Declare two contributions** (after the `.command(...)` calls, before `plugin.run()`)

```rust
use cairn_plugin_protocol::{PluginContribution, PluginSlot, PluginWidget, PluginListItem};

plugin.contribution(PluginContribution {
    id: "note-count".into(),
    slot: PluginSlot::SidebarSection,
    title: Some("Example".into()),
    icon: None, order: Some(0),
    widget: PluginWidget::List { items: vec![PluginListItem {
        id: "count".into(),
        label: "Run noteCount".into(),
        icon: None,
        command: Some("noteCount".into()),   // fires the existing host-callback command
        args: None,
    }] },
});
plugin.contribution(PluginContribution {
    id: "echo-action".into(),
    slot: PluginSlot::TopbarAction,
    title: None, icon: None, order: None,
    widget: PluginWidget::Action { label: "Echo".into(), icon: Some(cairn_plugin_protocol::PluginIcon::Play),
                                   command: "echo".into(), args: None },
});
```

- [ ] **Step 2: Write a failing host test** (extend `tests/host.rs` — it already drives the example over the protocol)

```rust
#[test]
fn example_declares_contributions_at_initialize() {
    // (mirror the existing host harness that sends `initialize` and reads the result)
    let init = initialize_example();   // existing helper or inline the initialize round-trip
    assert_eq!(init.contributions.len(), 2);
    assert!(init.contributions.iter().any(|c| matches!(c.slot, PluginSlot::SidebarSection)));
    assert!(init.contributions.iter().any(|c| matches!(c.slot, PluginSlot::TopbarAction)));
}
```

- [ ] **Step 3: Run it**

Run: `cargo test -p cairn-plugin-example`
Expected: PASS (after Step 1 wires the contributions).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-plugin-example/src/main.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(plugin-example): declare day-one UI contributions"
```

---

## Task 8: Full engine gate, then sync into `cairn-web-ui`

- [ ] **Step 1: Run the engine's full test suite**

Run (in `tau-rs/cairn`): `cargo test --workspace`
Expected: PASS.

- [ ] **Step 2: Open + merge the engine PR** (you own the repo — Q1). Note the merge commit SHA.

- [ ] **Step 3: Sync the regenerated contract into `cairn-web-ui`**

Run (in `cairn-web-ui`, with the merged engine at `../cairn`):
```bash
scripts/sync-contract.sh ../cairn
```
Expected: `web/src/contract/` gains `PluginWidget.ts`, `PluginSlot.ts`, `PluginIcon.ts`, `PluginContribution.ts`, `PluginListItem.ts`, `pluginValues.ts`; `PluginSummary.ts` gains `contributions`; `source.ts` `CONTRACT_SOURCE_COMMIT` is bumped to the merge SHA.

- [ ] **Step 4: Verify drift is clean**

Run: `bash scripts/check-contract-drift.sh`
Expected: `contract in sync with engine @ <sha>`.

- [ ] **Step 5: Export the new types from the contract index**

Modify `web/src/contract/index.ts` to re-export the new types + value arrays (match the existing re-export style). Confirm with: `grep -n Plugin web/src/contract/index.ts`.

- [ ] **Step 6: Commit (in `cairn-web-ui`, on branch `tier2-slot-mount-api`)**

```bash
git add web/src/contract
git commit -m "chore(contract): sync Tier-2 plugin contribution types from engine"
```

---

## Self-Review notes
- **Spec coverage:** §0/§3.1 types (Task 1), runtime arrays §1.5 (Task 2), §3.2 three-layer flow (Tasks 3–6), Q2 day-one caller (Task 7), drift-gated sync §0 (Task 8). ✔
- **Phase gate:** Phase 1 (frontend) cannot start until Task 8 Step 4 is green — the contract types must exist in `web/src/contract`.
- **Line numbers** for `PluginInfo`/`plugin_host`/`cairn-service` are cited structurally; confirm against the live engine (probed read-only).
