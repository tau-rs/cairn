# StructuralRevisions engine query — design

**Date:** 2026-08-09
**Repo:** tau-rs/cairn (engine)
**Status:** approved, pre-implementation
**Consumer:** cairn-web-ui Phase 2 of the vault-history timeline (PR #122)

## Problem

The cairn-web-ui temporal scrubber (PR #122) scrubs the whole vault's commit
history. It places a marker on every commit even though most commits only edit a
note's *text*. For a **graph** time-view only **structural** changes matter — a
note node or a link edge added or removed. Thinning the scrubber to structural
revisions makes it legible at scale.

This cannot be computed cheaply client-side. Verified against engine `origin/main`
(6217d07):

- `built_at(rev)` (`crates/cairn-app/src/lib.rs`) reads the **entire** vault tree
  at a commit (`Vcs::read_tree_at`) and `Note::parse`s every `.md` (LRU-16 cache
  keyed by commit oid). `graph_diff` builds **both** sides → two full tree parses
  per commit pair. Computing this for all commits is O(commits × vault size).
- `Query::VaultHistory` returns only `Revision { id, message, timestamp_secs,
  author }` — no structural signal. So the client has nothing to filter on and
  cannot derive one without the expensive per-commit graph rebuild above.

## Deliverable

A new read-only query that returns the subset of vault revisions that changed the
**link graph** (a note node or an edge added/removed), newest-first, capped at
`limit`.

### Contract (`crates/cairn-contract`)

New `Query` variant, **reusing** the existing `QueryResponse::History` response —
no new wire types:

```rust
/// Vault revisions that changed the link graph (a note node or a link edge
/// added/removed), newest first, capped at `limit`. Metadata-only edits
/// (title, tags, body text with no link change) are excluded. Response:
/// `QueryResponse::History`.
StructuralRevisions {
    /// Max structural revisions to return; `None` returns all.
    limit: Option<u32>,
},
```

Response: `QueryResponse::History { revisions: Vec<Revision> }` (already exists).

**Rationale (choice A — reuse `History`):** the scrubber only needs *which*
revisions are structural — a set of ids/timestamps to thin its marker set. That
is a filtered subset of `vault_history` with an identical shape. A richer variant
carrying a per-commit delta summary (`+nodes / −nodes / ±edges`) for tooltips was
considered and deferred (YAGNI); it is a clean additive follow-up if the tooltip
ever wants it, and does not block this work.

`Query` is `ts-rs`-generated and drift-checked. Adding the variant regenerates
`crates/cairn-contract/bindings` and must keep the contract lockstep test green.

## Algorithm

`cairn-app::structural_revisions(limit)`, walking history **newest → oldest**:

1. **Cheap skip (git tree-diff).** If a commit's tree differs from its first
   parent in **no `.md` path**, it cannot change the graph — the graph derives
   solely from `.md` note existence and `.md` body wikilinks (`Graph::build`,
   `read_tree_at` both filter to `.md`). Skip without parsing anything.
2. **Precise confirm (graph equality).** For a commit that *did* touch `.md`,
   compare `built_at(child).graph != built_at(parent).graph`. Domain `Graph` is
   `Eq`, and equality is exactly the node set plus the edge set (`forward` +
   `backward` maps). This **excludes** `BuiltGraph.meta` (title, tags, mtime), so
   a pure metadata or body-text edit that adds/removes no link is correctly *not*
   structural. Node add/remove and edge add/remove are all covered.
3. **Collect the `limit` most-recent structural revisions**, breaking early once
   `limit` are found.

**Reuse (the "incremental" win):** walking a mostly-linear history, `parent(Cᵢ)`
is `Cᵢ₊₁`, so the graph built as the *child* side of one step is the *parent*
side of the next. `built_at` already caches by commit oid (LRU-16), giving this
reuse for free; the walk holds the last-built graph so each commit's tree is
parsed **once**, not twice as `graph_diff` does. True note-level incremental
rebuild (reparse only the changed `.md` from the tree-diff) is a deferred
optimization — stem-based edge resolution makes it fiddly and it is not needed to
unblock the scrubber.

**`limit` semantics (choice B):** the **N most-recent structural revisions**
(walk + break early), matching `vault_history`'s "give me N" contract and
yielding a predictable marker count. `None` returns all.

**Edge cases:**
- **Root commit** (no parent): compare its graph against the empty graph — a root
  that introduces any note/link is structural.
- **Merge commits:** compare against the **first parent** (the pragmatic "what did
  this commit change on the mainline" and what a linear scrubber wants).
- **Empty repo / no HEAD:** `Ok(vec![])`, mirroring `vault_history`.

## Hexagonal split

git2 must stay in `cairn-infra`; the domain `Graph` (link extraction, stem
resolution) must stay out of infra. Therefore:

- **`cairn-ports` (`Vcs` trait):** new method returning the **`.md`-change log** —
  git-only, newest-first, each entry the commit's `Revision` plus its parent oid
  and the `.md` paths it changed vs its first parent (or a "touched `.md`?" flag).
  No note parsing, no graph.
- **`cairn-infra/git.rs`:** implement it with git2 (`revwalk` newest-first;
  per-commit first-parent tree-diff filtered to `.md`; build on the existing
  `commit_touched_path` first-parent helper).
- **`cairn-app`:** `structural_revisions` orchestrates — consumes the `.md`-change
  log, confirms real graph change via `built_at(...).graph` equality, applies
  `limit`, returns `Vec<Revision>`.

## Change surface

| Crate | Change |
|-------|--------|
| `cairn-contract` | `Query::StructuralRevisions { limit }` variant; regenerate ts-rs bindings; keep lockstep drift test green |
| `cairn-ports` | new `Vcs` trait method (`.md`-change log) |
| `cairn-infra` | git2 implementation in `git.rs` |
| `cairn-app` | `structural_revisions(limit)` method |
| `cairn-service` | one arm in `dispatch_query`: `StructuralRevisions { limit } => History { revisions }` |
| `cairn-daemon` | one arm in the `query_name` telemetry match: `=> "structural_revisions"` |

CLI/MCP surfaces map tool names → `Query` by string (not an exhaustive `match`),
so no forced change there; exposing a CLI/MCP entry point is out of scope (YAGNI).

## Tests (part of done)

- **infra (`git.rs`):** `.md`-only-untouched commit is skipped; an `.md`
  add/edit/remove is included with correct changed paths; root commit; newest-first
  ordering; empty repo → `[]`.
- **app (`lib.rs`):** text-only edit to a note (no link change) is **not** returned;
  adding a `[[link]]` **is**; adding a new note **is**; removing a note **is**; a
  commit touching only a non-`.md` file is **not**; `limit` caps the count; empty
  repo → `[]`.
- **service:** dispatch `Query::StructuralRevisions` → `QueryResponse::History`.

## Downstream (separate PRs, not this one)

1. **This engine PR** → merge → rev-bump.
2. **cairn-web-ui contract sync PR:** bump the six `cairn-*` git revs in
   `src-tauri/Cargo.toml`, regenerate/vendor the contract (`Query.ts`).
3. **cairn-web-ui UI PR:** add a structural mode to `loadVaultTimeline`
   (`web/src/store/store.ts`) and thin the markers in
   `web/src/components/graph/timelineDensity.ts` and `TemporalScrubber.tsx`.
   **Also blocked on PR #122 merging** (still open as of 2026-08-09).
