# Collab Recovery Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a view-only recovery surface so a collab client can see content the CRDT retains but does not materialize (deleted blocks and LWW losers).

**Architecture:** A `Recover`/`Recoverable` message pair on the existing `/collab` WebSocket. The daemon answers from the live in-memory `BlockDoc` (the only place recovery data exists) and replies to the requesting socket only — read-only, never fanned out. Four thin layers: a domain enumerator (`recoverable_blocks`), wire mirrors in the contract, a service mapping, and the daemon handler.

**Tech Stack:** Rust, axum (WS), serde, ts-rs (`TS` derive for web-ui type export), tokio broadcast/mpsc.

## Global Constraints

- Design spec: `docs/superpowers/specs/2026-08-09-collab-recovery-surface-design.md`.
- `cargo build/test --workspace --locked` green; `cargo clippy --all-targets --locked -- -D warnings` clean; `cargo fmt --all -- --check` clean. (Lefthook pre-commit re-runs the full suite ~60s; known-flaky `invoke_times_out_and_kills_plugin` / sandbox-win may eject — re-run.)
- `cairn-domain` stays serde-free (no `serde` derives on domain types).
- `cairn-contract` stays domain-independent; every wire type derives `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]` + `#[ts(export)]`. Collab enums are `#[serde(tag = "type", rename_all = "snake_case")]`.
- Domain↔wire mapping lives in `cairn-service` (contract has no domain dep).
- Scope: protocol + daemon + domain only. Rendering (web-ui repo) and *restore* (un-delete / promote) are OUT of scope — view-only.
- Conventional commits, imperative, scoped. End commit bodies with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Land: rebase on `origin/main` → PR `--base main` → `gh pr merge <#> --auto`. Merge queue owns strategy; never manually update a queued branch.

---

### Task 1: Domain enumerator — `recoverable_blocks`

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs` (add `RecoverableBlock` struct + `BlockDoc::recoverable_blocks`; export via existing `pub` surface)
- Test: `crates/cairn-domain/src/crdt.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `BlockDoc::recoverable(&self, id: BlockId) -> Vec<String>`, `BlockDoc::block_ids_in_order(&self) -> Vec<BlockId>`, the private `entries: HashMap<BlockId, Entry>` field, `Entry.tombstone`.
- Produces:
  ```rust
  pub struct RecoverableBlock { pub id: BlockId, pub tombstoned: bool, pub versions: Vec<String> }
  impl BlockDoc { pub fn recoverable_blocks(&self) -> Vec<RecoverableBlock> }
  ```
  Order: live blocks in materialized (`block_ids_in_order`) order first, then tombstoned blocks sorted by `id` ascending. Only blocks whose `recoverable(id)` is non-empty are included.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-domain/src/crdt.rs`:

```rust
#[test]
fn recoverable_blocks_enumerates_losers_and_tombstoned() {
    // Block 0: live, carries a stashed LWW loser. Block 1: deleted with content.
    let mut doc = BlockDoc::from_markdown(1, "keep\n\ndrop\n");
    let ids = doc.block_ids_in_order();
    let (keep, drop) = (ids[0], ids[1]);
    // Stash a loser on the live block: a lower-ranked SetContent loses to the seed's Human rank.
    doc.merge(BlockOp::SetContent {
        id: keep,
        text: "loser".into(),
        lamport: 3,
        author: Author::Agent,
    });
    // Delete the second block (its content stays recoverable).
    doc.merge(BlockOp::Delete { id: drop, lamport: 9 });

    let rec = doc.recoverable_blocks();
    assert_eq!(rec.len(), 2);
    // Live block first (materialized order), then tombstoned.
    assert_eq!(rec[0].id, keep);
    assert!(!rec[0].tombstoned);
    assert!(rec[0].versions.contains(&"loser".to_string()));
    assert!(!rec[0].versions.contains(&"keep".to_string())); // winner is visible, not recovery
    assert_eq!(rec[1].id, drop);
    assert!(rec[1].tombstoned);
    assert!(rec[1].versions.contains(&"drop".to_string()));
}

#[test]
fn recoverable_blocks_empty_when_nothing_retained() {
    let doc = BlockDoc::from_markdown(1, "a\n\nb\n");
    assert!(doc.recoverable_blocks().is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-domain recoverable_blocks --locked`
Expected: FAIL — `no method named recoverable_blocks` / `cannot find type RecoverableBlock`.

- [ ] **Step 3: Write minimal implementation**

Add near `recoverable`/`stashed` in `crates/cairn-domain/src/crdt.rs`:

```rust
/// One block's recoverable content: versions retained by the CRDT but not shown
/// by `materialize()`. `tombstoned` distinguishes a deleted block (versions = its
/// former content) from a live block (versions = stashed LWW losers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableBlock {
    pub id: BlockId,
    pub tombstoned: bool,
    pub versions: Vec<String>,
}
```

Add the method in `impl BlockDoc` (after `stashed`):

```rust
/// Every block with non-empty `recoverable` content, for a recovery view.
/// Order is deterministic and convergent: live blocks in materialized order
/// first, then tombstoned blocks by ascending id.
#[must_use]
pub fn recoverable_blocks(&self) -> Vec<RecoverableBlock> {
    let mut out = Vec::new();
    // Live blocks, in materialized document order.
    for id in self.block_ids_in_order() {
        let versions = self.recoverable(id);
        if !versions.is_empty() {
            out.push(RecoverableBlock { id, tombstoned: false, versions });
        }
    }
    // Tombstoned blocks, by ascending id (block_ids_in_order excludes them).
    let mut dead: Vec<BlockId> = self
        .entries
        .values()
        .filter(|e| e.tombstone)
        .map(|e| e.id)
        .collect();
    dead.sort();
    for id in dead {
        let versions = self.recoverable(id);
        if !versions.is_empty() {
            out.push(RecoverableBlock { id, tombstoned: true, versions });
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain recoverable_blocks --locked`
Expected: PASS (both tests).

- [ ] **Step 5: Verify the crate re-exports the new type**

`RecoverableBlock` must be reachable as `cairn_domain::RecoverableBlock`. Check `crates/cairn-domain/src/lib.rs` for how `BlockDoc`/`BlockId`/`BlockOp` are re-exported (e.g. `pub use crdt::{...}`) and add `RecoverableBlock` to the same list.

Run: `cargo build -p cairn-domain --locked`
Expected: builds clean.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-domain/src/crdt.rs crates/cairn-domain/src/lib.rs
git commit -m "feat(crdt): enumerate recoverable blocks for a recovery view

recoverable_blocks() returns every block with non-empty recoverable() content
(live LWW losers + tombstoned content), keyed by shared BlockId, in a
deterministic convergent order. First consumer of recoverable() (#159).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Contract — wire mirrors + collab messages

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (add `WireRecoverableBlock`; add `CollabClientMsg::Recover`; add `CollabServerMsg::Recoverable`)
- Test: `crates/cairn-contract/src/lib.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `WireBlockId { replica: u64, counter: u64 }`.
- Produces:
  ```rust
  pub struct WireRecoverableBlock { pub id: WireBlockId, pub tombstoned: bool, pub versions: Vec<String> }
  CollabClientMsg::Recover { note: String }
  CollabServerMsg::Recoverable { note: String, blocks: Vec<WireRecoverableBlock> }
  ```

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-contract/src/lib.rs`:

```rust
#[test]
fn recover_request_serde_round_trips() {
    let msg = CollabClientMsg::Recover { note: "n.md".into() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"recover\""));
    let back: CollabClientMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn recoverable_response_serde_round_trips() {
    let msg = CollabServerMsg::Recoverable {
        note: "n.md".into(),
        blocks: vec![WireRecoverableBlock {
            id: WireBlockId { replica: 1, counter: 2 },
            tombstoned: true,
            versions: vec!["old".into(), "older".into()],
        }],
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"recoverable\""));
    let back: CollabServerMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-contract recover --locked`
Expected: FAIL — `no variant named Recover` / `cannot find struct WireRecoverableBlock`.

- [ ] **Step 3: Write minimal implementation**

Add the struct just after `WireBlockOp` in `crates/cairn-contract/src/lib.rs`:

```rust
/// One block's recoverable content on the wire (mirror of `cairn-domain`
/// `RecoverableBlock`). `id` is the shared live-only block id, so a joined
/// client correlates it with blocks it already knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WireRecoverableBlock {
    pub id: WireBlockId,
    pub tombstoned: bool,
    pub versions: Vec<String>,
}
```

Add the request variant to `CollabClientMsg`:

```rust
    /// Ask for the note's recoverable content (view-only; answered to this
    /// socket only, never fanned out).
    Recover { note: String },
```

Add the response variant to `CollabServerMsg`:

```rust
    /// The note's recoverable content, in reply to `Recover`.
    Recoverable { note: String, blocks: Vec<WireRecoverableBlock> },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-contract recover --locked`
Expected: PASS (both).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-contract/src/lib.rs
git commit -m "feat(contract): Recover/Recoverable collab messages

Wire mirror WireRecoverableBlock + CollabClientMsg::Recover and
CollabServerMsg::Recoverable for the view-only recovery surface.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Service — domain→wire mapping

**Files:**
- Modify: `crates/cairn-service/src/lib.rs` (add `recoverable_block_to_wire`)
- Test: `crates/cairn-service/src/lib.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `cairn_domain::RecoverableBlock` (Task 1), `cairn_contract::WireRecoverableBlock` (Task 2), the existing private `block_id_to_wire(id: cairn_domain::BlockId) -> cairn_contract::WireBlockId`.
- Produces: `pub fn recoverable_block_to_wire(b: cairn_domain::RecoverableBlock) -> cairn_contract::WireRecoverableBlock`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/cairn-service/src/lib.rs`:

```rust
#[test]
fn recoverable_block_maps_to_wire() {
    let domain = cairn_domain::RecoverableBlock {
        id: cairn_domain::BlockId { replica: 4, counter: 7 },
        tombstoned: true,
        versions: vec!["v1".into(), "v2".into()],
    };
    let wire = recoverable_block_to_wire(domain);
    assert_eq!(wire.id, cairn_contract::WireBlockId { replica: 4, counter: 7 });
    assert!(wire.tombstoned);
    assert_eq!(wire.versions, vec!["v1".to_string(), "v2".to_string()]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-service recoverable_block --locked`
Expected: FAIL — `cannot find function recoverable_block_to_wire`.

- [ ] **Step 3: Write minimal implementation**

Add near `block_op_to_wire` in `crates/cairn-service/src/lib.rs`:

```rust
/// Map a domain `RecoverableBlock` to its wire mirror.
#[must_use]
pub fn recoverable_block_to_wire(
    b: cairn_domain::RecoverableBlock,
) -> cairn_contract::WireRecoverableBlock {
    cairn_contract::WireRecoverableBlock {
        id: block_id_to_wire(b.id),
        tombstoned: b.tombstoned,
        versions: b.versions,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-service recoverable_block --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): map RecoverableBlock to wire

recoverable_block_to_wire, symmetric with block_op_to_wire; keeps the domain
serde-free and the contract domain-independent.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Daemon — recovery helper + WS handler

**Files:**
- Modify: `crates/cairn-daemon/src/collab.rs` (add `recoverable_blocks` helper; handle nothing here — the WS arm lives in `run_collab`)
- Modify: `crates/cairn-daemon/src/collab.rs` `run_collab` match on `CollabClientMsg` (add the `Recover` arm)
- Test: `crates/cairn-daemon/src/collab.rs` (`#[cfg(test)] mod flush_tests`)

**Interfaces:**
- Consumes: `BlockDoc::recoverable_blocks` (Task 1), `cairn_service::recoverable_block_to_wire` (Task 3), existing `lock(&collab)`, `Session.doc`, `cairn_contract::{CollabClientMsg, CollabServerMsg}`, the per-connection `out_tx: mpsc::Sender<CollabServerMsg>`.
- Produces:
  ```rust
  pub(crate) fn recoverable_blocks(collab: &Collab, path: &NotePath) -> Vec<cairn_domain::RecoverableBlock>
  ```
  Returns the session's recoverable blocks, or an empty vec when there is no session.

- [ ] **Step 1: Write the failing test (helper is read-only + correct)**

Add to `mod flush_tests` in `crates/cairn-daemon/src/collab.rs`:

```rust
#[test]
fn recoverable_blocks_reads_session_and_does_not_fan_out() {
    let reg = registry();
    let p = NotePath::new("n.md").unwrap();
    insert_dirty_session(&reg, &p, "gone\n", vec![]);
    add_participant(&reg, &p, 7);
    // Delete the only block so its content is recoverable-but-hidden.
    let id = {
        let reg = lock(&reg);
        reg.get(&p).unwrap().doc.block_ids_in_order()[0]
    };
    merge_op(&reg, &p, BlockOp::Delete { id, lamport: 5 });

    // A peer is subscribed; a read-only recovery query must NOT fan anything out.
    let mut rx = test_subscribe(&reg, &p);
    let blocks = recoverable_blocks(&reg, &p);

    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].tombstoned);
    assert!(blocks[0].versions.contains(&"gone".to_string()));
    assert!(rx.try_recv().is_err(), "recovery is read-only, nothing fanned out");
}

#[test]
fn recoverable_blocks_absent_session_is_empty() {
    let reg = registry();
    let p = NotePath::new("nope.md").unwrap();
    assert!(recoverable_blocks(&reg, &p).is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-daemon recoverable_blocks --locked`
Expected: FAIL — `cannot find function recoverable_blocks` in scope.

- [ ] **Step 3: Write the helper**

Add to `crates/cairn-daemon/src/collab.rs` (near `fold_foreign`/`is_sessioned`):

```rust
/// The recoverable content of a session's live replica (view-only). Empty when
/// there is no session for `path`. Read-only: never merges, marks dirty, or fans
/// out — the caller replies to the requesting socket only.
pub(crate) fn recoverable_blocks(
    collab: &Collab,
    path: &NotePath,
) -> Vec<cairn_domain::RecoverableBlock> {
    lock(collab)
        .get(path)
        .map(|sess| sess.doc.recoverable_blocks())
        .unwrap_or_default()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-daemon recoverable_blocks --locked`
Expected: PASS (both).

- [ ] **Step 5: Wire the WS handler arm**

In `run_collab`, add a `Recover` arm to the `match msg { ... }` on `CollabClientMsg`, mirroring the `Op`/`Leave` arms' path-validation and using `out_tx`:

```rust
CollabClientMsg::Recover { note } => {
    let Ok(path) = NotePath::new(&note) else {
        let _ = out_tx
            .send(CollabServerMsg::Error {
                note,
                message: "invalid note path".into(),
            })
            .await;
        continue;
    };
    let blocks = recoverable_blocks(&collab, &path)
        .into_iter()
        .map(cairn_service::recoverable_block_to_wire)
        .collect();
    let _ = out_tx
        .send(CollabServerMsg::Recoverable { note, blocks })
        .await;
}
```

Add `recoverable_block_to_wire` to the existing service import at the top of `collab.rs`:
`use cairn_service::{block_op_from_wire, block_op_to_wire, recoverable_block_to_wire};`

- [ ] **Step 6: Verify the whole crate compiles and the handler match is exhaustive**

Run: `cargo build -p cairn-daemon --locked`
Expected: builds clean (adding the arm keeps the `match` exhaustive over `CollabClientMsg`).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-daemon/src/collab.rs
git commit -m "feat(collab): answer Recover with the session's recoverable blocks

New read-only recoverable_blocks() helper + a run_collab Recover arm that
replies Recoverable to the requesting socket only (never fanned out). Empty
when no session is open for the note.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Workspace gate + PR

**Files:** none (verification + integration).

- [ ] **Step 1: Full workspace build/test**

Run: `cargo test --workspace --locked`
Expected: green. (Known-flaky `invoke_times_out_and_kills_plugin` / sandbox-win may eject — re-run those.)

- [ ] **Step 2: Clippy + fmt**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 3: Rebase on origin/main (main moves fast under parallel workspaces)**

```bash
git fetch origin
git rebase origin/main
```

- [ ] **Step 4: Push + open PR**

```bash
git push -u origin collab-recovery-surface
gh pr create --base main --title "feat(collab): view-only recovery surface (Recover/Recoverable)" --body "..."
```

PR body: summarize the four layers, the ephemeral/live-session constraint, view-only + out-of-scope (rendering, restore), and the DoD evidence. Reference the design spec.

- [ ] **Step 5: Enable auto-merge**

```bash
gh pr merge <#> --auto
```

Merge queue owns strategy — do NOT pass `--squash` or manually update the branch.

## Self-Review

**Spec coverage:**
- Domain `recoverable_blocks` + `RecoverableBlock` → Task 1. ✅
- Contract `Recover`/`Recoverable`/`WireRecoverableBlock` → Task 2. ✅
- Service `recoverable_block_to_wire` → Task 3. ✅
- Daemon read-only handler, requester-only reply, no-session→empty → Task 4. ✅
- Ephemeral-only, over-preserve note → behavioral, encoded in tests (tombstoned block surfaces) + no persistence added. ✅
- DoD gate + land → Task 5. ✅

**Placeholder scan:** PR body `--body "..."` in Task 5 is intentionally filled at PR time from the spec; all code steps contain real content.

**Type consistency:** `RecoverableBlock { id, tombstoned, versions }` and `WireRecoverableBlock { id, tombstoned, versions }` are consistent across Tasks 1–4; `recoverable_block_to_wire` signature matches its call site in Task 4; `recoverable_blocks` helper name matches its test and call site.
