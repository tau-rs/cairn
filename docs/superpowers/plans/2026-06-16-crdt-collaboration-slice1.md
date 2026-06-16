# CRDT Collaboration — Slice 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the block-level CRDT convergence model as a pure, property-tested domain type, plus the `CollabSession` port shape and an in-memory `LocalCrdt` adapter — proving multi-writer convergence with no transport, UI, or relay.

**Architecture:** A pure `BlockDoc` in `cairn-domain` represents one note as an RGA sequence of blocks (each block's text an author-priority LWW register). Markdown is split into blocks by blank lines (code fences atomic, list items per-line); block IDs are live-only and stripped on `materialize`. The existing `CollabSession` port expands from `is_active()`-only to a transport-blind session seam; `LocalCrdt` (cairn-infra) wraps a per-note `BlockDoc`. Convergence (commutativity / associativity / idempotence) and markdown round-trip are property-tested with `proptest`.

**Tech Stack:** Rust (`forbid(unsafe_code)` workspace-wide), `thiserror` (already a domain dep), `proptest` (new dev-dependency), no runtime CRDT library (hand-rolled per ADR-0011 §5).

**Spec:** `docs/superpowers/specs/2026-06-16-crdt-collaboration-design.md`
**ADR:** `docs/decisions/0011-crdt-collaboration-model.md`

**Scope notes (decisions locked here, consistent with spec §9 open questions):**
- Slice 1 ops are **`Insert`, `Delete`, `SetContent`**. Reordering a block = `Delete` + `Insert` (new id). A native `Move` op is **deferred** (spec open question #4).
- Same-block content LWW uses a deterministic total order **(author-rank, lamport, replica)** — any `Human` edit beats any `Agent` edit; the loser's text is stashed. This is the strong, deterministic form of "human-wins"; true concurrency-aware policy (vector clocks) is deferred. The convergence proof only needs *a* deterministic total order; this is one.
- Round-trip normalization is **defined**: blocks joined by exactly one blank line (`\n\n`), no leading/trailing blank lines, single trailing newline. The round-trip property test generates normalized markdown.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/cairn-domain/src/block.rs` (create) | Block taxonomy + markdown ↔ blocks: `BlockKind`, `Block`, `parse_blocks`, `join_blocks`. Pure string work. |
| `crates/cairn-domain/src/crdt.rs` (create) | The CRDT: `BlockId`, `Author`, `BlockOp`, `Edit`, `BlockDoc` (RGA + LWW), `from_markdown`/`apply_local`/`merge`/`materialize`. |
| `crates/cairn-domain/src/lib.rs` (modify) | `pub mod block; pub mod crdt;` + re-exports. |
| `crates/cairn-domain/Cargo.toml` (modify) | Add `proptest` dev-dependency. |
| `Cargo.toml` (workspace, modify) | Add `proptest` to `[workspace.dependencies]`. |
| `crates/cairn-domain/tests/convergence.rs` (create) | The headline property tests: N-replica convergence + round-trip. |
| `crates/cairn-ports/src/lib.rs` (modify) | Expand `CollabSession` trait to the session seam. |
| `crates/cairn-infra/src/seams.rs` (modify) | Update `NoCollab` to the expanded trait (neutral no-ops). |
| `crates/cairn-infra/src/collab.rs` (create) | `LocalCrdt` adapter: per-note `BlockDoc` map behind `CollabSession`. |
| `crates/cairn-infra/src/lib.rs` (modify) | `mod collab; pub use collab::LocalCrdt;`. |

---

## Task 1: Scaffolding — proptest dep + empty modules

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`)
- Modify: `crates/cairn-domain/Cargo.toml`
- Create: `crates/cairn-domain/src/block.rs`
- Create: `crates/cairn-domain/src/crdt.rs`
- Modify: `crates/cairn-domain/src/lib.rs`

- [ ] **Step 1: Add `proptest` to workspace deps**

In `Cargo.toml`, add to the end of `[workspace.dependencies]`:

```toml
proptest = "1"
```

- [ ] **Step 2: Add `proptest` as a dev-dependency of cairn-domain**

In `crates/cairn-domain/Cargo.toml`, after the `[dependencies]` block:

```toml
[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 3: Create empty modules**

`crates/cairn-domain/src/block.rs`:

```rust
//! Splitting a note's markdown into blocks and joining blocks back to
//! canonical markdown. Pure string work, no I/O.
```

`crates/cairn-domain/src/crdt.rs`:

```rust
//! Block-level CRDT for one note: an RGA sequence of blocks whose content is
//! an author-priority LWW register. Block IDs are live-only and never reach
//! disk. See docs/decisions/0011-crdt-collaboration-model.md.
```

- [ ] **Step 4: Wire modules into the crate**

In `crates/cairn-domain/src/lib.rs`, after the existing `pub use graph::Graph;` line add:

```rust
pub mod block;
pub use block::{Block, BlockKind};

pub mod crdt;
pub use crdt::{Author, BlockDoc, BlockId, BlockOp, Edit};
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build -p cairn-domain`
Expected: PASS (empty modules compile; unused-warning-free because modules are just doc comments so far).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/cairn-domain/Cargo.toml crates/cairn-domain/src/
git commit -m "chore(domain): scaffold block + crdt modules, add proptest dev-dep"
```

---

## Task 2: Block parser — split markdown into blocks

**Files:**
- Modify: `crates/cairn-domain/src/block.rs`
- Test: inline `#[cfg(test)]` in `block.rs`

Granularity: blank line = boundary; a fenced code block (```` ``` ````-delimited) is one atomic block even with internal blank lines; consecutive list-item lines split one block per item.

- [ ] **Step 1: Write failing tests**

Append to `crates/cairn-domain/src/block.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_and_text(src: &str) -> Vec<(BlockKind, &str)> {
        // helper used by assertions below; returns owned via leak-free borrow
        unreachable!()
    }

    #[test]
    fn wrapped_paragraph_is_one_block() {
        let b = parse_blocks("The review went well.\nWe hit our targets.");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, BlockKind::Paragraph);
        assert_eq!(b[0].text, "The review went well.\nWe hit our targets.");
    }

    #[test]
    fn blank_line_separates_blocks() {
        let b = parse_blocks("First para.\n\nSecond para.");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].text, "First para.");
        assert_eq!(b[1].text, "Second para.");
    }

    #[test]
    fn each_list_item_is_a_block() {
        let b = parse_blocks("- call Bob\n- ship v1");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].kind, BlockKind::ListItem);
        assert_eq!(b[0].text, "- call Bob");
        assert_eq!(b[1].text, "- ship v1");
    }

    #[test]
    fn code_fence_is_one_atomic_block() {
        let src = "```rust\nfn main() {}\n\nlet x = 1;\n```";
        let b = parse_blocks(src);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, BlockKind::CodeFence);
        assert_eq!(b[0].text, src);
    }

    #[test]
    fn heading_and_thematic_break_classified() {
        let b = parse_blocks("# Title\n\n---");
        assert_eq!(b[0].kind, BlockKind::Heading);
        assert_eq!(b[1].kind, BlockKind::ThematicBreak);
    }

    let _ = kinds_and_text; // silence unused in case
}
```

(Delete the placeholder `kinds_and_text` helper and its `let _` line once real tests compile — it exists only so this snippet has no dangling reference; the real assertions above do not use it.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-domain block::`
Expected: FAIL — `parse_blocks`, `Block`, `BlockKind` not defined.

- [ ] **Step 3: Implement the parser**

At the top of `crates/cairn-domain/src/block.rs` (below the module doc comment):

```rust
/// The kind of a markdown block. Used as metadata on a CRDT block; it does
/// not affect convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Frontmatter,
    Heading,
    Paragraph,
    ListItem,
    CodeFence,
    BlockQuote,
    Table,
    ThematicBreak,
}

/// One parsed block: its kind and its exact source text (surrounding blank
/// lines trimmed off).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

/// Classify a single block's text by its first line.
fn classify(text: &str) -> BlockKind {
    let first = text.lines().next().unwrap_or("");
    let t = first.trim_start();
    if first.starts_with("```") || first.starts_with("~~~") {
        BlockKind::CodeFence
    } else if t == "---" || t == "***" || t == "___" {
        BlockKind::ThematicBreak
    } else if t.starts_with("# ")
        || t.starts_with("## ")
        || t.starts_with("### ")
        || t.starts_with("#### ")
        || t.starts_with("##### ")
        || t.starts_with("###### ")
    {
        BlockKind::Heading
    } else if is_list_item(first) {
        BlockKind::ListItem
    } else if t.starts_with('>') {
        BlockKind::BlockQuote
    } else if t.starts_with('|') {
        BlockKind::Table
    } else {
        BlockKind::Paragraph
    }
}

/// A list-item line: `- `, `* `, `+ `, or `<digits>. ` (ordered).
fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- ").or(t.strip_prefix("* ")).or(t.strip_prefix("+ ")) {
        return !rest.is_empty() || true; // marker alone still a list item
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && t[digits.len()..].starts_with(". ")
}

/// Split a note's markdown into blocks. Boundary = blank line. A fenced code
/// block is one atomic block. A run of consecutive list-item lines splits into
/// one block per item.
#[must_use]
pub fn parse_blocks(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        // Skip blank separator lines.
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        // Fenced code block: consume until the closing fence.
        if lines[i].starts_with("```") || lines[i].starts_with("~~~") {
            let fence = &lines[i][..3];
            let start = i;
            i += 1;
            while i < lines.len() && !lines[i].starts_with(fence) {
                i += 1;
            }
            if i < lines.len() {
                i += 1; // include closing fence
            }
            let text = lines[start..i].join("\n");
            blocks.push(Block { kind: BlockKind::CodeFence, text });
            continue;
        }
        // Gather a chunk: consecutive non-blank lines.
        let start = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            // A code fence inside a chunk starts a new block — stop here.
            if i > start && (lines[i].starts_with("```") || lines[i].starts_with("~~~")) {
                break;
            }
            i += 1;
        }
        let chunk = &lines[start..i];
        // If every line in the chunk is a list item, split one block per line.
        if chunk.iter().all(|l| is_list_item(l)) {
            for line in chunk {
                blocks.push(Block { kind: BlockKind::ListItem, text: (*line).to_string() });
            }
        } else {
            let text = chunk.join("\n");
            let kind = classify(&text);
            blocks.push(Block { kind, text });
        }
    }
    blocks
}
```

Delete the placeholder `kinds_and_text` helper / `let _` line from the test module now.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain block::`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/block.rs
git commit -m "feat(domain): parse markdown into blocks (blank-line split, atomic fences, per-item lists)"
```

---

## Task 3: Join blocks back to markdown + round-trip

**Files:**
- Modify: `crates/cairn-domain/src/block.rs`
- Test: inline `#[cfg(test)]` in `block.rs`

- [ ] **Step 1: Write failing tests**

Add inside the `mod tests` block in `block.rs`:

```rust
    #[test]
    fn join_separates_with_one_blank_line_and_trailing_newline() {
        let joined = join_blocks(&["# Title".into(), "Body para.".into()]);
        assert_eq!(joined, "# Title\n\nBody para.\n");
    }

    #[test]
    fn round_trip_normalized_markdown_is_identity() {
        let src = "# Title\n\nFirst para.\n\n- a\n- b\n";
        let blocks = parse_blocks(src);
        let texts: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
        assert_eq!(join_blocks(&texts), src);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-domain block::`
Expected: FAIL — `join_blocks` not defined.

- [ ] **Step 3: Implement `join_blocks`**

Add to `block.rs` (below `parse_blocks`):

```rust
/// Join block source texts into canonical markdown: one blank line between
/// blocks, a single trailing newline, no leading/trailing blank lines. This is
/// the normalization the round-trip property is defined against.
#[must_use]
pub fn join_blocks(texts: &[String]) -> String {
    if texts.is_empty() {
        return String::new();
    }
    let mut out = texts.join("\n\n");
    out.push('\n');
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain block::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/block.rs
git commit -m "feat(domain): join blocks to canonical markdown (round-trip normalization)"
```

---

## Task 4: CRDT types

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs`

- [ ] **Step 1: Write a failing test**

Append to `crates/cairn-domain/src/crdt.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_id_is_ordered_and_unique() {
        let a = BlockId { replica: 1, counter: 0 };
        let b = BlockId { replica: 1, counter: 1 };
        assert!(a < b);
        assert_ne!(a, b);
    }

    #[test]
    fn author_human_outranks_agent() {
        assert!(author_rank(Author::Human) > author_rank(Author::Agent));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-domain crdt::`
Expected: FAIL — types not defined.

- [ ] **Step 3: Define the types**

Add to the top of `crdt.rs` (below the module doc):

```rust
use crate::block::BlockKind;
use std::collections::HashMap;

/// Lamport timestamp.
pub type Lamport = u64;

/// A globally-unique, live-only block identity. Stripped on materialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId {
    pub replica: u64,
    pub counter: u64,
}

/// Who authored an edit. Drives the same-block LWW policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    Human,
    Agent,
}

/// Priority for same-block content LWW: Human beats Agent.
fn author_rank(a: Author) -> u8 {
    match a {
        Author::Human => 1,
        Author::Agent => 0,
    }
}

/// A replicated operation on a note's block document. Commutative + idempotent
/// under `merge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockOp {
    Insert {
        id: BlockId,
        after: Option<BlockId>,
        lamport: Lamport,
        kind: BlockKind,
        text: String,
    },
    Delete {
        id: BlockId,
        lamport: Lamport,
    },
    SetContent {
        id: BlockId,
        text: String,
        lamport: Lamport,
        author: Author,
    },
}

/// A local edit intent. `apply_local` turns it into `BlockOp`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    InsertAfter {
        after: Option<BlockId>,
        kind: BlockKind,
        text: String,
        author: Author,
    },
    UpdateText {
        id: BlockId,
        text: String,
        author: Author,
    },
    Remove {
        id: BlockId,
    },
}

/// Internal per-block state.
#[derive(Debug, Clone)]
struct Entry {
    id: BlockId,
    after: Option<BlockId>,
    ins_lamport: Lamport,
    kind: BlockKind,
    text: String,
    content_lamport: Lamport,
    content_author: Author,
    content_replica: u64,
    tombstone: bool,
    /// Loser content versions retained on conflict (never silently dropped).
    stash: Vec<String>,
}

/// A live, mergeable representation of one note's blocks.
#[derive(Debug, Clone)]
pub struct BlockDoc {
    replica: u64,
    counter: u64,
    clock: Lamport,
    entries: HashMap<BlockId, Entry>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-domain crdt::`
Expected: PASS (2 tests). Warnings about unused fields are expected until Task 5–7.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(domain): CRDT types (BlockId, Author, BlockOp, Edit, BlockDoc)"
```

---

## Task 5: `from_markdown` + `materialize` (RGA linearization) + round-trip

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs`

- [ ] **Step 1: Write failing tests**

Add to `crdt::tests`:

```rust
    use crate::block::join_blocks;

    #[test]
    fn from_markdown_then_materialize_round_trips() {
        let src = "# Title\n\nFirst para.\n\n- a\n- b\n";
        let doc = BlockDoc::from_markdown(1, src);
        assert_eq!(doc.materialize(), src);
    }

    #[test]
    fn empty_markdown_materializes_empty() {
        let doc = BlockDoc::from_markdown(1, "");
        assert_eq!(doc.materialize(), "");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-domain crdt::`
Expected: FAIL — `from_markdown` / `materialize` not defined.

- [ ] **Step 3: Implement**

Add an `impl BlockDoc` block in `crdt.rs`:

```rust
impl BlockDoc {
    /// Seed a fresh document from a note's markdown. Assigns fresh, live-only
    /// block IDs in document order (a simple RGA chain).
    #[must_use]
    pub fn from_markdown(replica: u64, src: &str) -> Self {
        let mut doc = Self {
            replica,
            counter: 0,
            clock: 0,
            entries: HashMap::new(),
        };
        let mut prev: Option<BlockId> = None;
        for block in crate::block::parse_blocks(src) {
            doc.clock += 1;
            let id = BlockId { replica, counter: doc.counter };
            doc.counter += 1;
            doc.entries.insert(
                id,
                Entry {
                    id,
                    after: prev,
                    ins_lamport: doc.clock,
                    kind: block.kind,
                    text: block.text,
                    content_lamport: doc.clock,
                    content_author: Author::Human,
                    content_replica: replica,
                    tombstone: false,
                    stash: Vec::new(),
                },
            );
            prev = Some(id);
        }
        doc
    }

    /// Project current state to canonical markdown. Block IDs are stripped;
    /// the output is pure plain markdown.
    #[must_use]
    pub fn materialize(&self) -> String {
        // children[anchor] = entries inserted directly after `anchor`.
        let mut children: HashMap<Option<BlockId>, Vec<&Entry>> = HashMap::new();
        for e in self.entries.values() {
            children.entry(e.after).or_default().push(e);
        }
        // Deterministic sibling order: higher insertion lamport first, id as
        // tiebreak. Total + independent of merge order ⇒ convergent.
        for v in children.values_mut() {
            v.sort_by(|a, b| {
                b.ins_lamport
                    .cmp(&a.ins_lamport)
                    .then_with(|| b.id.cmp(&a.id))
            });
        }
        let mut texts: Vec<String> = Vec::new();
        let mut stack: Vec<Option<BlockId>> = vec![None];
        let mut emitted: Vec<Option<BlockId>> = Vec::new();
        // Iterative pre-order DFS over the RGA tree.
        while let Some(anchor) = stack.pop() {
            if let Some(kids) = children.get(&anchor) {
                // Push in reverse so the first child is processed first.
                for e in kids.iter().rev() {
                    stack.push(Some(e.id));
                }
                for e in kids {
                    emitted.push(Some(e.id));
                    if !e.tombstone {
                        texts.push(e.text.clone());
                    }
                }
            }
        }
        let _ = emitted;
        join_blocks(&texts)
    }
}
```

> Note: the iterative DFS above must emit in true pre-order. Replace the body with the simpler recursive helper if clearer:
>
> ```rust
> fn walk(anchor: Option<BlockId>, children: &HashMap<Option<BlockId>, Vec<&Entry>>, out: &mut Vec<String>) {
>     if let Some(kids) = children.get(&anchor) {
>         for e in kids {
>             if !e.tombstone { out.push(e.text.clone()); }
>             walk(Some(e.id), children, out);
>         }
>     }
> }
> ```
>
> and call `walk(None, &children, &mut texts);` then `join_blocks(&texts)`. Use the recursive form — it is the canonical pre-order and avoids the stack/emit bookkeeping. Delete the iterative version and the `stack`/`emitted` locals.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain crdt::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(domain): BlockDoc from_markdown + materialize via RGA linearization"
```

---

## Task 6: `merge` — Insert / Delete (sequence convergence)

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs`

- [ ] **Step 1: Write failing tests**

Add to `crdt::tests`:

```rust
    #[test]
    fn merge_insert_is_idempotent() {
        let mut doc = BlockDoc::from_markdown(1, "a\n");
        let op = BlockOp::Insert {
            id: BlockId { replica: 2, counter: 0 },
            after: None,
            lamport: 5,
            kind: BlockKind::Paragraph,
            text: "z".into(),
        };
        doc.merge(op.clone());
        let once = doc.materialize();
        doc.merge(op); // applying twice changes nothing
        assert_eq!(doc.materialize(), once);
    }

    #[test]
    fn merge_delete_tombstones_block() {
        let mut doc = BlockDoc::from_markdown(1, "keep\n\ndrop\n");
        // Find the id of the second block ("drop").
        let drop_id = doc.block_ids_in_order()[1];
        doc.merge(BlockOp::Delete { id: drop_id, lamport: 9 });
        assert_eq!(doc.materialize(), "keep\n");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-domain crdt::`
Expected: FAIL — `merge` / `block_ids_in_order` not defined.

- [ ] **Step 3: Implement `merge` (Insert + Delete arms) and the test helper**

Add to `impl BlockDoc`:

```rust
    /// Merge a replicated op. Commutative and idempotent.
    pub fn merge(&mut self, op: BlockOp) {
        match op {
            BlockOp::Insert { id, after, lamport, kind, text } => {
                self.clock = self.clock.max(lamport);
                self.entries.entry(id).or_insert(Entry {
                    id,
                    after,
                    ins_lamport: lamport,
                    kind,
                    text,
                    content_lamport: lamport,
                    content_author: Author::Human,
                    content_replica: id.replica,
                    tombstone: false,
                    stash: Vec::new(),
                });
            }
            BlockOp::Delete { id, lamport } => {
                self.clock = self.clock.max(lamport);
                if let Some(e) = self.entries.get_mut(&id) {
                    e.tombstone = true;
                }
            }
            BlockOp::SetContent { .. } => {
                // Implemented in Task 7.
                self.merge_set_content(op);
            }
        }
    }

    /// Live (non-tombstoned) block IDs in materialized order. Test/lookup aid.
    #[must_use]
    pub fn block_ids_in_order(&self) -> Vec<BlockId> {
        let mut children: HashMap<Option<BlockId>, Vec<&Entry>> = HashMap::new();
        for e in self.entries.values() {
            children.entry(e.after).or_default().push(e);
        }
        for v in children.values_mut() {
            v.sort_by(|a, b| b.ins_lamport.cmp(&a.ins_lamport).then_with(|| b.id.cmp(&a.id)));
        }
        let mut out = Vec::new();
        fn walk(anchor: Option<BlockId>, children: &HashMap<Option<BlockId>, Vec<&Entry>>, out: &mut Vec<BlockId>) {
            if let Some(kids) = children.get(&anchor) {
                for e in kids {
                    if !e.tombstone {
                        out.push(e.id);
                    }
                    walk(Some(e.id), children, out);
                }
            }
        }
        walk(None, &children, &mut out);
        out
    }
```

Add a stub `merge_set_content` so the crate compiles before Task 7:

```rust
    fn merge_set_content(&mut self, _op: BlockOp) {
        // Filled in Task 7.
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain crdt::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(domain): merge Insert/Delete (idempotent sequence convergence)"
```

---

## Task 7: `SetContent` LWW + human-wins + stash, and `apply_local`

**Files:**
- Modify: `crates/cairn-domain/src/crdt.rs`

- [ ] **Step 1: Write failing tests**

Add to `crdt::tests`:

```rust
    #[test]
    fn human_edit_beats_agent_edit_same_block_and_stashes_loser() {
        let mut doc = BlockDoc::from_markdown(1, "original\n");
        let id = doc.block_ids_in_order()[0];
        // Agent writes with a HIGHER lamport, human with a lower one.
        doc.merge(BlockOp::SetContent { id, text: "agent version".into(), lamport: 10, author: Author::Agent });
        doc.merge(BlockOp::SetContent { id, text: "human version".into(), lamport: 3, author: Author::Human });
        assert_eq!(doc.materialize(), "human version\n");
        // Agent's losing text is stashed, not lost.
        assert!(doc.stashed(id).contains(&"agent version".to_string()));
    }

    #[test]
    fn set_content_is_order_independent() {
        let ops = |seed: bool| {
            let mut d = BlockDoc::from_markdown(1, "x\n");
            let id = d.block_ids_in_order()[0];
            let a = BlockOp::SetContent { id, text: "A".into(), lamport: 4, author: Author::Human };
            let b = BlockOp::SetContent { id, text: "B".into(), lamport: 7, author: Author::Human };
            if seed { d.merge(a); d.merge(b); } else { d.merge(b); d.merge(a); }
            d.materialize()
        };
        assert_eq!(ops(true), ops(false));
    }

    #[test]
    fn apply_local_returns_op_and_applies_it() {
        let mut doc = BlockDoc::from_markdown(1, "hello\n");
        let id = doc.block_ids_in_order()[0];
        let ops = doc.apply_local(Edit::UpdateText { id, text: "hi".into(), author: Author::Human });
        assert_eq!(ops.len(), 1);
        assert_eq!(doc.materialize(), "hi\n");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-domain crdt::`
Expected: FAIL — `stashed` / `apply_local` not defined; `merge_set_content` is a no-op so content tests fail.

- [ ] **Step 3: Implement LWW + apply_local**

Replace the `merge_set_content` stub with:

```rust
    fn merge_set_content(&mut self, op: BlockOp) {
        let BlockOp::SetContent { id, text, lamport, author } = op else {
            return;
        };
        self.clock = self.clock.max(lamport);
        let Some(e) = self.entries.get_mut(&id) else {
            return;
        };
        // Deterministic total order: (author_rank, lamport, replica). Higher
        // wins. Human always beats Agent; the loser's text is stashed.
        let incoming = (author_rank(author), lamport, id.replica);
        let current = (author_rank(e.content_author), e.content_lamport, e.content_replica);
        if incoming > current {
            e.stash.push(std::mem::replace(&mut e.text, text));
            e.content_author = author;
            e.content_lamport = lamport;
            e.content_replica = id.replica;
        } else if incoming < current {
            e.stash.push(text);
        }
        // incoming == current ⇒ identical winner key: ignore (idempotent).
    }
```

Add `apply_local` and `stashed` to `impl BlockDoc`:

```rust
    /// Apply a local edit, mutating this doc and returning the op(s) to share.
    pub fn apply_local(&mut self, edit: Edit) -> Vec<BlockOp> {
        self.clock += 1;
        let lamport = self.clock;
        let op = match edit {
            Edit::InsertAfter { after, kind, text, author } => {
                let id = BlockId { replica: self.replica, counter: self.counter };
                self.counter += 1;
                let _ = author; // insert content author defaults to Human seed; refined later
                BlockOp::Insert { id, after, lamport, kind, text }
            }
            Edit::UpdateText { id, text, author } => {
                BlockOp::SetContent { id, text, lamport, author }
            }
            Edit::Remove { id } => BlockOp::Delete { id, lamport },
        };
        self.merge(op.clone());
        vec![op]
    }

    /// Stashed loser content versions for a block (recoverable). Test/inspect aid.
    #[must_use]
    pub fn stashed(&self, id: BlockId) -> Vec<String> {
        self.entries.get(&id).map(|e| e.stash.clone()).unwrap_or_default()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-domain crdt::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/crdt.rs
git commit -m "feat(domain): SetContent LWW (human-wins + stash) and apply_local"
```

---

## Task 8: Convergence property tests (the headline proof)

**Files:**
- Create: `crates/cairn-domain/tests/convergence.rs`

The CRDT law: for any set of ops applied to any number of replicas in any order, with arbitrary duplication, all replicas `materialize()` identically.

- [ ] **Step 1: Write the property tests**

Create `crates/cairn-domain/tests/convergence.rs`:

```rust
//! Property tests for BlockDoc convergence (commutativity, associativity,
//! idempotence) and markdown round-trip. See ADR-0011.

use cairn_domain::block::{join_blocks, parse_blocks};
use cairn_domain::crdt::{Author, BlockDoc, BlockOp};
use cairn_domain::{BlockId, Edit};
use proptest::prelude::*;

/// A small, normalized-markdown generator: 1–6 blocks, each a heading,
/// paragraph, or list item, joined by single blank lines + trailing newline.
fn normalized_markdown() -> impl Strategy<Value = String> {
    let block = prop_oneof![
        "# [A-Za-z ]{1,12}".prop_map(|s| s.trim_end().to_string()),
        "[A-Za-z][A-Za-z ]{0,20}".prop_map(|s| s.trim_end().to_string()),
        "- [A-Za-z][A-Za-z ]{0,12}".prop_map(|s| s.trim_end().to_string()),
    ]
    .prop_filter("non-empty", |s| !s.trim().is_empty());
    prop::collection::vec(block, 1..6).prop_map(|texts| join_blocks(&texts))
}

proptest! {
    /// Round-trip: parse then join is the identity on normalized markdown.
    #[test]
    fn round_trip_is_identity(src in normalized_markdown()) {
        let texts: Vec<String> = parse_blocks(&src).iter().map(|b| b.text.clone()).collect();
        prop_assert_eq!(join_blocks(&texts), src);
    }

    /// from_markdown -> materialize is the identity on normalized markdown.
    #[test]
    fn doc_round_trip_is_identity(src in normalized_markdown()) {
        let doc = BlockDoc::from_markdown(1, &src);
        prop_assert_eq!(doc.materialize(), src);
    }
}

/// Generate a pool of ops by having two replicas each make a few local edits
/// against the same seed, collecting the emitted ops.
fn op_pool(seed: &str) -> Vec<BlockOp> {
    let mut ops = Vec::new();
    let mut a = BlockDoc::from_markdown(1, seed);
    let mut b = BlockDoc::from_markdown(2, seed);
    // NOTE: replicas 1 and 2 seed identical structure but with different
    // replica ids on their block ids, so cross-merge exercises real concurrency.
    let a_ids: Vec<BlockId> = a.block_ids_in_order();
    let b_ids: Vec<BlockId> = b.block_ids_in_order();
    if let Some(&id) = a_ids.first() {
        ops.extend(a.apply_local(Edit::UpdateText { id, text: "A-edit".into(), author: Author::Human }));
        ops.extend(a.apply_local(Edit::InsertAfter { after: Some(id), kind: cairn_domain::BlockKind::Paragraph, text: "A-new".into(), author: Author::Human }));
    }
    if let Some(&id) = b_ids.first() {
        ops.extend(b.apply_local(Edit::UpdateText { id, text: "B-edit".into(), author: Author::Agent }));
        ops.extend(b.apply_local(Edit::Remove { id }));
    }
    ops
}

proptest! {
    /// Convergence: applying the same op pool in any permutation, with one op
    /// duplicated, yields identical materialized markdown on every replica.
    #[test]
    fn replicas_converge_under_any_order(perm in any::<u64>()) {
        let seed = "seed one\n\nseed two\n";
        let mut pool = op_pool(seed);
        if let Some(first) = pool.first().cloned() {
            pool.push(first); // duplication ⇒ exercises idempotence
        }

        // Deterministic shuffle of `pool` driven by `perm` (no external rng).
        let mut order: Vec<usize> = (0..pool.len()).collect();
        let mut s = perm | 1;
        for i in (1..order.len()).rev() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (s >> 33) as usize % (i + 1);
            order.swap(i, j);
        }

        // Replica X: apply in shuffled order. Replica Y: apply in pool order.
        let mut x = BlockDoc::from_markdown(1, seed);
        let mut y = BlockDoc::from_markdown(1, seed);
        for &k in &order {
            x.merge(pool[k].clone());
        }
        for op in &pool {
            y.merge(op.clone());
        }
        prop_assert_eq!(x.materialize(), y.materialize());
    }
}
```

- [ ] **Step 2: Run the property tests**

Run: `cargo test -p cairn-domain --test convergence`
Expected: PASS (proptest runs 256 cases per property). If `replicas_converge_under_any_order` fails, the linearization or LWW total order is not deterministic — fix `materialize`/`merge_set_content`, do **not** weaken the assertion.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-domain/tests/convergence.rs
git commit -m "test(domain): property-test BlockDoc convergence + markdown round-trip"
```

---

## Task 9: Expand the `CollabSession` port

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (the `CollabSession` trait, ~lines 259-262)
- Modify: `crates/cairn-infra/src/seams.rs` (`NoCollab`)

- [ ] **Step 1: Write a failing test (NoCollab stays neutral under the new trait)**

In `crates/cairn-infra/src/seams.rs`, extend the existing `seams_have_expected_neutral_behavior` test (add after the `assert!(!NoCollab.is_active());` line):

```rust
        // Expanded CollabSession: NoCollab is inert — no docs, no ops.
        let mut nc = NoCollab;
        let path = cairn_domain::NotePath::new("a.md").unwrap();
        nc.open(&path, "hello\n");
        assert!(nc.materialize(&path).is_none());
        assert!(nc
            .edit(
                &path,
                cairn_domain::Edit::UpdateText {
                    id: cairn_domain::BlockId { replica: 0, counter: 0 },
                    text: "x".into(),
                    author: cairn_domain::Author::Human,
                },
            )
            .is_empty());
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra seams::`
Expected: FAIL — `open`/`edit`/`materialize` not on the trait.

- [ ] **Step 3: Expand the trait**

In `crates/cairn-ports/src/lib.rs`, replace the existing `CollabSession` trait:

```rust
pub trait CollabSession {
    fn is_active(&self) -> bool;
}
```

with (keep the doc comment style of neighboring traits):

```rust
/// A live, transport-blind collaboration seam over a note. Default adapter
/// `NoCollab` is inert; `LocalCrdt` wraps an in-memory block CRDT. Transport
/// (relay / daemon `/events`) is a later slice behind this same port.
pub trait CollabSession {
    /// Whether a live session is active.
    fn is_active(&self) -> bool;

    /// Open (or replace) the live document for `path`, seeded from `markdown`.
    fn open(&mut self, path: &NotePath, markdown: &str);

    /// Apply a local edit to the open document, returning ops to broadcast.
    fn edit(&mut self, path: &NotePath, edit: Edit) -> Vec<BlockOp>;

    /// Merge a remote op into the open document for `path`.
    fn merge_remote(&mut self, path: &NotePath, op: BlockOp);

    /// Materialize the open document for `path` to canonical markdown, or
    /// `None` if no document is open.
    fn materialize(&self, path: &NotePath) -> Option<String>;
}
```

Ensure the imports at the top of `cairn-ports/src/lib.rs` bring in the domain types (find the existing `use cairn_domain::...;` line and add `BlockOp, Edit`):

```rust
use cairn_domain::{BlockOp, Edit, NotePath};
```

(Adjust to merge with the existing import of `NotePath` rather than duplicating it.)

- [ ] **Step 4: Update `NoCollab` to the expanded trait**

In `crates/cairn-infra/src/seams.rs`, replace the `NoCollab` impl:

```rust
impl CollabSession for NoCollab {
    fn is_active(&self) -> bool {
        false
    }
    fn open(&mut self, _path: &cairn_domain::NotePath, _markdown: &str) {}
    fn edit(
        &mut self,
        _path: &cairn_domain::NotePath,
        _edit: cairn_domain::Edit,
    ) -> Vec<cairn_domain::BlockOp> {
        Vec::new()
    }
    fn merge_remote(&mut self, _path: &cairn_domain::NotePath, _op: cairn_domain::BlockOp) {}
    fn materialize(&self, _path: &cairn_domain::NotePath) -> Option<String> {
        None
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p cairn-ports -p cairn-infra`
Expected: PASS. Then `cargo build --workspace` to confirm no other `CollabSession` impls broke.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/seams.rs
git commit -m "feat(ports): expand CollabSession to transport-blind session seam"
```

---

## Task 10: `LocalCrdt` adapter

**Files:**
- Create: `crates/cairn-infra/src/collab.rs`
- Modify: `crates/cairn-infra/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/cairn-infra/src/collab.rs`:

```rust
//! `LocalCrdt`: an in-memory `CollabSession` adapter holding one `BlockDoc`
//! per open note. No transport — ops are returned to the caller. See ADR-0011.

use cairn_domain::{BlockDoc, BlockOp, Edit, NotePath};
use cairn_ports::CollabSession;
use std::collections::HashMap;

/// In-memory collaboration session: a `BlockDoc` per open note.
#[derive(Debug, Default)]
pub struct LocalCrdt {
    replica: u64,
    docs: HashMap<NotePath, BlockDoc>,
}

impl LocalCrdt {
    /// Create a session for a given replica id (unique per writer/surface).
    #[must_use]
    pub fn new(replica: u64) -> Self {
        Self { replica, docs: HashMap::new() }
    }
}

impl CollabSession for LocalCrdt {
    fn is_active(&self) -> bool {
        !self.docs.is_empty()
    }
    fn open(&mut self, path: &NotePath, markdown: &str) {
        self.docs.insert(path.clone(), BlockDoc::from_markdown(self.replica, markdown));
    }
    fn edit(&mut self, path: &NotePath, edit: Edit) -> Vec<BlockOp> {
        self.docs.get_mut(path).map(|d| d.apply_local(edit)).unwrap_or_default()
    }
    fn merge_remote(&mut self, path: &NotePath, op: BlockOp) {
        if let Some(d) = self.docs.get_mut(path) {
            d.merge(op);
        }
    }
    fn materialize(&self, path: &NotePath) -> Option<String> {
        self.docs.get(path).map(BlockDoc::materialize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::{Author, BlockKind};

    #[test]
    fn two_replicas_converge_through_the_port() {
        let path = NotePath::new("note.md").unwrap();
        let seed = "shared line\n";
        let mut a = LocalCrdt::new(1);
        let mut b = LocalCrdt::new(2);
        a.open(&path, seed);
        b.open(&path, seed);

        // A appends a block; B appends a different block. Exchange ops.
        let a_ops = a.edit(&path, Edit::InsertAfter {
            after: None, kind: BlockKind::Paragraph, text: "from A".into(), author: Author::Human,
        });
        let b_ops = b.edit(&path, Edit::InsertAfter {
            after: None, kind: BlockKind::Paragraph, text: "from B".into(), author: Author::Human,
        });
        for op in &b_ops { a.merge_remote(&path, op.clone()); }
        for op in &a_ops { b.merge_remote(&path, op.clone()); }

        assert_eq!(a.materialize(&path), b.materialize(&path));
    }

    #[test]
    fn is_active_reflects_open_docs() {
        let mut s = LocalCrdt::new(1);
        assert!(!s.is_active());
        s.open(&NotePath::new("a.md").unwrap(), "x\n");
        assert!(s.is_active());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra collab::`
Expected: FAIL — `collab` module not declared in `lib.rs`.

- [ ] **Step 3: Declare the module**

In `crates/cairn-infra/src/lib.rs`, add alongside the other `mod`/`pub use` lines (match the file's existing ordering):

```rust
mod collab;
pub use collab::LocalCrdt;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra collab::`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/collab.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): LocalCrdt — in-memory CollabSession adapter over BlockDoc"
```

---

## Task 11: Final verification

- [ ] **Step 1: Full workspace build + test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS, no warnings.

- [ ] **Step 2: Clippy + fmt (CI parity)**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS. Fix any clippy findings (e.g. `unwrap_or_default` preferences, needless clones) without changing behavior.

- [ ] **Step 3: Confirm no CRDT artifacts leak to disk**

Confirm `BlockDoc` is pure in-memory (no `std::fs`, no serialization to `.cairn/`): `rg "std::fs|serde" crates/cairn-domain/src/crdt.rs crates/cairn-domain/src/block.rs` returns nothing. Block IDs appear only in memory; `materialize` output contains no IDs.

- [ ] **Step 4: Final commit (if clippy/fmt changed anything)**

```bash
git add -A
git commit -m "chore: clippy + fmt for crdt slice 1"
```

---

## Self-review against the spec

- **Spec §2 (one core, pluggable transport):** `BlockDoc` is transport-free; ops are values the caller moves. Transport deferred (non-goal). ✓
- **Spec §3.1 (block taxonomy):** Task 2 — heading/paragraph/list-item/code-fence/blockquote/table/thematic-break/frontmatter; blank-line boundary; per-item lists; atomic fences. ✓
- **Spec §3.2 (sequence CRDT + opaque LWW register, no inner text CRDT):** Tasks 5–7. ✓
- **Spec §3.2 (agent⇄human → human-wins + stash):** Task 7. ✓
- **Spec §3.3 (live-only IDs, stripped on materialize):** Tasks 4–5; verified Task 11 step 3. ✓
- **Spec §3.4 (byte round-trip, slice not re-render):** Task 3 + Task 8 round-trip properties. ✓
- **Spec §5 (hexagonal: domain type, port, infra adapter):** Tasks 4–7 (domain), 9 (port), 10 (infra). ✓
- **Spec §6 / §7 (slice-1 deliverable + property tests):** Task 8 convergence property tests; Tasks 2,3,6,7 unit tests. ✓
- **Spec §8 non-goals:** no transport, UI, relay, persistence, inner text CRDT — none added. ✓
- **Deferred (spec §9):** native `Move` op (reorder = delete+insert), `.cairn/` sidecar, materialize cadence, automerge adapter — none in this slice, by design.
```
