# Temporal Knowledge Graph — design

- **Date:** 2026-06-29
- **Status:** Approved (brainstorm), pending implementation plan
- **Branch:** `cairn-temporal-graph-at` (worktree `surabaya`), based on `main`
- **Supersedes premise of:** the (non-existent) "graph contract seam" — see Context.

## Context

Cairn's differentiator is a **git-native temporal view of the knowledge graph**: the
link graph *as of any past commit*, and the *diff of knowledge* between two commits.
No Obsidian/Roam/Logseq competitor has real commit history to draw on.

The original task framed this as "implement the backend behind the already-frozen
`GraphAt` query." That premise does not match the repository: there is **no `GraphAt`
query, no `GraphScope`, no enriched `GraphNode`, no scope on `GetGraph`, and no seam
stub** anywhere on `main`, in any open PR, or in any sibling worktree. `GetGraph`
today returns `QueryResponse::Graph { nodes: Vec<String>, edges: Vec<GraphEdge> }`
— plain path strings, whole-graph-only.

Decision (with the user): **own the whole vertical in one spec** — freeze the contract
seam *and* implement the temporal backend behind it.

### What exists to build on

| Layer | Asset | Reuse |
|---|---|---|
| Domain | `Note::parse(path, raw)`, `display_title()`, `tags()` | parse past blobs identically to HEAD |
| Domain | `Graph::build(notes)` — resolves `[[wikilinks]]` by stem → `nodes()`/`edges()` | takes *any* note set; reuse verbatim for a past set |
| Domain | `NotePath::new` — rejects `.git`/`.cairn`/dotfiles/`..` | reuse to filter tree entries (security for free) |
| Port | `Vcs { commit_all, history, show, is_dirty }` | `show` is single-blob-at-rev; generalize to a tree-walk |
| Adapter | `GitVcs::show` = `revparse_single → peel_to_commit → tree → get_path → blob` | tree-walk is the same path, over the whole tree |
| App | `Engine::graph()` = `with_notes(\|m\| Graph::build(m.values()))`; `notes_cache: RefCell` | mirror with a historical path + its own cache |
| Port | `FileStamp { modified: SystemTime, len }` via `VaultStore::stamp` | HEAD mtime source |

## Goals

1. `GraphAt { revision, scope }` — the link graph as of a past git revision.
2. `GraphDiff { from, to, scope }` — added/removed nodes & edges between two revisions.
3. Enrich graph nodes (`Vec<String>` → `Vec<GraphNode>`) and add `scope` to `GetGraph`,
   so HEAD and historical graphs share one response shape.
4. Tests as part of done (domain unit, adapter integration over real temp repos, service dispatch).

## Non-goals (explicit scope discipline)

- **Scrubber UX / animation / timeline / small-multiples** — lives in the
  `knowledge-graph-viz-ui` branch. This spec defines the *query surface* that UX consumes.
- **Githru-style clustering / hairball-over-time mitigation** — viz-layer concern, future.
- **`GraphHistory` batch multi-revision query** — deferred (YAGNI). The scrubber preloads
  via repeated `GraphAt`; the oid-keyed cache (§ Performance) makes that cheap. We add a
  batch query only if measurement shows it necessary.
- **Changing `NoteAt`'s existing error mapping** — no drive-by edits.

## Design decisions (locked)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| ① | Query surface | `GraphAt` + `GraphDiff`; diff is a **pure domain function** | diff = set-math over two snapshots, no extra git; the marquee feature; unit-testable in isolation |
| ② | Historical `mtime_secs` | per-note **last-touch commit time ≤ revision** | only option where a recency/staleness encoding survives inside a historical snapshot; single backward revwalk, cacheable |
| ③ | Contract shape | **unified**: all three graph queries take `scope`, all return `Vec<GraphNode>`; `GetGraph { scope }` | `GetGraph` must change for enrichment anyway → pay the breaking cost once, uniform surface |
| ④ | HEAD `mtime_secs` source | **fs mtime** (`FileStamp.modified`), commit-time only historically | `GetGraph` is a hot UI path; keep it pure-fs, no revwalk. `mtime_secs` = "last modified" in both modes |
| ④ | Repeated tree-walks | **oid-keyed bounded LRU** in the Engine | scrubbing *is* the stated use case; git history is immutable → **zero invalidation logic** |

## Contract changes (`cairn-contract`)

```rust
/// A node in the link graph: a note plus light display metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphNode {
    /// Relative note path.
    pub path: String,
    /// Display title at this revision (frontmatter `title:` → first `# ` → stem).
    pub title: String,
    /// Last-modified, seconds since the Unix epoch. At HEAD: filesystem mtime.
    /// Historically: the newest commit ≤ `revision` that touched the note.
    pub mtime_secs: i64,
}

/// Which slice of the graph to return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphScope {
    /// The whole graph.
    Full,
    /// The undirected neighborhood of `path` out to `depth` hops (path = depth 0).
    Focused { path: String, depth: u32 },
}

// Query enum: GetGraph gains scope; two new variants.
pub enum Query {
    // ...
    GetGraph { scope: GraphScope },                                  // CHANGED (was unit)
    GraphAt   { revision: String, scope: GraphScope },               // NEW
    GraphDiff { from: String, to: String, scope: GraphScope },       // NEW
    // ...
}

// QueryResponse: Graph node type changes; one new variant.
pub enum QueryResponse {
    // ...
    Graph {
        nodes: Vec<GraphNode>,   // CHANGED (was Vec<String>)
        edges: Vec<GraphEdge>,
    },
    GraphDiff {                  // NEW — response to Query::GraphDiff
        nodes_added:   Vec<GraphNode>,
        nodes_removed: Vec<GraphNode>,
        edges_added:   Vec<GraphEdge>,
        edges_removed: Vec<GraphEdge>,
    },
    // ...
}
```

`GraphEdge { from: String, to: String }` is unchanged. Regenerate ts-rs bindings;
update the `codegen` test.

### Scope semantics

- `Full`: every node/edge in the built graph.
- `Focused { path, depth }`: BFS from `path` over the **undirected** union of forward
  links + backlinks; include nodes within `depth` hops (`path` itself = depth 0); include
  an edge iff **both** endpoints are in the kept set. If `path` is absent from the graph,
  return an empty graph (not an error) — a note may simply not exist at that revision.

## Domain changes (`cairn-domain`)

Pure, no I/O. Two additions to `graph.rs`:

```rust
impl Graph {
    /// Restrict to the undirected neighborhood of `path` within `depth` hops.
    /// Returns a new Graph; empty if `path` is absent.
    pub fn focused(&self, path: &NotePath, depth: u32) -> Graph;
}

/// A set-diff of two graphs by node path and (from,to) edge.
pub struct GraphDelta {
    pub nodes_added:   Vec<NotePath>,
    pub nodes_removed: Vec<NotePath>,
    pub edges_added:   Vec<(NotePath, NotePath)>,
    pub edges_removed: Vec<(NotePath, NotePath)>,
}

impl Graph {
    /// `self` = older (`from`), `other` = newer (`to`). Added = in `other` not `self`.
    pub fn diff(&self, other: &Graph) -> GraphDelta;
}
```

Diff is over **resolved** nodes/edges (post-build, post-scope) so it composes with
`Focused`. `GraphNode` enrichment (title, mtime) is applied in the app layer where the
`Note` set and stamps live — the domain stays path-only.

## Port changes (`cairn-ports`)

Two methods on `Vcs`, generalizing `show`:

```rust
pub trait Vcs {
    // ... existing ...

    /// Resolve a revspec to its full commit oid (40-hex). Cheap; for cache keying.
    /// `PortError::NotFound` if the revspec doesn't resolve.
    fn resolve(&self, revision: &str) -> Result<String, PortError>;

    /// Every `.md` blob in the tree at `revision`, each tagged with the newest
    /// commit time ≤ `revision` that touched it. `PortError::NotFound` if the
    /// revspec doesn't resolve.
    fn read_tree_at(&self, revision: &str) -> Result<Vec<HistoricalBlob>, PortError>;
}

/// A markdown note as of a revision: its tree path, raw content, last-touch time.
pub struct HistoricalBlob {
    pub path: String,
    pub content: String,
    pub mtime_secs: i64,
}
```

**Error choice:** an unresolvable revspec maps to `PortError::NotFound` →
`ContractError::NotFound { what }` (a bad client revspec is a missing thing, not an
internal fault). This deliberately differs from `NoteAt`/`show`, which currently maps a
bad revspec to `Adapter → Internal`; aligning `NoteAt` is out of scope here.

## Adapter changes (`cairn-infra/git.rs` — `GitVcs`)

```rust
fn resolve(&self, revision) -> revparse_single(revision)
    .map_err(|_| PortError::NotFound(format!("revision {revision}")))?
    .peel_to_commit()?.id().to_string()

fn read_tree_at(&self, revision):
    1. commit = revparse_single(revision).peel_to_commit()    // NotFound on failure
    2. tree.walk(PreOrder): collect (full_path, blob_oid) for entries ending in ".md"
       (skip submodules / non-blobs)
    3. read each blob → String::from_utf8_lossy (same as `show`)
    4. mtime pass — ONE backward revwalk from `commit` (TIME|TOPOLOGICAL):
         remaining = set of all collected paths
         for each commit c (newest→oldest):
           for path in remaining still touched by c (commit_touched_path, reuse helper):
             mtime[path] = c.time().seconds(); remove from remaining
           if remaining empty: break
       (every path is present in `tree`, so each resolves within ≤ its own history)
    5. zip into Vec<HistoricalBlob>
```

`commit_touched_path` already exists in `git.rs` and is reused. `.md` filtering +
`NotePath::new` validation happen in the app layer (engine), keeping path-security rules
in one place; the adapter only excludes non-blob/non-`.md` tree entries.

**Performance note:** the mtime revwalk is O(commits visited until all paths resolved).
For pathological histories this can walk far; the oid-keyed cache makes it a once-per-
revision cost. If profiling shows it dominates, a follow-up can bound the walk depth and
fall back to the snapshot commit time for unresolved paths (no contract change).

## App / Engine changes (`cairn-app`)

```rust
pub struct Engine {
    // ... existing: store, index, vcs, notes_cache: RefCell<Option<HashMap<..>>> ...
    /// Full (unscoped) historical graphs, keyed by resolved commit oid. History
    /// is immutable, so entries never invalidate. Bounded LRU.
    graph_at_cache: RefCell<LruCache<String, Arc<BuiltGraph>>>,
}

/// A built graph plus the per-note enrichment needed to render `GraphNode`s.
/// Cached whole (scope `Full`); `focused()` + flattening happen per call on read.
pub struct BuiltGraph {
    pub graph: Graph,                                   // domain adjacency (for focused/diff)
    pub meta: HashMap<NotePath, (String, i64)>,         // path -> (title, mtime_secs)
}

/// Flattened, scoped, enriched graph handed to the service layer.
pub struct GraphResult {
    pub nodes: Vec<(NotePath, String /*title*/, i64 /*mtime_secs*/)>,
    pub edges: Vec<(NotePath, NotePath)>,
}
```

New methods (mirroring `graph()`):

```rust
// HEAD, now scoped + enriched. Builds a BuiltGraph from notes_cache (NOT cached in
// graph_at_cache — HEAD is mutable); mtime from VaultStore::stamp (fs). focused → flatten.
pub fn graph(&self, scope: &GraphScope) -> Result<GraphResult, PortError>;

// As-of `revision`: resolve → graph_at_cache hit returns the Full BuiltGraph; miss builds
// it (read_tree_at → NotePath::new filter → Note::parse → Graph::build; meta = title from
// note, mtime from HistoricalBlob) and inserts. Then focused(scope) + flatten on the way out.
pub fn graph_at(&self, revision: &str, scope: &GraphScope) -> Result<GraphResult, PortError>;

// Diff: obtain the Full BuiltGraph for both sides (cache-shared with GraphAt), apply
// focused(scope) to each, Graph::diff, then enrich. Removed nodes' title/mtime come from
// `from`'s meta; added from `to`'s meta.
pub fn graph_diff(&self, from: &str, to: &str, scope: &GraphScope)
    -> Result<GraphDeltaResult, PortError>;
```

- `forbid(unsafe_code)` retained. `anyhow` internally where used; `PortError` at the
  boundary; the service maps to `ContractError`.
- **Cache the Full `BuiltGraph` by oid, apply `focused()` on read** — not keyed by
  `(oid, scope)`. `focused()` is a cheap in-memory BFS, so caching the unscoped graph
  maximizes hit rate across different focuses of the same revision (a scrubber that also
  re-focuses pays one walk per revision total).
- LRU: small fixed capacity (start 16). Prefer the `lru` crate if already vendored, else a
  tiny hand-rolled capacity map — decide in the plan.

## Service dispatch (`cairn-service`)

```rust
Query::GetGraph { scope }            => engine.graph(&scope)        -> Graph { nodes, edges }
Query::GraphAt { revision, scope }   => engine.graph_at(&revision, &scope) -> Graph { .. }
Query::GraphDiff { from, to, scope } => engine.graph_diff(&from, &to, &scope) -> GraphDiff { .. }
```

`GraphResult`/`GraphDeltaResult` map to `QueryResponse::Graph`/`GraphDiff` by stringifying
`NotePath`s and wrapping nodes in `GraphNode`. `PortError::NotFound` (bad revision) →
`ContractError::NotFound` via the existing `From` chain — no new error plumbing.

## Downstream updates (all in-repo)

- **`cairn-daemon`** — query-name map: add `graph_at`, `graph_diff`; `get_graph` already
  exists (now carries `scope`, serde-automatic).
- **`cairn-mcp`** — the `graph` tool maps to `GetGraph { scope: Full }`; optionally expose
  `graph_at`/`graph_diff` tools (decide in plan — minimum: keep `graph` working).
- **`cairn-cli`** — `graph` command passes `scope: Full`; render enriched nodes (title).
  Add `graph-at <revision>` / `graph-diff <from> <to>` subcommands (mirrors `note-at`).
- **`cairn-contract/tests/codegen.rs`** — assert new decls; regenerate `.ts` bindings.
- **`knowledge-graph-viz-ui` branch** (separate, unmerged) — will consume the new
  `GraphNode`/`GraphScope`/`GraphDiff` shapes. Not broken on `main`; flagged for that session.

## Error handling

- Boundaries: `thiserror` (`PortError`, `ContractError`) unchanged.
- Unresolvable revspec → `PortError::NotFound` → `ContractError::NotFound`.
- A blob that isn't valid UTF-8 → lossy decode (matches `show`).
- A tree path that fails `NotePath::new` (dotfiles, `..`) → silently skipped (not a note).
- A `Focused` path absent at that revision → empty graph, `Ok`.

## Performance

- HEAD `GetGraph`: unchanged hot path (fs reads via existing `notes_cache` + `stamp`).
- `GraphAt`: cold = one tree-walk + parse + one mtime revwalk; warm (same oid) = LRU hit,
  in-memory `focused()` only.
- `GraphDiff`: two `graph_at` builds (cache-shared with `GraphAt` calls) + O(n) set diff.
- Cache invalidation: none — past commits are immutable.

## Testing strategy

- **Domain unit** (`graph.rs`): `focused()` depth/undirected/edge-inclusion/missing-path;
  `diff()` added/removed nodes & edges, empty-vs-empty, identical graphs.
- **Adapter integration** (`git.rs`, real `tempfile` repos, the established pattern):
  `read_tree_at` returns all `.md` across nested dirs, excludes non-`.md` & `.cairn`;
  `mtime_secs` = newest touching commit ≤ rev (multi-commit fixture); `resolve` on
  hash/`HEAD~n`/bad revspec (→ NotFound).
- **App** (`cairn-app`): `graph_at` builds the historical graph; cache hit avoids a second
  walk (spy/Cell counter on a fake `Vcs`); `Focused` scoping; `graph_diff` correctness.
- **Service** (`cairn-service`): dispatch arms for `GraphAt`/`GraphDiff`/scoped `GetGraph`;
  bad revision → `ContractError::NotFound`.
- **MSRV** Rust 1.88; `forbid(unsafe_code)`.

## Implementation phasing (for the plan)

1. Contract: `GraphNode`, `GraphScope`, `Query`/`QueryResponse` changes + codegen.
2. Domain: `Graph::focused`, `Graph::diff` (+ unit tests).
3. Port + adapter: `resolve`, `read_tree_at`, `HistoricalBlob` (+ integration tests).
4. Engine: scoped/enriched `graph`, `graph_at`, `graph_diff`, oid LRU (+ tests).
5. Service dispatch + the three downstream consumers (daemon/mcp/cli) + codegen test.

## Risks / open items

- **mtime revwalk cost** on deep histories — mitigated by cache; bounded-walk fallback is a
  no-contract-change follow-up.
- **Contract is authored here, not consumed** — if a parallel effort later freezes these
  types differently, reconcile. Kept faithful to the task's stated vocabulary to minimize.
- **`lru` dependency** — confirm it's acceptable / already vendored; else hand-roll.
