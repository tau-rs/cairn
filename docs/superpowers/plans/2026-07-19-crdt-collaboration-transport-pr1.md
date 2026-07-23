# CRDT Live Collaboration Transport — PR-1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the daemon op-relay and wire protocol for CRDT collaboration — two raw WebSocket clients editing one note converge over the wire — with **no disk writes** (materialize/commit and the client adapter are PR-2).

**Architecture:** Peer-replica model (spec §2, decision P): the daemon runs a dumb fan-out relay on a new bidirectional `/collab` WebSocket and also holds one in-memory `BlockDoc` replica per open note. A joining peer is caught up with a `Snapshot` = **state-as-ops** (`Vec<BlockOp>`, spec §4.2). `BlockOp` is the only CRDT type on the wire, mirrored as `WireBlockOp` in `cairn-contract` with mappings in `cairn-service` (spec §5.2, corrected: contract is domain-independent, so mappings live in service alongside `node_to_wire`/`graph_to_wire`).

**Tech Stack:** Rust (MSRV 1.88), axum WebSockets, `tokio::sync::{broadcast,mpsc}`, `futures-util` split-sink, `serde`/`ts-rs` (contract), `proptest`-free unit tests.

## Global Constraints

- MSRV 1.88; `#![forbid(unsafe_code)]` — no `unsafe`.
- `thiserror` at boundaries, `anyhow` internally. (PR-1 adds no new error types; the relay logs-and-drops best-effort, mirroring the existing `/events` forward loop.)
- `cairn-domain` stays **serde-free** — no `serde` derive or dependency added to it.
- `cairn-contract` stays **independent of `cairn-domain`** — its DTOs reference no domain type; domain↔wire mappings live in `cairn-service`.
- Every wire type in `cairn-contract` derives `Serialize, Deserialize, TS` and carries `#[ts(export)]`, matching the existing `Command`/`Query`/`Event` DTOs.
- Merge queue: branch off `main` → PR → `gh pr merge --auto --squash`. No manual rebase/local-merge. Shared working dir → check `git branch` before every commit.
- New dependency ⇒ `git add Cargo.lock`. (PR-1 adds **no** new crate dependencies.)
- DoD per PR: `cargo test --workspace` + `cargo clippy --workspace --all-targets --locked` + `cargo fmt --check` all green.
- Conventional commits, imperative, scoped.

---

## File Structure

- `crates/cairn-domain/src/crdt.rs` — **modify**: add `BlockDoc::state_as_ops(&self) -> Vec<BlockOp>` (the catch-up primitive; also the future `.cairn/` sidecar primitive).
- `crates/cairn-contract/src/lib.rs` — **modify**: add wire types `WireBlockId`, `WireAuthor`, `WireBlockKind`, `WireBlockOp`, and the envelope `CollabClientMsg` / `CollabServerMsg`.
- `crates/cairn-service/src/lib.rs` — **modify**: add domain↔wire mappings `block_op_to_wire` / `block_op_from_wire` (+ id/author/kind helpers) and a round-trip test.
- `crates/cairn-daemon/src/collab.rs` — **create**: the session registry (`Collab`, `Session`) and the `run_collab` connection driver (relay + snapshot; no disk).
- `crates/cairn-daemon/src/lib.rs` — **modify**: add the `collab` field to `AppState`, the `/collab` route + `collab_handler`, wire the module in.
- `crates/cairn-daemon/tests/collab.rs` — **create**: integration tests (two-client convergence, late-joiner snapshot, auth rejection).

---

## Task 1: `BlockDoc::state_as_ops` (catch-up primitive)

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs`
- Test: `crates/cairn-domain/src/crdt.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `BlockDoc`, `BlockOp{Insert,Delete,SetContent}`, `BlockId`, `Author`, `Edit`, private `Entry` fields (`id, after, ins_lamport, kind, text, content_lamport, content_author, tombstone`).
- Produces: `pub fn state_as_ops(&self) -> Vec<BlockOp>` — the minimal op-set that, applied via `merge` to a fresh empty `BlockDoc`, reconstructs this document's materialized state (and seeds the joiner's clock).

- [ ] **Step 1: Write the failing test**

Add to `crates/cairn-domain/src/crdt.rs` inside `mod tests`:

```rust
#[test]
fn state_as_ops_reconstructs_into_a_fresh_replica() {
    // Build a doc, then mutate: edit a block, append a block, delete a block.
    let mut a = BlockDoc::from_markdown(1, "# Title\n\nbody\n");
    let ids = a.block_ids_in_order();
    let (title, body) = (ids[0], ids[1]);
    a.apply_local(Edit::UpdateText {
        id: body,
        text: "new body".into(),
        author: Author::Human,
    });
    a.apply_local(Edit::InsertAfter {
        after: Some(body),
        kind: crate::block::BlockKind::Paragraph,
        text: "tail".into(),
        author: Author::Human,
    });
    a.apply_local(Edit::Remove { id: title });

    // Replay the snapshot op-set into an empty replica.
    let mut b = BlockDoc::from_markdown(2, "");
    for op in a.state_as_ops() {
        b.merge(op);
    }

    assert_eq!(a.materialize(), b.materialize());
    // Replay is idempotent (the CRDT law): applying twice changes nothing.
    for op in a.state_as_ops() {
        b.merge(op);
    }
    assert_eq!(a.materialize(), b.materialize());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-domain state_as_ops_reconstructs -- --nocapture`
Expected: FAIL — `no method named state_as_ops found for struct BlockDoc`.

- [ ] **Step 3: Write minimal implementation**

Add this method inside `impl BlockDoc` in `crates/cairn-domain/src/crdt.rs` (place it next to `block_ids_in_order`):

```rust
/// The minimal op-set that reconstructs this document's current state when
/// merged into a fresh replica. Used to catch up a peer joining a live
/// session (and, later, to persist a `.cairn/` resume sidecar). Every block
/// is re-created with its live-only `BlockId`, so a joiner adopts the shared
/// identities rather than minting fresh ones. Order-independent: `merge` is
/// commutative and idempotent, so the receiver may apply these in any order.
#[must_use]
pub fn state_as_ops(&self) -> Vec<BlockOp> {
    let mut ops = Vec::with_capacity(self.entries.len());
    for e in self.entries.values() {
        // Re-establish the block at its position with its current content.
        ops.push(BlockOp::Insert {
            id: e.id,
            after: e.after,
            lamport: e.ins_lamport,
            kind: e.kind,
            text: e.text.clone(),
        });
        // If content advanced past the insert seed, re-affirm it at the right
        // Lamport/author so a joiner's later edits order correctly against it.
        if e.content_lamport != e.ins_lamport || e.content_author != Author::Human {
            ops.push(BlockOp::SetContent {
                id: e.id,
                text: e.text.clone(),
                lamport: e.content_lamport,
                author: e.content_author,
            });
        }
        // Preserve deletions.
        if e.tombstone {
            ops.push(BlockOp::Delete {
                id: e.id,
                lamport: e.content_lamport.max(e.ins_lamport),
            });
        }
    }
    ops
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-domain state_as_ops_reconstructs`
Expected: PASS. Also run the whole crate to confirm no regression: `cargo test -p cairn-domain`.

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: crdt-live-collab-transport
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(crdt): BlockDoc::state_as_ops catch-up op-set

Emit the minimal op-set that reconstructs a document's materialized state
in a fresh replica (shared block IDs preserved). The join-snapshot
primitive for the collab transport; property-independent since merge is
commutative + idempotent.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Wire types in `cairn-contract`

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- Test: `crates/cairn-contract/src/lib.rs` (`#[cfg(test)] mod tests` — add if absent)

**Interfaces:**
- Consumes: nothing from other tasks (contract is leaf; domain-independent).
- Produces (all `pub`, deriving `Serialize, Deserialize, TS`, `#[ts(export)]`):
  - `WireBlockId { replica: u64, counter: u64 }`
  - `WireAuthor { Human, Agent }`
  - `WireBlockKind { Frontmatter, Heading, Paragraph, ListItem, CodeFence, BlockQuote, Table, ThematicBreak }`
  - `WireBlockOp` — internally tagged on `"op"`, variants `Insert{id,after,lamport,kind,text}`, `Delete{id,lamport}`, `SetContent{id,text,lamport,author}`.
  - `CollabClientMsg` — tagged on `"type"`: `Join{note,replica}`, `Op{note,op}`, `Leave{note}`.
  - `CollabServerMsg` — tagged on `"type"`: `Joined{note}`, `Snapshot{note,ops}`, `Op{note,op}`, `Error{note,message}`. Derives `Clone` (the daemon fans it out over a broadcast channel).

- [ ] **Step 1: Write the failing test**

Add to `crates/cairn-contract/src/lib.rs` (create a `mod tests` at end of file if none exists):

```rust
#[cfg(test)]
mod collab_wire_tests {
    use super::*;

    #[test]
    fn wire_block_op_json_is_tagged_on_op() {
        let op = WireBlockOp::Insert {
            id: WireBlockId { replica: 1, counter: 0 },
            after: None,
            lamport: 1,
            kind: WireBlockKind::Paragraph,
            text: "hi".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&op).unwrap();
        assert_eq!(v["op"], "insert");
        assert_eq!(v["kind"], "paragraph");
        assert_eq!(v["id"]["replica"], 1);
    }

    #[test]
    fn collab_client_msg_round_trips() {
        let msg = CollabClientMsg::Join { note: "n.md".into(), replica: 7 };
        let text = serde_json::to_string(&msg).unwrap();
        let back: CollabClientMsg = serde_json::from_str(&text).unwrap();
        assert_eq!(msg, back);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-contract collab_wire`
Expected: FAIL — `cannot find type WireBlockOp` / `CollabClientMsg` in this scope.

- [ ] **Step 3: Write minimal implementation**

Append to `crates/cairn-contract/src/lib.rs` (the `use serde::{Deserialize, Serialize};` and `use ts_rs::TS;` are already imported at the top):

```rust
/// A block's live-only identity, mirrored for the wire. See `cairn-domain`
/// `BlockId`. Stripped on materialize; meaningful only within a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WireBlockId {
    pub replica: u64,
    pub counter: u64,
}

/// Who authored an edit (wire mirror of `cairn-domain` `Author`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum WireAuthor {
    Human,
    Agent,
}

/// Block taxonomy (wire mirror of `cairn-domain` `BlockKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum WireBlockKind {
    Frontmatter,
    Heading,
    Paragraph,
    ListItem,
    CodeFence,
    BlockQuote,
    Table,
    ThematicBreak,
}

/// A replicated block operation on the wire (mirror of `cairn-domain`
/// `BlockOp`). The only CRDT type carried by `/collab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WireBlockOp {
    Insert {
        id: WireBlockId,
        after: Option<WireBlockId>,
        lamport: u64,
        kind: WireBlockKind,
        text: String,
    },
    Delete {
        id: WireBlockId,
        lamport: u64,
    },
    SetContent {
        id: WireBlockId,
        text: String,
        lamport: u64,
        author: WireAuthor,
    },
}

/// Messages a collaboration client sends to the daemon over `/collab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollabClientMsg {
    /// Seed me from the live session and subscribe to `note`.
    Join { note: String, replica: u64 },
    /// One local edit to broadcast.
    Op { note: String, op: WireBlockOp },
    /// Unsubscribe from `note`.
    Leave { note: String },
}

/// Messages the daemon sends to a collaboration client over `/collab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollabServerMsg {
    /// Acknowledges a `Join`.
    Joined { note: String },
    /// Catch-up: the current state expressed as an op-set (apply all).
    Snapshot { note: String, ops: Vec<WireBlockOp> },
    /// A peer's edit, fanned out.
    Op { note: String, op: WireBlockOp },
    /// A per-note error (e.g. replica-id collision, bad path).
    Error { note: String, message: String },
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-contract collab_wire`
Expected: PASS. Then `cargo test -p cairn-contract` to confirm the TS-export tests (if any) still pass.

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: crdt-live-collab-transport
git add crates/cairn-contract/src/lib.rs
git commit -m "feat(contract): collab wire types (WireBlockOp + envelope)

Add WireBlockId/WireAuthor/WireBlockKind/WireBlockOp and the
CollabClientMsg/CollabServerMsg envelope, all Ser/De/TS. Contract stays
independent of cairn-domain; the domain<->wire mapping lands in
cairn-service next.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Domain↔wire mappings in `cairn-service`

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`
- Test: `crates/cairn-service/src/lib.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `cairn_domain::{BlockOp, BlockId, Author, block::BlockKind}` (Task-independent, already landed) and `cairn_contract::{WireBlockOp, WireBlockId, WireAuthor, WireBlockKind}` (Task 2).
- Produces (both `pub`):
  - `pub fn block_op_to_wire(op: BlockOp) -> WireBlockOp`
  - `pub fn block_op_from_wire(op: WireBlockOp) -> BlockOp`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/cairn-service/src/lib.rs`:

```rust
#[test]
fn block_op_round_trips_through_the_wire() {
    use cairn_domain::{block::BlockKind, Author, BlockId, BlockOp};

    let cases = vec![
        BlockOp::Insert {
            id: BlockId { replica: 3, counter: 9 },
            after: Some(BlockId { replica: 1, counter: 0 }),
            lamport: 5,
            kind: BlockKind::ListItem,
            text: "- a".into(),
        },
        BlockOp::Delete {
            id: BlockId { replica: 2, counter: 4 },
            lamport: 8,
        },
        BlockOp::SetContent {
            id: BlockId { replica: 7, counter: 1 },
            text: "hello".into(),
            lamport: 12,
            author: Author::Agent,
        },
    ];
    for op in cases {
        let round = block_op_from_wire(block_op_to_wire(op.clone()));
        assert_eq!(op, round);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-service block_op_round_trips`
Expected: FAIL — `cannot find function block_op_to_wire`.

- [ ] **Step 3: Write minimal implementation**

Add near the other `*_to_wire` mappings in `crates/cairn-service/src/lib.rs` (the file already imports `cairn_contract` and `cairn_domain`; add explicit `use` paths at the top of these fns if the wildcard imports don't already cover them):

```rust
/// Map a domain `BlockId` to its wire mirror.
fn block_id_to_wire(id: cairn_domain::BlockId) -> cairn_contract::WireBlockId {
    cairn_contract::WireBlockId { replica: id.replica, counter: id.counter }
}
fn block_id_from_wire(id: cairn_contract::WireBlockId) -> cairn_domain::BlockId {
    cairn_domain::BlockId { replica: id.replica, counter: id.counter }
}

fn author_to_wire(a: cairn_domain::Author) -> cairn_contract::WireAuthor {
    match a {
        cairn_domain::Author::Human => cairn_contract::WireAuthor::Human,
        cairn_domain::Author::Agent => cairn_contract::WireAuthor::Agent,
    }
}
fn author_from_wire(a: cairn_contract::WireAuthor) -> cairn_domain::Author {
    match a {
        cairn_contract::WireAuthor::Human => cairn_domain::Author::Human,
        cairn_contract::WireAuthor::Agent => cairn_domain::Author::Agent,
    }
}

fn block_kind_to_wire(k: cairn_domain::block::BlockKind) -> cairn_contract::WireBlockKind {
    use cairn_contract::WireBlockKind as W;
    use cairn_domain::block::BlockKind as K;
    match k {
        K::Frontmatter => W::Frontmatter,
        K::Heading => W::Heading,
        K::Paragraph => W::Paragraph,
        K::ListItem => W::ListItem,
        K::CodeFence => W::CodeFence,
        K::BlockQuote => W::BlockQuote,
        K::Table => W::Table,
        K::ThematicBreak => W::ThematicBreak,
    }
}
fn block_kind_from_wire(k: cairn_contract::WireBlockKind) -> cairn_domain::block::BlockKind {
    use cairn_contract::WireBlockKind as W;
    use cairn_domain::block::BlockKind as K;
    match k {
        W::Frontmatter => K::Frontmatter,
        W::Heading => K::Heading,
        W::Paragraph => K::Paragraph,
        W::ListItem => K::ListItem,
        W::CodeFence => K::CodeFence,
        W::BlockQuote => K::BlockQuote,
        W::Table => K::Table,
        W::ThematicBreak => K::ThematicBreak,
    }
}

/// Map a domain `BlockOp` to its wire mirror (the only CRDT type on the wire).
#[must_use]
pub fn block_op_to_wire(op: cairn_domain::BlockOp) -> cairn_contract::WireBlockOp {
    use cairn_contract::WireBlockOp as W;
    use cairn_domain::BlockOp as B;
    match op {
        B::Insert { id, after, lamport, kind, text } => W::Insert {
            id: block_id_to_wire(id),
            after: after.map(block_id_to_wire),
            lamport,
            kind: block_kind_to_wire(kind),
            text,
        },
        B::Delete { id, lamport } => W::Delete { id: block_id_to_wire(id), lamport },
        B::SetContent { id, text, lamport, author } => W::SetContent {
            id: block_id_to_wire(id),
            text,
            lamport,
            author: author_to_wire(author),
        },
    }
}

/// Map a wire `WireBlockOp` back to the domain `BlockOp`.
#[must_use]
pub fn block_op_from_wire(op: cairn_contract::WireBlockOp) -> cairn_domain::BlockOp {
    use cairn_contract::WireBlockOp as W;
    use cairn_domain::BlockOp as B;
    match op {
        W::Insert { id, after, lamport, kind, text } => B::Insert {
            id: block_id_from_wire(id),
            after: after.map(block_id_from_wire),
            lamport,
            kind: block_kind_from_wire(kind),
            text,
        },
        W::Delete { id, lamport } => B::Delete { id: block_id_from_wire(id), lamport },
        W::SetContent { id, text, lamport, author } => B::SetContent {
            id: block_id_from_wire(id),
            text,
            lamport,
            author: author_from_wire(author),
        },
    }
}
```

Note: if `BlockId`/`Author` are not re-exported at `cairn_domain::` root, use the paths they actually live at (`cairn_domain::BlockId`, `cairn_domain::Author` are re-exported per the existing `use cairn_domain::...` in `collab.rs`; adjust to `cairn_domain::crdt::...` only if the compiler reports them private). `BlockKind` lives at `cairn_domain::block::BlockKind`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-service block_op_round_trips`
Expected: PASS. Then `cargo test -p cairn-service`.

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: crdt-live-collab-transport
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): BlockOp<->WireBlockOp mappings

Bridge the domain CRDT op to its contract wire mirror (and back),
alongside the existing domain<->wire mappings. Round-trip tested.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Daemon `/collab` relay + session registry

**Files:**
- Create: `crates/cairn-daemon/src/collab.rs`
- Modify: `crates/cairn-daemon/src/lib.rs` (add `mod collab;`, `AppState.collab` field + init, `/collab` route + `collab_handler`)
- Test: `crates/cairn-daemon/tests/collab.rs` (create)

**Interfaces:**
- Consumes: `cairn_contract::{CollabClientMsg, CollabServerMsg, WireBlockOp}` (Task 2), `cairn_service::{block_op_to_wire, block_op_from_wire}` (Task 3), `cairn_domain::{BlockDoc, NotePath}`, `BlockDoc::state_as_ops` (Task 1), and the existing `AppState`, `ws_origin_allowed`, `auth::mcp_require_token`, `Engine::read_note`.
- Produces: `pub const collab::DAEMON_REPLICA: u64`, `pub type collab::Collab`, `collab::registry()`, `collab::run_collab(...)`; a `/collab` route on the router.

- [ ] **Step 1: Write the failing integration test**

Create `crates/cairn-daemon/tests/collab.rs`:

```rust
//! `/collab` op-relay: two raw WS clients converge over the wire; a late
//! joiner is caught up by the snapshot; auth is enforced. See spec §8.

use cairn_app::Engine;
use cairn_contract::{CollabClientMsg, CollabServerMsg, WireBlockOp};
use cairn_daemon::{build_router, AppState};
use cairn_domain::{block::BlockKind, Author, BlockDoc, BlockId, BlockOp, NotePath};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_service::{block_op_from_wire, block_op_to_wire};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

const ORIGIN: &str = "http://localhost:5173";
const TOKEN: &str = "secret";

type Ws = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn serve() -> std::net::SocketAddr {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine)
        .with_allowed_origins(vec![ORIGIN.to_string()])
        .with_token(TOKEN);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
        drop(tmp);
    });
    addr
}

fn req(addr: std::net::SocketAddr, origin: Option<&str>, token: Option<&str>) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let url = match token {
        Some(t) => format!("ws://{addr}/collab?token={t}"),
        None => format!("ws://{addr}/collab"),
    };
    let mut r = url.into_client_request().unwrap();
    if let Some(o) = origin {
        r.headers_mut().insert("origin", o.parse().unwrap());
    }
    r
}

async fn connect(addr: std::net::SocketAddr) -> Ws {
    tokio_tungstenite::connect_async(req(addr, Some(ORIGIN), Some(TOKEN)))
        .await
        .expect("handshake")
        .0
}

async fn send(ws: &mut Ws, msg: &CollabClientMsg) {
    ws.send(Message::Text(serde_json::to_string(msg).unwrap().into()))
        .await
        .unwrap();
}

async fn recv(ws: &mut Ws) -> CollabServerMsg {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

fn insert(replica: u64, text: &str) -> BlockOp {
    BlockOp::Insert {
        id: BlockId { replica, counter: 0 },
        after: None,
        lamport: 1,
        kind: BlockKind::Paragraph,
        text: text.into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_clients_converge_over_the_wire() {
    let addr = serve().await;
    let note = "n.md";

    let mut c1 = connect(addr).await;
    let mut c2 = connect(addr).await;
    send(&mut c1, &CollabClientMsg::Join { note: note.into(), replica: 1 }).await;
    send(&mut c2, &CollabClientMsg::Join { note: note.into(), replica: 2 }).await;
    // Each gets Joined + (empty) Snapshot.
    assert!(matches!(recv(&mut c1).await, CollabServerMsg::Joined { .. }));
    assert!(matches!(recv(&mut c1).await, CollabServerMsg::Snapshot { .. }));
    assert!(matches!(recv(&mut c2).await, CollabServerMsg::Joined { .. }));
    assert!(matches!(recv(&mut c2).await, CollabServerMsg::Snapshot { .. }));

    let op1 = insert(1, "from one");
    let op2 = insert(2, "from two");
    send(&mut c1, &CollabClientMsg::Op { note: note.into(), op: block_op_to_wire(op1.clone()) }).await;
    send(&mut c2, &CollabClientMsg::Op { note: note.into(), op: block_op_to_wire(op2.clone()) }).await;

    // Each client receives the OTHER's op (self-echo suppressed).
    let got1 = match recv(&mut c1).await {
        CollabServerMsg::Op { op, .. } => block_op_from_wire(op),
        other => panic!("expected Op, got {other:?}"),
    };
    let got2 = match recv(&mut c2).await {
        CollabServerMsg::Op { op, .. } => block_op_from_wire(op),
        other => panic!("expected Op, got {other:?}"),
    };

    // Reconstruct each replica and assert identical materialize.
    let mut d1 = BlockDoc::from_markdown(1, "");
    d1.merge(op1.clone());
    d1.merge(got1);
    let mut d2 = BlockDoc::from_markdown(2, "");
    d2.merge(op2.clone());
    d2.merge(got2);
    assert_eq!(d1.materialize(), d2.materialize());
    assert!(d1.materialize().contains("from one"));
    assert!(d1.materialize().contains("from two"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_joiner_is_caught_up_by_snapshot() {
    let addr = serve().await;
    let note = "n.md";

    let mut c1 = connect(addr).await;
    send(&mut c1, &CollabClientMsg::Join { note: note.into(), replica: 1 }).await;
    let _ = recv(&mut c1).await; // Joined
    let _ = recv(&mut c1).await; // Snapshot (empty)

    let op1 = insert(1, "seeded");
    send(&mut c1, &CollabClientMsg::Op { note: note.into(), op: block_op_to_wire(op1.clone()) }).await;
    // Let the daemon merge op1 into its replica before the late join.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut c2 = connect(addr).await;
    send(&mut c2, &CollabClientMsg::Join { note: note.into(), replica: 2 }).await;
    assert!(matches!(recv(&mut c2).await, CollabServerMsg::Joined { .. }));
    let snap = match recv(&mut c2).await {
        CollabServerMsg::Snapshot { ops, .. } => ops,
        other => panic!("expected Snapshot, got {other:?}"),
    };

    let mut d2 = BlockDoc::from_markdown(2, "");
    for op in snap {
        d2.merge(block_op_from_wire(op));
    }
    assert!(d2.materialize().contains("seeded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collab_rejects_bad_token_and_origin() {
    let addr = serve().await;
    // Bad token -> 401.
    let err = tokio_tungstenite::connect_async(req(addr, Some(ORIGIN), Some("wrong")))
        .await
        .expect_err("bad token must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
        other => panic!("expected 401, got {other:?}"),
    }
    // Bad origin (valid token) -> 403.
    let err = tokio_tungstenite::connect_async(req(addr, Some("http://evil.example"), Some(TOKEN)))
        .await
        .expect_err("bad origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected 403, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-daemon --test collab`
Expected: FAIL to compile — `no route /collab` isn't a compile error, but `AppState` has no `collab` field / `mod collab` missing. First failure is the missing module + handler. (If it compiles, tests fail with handshake/timeouts.)

- [ ] **Step 3a: Create the collab module**

Create `crates/cairn-daemon/src/collab.rs`:

```rust
//! Bidirectional CRDT op-relay over `/collab`. The daemon is a dumb fan-out
//! that also holds one `BlockDoc` replica per open note (the git bridge). PR-1:
//! relay + catch-up snapshot only — no disk writes (materialize/commit and the
//! client adapter land in PR-2). See docs/superpowers/specs/
//! 2026-07-19-crdt-collaboration-transport-design.md.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use cairn_contract::{CollabClientMsg, CollabServerMsg};
use cairn_domain::{BlockDoc, NotePath};
use cairn_service::{block_op_from_wire, block_op_to_wire};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};

/// Replica id reserved for the daemon's own commit-agent replica. Clients must
/// choose other ids (the app assigns small positive ints).
pub const DAEMON_REPLICA: u64 = u64::MAX;

/// A fan-out envelope carrying the originating replica so a peer skips its own
/// echo (merge is idempotent, but skipping avoids redundant traffic).
#[derive(Clone)]
struct Fanout {
    origin: u64,
    msg: CollabServerMsg,
}

/// One live note session: the daemon's replica + a fan-out channel + the set of
/// joined replica ids (last one out tears the session down).
struct Session {
    doc: BlockDoc,
    peers: broadcast::Sender<Fanout>,
    participants: HashSet<u64>,
}

/// The daemon's collab registry: one `Session` per open note.
pub type Collab = Arc<Mutex<HashMap<NotePath, Session>>>;

/// Build an empty collab registry.
#[must_use]
pub fn registry() -> Collab {
    Arc::new(Mutex::new(HashMap::new()))
}

fn lock(collab: &Collab) -> std::sync::MutexGuard<'_, HashMap<NotePath, Session>> {
    collab.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drive one upgraded `/collab` socket. `seed` reads a note's current markdown
/// to seed a fresh session (empty string when the note does not exist yet).
pub async fn run_collab<S>(socket: WebSocket, collab: Collab, seed: S)
where
    S: Fn(&NotePath) -> String + Send + 'static,
{
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<CollabServerMsg>(64);

    // One writer task owns the socket sink.
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "collab: drop msg, serialize failed"),
            }
        }
    });

    let mut my_replica: Option<u64> = None;
    let mut forwarders: Vec<(NotePath, tokio::task::JoinHandle<()>)> = Vec::new();

    while let Some(Ok(frame)) = stream.next().await {
        let Message::Text(text) = frame else { continue };
        let msg: CollabClientMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "collab: drop unparseable client frame");
                continue;
            }
        };
        match msg {
            CollabClientMsg::Join { note, replica } => {
                let Ok(path) = NotePath::new(&note) else {
                    let _ = out_tx
                        .send(CollabServerMsg::Error { note, message: "invalid note path".into() })
                        .await;
                    continue;
                };
                my_replica = Some(replica);
                // Get-or-create the session; take a snapshot + a subscription.
                let joined = {
                    let mut reg = lock(&collab);
                    let sess = reg.entry(path.clone()).or_insert_with(|| {
                        let (tx, _rx) = broadcast::channel(256);
                        Session {
                            doc: BlockDoc::from_markdown(DAEMON_REPLICA, &seed(&path)),
                            peers: tx,
                            participants: HashSet::new(),
                        }
                    });
                    if sess.participants.insert(replica) {
                        Some((sess.doc.state_as_ops(), sess.peers.subscribe()))
                    } else {
                        None
                    }
                };
                let Some((ops, rx)) = joined else {
                    let _ = out_tx
                        .send(CollabServerMsg::Error {
                            note,
                            message: "replica id already joined".into(),
                        })
                        .await;
                    continue;
                };
                let _ = out_tx.send(CollabServerMsg::Joined { note: note.clone() }).await;
                let wire_ops = ops.into_iter().map(block_op_to_wire).collect();
                let _ = out_tx
                    .send(CollabServerMsg::Snapshot { note: note.clone(), ops: wire_ops })
                    .await;

                // Per-note forwarder: fan the session broadcast to this socket,
                // skipping this connection's own ops.
                let out2 = out_tx.clone();
                let handle = tokio::spawn(async move {
                    let mut rx = rx;
                    loop {
                        match rx.recv().await {
                            Ok(f) => {
                                if f.origin == replica {
                                    continue;
                                }
                                if out2.send(f.msg).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
                forwarders.push((path, handle));
            }
            CollabClientMsg::Op { note, op } => {
                let Ok(path) = NotePath::new(&note) else { continue };
                let domain_op = block_op_from_wire(op);
                let mut reg = lock(&collab);
                if let Some(sess) = reg.get_mut(&path) {
                    sess.doc.merge(domain_op.clone());
                    let _ = sess.peers.send(Fanout {
                        origin: my_replica.unwrap_or(DAEMON_REPLICA),
                        msg: CollabServerMsg::Op { note, op: block_op_to_wire(domain_op) },
                    });
                    // PR-1: no materialize/commit — the daemon replica stays in memory.
                }
            }
            CollabClientMsg::Leave { note } => {
                if let Ok(path) = NotePath::new(&note) {
                    leave(&collab, &path, my_replica);
                    forwarders.retain(|(p, h)| {
                        if *p == path {
                            h.abort();
                            false
                        } else {
                            true
                        }
                    });
                }
            }
        }
    }

    // Disconnect: drop this replica from every joined note; stop tasks.
    for (path, handle) in forwarders {
        handle.abort();
        leave(&collab, &path, my_replica);
    }
    writer.abort();
}

/// Remove `replica` from a note's session; drop the session when empty.
fn leave(collab: &Collab, path: &NotePath, replica: Option<u64>) {
    let Some(replica) = replica else { return };
    let mut reg = lock(collab);
    if let Some(sess) = reg.get_mut(path) {
        sess.participants.remove(&replica);
        if sess.participants.is_empty() {
            reg.remove(path);
        }
    }
}
```

- [ ] **Step 3b: Wire the module into `AppState` and the router**

In `crates/cairn-daemon/src/lib.rs`:

1. Add the module declaration near `mod mcp;`:
```rust
pub mod collab;
```

2. Add the field to `AppState` (after `mcp_write: bool,`):
```rust
    /// Live CRDT collaboration sessions, one per open note. Independent of the
    /// engine mutex so a relay fault cannot stall `/command`.
    collab: collab::Collab,
```

3. Initialize it in `AppState::new` (add to the struct literal):
```rust
            collab: collab::registry(),
```

4. Add the handler (place it next to `events_handler`):
```rust
async fn collab_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Same Origin gate as `/events` (browsers skip CORS on WS upgrades). The
    // token is enforced by the `mcp_require_token` route layer.
    if !ws_origin_allowed(&state.allowed_origins, headers.get(header::ORIGIN)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let collab = state.collab.clone();
    let seed_state = state.clone();
    ws.on_upgrade(move |socket| {
        collab::run_collab(socket, collab, move |path| {
            // Seed from the note's current content; empty if it does not exist.
            seed_state.engine().read_note(path).unwrap_or_default()
        })
    })
}
```

5. Register the route in `build_router` (add a token-gated group; reuse `mcp_require_token` so `?token=` works on the WS handshake):
```rust
    let collab = Router::new()
        .route("/collab", get(collab_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::mcp_require_token,
        ));
```
and include it in the merge:
```rust
    protected.merge(mcp).merge(collab).merge(open).with_state(state)
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-daemon --test collab`
Expected: PASS (all three tests). Then the full guardrails:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets --locked
cargo fmt --check
```
Expected: all green. (Note: `invoke_times_out_and_kills_plugin` is a known-flaky plugin test in this sandbox, unrelated to this change — see project memory.)

- [ ] **Step 5: Commit**

```bash
git branch   # confirm: crdt-live-collab-transport
git add crates/cairn-daemon/src/collab.rs crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/collab.rs
git commit -m "feat(daemon): /collab op-relay + per-note session registry

Bidirectional, note-multiplexed /collab WebSocket (Origin + ?token=): a
dumb fan-out of BlockOps that also holds one BlockDoc replica per open
note and catches up joiners with a state-as-ops Snapshot. No disk writes
yet (materialize/commit + client adapter are PR-2). Integration-tested:
two clients converge over the wire, late joiner is caught up, auth is
enforced.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Open the PR

- [ ] **Step 1: Push and open a PR against `main`**

```bash
git push -u origin crdt-live-collab-transport
gh pr create --base main --title "feat(collab): CRDT live collaboration transport — PR-1 (relay + protocol)" \
  --body "Implements PR-1 of docs/superpowers/plans/2026-07-19-crdt-collaboration-transport-pr1.md (spec §9). Daemon /collab op-relay + wire protocol + state-as-ops catch-up. No disk writes (PR-2 adds the git bridge + client adapter).

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

- [ ] **Step 2: Enqueue via the merge queue (only after review approval)**

```bash
gh pr merge --auto --squash
```

---

## Self-Review

**1. Spec coverage (PR-1 scope, spec §9):**
- `WireBlockOp` + envelope in contract → Task 2. ✅
- `BlockOp` mapping (in service, not contract — spec §5.2 correction) → Task 3. ✅
- `BlockDoc::state_as_ops` → Task 1. ✅
- Daemon `/collab` route + session registry + fan-out → Task 4. ✅
- Wire-convergence + join-catch-up + auth tests (spec §8 items 1, 3, 6) → Task 4 tests. ✅
- Snapshot-as-op-set (spec §4.2) → Task 1 + Task 4 Join path. ✅
- `/collab` note-multiplexed, Origin + `?token=` (spec §5.1) → Task 4 handler + `mcp_require_token` layer. ✅
- Materialize is a no-op stub, no disk (spec §9 PR-1) → Task 4 `Op` arm comment; no store write. ✅
- **Deferred to PR-2 (correctly absent here):** daemon materialize/commit, self-write suppression, external-edit fold-back (spec §3), the client actor adapter (spec §7). Not in this plan. ✅

**2. Placeholder scan:** No TBD/TODO/"handle errors"/"similar to". Every code step is complete. The one conditional note (Task 3 `use`-path fallback if `BlockId`/`Author` aren't root re-exports) is a concrete compiler-guided instruction, not a placeholder. ✅

**3. Type consistency:** `state_as_ops` (Task 1) ↔ used in Task 4. `WireBlockOp`/`CollabClientMsg`/`CollabServerMsg` (Task 2) ↔ mapped in Task 3 (`block_op_to_wire`/`block_op_from_wire`) ↔ consumed in Task 4 and the test. `DAEMON_REPLICA`, `Collab`, `registry`, `run_collab` all defined in Task 4's `collab.rs` and referenced consistently in `lib.rs`. `CollabServerMsg` derives `Clone` (needed by `Fanout`/broadcast). Field/variant names match across contract, service, daemon, and tests. ✅

## Notes carried to PR-2 (not this plan)

- Seeding uses `read_note` (working-tree read); switch to an explicit git-HEAD read if PR-2's commit boundary needs it.
- `state_as_ops` self-affirming `SetContent` puts identical text into the block stash on the joiner (cosmetic). Revisit minimality (spec §11) when stashes surface in the UI.
- Nested lock order is engine-then-collab only inside the `seed` closure; PR-2's commit path must preserve a single global order to avoid deadlock.
