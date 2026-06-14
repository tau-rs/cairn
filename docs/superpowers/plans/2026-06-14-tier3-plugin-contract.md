# Tier-3 plugin contract surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three additive ts-rs contract types (`PluginWidget::Iframe`, `PluginCapability`, `PluginSummary.capabilities`) so `cairn-web-ui`'s Tier-3 sandboxed-iframe feature can re-sync the vendored contract.

**Architecture:** Forward-declaration only. Add the types to the `cairn-contract` crate and regenerate ts-rs bindings. The engine never *produces* the new variant or non-`None` capabilities — the internal `cairn-plugin-protocol` enum, `map_widget`, the SDK, and the example plugin are untouched. The only edit outside the contract crate is one `capabilities: None,` line forced by Rust struct-literal completeness.

**Tech Stack:** Rust, serde, ts-rs 11.x (convention: bindings → `crates/cairn-contract/bindings/`), `cargo nextest`, `just`.

**Worktree:** `/Users/titouanlebocq/code/cairn-worktrees/tier3-contract`, branch `feat/tier3-plugin-contract` (off `origin/main`). All commands below run from this directory.

---

## File Structure

- **Modify** `crates/cairn-contract/src/lib.rs` — add `PluginCapability` enum, `PluginWidget::Iframe` variant, `PluginSummary.capabilities` field; extend the `plugin_value_arrays_match_enums` test and add one round-trip test.
- **Modify** `crates/cairn-service/src/lib.rs` — add `capabilities: None,` to the single `PluginSummary` literal (~line 255).
- **Regenerated (not hand-edited)** `crates/cairn-contract/bindings/PluginWidget.ts`, `bindings/PluginSummary.ts`, new `bindings/PluginCapability.ts` — written by ts-rs when tests run.

The existing test module in `lib.rs` already references `PluginWidget::`, `PluginSlot::`, `PluginIcon::` unqualified, so it has `use super::*;` — new types are automatically in scope.

---

### Task 1: `PluginCapability` enum

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (enum near the other plugin types, ~line 175; test in the `#[cfg(test)]` module, inside `plugin_value_arrays_match_enums`, ~line 586)

- [ ] **Step 1: Write the failing test**

In `crates/cairn-contract/src/lib.rs`, inside the existing `plugin_value_arrays_match_enums` test, append before its closing `}` (after the `assert_eq!(kinds, ["text", "action", "list"]);` line):

```rust
        // Capability wire strings (the contract): dotted, exact, ordered.
        let caps = [
            PluginCapability::ActiveNoteRead,
            PluginCapability::ActiveNoteWrite,
            PluginCapability::NotesRead,
            PluginCapability::NotesSearch,
            PluginCapability::CommandInvoke,
        ];
        let cap_strs: Vec<String> = caps
            .iter()
            .map(|c| to_value(c).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            cap_strs,
            [
                "activeNote.read",
                "activeNote.write",
                "notes.read",
                "notes.search",
                "command.invoke"
            ]
        );
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-contract plugin_value_arrays_match_enums`
Expected: FAIL — compile error `cannot find type/value 'PluginCapability' in this scope`.

- [ ] **Step 3: Write minimal implementation**

In `crates/cairn-contract/src/lib.rs`, add the enum immediately after `PluginSlot` (after its closing `}` at ~line 175), mirroring `PluginSlot`'s inline-rename style:

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

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-contract plugin_value_arrays_match_enums`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-contract/src/lib.rs
git commit -m "feat(contract): add PluginCapability enum"
```

---

### Task 2: `PluginWidget::Iframe` variant

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (`PluginWidget` enum ~line 195–212; test ~end of `plugin_value_arrays_match_enums`)

- [ ] **Step 1: Write the failing test**

In `crates/cairn-contract/src/lib.rs`, inside `plugin_value_arrays_match_enums`, append after the capability block from Task 1:

```rust
        // The Iframe widget kind serializes to "iframe".
        let iframe_kind = to_value(PluginWidget::Iframe {
            html: "<p>x</p>".into(),
            height: None,
        })
        .unwrap();
        assert_eq!(iframe_kind["kind"].as_str().unwrap(), "iframe");
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-contract plugin_value_arrays_match_enums`
Expected: FAIL — compile error `no variant named 'Iframe' found for enum 'PluginWidget'`.

- [ ] **Step 3: Write minimal implementation**

In `crates/cairn-contract/src/lib.rs`, append the variant to `PluginWidget`, after the `List { items: Vec<PluginListItem> },` variant (before the enum's closing `}` at ~line 212):

```rust
    Iframe {
        html: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-contract plugin_value_arrays_match_enums`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-contract/src/lib.rs
git commit -m "feat(contract): add PluginWidget::Iframe variant"
```

---

### Task 3: `PluginSummary.capabilities` field + service literal fix

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (`PluginSummary` struct ~line 232–244; new test in the `#[cfg(test)]` module)
- Modify: `crates/cairn-service/src/lib.rs` (`PluginSummary` literal ~line 243–256)

- [ ] **Step 1: Write the failing test**

In `crates/cairn-contract/src/lib.rs`, add a new test inside the `#[cfg(test)]` module (after `plugin_value_arrays_match_enums`'s closing `}`):

```rust
    #[test]
    fn plugin_summary_capabilities_round_trip() {
        // Tier-2 payloads (no `capabilities` key) must still deserialize.
        let legacy = r#"{"id":"p","name":"P","version":"1","commands":[],"contributions":[]}"#;
        let s: PluginSummary = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.capabilities, None);

        // Round-trip with capabilities present.
        let s2 = PluginSummary {
            id: "p".into(),
            name: "P".into(),
            version: "1".into(),
            commands: vec![],
            contributions: vec![],
            capabilities: Some(vec![PluginCapability::NotesRead]),
        };
        let j = serde_json::to_string(&s2).unwrap();
        assert!(j.contains("\"capabilities\":[\"notes.read\"]"));
        assert_eq!(serde_json::from_str::<PluginSummary>(&j).unwrap(), s2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-contract plugin_summary_capabilities_round_trip`
Expected: FAIL — compile error `struct 'PluginSummary' has no field named 'capabilities'`.

- [ ] **Step 3: Write minimal implementation**

(a) In `crates/cairn-contract/src/lib.rs`, append the field to `PluginSummary`, after the `contributions` field (before the struct's closing `}` at ~line 244):

```rust
    /// Capabilities a Tier-3 plugin requests. None for plugins that declare none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<PluginCapability>>,
```

(b) In `crates/cairn-service/src/lib.rs`, add `capabilities: None,` to the `PluginSummary` literal. The literal currently ends:

```rust
                    contributions: p.contributions.into_iter().map(map_contribution).collect(),
                })
```

Change to:

```rust
                    contributions: p.contributions.into_iter().map(map_contribution).collect(),
                    capabilities: None,
                })
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-contract plugin_summary_capabilities_round_trip`
Expected: PASS.

- [ ] **Step 5: Verify the service crate compiles**

Run: `cargo check -p cairn-service`
Expected: PASS (the new field is now supplied at the literal).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-service/src/lib.rs
git commit -m "feat(contract): add PluginSummary.capabilities field"
```

---

### Task 4: Regenerate bindings + full gate + PR

**Files:**
- Regenerated: `crates/cairn-contract/bindings/PluginWidget.ts`, `bindings/PluginSummary.ts`, new `bindings/PluginCapability.ts`

- [ ] **Step 1: Regenerate the ts-rs bindings**

The `#[ts(export)]` macro writes `.ts` files when the auto-generated `export_bindings_*` tests run.

Run: `just test`
Expected: all tests PASS (incl. `export_bindings_plugincapability`, `export_bindings_pluginwidget`, `export_bindings_pluginsummary`).

- [ ] **Step 2: Verify exactly the expected files changed**

Run: `git status --porcelain`
Expected — only these (plus nothing else):
```
 M crates/cairn-contract/bindings/PluginSummary.ts
 M crates/cairn-contract/bindings/PluginWidget.ts
?? crates/cairn-contract/bindings/PluginCapability.ts
```
If any *other* binding shows as modified, stop — that's unexpected drift; investigate before continuing.

- [ ] **Step 3: Verify the generated content**

Run: `grep -n "iframe\|capabilities\|notes.read" crates/cairn-contract/bindings/PluginWidget.ts crates/cairn-contract/bindings/PluginSummary.ts crates/cairn-contract/bindings/PluginCapability.ts`
Expected:
- `PluginWidget.ts` contains a `{ "kind": "iframe", html: string, height: number | null, }` member.
- `PluginSummary.ts` contains `capabilities: Array<PluginCapability> | null,`.
- `PluginCapability.ts` contains `"activeNote.read" | "activeNote.write" | "notes.read" | "notes.search" | "command.invoke"`.

- [ ] **Step 4: Run the full gate**

Run: `just ci`
Expected: PASS — `fmt`, `lint` (clippy `-D warnings`), `test`, `doc-test`, `deny`, `locked-check` all green.

- [ ] **Step 5: Commit the bindings**

```bash
git add crates/cairn-contract/bindings/
git commit -m "chore(contract): regenerate ts-rs bindings for Tier-3 surface"
```

- [ ] **Step 6: Push and open the PR**

```bash
git push -u origin feat/tier3-plugin-contract
gh pr create --base main \
  --title "feat(contract): Tier-3 plugin contract surface" \
  --body "$(cat <<'EOF'
Additive contract surface for cairn-web-ui's Tier-3 sandboxed-iframe plugins. Forward-declaration only — host-side logic lives in cairn-web-ui.

- `PluginWidget` gains an `Iframe { html, height }` variant (wire `kind: "iframe"`).
- New `PluginCapability` enum: `activeNote.read` | `activeNote.write` | `notes.read` | `notes.search` | `command.invoke`.
- `PluginSummary` gains `capabilities: Option<Vec<PluginCapability>>` (serde `default` → Tier-2 payloads still deserialize).

The engine does not yet *produce* these (internal `cairn-plugin-protocol`, `map_widget`, SDK untouched; service emits `capabilities: None`). The vendored contract is re-synced in cairn-web-ui post-merge via its sync script.

Regenerated ts-rs bindings included. `just ci` green. Spec + plan under `docs/superpowers/`.
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- Iframe variant → Task 2 ✓
- PluginCapability enum (5 exact strings) → Task 1 + drift test ✓
- capabilities field (backward-compatible) → Task 3 + legacy-deserialize test ✓
- Forced `cairn-service` `capabilities: None` → Task 3 step 3(b) ✓
- Drift test on wire strings → Tasks 1–2 extend `plugin_value_arrays_match_enums` ✓
- Regenerate + `just ci` + git-status drift check → Task 4 ✓
- PR base main / push-protected → Task 4 step 6 ✓

**Placeholder scan:** none — all steps carry concrete code/commands.

**Type consistency:** variant names `ActiveNoteRead`/`ActiveNoteWrite`/`NotesRead`/`NotesSearch`/`CommandInvoke` and field `capabilities: Option<Vec<PluginCapability>>` used identically in enum def, tests, and service literal. `Iframe { html, height }` consistent across def + test + bindings check.
