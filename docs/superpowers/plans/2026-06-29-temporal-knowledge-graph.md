# Temporal Knowledge Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Cairn's git-native temporal knowledge graph — the link graph as of any past revision (`GraphAt`) and the diff of knowledge between two revisions (`GraphDiff`) — and freeze the shared graph contract (scoped, enriched nodes) it rides on.

**Architecture:** Bottom-up so every commit keeps the workspace green. Pure domain primitives first (`focused`, `diff`, `scoped`), then a VCS tree-walk port + `GitVcs` adapter, then engine methods with an oid-keyed cache, then the breaking contract flip rewired across all consumers atomically, then CLI surface. Hexagonal: `cairn-app` never imports `cairn-contract`; scope is a domain type and the service maps the wire type to it.

**Tech Stack:** Rust (workspace), `git2` 0.21, `ts-rs` (TypeScript bindings), `serde`, `thiserror`/`anyhow`, `tempfile` for tests.

## Global Constraints

- MSRV Rust **1.88**.
- `#![forbid(unsafe_code)]` in every crate — retained; add no `unsafe`.
- `thiserror` at boundaries (`PortError`, `ContractError`), `anyhow` internally.
- `cairn-app` depends ONLY on `cairn-domain` + `cairn-ports` (+ `serde`, `serde_json`, `tracing`). It must NOT import `cairn-contract`. Scope is a **domain** type; the **service** maps `cairn_contract::GraphScope` → `cairn_domain::GraphScope`.
- Conventional commits, imperative, scoped. Commit message trailer is NOT required by this repo's hooks; keep messages plain.
- Every task ends green: `cargo test` for the touched crate passes; `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean.
- Lefthook runs fmt/clippy/test on staged Rust files at commit; expect it to fire.
- Wikilink resolution is by **stem**, case-sensitive (existing `Graph::build` rule) — do not change it.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/cairn-domain/src/graph.rs` | + `GraphScope`, `GraphDelta`, `Graph::focused/diff/scoped`, `#[derive(Clone)]` on `Graph` | 1 |
| `crates/cairn-domain/src/lib.rs` | re-export `GraphScope`, `GraphDelta` | 1 |
| `crates/cairn-ports/src/lib.rs` | + `HistoricalBlob`, `Vcs::resolve`, `Vcs::read_tree_at` | 2 |
| `crates/cairn-infra/src/git.rs` | implement `resolve` + `read_tree_at` on `GitVcs` | 2 |
| `crates/cairn-app/src/lib.rs` | + `BuiltGraph`, `GraphResult`, `GraphDeltaResult`, `LruCache`, `graph_view`/`graph_at`/`graph_diff` | 3 |
| `crates/cairn-app/Cargo.toml` | (no change — `std` only) | 3 |
| `crates/cairn-contract/src/lib.rs` | + `GraphNode`, `GraphScope`; `GetGraph{scope}`; `GraphAt`/`GraphDiff`; `Graph` nodes → `Vec<GraphNode>`; `GraphDiff` response | 4 |
| `crates/cairn-contract/tests/codegen.rs` | assert new decls | 4 |
| `crates/cairn-contract/bindings/*.ts` | regenerated | 4 |
| `crates/cairn-service/src/lib.rs` | scope mapping + 3 dispatch arms | 4 |
| `crates/cairn-daemon/src/lib.rs` | `query_kind` arms | 4 |
| `crates/cairn-mcp/src/lib.rs` | `graph` tool → `GetGraph{Full}` + test | 4 |
| `crates/cairn-cli/src/main.rs` | `Graph` passes `Full`; render nodes; `graph-at`/`graph-diff` subcommands | 4, 5 |

**Out of scope (decided):** MCP `graph_at`/`graph_diff` tools (YAGNI — minimum is keeping `graph` working); `GraphHistory` batch query; any scrubber UI; changing `NoteAt`'s existing error mapping.

---

### Task 1: Domain primitives — scope, focus, diff

**Files:**
- Modify: `crates/cairn-domain/src/graph.rs`
- Modify: `crates/cairn-domain/src/lib.rs:11`
- Test: `crates/cairn-domain/src/graph.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `Graph: Clone` (added derive)
  - `enum GraphScope { Full, Focused { path: NotePath, depth: u32 } }`
  - `struct GraphDelta { nodes_added: Vec<NotePath>, nodes_removed: Vec<NotePath>, edges_added: Vec<(NotePath, NotePath)>, edges_removed: Vec<(NotePath, NotePath)> }`
  - `Graph::focused(&self, path: &NotePath, depth: u32) -> Graph`
  - `Graph::scoped(&self, scope: &GraphScope) -> Graph`
  - `Graph::diff(&self, other: &Graph) -> GraphDelta` (`self` = older/from, `other` = newer/to)

- [ ] **Step 1: Write the failing tests** — append to `mod tests` in `graph.rs`:

```rust
    #[test]
    fn focused_returns_undirected_neighborhood_within_depth() {
        // a -> b -> c -> d ; focus b depth 1 = {a,b,c} (undirected), edges among them.
        let notes = [
            note("a.md", "[[b]]"),
            note("b.md", "[[c]]"),
            note("c.md", "[[d]]"),
            note("d.md", "x"),
        ];
        let g = Graph::build(notes.iter());
        let b = NotePath::new("b.md").unwrap();
        let f = g.focused(&b, 1);
        let mut nodes: Vec<&str> = f.nodes().iter().map(|p| p.as_str()).collect();
        nodes.sort_unstable();
        assert_eq!(nodes, vec!["a.md", "b.md", "c.md"]);
        // Edge a->b and b->c are kept; c->d is dropped (d not in set).
        let edges: Vec<(String, String)> = f
            .edges()
            .iter()
            .map(|(x, y)| (x.as_str().to_string(), y.as_str().to_string()))
            .collect();
        assert!(edges.contains(&("a.md".into(), "b.md".into())));
        assert!(edges.contains(&("b.md".into(), "c.md".into())));
        assert!(!edges.iter().any(|(_, t)| t == "d.md"));
    }

    #[test]
    fn focused_on_absent_path_is_empty() {
        let g = Graph::build([note("a.md", "x")].iter());
        let missing = NotePath::new("zzz.md").unwrap();
        assert!(g.focused(&missing, 5).nodes().is_empty());
    }

    #[test]
    fn scoped_full_is_identity_focused_delegates() {
        let notes = [note("a.md", "[[b]]"), note("b.md", "x")];
        let g = Graph::build(notes.iter());
        assert_eq!(g.scoped(&GraphScope::Full), g);
        let b = NotePath::new("b.md").unwrap();
        assert_eq!(g.scoped(&GraphScope::Focused { path: b.clone(), depth: 0 }), g.focused(&b, 0));
    }

    #[test]
    fn diff_reports_added_and_removed_nodes_and_edges() {
        // from: a->b ; to: a (no link) + c->b
        let from = Graph::build([note("a.md", "[[b]]"), note("b.md", "x")].iter());
        let to = Graph::build(
            [note("a.md", "no link"), note("b.md", "x"), note("c.md", "[[b]]")].iter(),
        );
        let d = from.diff(&to);
        assert_eq!(d.nodes_added.iter().map(|p| p.as_str()).collect::<Vec<_>>(), vec!["c.md"]);
        assert!(d.nodes_removed.is_empty());
        assert_eq!(
            d.edges_added.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(),
            vec![("c.md", "b.md")]
        );
        assert_eq!(
            d.edges_removed.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(),
            vec![("a.md", "b.md")]
        );
    }
```

- [ ] **Step 2: Run tests, verify they fail to compile**

Run: `cargo test -p cairn-domain graph`
Expected: FAIL — `no method named focused`/`scoped`/`diff`, `GraphScope`/`GraphDelta` not found.

- [ ] **Step 3: Implement** — in `graph.rs`:

Change the `Graph` derive line to add `Clone`:
```rust
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Graph {
```

Add the imports at the top (next to the existing `use std::collections::BTreeMap;`):
```rust
use std::collections::{BTreeMap, BTreeSet};
```

Add after the `impl Graph { ... }` block:
```rust
/// Which slice of a graph to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphScope {
    /// The whole graph.
    Full,
    /// The undirected neighborhood of `path` out to `depth` hops (path = depth 0).
    Focused { path: NotePath, depth: u32 },
}

/// A set-diff between two graphs, by node path and `(from, to)` edge. All
/// vectors are sorted ascending.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphDelta {
    /// Nodes present in `other` but not `self`.
    pub nodes_added: Vec<NotePath>,
    /// Nodes present in `self` but not `other`.
    pub nodes_removed: Vec<NotePath>,
    /// Edges present in `other` but not `self`.
    pub edges_added: Vec<(NotePath, NotePath)>,
    /// Edges present in `self` but not `other`.
    pub edges_removed: Vec<(NotePath, NotePath)>,
}

impl Graph {
    /// Restrict to the undirected neighborhood of `path` within `depth` hops.
    /// `path` itself is depth 0. An edge is kept iff both endpoints are kept.
    /// Empty if `path` is absent from the graph.
    #[must_use]
    pub fn focused(&self, path: &NotePath, depth: u32) -> Graph {
        if !self.forward.contains_key(path) {
            return Graph::default();
        }
        let mut kept: BTreeSet<NotePath> = BTreeSet::new();
        kept.insert(path.clone());
        let mut frontier = vec![path.clone()];
        for _ in 0..depth {
            let mut next = Vec::new();
            for n in &frontier {
                for nb in self.forward_links(n).iter().chain(self.backlinks(n).iter()) {
                    if kept.insert(nb.clone()) {
                        next.push(nb.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        for n in &kept {
            forward.insert(
                n.clone(),
                self.forward_links(n).iter().filter(|t| kept.contains(*t)).cloned().collect(),
            );
            backward.insert(
                n.clone(),
                self.backlinks(n).iter().filter(|s| kept.contains(*s)).cloned().collect(),
            );
        }
        Graph { forward, backward }
    }

    /// Apply a [`GraphScope`]: `Full` clones, `Focused` delegates to [`Self::focused`].
    #[must_use]
    pub fn scoped(&self, scope: &GraphScope) -> Graph {
        match scope {
            GraphScope::Full => self.clone(),
            GraphScope::Focused { path, depth } => self.focused(path, *depth),
        }
    }

    /// Set-diff `self` (older) against `other` (newer).
    #[must_use]
    pub fn diff(&self, other: &Graph) -> GraphDelta {
        let a_nodes: BTreeSet<NotePath> = self.nodes().into_iter().cloned().collect();
        let b_nodes: BTreeSet<NotePath> = other.nodes().into_iter().cloned().collect();
        let a_edges: BTreeSet<(NotePath, NotePath)> =
            self.edges().into_iter().map(|(a, b)| (a.clone(), b.clone())).collect();
        let b_edges: BTreeSet<(NotePath, NotePath)> =
            other.edges().into_iter().map(|(a, b)| (a.clone(), b.clone())).collect();
        GraphDelta {
            nodes_added: b_nodes.difference(&a_nodes).cloned().collect(),
            nodes_removed: a_nodes.difference(&b_nodes).cloned().collect(),
            edges_added: b_edges.difference(&a_edges).cloned().collect(),
            edges_removed: a_edges.difference(&b_edges).cloned().collect(),
        }
    }
}
```

In `crates/cairn-domain/src/lib.rs`, change line 11:
```rust
pub use graph::{Graph, GraphDelta, GraphScope};
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test -p cairn-domain graph`
Expected: PASS (new + existing `graph` tests).

- [ ] **Step 5: Lint + commit**

```bash
cargo clippy -p cairn-domain --all-targets -- -D warnings && cargo fmt -p cairn-domain
git add crates/cairn-domain/src/graph.rs crates/cairn-domain/src/lib.rs
git commit -m "feat(domain): graph focus, scope, and diff primitives"
```

---

### Task 2: VCS port — resolve + tree-walk

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (`Vcs` trait + new `HistoricalBlob`)
- Modify: `crates/cairn-infra/src/git.rs` (impl on `GitVcs` + tests)
- Test: `crates/cairn-infra/src/git.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `commit_touched_path(&git2::Commit, &Path)` helper in `git.rs`; existing `adapt` error mapper.
- Produces:
  - `struct HistoricalBlob { pub path: String, pub content: String, pub mtime_secs: i64 }`
  - `Vcs::resolve(&self, revision: &str) -> Result<String, PortError>` — full 40-hex commit oid; `NotFound` if unresolvable.
  - `Vcs::read_tree_at(&self, revision: &str) -> Result<Vec<HistoricalBlob>, PortError>` — every `.md` blob in the tree at `revision`, each tagged with the newest commit time ≤ `revision` that touched it; `NotFound` if unresolvable.

- [ ] **Step 1: Write the failing adapter tests** — append to `mod tests` in `git.rs`:

```rust
    #[test]
    fn resolve_returns_full_oid_and_notfound_on_bad_revspec() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("v1").unwrap();
        let oid = vcs.resolve("HEAD").unwrap();
        assert_eq!(oid.len(), 40);
        assert!(matches!(vcs.resolve("no-such-rev"), Err(PortError::NotFound(_))));
    }

    #[test]
    fn read_tree_at_collects_md_blobs_excluding_non_md_and_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "[[b]]").unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/b.md"), "x").unwrap();
        fs::write(tmp.path().join("notes.txt"), "ignored").unwrap();
        vcs.commit_all("c1").unwrap();

        let mut blobs = vcs.read_tree_at("HEAD").unwrap();
        blobs.sort_by(|x, y| x.path.cmp(&y.path));
        let paths: Vec<&str> = blobs.iter().map(|b| b.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "sub/b.md"]); // .txt excluded, nested included
        assert_eq!(blobs[0].content, "[[b]]");
    }

    #[test]
    fn read_tree_at_mtime_is_newest_touching_commit_at_or_before_rev() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("c1 add a").unwrap();
        // sleep not needed: commit times are seconds; assert relative ordering by content.
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        fs::write(tmp.path().join("b.md"), "new").unwrap();
        let c2 = vcs.commit_all("c2 update a, add b").unwrap();

        let blobs = vcs.read_tree_at(&c2).unwrap();
        let a = blobs.iter().find(|b| b.path == "a.md").unwrap();
        let b = blobs.iter().find(|b| b.path == "b.md").unwrap();
        // Both last touched at c2; a's mtime must be >= its c1 time, and equal to b's (same commit).
        assert_eq!(a.mtime_secs, b.mtime_secs);
        assert!(a.mtime_secs > 0);
    }

    #[test]
    fn read_tree_at_notfound_on_bad_revspec() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(matches!(vcs.read_tree_at("nope"), Err(PortError::NotFound(_))));
    }
```

- [ ] **Step 2: Run, verify fail**

Run: `cargo test -p cairn-infra git`
Expected: FAIL — `no method named resolve`/`read_tree_at`.

- [ ] **Step 3: Add the port types** — in `crates/cairn-ports/src/lib.rs`, inside the `pub trait Vcs` block, after the `show` method:

```rust
    /// Resolve a revspec to its full commit oid (40-hex). Cheap; for cache keying.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revspec does not resolve to a commit.
    fn resolve(&self, revision: &str) -> Result<String, PortError>;

    /// Every `.md` blob in the tree at `revision`, each tagged with the newest
    /// commit time (Unix seconds) ≤ `revision` that touched it.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revspec does not resolve; [`PortError::Adapter`]
    /// on a git failure.
    fn read_tree_at(&self, revision: &str) -> Result<Vec<HistoricalBlob>, PortError>;
```

And add this struct near `Revision` (above the `Vcs` trait):
```rust
/// A markdown note as of a revision: tree path, raw content, and last-touch time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalBlob {
    /// Forward-slash relative path inside the tree.
    pub path: String,
    /// Raw file contents (lossy UTF-8, matching `show`).
    pub content: String,
    /// Newest commit time ≤ the revision that touched this note (Unix seconds).
    pub mtime_secs: i64,
}
```

- [ ] **Step 4: Implement on `GitVcs`** — in `git.rs`, add `use cairn_ports::HistoricalBlob;` to the existing `use cairn_ports::{...}` line, then add inside `impl Vcs for GitVcs` (after `show`):

```rust
    fn resolve(&self, revision: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let commit = repo
            .revparse_single(revision)
            .and_then(|o| o.peel_to_commit())
            .map_err(|_| PortError::NotFound(format!("revision {revision}")))?;
        Ok(commit.id().to_string())
    }

    fn read_tree_at(&self, revision: &str) -> Result<Vec<HistoricalBlob>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let commit = repo
            .revparse_single(revision)
            .and_then(|o| o.peel_to_commit())
            .map_err(|_| PortError::NotFound(format!("revision {revision}")))?;
        let tree = commit.tree().map_err(adapt)?;

        // 1. Collect every `.md` blob (path + content). `dir` ends in '/' or is "".
        let mut paths: Vec<String> = Vec::new();
        let mut contents: Vec<String> = Vec::new();
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                if let Some(name) = entry.name() {
                    if name.ends_with(".md") {
                        if let Ok(blob) =
                            entry.to_object(&repo).and_then(|o| o.peel_to_blob())
                        {
                            paths.push(format!("{dir}{name}"));
                            contents.push(
                                String::from_utf8_lossy(blob.content()).into_owned(),
                            );
                        }
                    }
                }
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(adapt)?;

        // 2. One backward revwalk: newest touching-commit time per path.
        let snapshot_secs = commit.time().seconds();
        let mut mtimes: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        if !paths.is_empty() {
            let mut remaining: std::collections::HashSet<String> =
                paths.iter().cloned().collect();
            let mut walk = repo.revwalk().map_err(adapt)?;
            walk.push(commit.id()).map_err(adapt)?;
            walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
                .map_err(adapt)?;
            for oid in walk {
                if remaining.is_empty() {
                    break;
                }
                let oid = oid.map_err(adapt)?;
                let c = repo.find_commit(oid).map_err(adapt)?;
                let secs = c.time().seconds();
                let mut done = Vec::new();
                for p in &remaining {
                    if commit_touched_path(&c, Path::new(p)).map_err(adapt)? {
                        mtimes.insert(p.clone(), secs);
                        done.push(p.clone());
                    }
                }
                for p in done {
                    remaining.remove(&p);
                }
            }
        }

        // 3. Zip. Any path unresolved by the walk falls back to the snapshot time.
        Ok(paths
            .into_iter()
            .zip(contents)
            .map(|(path, content)| {
                let mtime_secs = mtimes.get(&path).copied().unwrap_or(snapshot_secs);
                HistoricalBlob { path, content, mtime_secs }
            })
            .collect())
    }
```

- [ ] **Step 5: Run, verify pass**

Run: `cargo test -p cairn-infra git && cargo test -p cairn-ports`
Expected: PASS.

- [ ] **Step 6: Lint + commit**

```bash
cargo clippy -p cairn-infra -p cairn-ports --all-targets -- -D warnings && cargo fmt -p cairn-infra -p cairn-ports
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/git.rs
git commit -m "feat(vcs): resolve + read_tree_at tree-walk with last-touch mtime"
```

---

### Task 3: Engine — enriched, scoped, cached graphs

**Files:**
- Modify: `crates/cairn-app/src/lib.rs` (struct fields, `Engine::new`, new methods, test module)
- Test: `crates/cairn-app/src/lib.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `cairn_domain::{Graph, GraphScope, GraphDelta, Note, NotePath}`; `cairn_ports::{Vcs, HistoricalBlob, FileStamp}`; existing `with_notes`, `store`, `vcs`.
- Produces:
  - `struct BuiltGraph { graph: Graph, meta: HashMap<NotePath, (String, i64)> }` (private)
  - `pub struct GraphResult { pub nodes: Vec<(NotePath, String, i64)>, pub edges: Vec<(NotePath, NotePath)> }`
  - `pub struct GraphDeltaResult { pub nodes_added: Vec<(NotePath, String, i64)>, pub nodes_removed: Vec<(NotePath, String, i64)>, pub edges_added: Vec<(NotePath, NotePath)>, pub edges_removed: Vec<(NotePath, NotePath)> }`
  - `Engine::graph_view(&self, scope: &GraphScope) -> Result<GraphResult, PortError>` (HEAD; fs mtime)
  - `Engine::graph_at(&self, revision: &str, scope: &GraphScope) -> Result<GraphResult, PortError>`
  - `Engine::graph_diff(&self, from: &str, to: &str, scope: &GraphScope) -> Result<GraphDeltaResult, PortError>`
  - Existing `Engine::graph(&self) -> Result<Graph>` stays (still used by the service until Task 4).

- [ ] **Step 1: Write failing tests** — append to `mod tests` in `cairn-app/src/lib.rs`. Add a counting VCS decorator and the cases:

```rust
    use std::sync::atomic::{AtomicUsize as Au, Ordering as Ord2};

    /// A `Vcs` that counts `read_tree_at` calls, delegating to an inner `GitVcs`.
    struct CountingVcs {
        inner: GitVcs,
        tree_reads: Arc<Au>,
    }
    impl Vcs for CountingVcs {
        fn commit_all(&mut self, m: &str) -> Result<String, PortError> {
            self.inner.commit_all(m)
        }
        fn history(&self, p: &str) -> Result<Vec<cairn_ports::Revision>, PortError> {
            self.inner.history(p)
        }
        fn show(&self, p: &str, r: &str) -> Result<String, PortError> {
            self.inner.show(p, r)
        }
        fn is_dirty(&self) -> Result<bool, PortError> {
            self.inner.is_dirty()
        }
        fn resolve(&self, r: &str) -> Result<String, PortError> {
            self.inner.resolve(r)
        }
        fn read_tree_at(&self, r: &str) -> Result<Vec<cairn_ports::HistoricalBlob>, PortError> {
            self.tree_reads.fetch_add(1, Ord2::SeqCst);
            self.inner.read_tree_at(r)
        }
    }

    #[test]
    fn graph_at_builds_historical_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "[[b]]", &mut ev).unwrap();
        eng.write_note(&b, "x", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();
        eng.write_note(&a, "no link now", &mut ev).unwrap();
        eng.commit("c2", &mut ev).unwrap();

        // As of c1: a -> b present.
        let at = eng.graph_at(&c1, &GraphScope::Full).unwrap();
        assert!(at.edges.iter().any(|(x, y)| x.as_str() == "a.md" && y.as_str() == "b.md"));
        // At HEAD: the link is gone.
        let head = eng.graph_view(&GraphScope::Full).unwrap();
        assert!(!head.edges.iter().any(|(x, y)| x.as_str() == "a.md" && y.as_str() == "b.md"));
        // Nodes are enriched with a title (stem fallback here).
        assert!(at.nodes.iter().any(|(p, title, _)| p.as_str() == "a.md" && title == "a"));
    }

    #[test]
    fn graph_at_caches_by_oid() {
        let tmp = tempfile::tempdir().unwrap();
        let reads = Arc::new(Au::new(0));
        let vcs = CountingVcs {
            inner: GitVcs::open_or_init(tmp.path()).unwrap(),
            tree_reads: reads.clone(),
        };
        let mut eng = Engine::new(
            LocalFsStore::open(tmp.path()).unwrap(),
            InMemoryIndex::default(),
            vcs,
        );
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "[[b]]", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();

        let _ = eng.graph_at(&c1, &GraphScope::Full).unwrap();
        let _ = eng.graph_at(&c1, &GraphScope::Full).unwrap(); // same oid
        let _ = eng
            .graph_at(&c1, &GraphScope::Focused { path: NotePath::new("a.md").unwrap(), depth: 0 })
            .unwrap(); // different scope, same oid -> still cached
        assert_eq!(reads.load(Ord2::SeqCst), 1, "tree walked once per oid");
    }

    #[test]
    fn graph_diff_reports_added_link_between_revisions() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        let c = NotePath::new("c.md").unwrap();
        eng.write_note(&a, "[[b]]", &mut ev).unwrap();
        eng.write_note(&b, "x", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();
        eng.write_note(&c, "[[b]]", &mut ev).unwrap();
        let c2 = eng.commit("c2", &mut ev).unwrap();

        let d = eng.graph_diff(&c1, &c2, &GraphScope::Full).unwrap();
        assert!(d.nodes_added.iter().any(|(p, _, _)| p.as_str() == "c.md"));
        assert!(d.edges_added.iter().any(|(x, y)| x.as_str() == "c.md" && y.as_str() == "b.md"));
        assert!(d.nodes_removed.is_empty());
    }
```

- [ ] **Step 2: Run, verify fail**

Run: `cargo test -p cairn-app graph_at`
Expected: FAIL — `no method named graph_at`/`graph_view`/`graph_diff`; `GraphScope` unresolved.

- [ ] **Step 3: Implement** — in `cairn-app/src/lib.rs`:

Extend the domain import (find the existing `use cairn_domain::{...}`) to include `Graph, GraphScope`:
```rust
use cairn_domain::{Graph, GraphScope, Note, NotePath};
```
(Keep any other items already imported from `cairn_domain` on that line.)

Add `use std::sync::Arc;` and `use std::time::UNIX_EPOCH;` to the imports if not present.

Add a tiny LRU + the result types (place above `pub struct Engine`):
```rust
/// A capacity-bounded LRU: most-recently-used at the back, evict from the front.
struct LruCache<K: Eq, V> {
    cap: usize,
    items: Vec<(K, V)>,
}
impl<K: Eq, V> LruCache<K, V> {
    fn new(cap: usize) -> Self {
        Self { cap, items: Vec::new() }
    }
    fn get(&mut self, key: &K) -> Option<&V> {
        let i = self.items.iter().position(|(k, _)| k == key)?;
        let item = self.items.remove(i);
        self.items.push(item);
        self.items.last().map(|(_, v)| v)
    }
    fn put(&mut self, key: K, val: V) {
        if let Some(i) = self.items.iter().position(|(k, _)| *k == key) {
            self.items.remove(i);
        } else if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push((key, val));
    }
}

/// A built graph plus per-note enrichment, cached whole (scope `Full`).
struct BuiltGraph {
    graph: Graph,
    meta: HashMap<NotePath, (String, i64)>,
}

/// Flattened, scoped, enriched graph for the service layer.
pub struct GraphResult {
    /// `(path, title, mtime_secs)` per node.
    pub nodes: Vec<(NotePath, String, i64)>,
    /// `(from, to)` link edges.
    pub edges: Vec<(NotePath, NotePath)>,
}

/// The enriched diff of two graphs.
pub struct GraphDeltaResult {
    /// Nodes in `to` not in `from` (enriched from `to`).
    pub nodes_added: Vec<(NotePath, String, i64)>,
    /// Nodes in `from` not in `to` (enriched from `from`).
    pub nodes_removed: Vec<(NotePath, String, i64)>,
    /// Edges added.
    pub edges_added: Vec<(NotePath, NotePath)>,
    /// Edges removed.
    pub edges_removed: Vec<(NotePath, NotePath)>,
}

/// Flatten a built graph through a scope into wire-ready node/edge tuples.
fn scope_and_flatten(built: &BuiltGraph, scope: &GraphScope) -> GraphResult {
    let g = built.graph.scoped(scope);
    let nodes = g
        .nodes()
        .into_iter()
        .map(|p| {
            let (title, mtime) = built.meta.get(p).cloned().unwrap_or_default();
            (p.clone(), title, mtime)
        })
        .collect();
    let edges = g.edges().into_iter().map(|(a, b)| (a.clone(), b.clone())).collect();
    GraphResult { nodes, edges }
}

/// Look a node's enrichment up, defaulting to empty title / 0 time.
fn enrich(p: &NotePath, meta: &HashMap<NotePath, (String, i64)>) -> (NotePath, String, i64) {
    let (title, mtime) = meta.get(p).cloned().unwrap_or_default();
    (p.clone(), title, mtime)
}

/// Unix seconds from a filesystem `SystemTime` (0 if before the epoch).
fn system_secs(t: std::time::SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
```

Add the cache field to `pub struct Engine`:
```rust
    graph_at_cache: RefCell<LruCache<String, Arc<BuiltGraph>>>,
```
And initialize it in `Engine::new` (alongside `notes_cache: RefCell::new(None),`):
```rust
            graph_at_cache: RefCell::new(LruCache::new(16)),
```

Add the methods inside `impl Engine` (next to the existing `graph`):
```rust
    /// The HEAD link graph, scoped and enriched. `mtime_secs` is the filesystem
    /// mtime (`VaultStore::stamp`).
    ///
    /// # Errors
    /// [`PortError`] if a port fails.
    pub fn graph_view(&self, scope: &GraphScope) -> Result<GraphResult, PortError> {
        let (graph, titles) = self.with_notes(|m| {
            let graph = Graph::build(m.values());
            let titles: Vec<(NotePath, String)> =
                m.iter().map(|(p, n)| (p.clone(), n.display_title())).collect();
            (graph, titles)
        })?;
        let mut meta = HashMap::new();
        for (p, title) in titles {
            let mtime = self.store.stamp(&p).map(|s| system_secs(s.modified)).unwrap_or(0);
            meta.insert(p, (title, mtime));
        }
        Ok(scope_and_flatten(&BuiltGraph { graph, meta }, scope))
    }

    /// The link graph as of a past `revision`, scoped and enriched. Cached by
    /// resolved commit oid (immutable history → no invalidation).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revision does not resolve; [`PortError`] on a port failure.
    pub fn graph_at(&self, revision: &str, scope: &GraphScope) -> Result<GraphResult, PortError> {
        let built = self.built_at(revision)?;
        Ok(scope_and_flatten(&built, scope))
    }

    /// The diff of the link graph between `from` (older) and `to` (newer).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if either revision does not resolve.
    pub fn graph_diff(
        &self,
        from: &str,
        to: &str,
        scope: &GraphScope,
    ) -> Result<GraphDeltaResult, PortError> {
        let a = self.built_at(from)?;
        let b = self.built_at(to)?;
        let delta = a.graph.scoped(scope).diff(&b.graph.scoped(scope));
        Ok(GraphDeltaResult {
            nodes_added: delta.nodes_added.iter().map(|p| enrich(p, &b.meta)).collect(),
            nodes_removed: delta.nodes_removed.iter().map(|p| enrich(p, &a.meta)).collect(),
            edges_added: delta.edges_added,
            edges_removed: delta.edges_removed,
        })
    }

    /// Resolve → cache-or-build the Full enriched graph as of `revision`.
    fn built_at(&self, revision: &str) -> Result<Arc<BuiltGraph>, PortError> {
        let oid = self.vcs.resolve(revision)?;
        if let Some(hit) = self.graph_at_cache.borrow_mut().get(&oid).cloned() {
            return Ok(hit);
        }
        let blobs = self.vcs.read_tree_at(revision)?;
        let mut notes = Vec::with_capacity(blobs.len());
        let mut meta = HashMap::new();
        for b in &blobs {
            let Ok(path) = NotePath::new(&b.path) else {
                continue; // dotfiles / control paths are not notes
            };
            let note = Note::parse(path.clone(), &b.content);
            meta.insert(path.clone(), (note.display_title(), b.mtime_secs));
            notes.push(note);
        }
        let built = Arc::new(BuiltGraph { graph: Graph::build(notes.iter()), meta });
        self.graph_at_cache.borrow_mut().put(oid, built.clone());
        Ok(built)
    }
```

- [ ] **Step 4: Run, verify pass**

Run: `cargo test -p cairn-app`
Expected: PASS (new graph tests + all existing).

- [ ] **Step 5: Lint + commit**

```bash
cargo clippy -p cairn-app --all-targets -- -D warnings && cargo fmt -p cairn-app
git add crates/cairn-app/src/lib.rs
git commit -m "feat(engine): scoped/enriched graph_view, graph_at, graph_diff with oid LRU"
```

---

### Task 4: Contract flip + rewire all consumers (atomic)

This is the single breaking commit. It changes shared `cairn-contract` types and updates every dependent crate in the same commit so the workspace never goes red.

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- Modify: `crates/cairn-contract/tests/codegen.rs`
- Modify: `crates/cairn-service/src/lib.rs`
- Modify: `crates/cairn-daemon/src/lib.rs`
- Modify: `crates/cairn-mcp/src/lib.rs`
- Modify: `crates/cairn-cli/src/main.rs`
- Regenerate: `crates/cairn-contract/bindings/*.ts`

**Interfaces:**
- Consumes: `Engine::graph_view/graph_at/graph_diff`, `GraphResult`, `GraphDeltaResult` (Task 3); `cairn_domain::GraphScope`, `NotePath` (Task 1).
- Produces (contract):
  - `struct GraphNode { path: String, title: String, mtime_secs: i64 }`
  - `enum GraphScope { Full, Focused { path: String, depth: u32 } }`
  - `Query::GetGraph { scope: GraphScope }`, `Query::GraphAt { revision: String, scope: GraphScope }`, `Query::GraphDiff { from: String, to: String, scope: GraphScope }`
  - `QueryResponse::Graph { nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }`
  - `QueryResponse::GraphDiff { nodes_added, nodes_removed: Vec<GraphNode>, edges_added, edges_removed: Vec<GraphEdge> }`

- [ ] **Step 1: Write the failing service test** — append to `cairn-service` `mod tests` (it already has `fn engine(dir)`):

```rust
    #[test]
    fn graph_at_dispatch_returns_enriched_historical_graph() {
        use cairn_contract::{GraphScope, QueryResponse};
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "[[b]]", &mut ev).unwrap();
        eng.write_note(&NotePath::new("b.md").unwrap(), "x", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();

        let resp = dispatch_query(
            &eng,
            &Query::GraphAt { revision: c1, scope: GraphScope::Full },
        )
        .unwrap();
        match resp {
            QueryResponse::Graph { nodes, edges } => {
                assert!(nodes.iter().any(|n| n.path == "a.md" && n.title == "a"));
                assert!(edges.iter().any(|e| e.from == "a.md" && e.to == "b.md"));
            }
            _ => panic!("expected Graph"),
        }
    }

    #[test]
    fn graph_at_bad_revision_is_not_found() {
        use cairn_contract::GraphScope;
        let tmp = tempfile::tempdir().unwrap();
        let eng = engine(tmp.path());
        let err = dispatch_query(
            &eng,
            &Query::GraphAt { revision: "no-such-rev".into(), scope: GraphScope::Full },
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }
```

- [ ] **Step 2: Run, verify fail**

Run: `cargo test -p cairn-service graph_at`
Expected: FAIL to compile — `GraphScope`/`Query::GraphAt` not found.

- [ ] **Step 3: Edit the contract** — in `crates/cairn-contract/src/lib.rs`:

Add these types (place near `GraphEdge`, ~line 360):
```rust
/// A node in the link graph: a note plus light display metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphNode {
    /// Relative note path.
    pub path: String,
    /// Display title at this revision (frontmatter `title:` → first `# ` → stem).
    pub title: String,
    /// Last-modified, Unix seconds. HEAD: filesystem mtime. Historical: newest
    /// commit ≤ the revision that touched the note.
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
    Focused {
        /// Focus note path.
        path: String,
        /// Hop radius.
        depth: u32,
    },
}
```

Change the `GetGraph` variant (line 80) and add two variants in `enum Query`:
```rust
    /// Fetch the link graph (whole or focused).
    GetGraph {
        /// Which slice to return.
        scope: GraphScope,
    },
    /// The link graph as of a past revision.
    GraphAt {
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
        /// Which slice to return.
        scope: GraphScope,
    },
    /// The diff of the link graph between two revisions (`from` older, `to` newer).
    GraphDiff {
        /// Older revspec.
        from: String,
        /// Newer revspec.
        to: String,
        /// Which slice to diff.
        scope: GraphScope,
    },
```

Change the `Graph` response variant (line 426) and add `GraphDiff` in `enum QueryResponse`:
```rust
    /// The link graph (response to `GetGraph` / `GraphAt`).
    Graph {
        /// Enriched nodes.
        nodes: Vec<GraphNode>,
        /// Directed link edges.
        edges: Vec<GraphEdge>,
    },
    /// The diff of two graphs (response to `GraphDiff`).
    GraphDiff {
        /// Nodes present in `to` not `from`.
        nodes_added: Vec<GraphNode>,
        /// Nodes present in `from` not `to`.
        nodes_removed: Vec<GraphNode>,
        /// Edges present in `to` not `from`.
        edges_added: Vec<GraphEdge>,
        /// Edges present in `from` not `to`.
        edges_removed: Vec<GraphEdge>,
    },
```

- [ ] **Step 4: Update the codegen test** — in `crates/cairn-contract/tests/codegen.rs`, add `GraphNode, GraphScope` to the `use` list, and add inside the test:
```rust
    assert!(GraphNode::decl().contains("GraphNode"));
    assert!(GraphScope::decl().contains("GraphScope"));
    GraphNode::export_all().unwrap();
    GraphScope::export_all().unwrap();
```

- [ ] **Step 5: Rewire the service** — in `crates/cairn-service/src/lib.rs`:

Add `GraphNode` to the `cairn_contract::{...}` import. Add these helpers (above `dispatch_query`):
```rust
/// Map the wire scope to the domain scope, validating any focus path.
fn to_domain_scope(s: &cairn_contract::GraphScope) -> Result<cairn_domain::GraphScope, ServiceError> {
    Ok(match s {
        cairn_contract::GraphScope::Full => cairn_domain::GraphScope::Full,
        cairn_contract::GraphScope::Focused { path, depth } => {
            let p = NotePath::new(path)
                .map_err(|_| ServiceError::InvalidRequest(format!("invalid focus path: {path}")))?;
            cairn_domain::GraphScope::Focused { path: p, depth: *depth }
        }
    })
}

fn node_to_wire((p, title, mtime): (NotePath, String, i64)) -> GraphNode {
    GraphNode { path: p.as_str().to_string(), title, mtime_secs: mtime }
}
fn edge_to_wire((a, b): (NotePath, NotePath)) -> GraphEdge {
    GraphEdge { from: a.as_str().to_string(), to: b.as_str().to_string() }
}
fn graph_to_wire(g: cairn_app::GraphResult) -> QueryResponse {
    QueryResponse::Graph {
        nodes: g.nodes.into_iter().map(node_to_wire).collect(),
        edges: g.edges.into_iter().map(edge_to_wire).collect(),
    }
}
```

Replace the `Query::GetGraph => { ... }` arm and add the two new arms:
```rust
        Query::GetGraph { scope } => {
            Ok(graph_to_wire(engine.graph_view(&to_domain_scope(scope)?)?))
        }
        Query::GraphAt { revision, scope } => {
            Ok(graph_to_wire(engine.graph_at(revision, &to_domain_scope(scope)?)?))
        }
        Query::GraphDiff { from, to, scope } => {
            let d = engine.graph_diff(from, to, &to_domain_scope(scope)?)?;
            Ok(QueryResponse::GraphDiff {
                nodes_added: d.nodes_added.into_iter().map(node_to_wire).collect(),
                nodes_removed: d.nodes_removed.into_iter().map(node_to_wire).collect(),
                edges_added: d.edges_added.into_iter().map(edge_to_wire).collect(),
                edges_removed: d.edges_removed.into_iter().map(edge_to_wire).collect(),
            })
        }
```
(`cairn_app::GraphResult` may need adding to the `cairn_app::{...}` import.)

Update the existing GetGraph dispatch test (the one around `dispatch_query(&eng, &Query::GetGraph)`) to `Query::GetGraph { scope: cairn_contract::GraphScope::Full }`, and its assertion to read `nodes` as `GraphNode`s (`nodes.iter().any(|n| n.path == "...")`).

- [ ] **Step 6: Rewire the daemon** — in `crates/cairn-daemon/src/lib.rs` `query_kind`, replace the `GetGraph` arm and add two:
```rust
        Query::GetGraph { .. } => "get_graph",
        Query::GraphAt { .. } => "graph_at",
        Query::GraphDiff { .. } => "graph_diff",
```

- [ ] **Step 7: Rewire MCP** — in `crates/cairn-mcp/src/lib.rs`, change the `"graph"` mapping and its test:
```rust
        "graph" => Q(Query::GetGraph { scope: cairn_contract::GraphScope::Full }),
```
and in the test:
```rust
            ToolDispatch::Query(Query::GetGraph { scope: cairn_contract::GraphScope::Full })
```
(Use whatever path alias the file already imports for `cairn_contract`; if it imports `Query` directly, add `GraphScope` to that import and write `GraphScope::Full`.)

- [ ] **Step 8: Rewire the CLI graph command** — in `crates/cairn-cli/src/main.rs`, the `Command::Graph` arm:
```rust
        Command::Graph => {
            if let QueryResponse::Graph { nodes, edges } = dispatch_query(
                &engine,
                &WireQuery::GetGraph { scope: cairn_contract::GraphScope::Full },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes {
                    println!("{}\t{}", n.path, n.title);
                }
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
                }
            }
        }
```
Add `GraphScope` to the `cairn_contract::{...}` import (or reference via the existing alias).

- [ ] **Step 9: Build, regenerate bindings, run the full workspace**

```bash
cargo build --workspace
cargo test -p cairn-contract           # regenerates crates/cairn-contract/bindings/*.ts
cargo test --workspace
```
Expected: all PASS; `git status` shows new `bindings/GraphNode.ts`, `bindings/GraphScope.ts`, and modified `bindings/Query.ts`, `bindings/QueryResponse.ts`.

- [ ] **Step 10: Lint + commit (include regenerated bindings)**

```bash
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
git add crates/cairn-contract crates/cairn-service crates/cairn-daemon crates/cairn-mcp crates/cairn-cli
git commit -m "feat(contract): scoped enriched graph + GraphAt/GraphDiff queries

BREAKING: QueryResponse::Graph nodes are GraphNode (was Vec<String>); GetGraph
takes a scope. Rewires service/daemon/mcp/cli and regenerates ts bindings."
```

---

### Task 5: CLI temporal subcommands

**Files:**
- Modify: `crates/cairn-cli/src/main.rs` (`enum Command` + dispatch arms)

**Interfaces:**
- Consumes: `WireQuery::GraphAt`, `WireQuery::GraphDiff`, `QueryResponse::{Graph, GraphDiff}`, `cairn_contract::GraphScope` (Task 4).

- [ ] **Step 1: Add the subcommands** — in `enum Command`, after `Show { .. }`:
```rust
    /// Print the link graph as of a past revision.
    GraphAt {
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
    },
    /// Print what changed in the link graph between two revisions.
    GraphDiff {
        /// Older revspec.
        from: String,
        /// Newer revspec.
        to: String,
    },
```

- [ ] **Step 2: Add dispatch arms** — after the `Command::Graph` arm:
```rust
        Command::GraphAt { revision } => {
            if let QueryResponse::Graph { nodes, edges } = dispatch_query(
                &engine,
                &WireQuery::GraphAt { revision, scope: cairn_contract::GraphScope::Full },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes {
                    println!("{}\t{}", n.path, n.title);
                }
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
                }
            }
        }
        Command::GraphDiff { from, to } => {
            if let QueryResponse::GraphDiff {
                nodes_added,
                nodes_removed,
                edges_added,
                edges_removed,
            } = dispatch_query(
                &engine,
                &WireQuery::GraphDiff { from, to, scope: cairn_contract::GraphScope::Full },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes_added {
                    println!("+ {}", n.path);
                }
                for n in &nodes_removed {
                    println!("- {}", n.path);
                }
                for e in &edges_added {
                    println!("+ {} -> {}", e.from, e.to);
                }
                for e in &edges_removed {
                    println!("- {} -> {}", e.from, e.to);
                }
            }
        }
```

- [ ] **Step 3: Build + manual smoke (optional) + commit**

```bash
cargo build -p cairn-cli && cargo clippy -p cairn-cli --all-targets -- -D warnings && cargo fmt -p cairn-cli
git add crates/cairn-cli/src/main.rs
git commit -m "feat(cli): graph-at and graph-diff subcommands"
```

---

### Task 6: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Full green build, test, lint, format**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
Expected: all PASS, no clippy warnings, format clean.

- [ ] **Step 2: MSRV sanity** (if `rustup` toolchains available)

```bash
cargo +1.88.0 check --workspace
```
Expected: PASS. (Skip if the 1.88 toolchain is not installed; note it in the handoff.)

- [ ] **Step 3: Confirm no `cairn-app → cairn-contract` leak**

```bash
grep -n "cairn_contract\|cairn-contract" crates/cairn-app/src/lib.rs crates/cairn-app/Cargo.toml
```
Expected: NO matches (hexagonal boundary intact).

- [ ] **Step 4: Confirm bindings committed**

```bash
git status --porcelain crates/cairn-contract/bindings
```
Expected: empty (all regenerated bindings already committed in Task 4).

---

## Self-Review

**Spec coverage:**
- GraphAt query + backend → Tasks 2, 3, 4. ✓
- GraphDiff (pure `Graph::diff`) → Tasks 1 (diff), 3 (engine), 4 (wire). ✓
- Enriched `GraphNode` + scoped `GetGraph` (unified) → Task 4 (contract), 3 (`graph_view`). ✓
- mtime: historical last-touch ≤ rev → Task 2 (`read_tree_at` revwalk); HEAD fs mtime → Task 3 (`graph_view` via `stamp`). ✓
- Vcs port `resolve`/`read_tree_at` + `HistoricalBlob` → Task 2. ✓
- oid-keyed LRU, cache Full + focus-on-read → Task 3 (`built_at`, `scope_and_flatten`, cache test). ✓
- Scope semantics (undirected BFS, both-endpoints edge rule, absent-path = empty) → Task 1 (`focused` + tests). ✓
- Error mapping bad revspec → `NotFound` → Task 2 (adapter) + Task 4 (service test). ✓
- Downstream daemon/mcp/cli + codegen/bindings → Task 4. ✓
- CLI temporal commands → Task 5. ✓
- Hexagonal (no contract in app; domain scope type) → Tasks 1/3 + Task 6 Step 3 guard. ✓
- Out-of-scope (MCP temporal tools, GraphHistory, scrubber UI) → not planned, by decision. ✓

**Type consistency:** `GraphScope`/`GraphDelta`/`Graph::scoped` (domain) consumed by engine; `GraphResult`/`GraphDeltaResult` (app) consumed by service; `GraphNode`/`GraphScope`/`Query`/`QueryResponse` (contract) consumed by service/daemon/mcp/cli. `graph_view`/`graph_at`/`graph_diff`/`built_at` names consistent across Tasks 3–5. ✓

**Placeholder scan:** no TBD/TODO; every code step shows complete code. ✓

---

## Risks / notes for the executor

- The mtime revwalk in `read_tree_at` can walk deep on long histories; the oid LRU makes it once-per-revision. A bounded-walk fallback (cap depth, use snapshot time for unresolved paths) is a no-contract-change follow-up if profiling demands it.
- Task 4 is the one intentionally large commit (a shared enum flip cannot be split without a red workspace). Keep its steps in order; build the workspace before committing.
- We are *authoring* this contract, not consuming a frozen one. The `knowledge-graph-viz-ui` branch will adopt `GraphNode`/`GraphScope`/`GraphDiff` — flag that session when this merges.
- `git2` `tree.walk` callback returns `git2::TreeWalkResult::Ok` to continue; `dir` carries a trailing slash (or is `""` at root), so `format!("{dir}{name}")` yields the correct forward-slash path.
