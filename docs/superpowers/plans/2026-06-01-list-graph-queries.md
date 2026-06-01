# List & Graph Queries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two read queries to the Cairn contract — `ListNotes` (path + display title) and `GetGraph` (nodes + directed edges) — served through the existing dispatcher and daemon.

**Architecture:** New domain logic (`NotePath::stem`, `Note::display_title`, `Graph::nodes`/`edges`), new contract DTOs (`NoteSummary`, `GraphEdge`, `Query::ListNotes`/`GetGraph`, `QueryResponse::Notes`/`Graph`), two `Engine` accessors, two `dispatch_query` arms, and two CLI subcommands. The daemon serves the new queries with NO code change (it calls `dispatch_query` for any query).

**Tech Stack:** Rust 1.85 (`forbid(unsafe_code)`), serde + ts-rs (contract), clap (CLI), axum (daemon test only).

**Verified shapes:** `NotePath(String)` with `as_str(&self)->&str`; `Note { path: NotePath, frontmatter: Option<String>, body: String }`, `Note::parse(NotePath,&str)`; `graph.rs` has a private `fn stem(&NotePath)->&str` and `Graph { forward, backward: BTreeMap<NotePath,Vec<NotePath>> }` with a `note(path,body)` test helper; `Engine::load_all_notes` is private; `cairn-app` already imports `cairn_domain::{Graph, Note, NotePath}`; `QueryResponse` is `Note{contents}|Paths{paths}`; `cairn-service` `dispatch_query` matches `Query::{GetNote,Search,GetBacklinks}`.

---

## Task 1: Domain — stem, display_title, graph accessors

**Files:**
- Modify: `crates/cairn-domain/src/note.rs`, `crates/cairn-domain/src/graph.rs`

- [ ] **Step 1: Add `NotePath::stem` and refactor graph.rs to use it**

In `crates/cairn-domain/src/note.rs`, inside `impl NotePath` (right after `as_str`), add:
```rust
    /// The note's stem: the filename without its directory or `.md`
    /// extension (e.g. `dir/a.md` -> `a`).
    #[must_use]
    pub fn stem(&self) -> &str {
        let after_slash = self.0.rsplit('/').next().unwrap_or(&self.0);
        after_slash.strip_suffix(".md").unwrap_or(after_slash)
    }
```
In `crates/cairn-domain/src/graph.rs`, DELETE the private free function:
```rust
fn stem(path: &NotePath) -> &str {
    let s = path.as_str();
    let after_slash = s.rsplit('/').next().unwrap_or(s);
    after_slash.strip_suffix(".md").unwrap_or(after_slash)
}
```
and in `Graph::build`, replace the `by_stem` construction call `stem(&n.path)` with `n.path.stem()`:
```rust
        let by_stem: BTreeMap<&str, &NotePath> =
            notes.iter().map(|n| (n.path.stem(), &n.path)).collect();
```

- [ ] **Step 2: Add `Note::display_title` + test**

In `crates/cairn-domain/src/note.rs`, inside `impl Note` (after `parse`), add:
```rust
    /// A human display title: the frontmatter `title:` value if present,
    /// else the first Markdown `# ` heading in the body, else the path stem.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let Some(fm) = &self.frontmatter {
            for line in fm.lines() {
                if let Some(rest) = line.trim_start().strip_prefix("title:") {
                    let t = rest.trim().trim_matches('"').trim_matches('\'').trim();
                    if !t.is_empty() {
                        return t.to_string();
                    }
                }
            }
        }
        for line in self.body.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("# ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
        self.path.stem().to_string()
    }
```
Add to the `#[cfg(test)] mod tests` block in `note.rs`:
```rust
    #[test]
    fn stem_strips_dir_and_extension() {
        assert_eq!(NotePath::new("dir/sub/a.md").unwrap().stem(), "a");
        assert_eq!(NotePath::new("b").unwrap().stem(), "b");
    }

    #[test]
    fn display_title_prefers_frontmatter_then_heading_then_stem() {
        let p = NotePath::new("a.md").unwrap();
        let fm = Note::parse(p.clone(), "---\ntitle: \"My Title\"\n---\n# Heading\nbody");
        assert_eq!(fm.display_title(), "My Title");

        let heading = Note::parse(p.clone(), "# The Heading\nbody");
        assert_eq!(heading.display_title(), "The Heading");

        let plain = Note::parse(p, "just text");
        assert_eq!(plain.display_title(), "a");
    }
```

- [ ] **Step 3: Add `Graph::nodes`/`edges` + test**

In `crates/cairn-domain/src/graph.rs`, inside `impl Graph` (after `backlinks`), add:
```rust
    /// All note paths in the graph, sorted.
    #[must_use]
    pub fn nodes(&self) -> Vec<&NotePath> {
        self.forward.keys().collect()
    }

    /// All directed `(from, to)` link edges.
    #[must_use]
    pub fn edges(&self) -> Vec<(&NotePath, &NotePath)> {
        self.forward
            .iter()
            .flat_map(|(from, tos)| tos.iter().map(move |to| (from, to)))
            .collect()
    }
```
Add to the `#[cfg(test)] mod tests` block in `graph.rs` (the `note` helper already exists there):
```rust
    #[test]
    fn nodes_and_edges_expose_the_graph() {
        let notes = vec![note("a.md", "see [[b]]"), note("b.md", "no links")];
        let g = Graph::build(&notes);
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        assert_eq!(g.nodes(), vec![&a, &b]);
        assert_eq!(g.edges(), vec![(&a, &b)]);
    }
```

- [ ] **Step 4: Run tests + lint**

Run: `cargo test -p cairn-domain`
Expected: all PASS (existing + 3 new).
Then: `cargo clippy -p cairn-domain --all-targets -- -D warnings` and `cargo fmt --all` then `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(domain): NotePath::stem, Note::display_title, Graph nodes/edges"
```

---

## Task 2: Contract — ListNotes/GetGraph + NoteSummary/GraphEdge + responses

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`, `crates/cairn-contract/tests/codegen.rs`
- Regenerated: `crates/cairn-contract/bindings/*.ts`

- [ ] **Step 1: Add the `Query` variants**

In `crates/cairn-contract/src/lib.rs`, in the `Query` enum, add two field-less variants after `GetBacklinks`:
```rust
    /// List every note with a display title.
    ListNotes,
    /// Fetch the full link graph.
    GetGraph,
```

- [ ] **Step 2: Add the DTO structs**

In `crates/cairn-contract/src/lib.rs`, add (just before the `QueryResponse` enum):
```rust
/// A note's path and display title, for list views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NoteSummary {
    /// Relative note path.
    pub path: String,
    /// Display title (frontmatter title, first heading, or filename).
    pub title: String,
}

/// A directed link edge between two notes, by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphEdge {
    /// Source note path.
    pub from: String,
    /// Target note path.
    pub to: String,
}
```

- [ ] **Step 3: Add the `QueryResponse` variants**

In the `QueryResponse` enum, add after `Paths`:
```rust
    /// Note summaries (response to `ListNotes`).
    Notes {
        /// One per note.
        notes: Vec<NoteSummary>,
    },
    /// The link graph (response to `GetGraph`).
    Graph {
        /// All note paths.
        nodes: Vec<String>,
        /// Directed link edges.
        edges: Vec<GraphEdge>,
    },
```

- [ ] **Step 4: Extend the codegen + round-trip tests**

In `crates/cairn-contract/tests/codegen.rs`, add `GraphEdge, NoteSummary` to the import and add export assertions:
```rust
use cairn_contract::{
    Command, CommandResponse, ContractError, Event, GraphEdge, NoteSummary, Query, QueryResponse,
};
```
and inside the test body, before the final `export_all` calls, add:
```rust
    assert!(NoteSummary::decl().contains("NoteSummary"));
    assert!(GraphEdge::decl().contains("GraphEdge"));
```
and add these two lines alongside the other `export_all()` calls:
```rust
    NoteSummary::export_all().unwrap();
    GraphEdge::export_all().unwrap();
```

In `crates/cairn-contract/src/lib.rs` `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn list_and_graph_responses_roundtrip() {
        let n = QueryResponse::Notes {
            notes: vec![NoteSummary { path: "a.md".into(), title: "Alpha".into() }],
        };
        let j = serde_json::to_string(&n).unwrap();
        assert!(j.contains("\"type\":\"notes\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), n);

        let g = QueryResponse::Graph {
            nodes: vec!["a.md".into(), "b.md".into()],
            edges: vec![GraphEdge { from: "a.md".into(), to: "b.md".into() }],
        };
        let j = serde_json::to_string(&g).unwrap();
        assert!(j.contains("\"type\":\"graph\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), g);

        assert_eq!(serde_json::to_string(&Query::ListNotes).unwrap(), "{\"type\":\"list_notes\"}");
        assert_eq!(
            serde_json::from_str::<Query>("{\"type\":\"get_graph\"}").unwrap(),
            Query::GetGraph
        );
    }
```

- [ ] **Step 5: Run tests + verify bindings**

Run: `cargo test -p cairn-contract`
Expected: PASS. New files exist: `crates/cairn-contract/bindings/NoteSummary.ts`, `GraphEdge.ts`; `Query.ts` and `QueryResponse.ts` updated.
Then: `cargo clippy -p cairn-contract --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

- [ ] **Step 6: Commit (incl. bindings)**

```bash
git add -A && git commit -m "feat(contract): ListNotes/GetGraph queries + NoteSummary/GraphEdge DTOs"
```

---

## Task 3: App — list_notes & graph accessors

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Add the two Engine methods**

In `crates/cairn-app/src/lib.rs`, inside `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` (after `backlinks`), add:
```rust
    /// All parsed notes in the cairn.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn list_notes(&self) -> Result<Vec<Note>, PortError> {
        self.load_all_notes()
    }

    /// The link graph derived from the current notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn graph(&self) -> Result<Graph, PortError> {
        Ok(Graph::build(&self.load_all_notes()?))
    }
```
(`Graph` and `Note` are already imported in this file.)

- [ ] **Step 2: Add a test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-app/src/lib.rs`, add:
```rust
    #[test]
    fn list_notes_and_graph_expose_engine_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "see [[b]]", &mut events).unwrap();
        eng.write_note(&NotePath::new("b.md").unwrap(), "hi", &mut events).unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 2);
        assert_eq!(eng.graph().unwrap().edges().len(), 1);
    }
```
(The `engine(dir)` test helper already exists in this module.)

- [ ] **Step 3: Run tests + lint**

Run: `cargo test -p cairn-app`
Expected: all PASS.
Then: `cargo clippy -p cairn-app --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(app): Engine::list_notes and Engine::graph"
```

---

## Task 4: Dispatcher — ListNotes/GetGraph arms

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`

- [ ] **Step 1: Import the new DTOs**

In `crates/cairn-service/src/lib.rs`, extend the `cairn_contract` import to include `GraphEdge` and `NoteSummary`:
```rust
use cairn_contract::{
    Command, CommandResponse, ContractError, Event as WireEvent, GraphEdge, NoteSummary, Query,
    QueryResponse,
};
```

- [ ] **Step 2: Add the two `dispatch_query` arms**

In `dispatch_query`, add these arms to the `match query` block (after `GetBacklinks`):
```rust
        Query::ListNotes => {
            let notes = engine
                .list_notes()?
                .into_iter()
                .map(|n| NoteSummary {
                    path: n.path.as_str().to_string(),
                    title: n.display_title(),
                })
                .collect();
            Ok(QueryResponse::Notes { notes })
        }
        Query::GetGraph => {
            let graph = engine.graph()?;
            let nodes = graph
                .nodes()
                .into_iter()
                .map(|p| p.as_str().to_string())
                .collect();
            let edges = graph
                .edges()
                .into_iter()
                .map(|(from, to)| GraphEdge {
                    from: from.as_str().to_string(),
                    to: to.as_str().to_string(),
                })
                .collect();
            Ok(QueryResponse::Graph { nodes, edges })
        }
```

- [ ] **Step 3: Add a test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-service/src/lib.rs`, add:
```rust
    #[test]
    fn list_notes_and_graph_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "---\ntitle: Alpha\n---\nsee [[b]]".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "b.md".into(), contents: "hi".into() },
            &mut sink,
        )
        .unwrap();

        match dispatch_query(&eng, &Query::ListNotes).unwrap() {
            QueryResponse::Notes { notes } => {
                assert_eq!(notes.len(), 2);
                assert!(notes.iter().any(|n| n.path == "a.md" && n.title == "Alpha"));
                assert!(notes.iter().any(|n| n.path == "b.md" && n.title == "b"));
            }
            other => panic!("expected Notes, got {other:?}"),
        }

        match dispatch_query(&eng, &Query::GetGraph).unwrap() {
            QueryResponse::Graph { nodes, edges } => {
                assert_eq!(nodes, vec!["a.md".to_string(), "b.md".to_string()]);
                assert_eq!(edges, vec![GraphEdge { from: "a.md".into(), to: "b.md".into() }]);
            }
            other => panic!("expected Graph, got {other:?}"),
        }
    }
```

- [ ] **Step 4: Run tests + lint**

Run: `cargo test -p cairn-service`
Expected: all PASS.
Then: `cargo clippy -p cairn-service --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(service): dispatch ListNotes and GetGraph queries"
```

---

## Task 5: CLI — `list` and `graph` subcommands

**Files:**
- Modify: `crates/cairn-cli/src/main.rs`, `crates/cairn-cli/tests/cli.rs`

- [ ] **Step 1: Add the subcommands**

In `crates/cairn-cli/src/main.rs`, in the clap `Command` enum, add after `Backlinks`:
```rust
    /// List all notes with their titles.
    List,
    /// Print the link graph as `from -> to` edges.
    Graph,
```

- [ ] **Step 2: Add the match arms**

In `run()`'s `match cli.command` block, add (after the `Backlinks` arm):
```rust
        Command::List => {
            if let QueryResponse::Notes { notes } =
                dispatch_query(&engine, &WireQuery::ListNotes).map_err(|e| e.to_string())?
            {
                for n in notes {
                    println!("{}\t{}", n.path, n.title);
                }
            }
        }
        Command::Graph => {
            if let QueryResponse::Graph { edges, .. } =
                dispatch_query(&engine, &WireQuery::GetGraph).map_err(|e| e.to_string())?
            {
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
                }
            }
        }
```
(`WireQuery` and `QueryResponse` are already imported in this file.)

- [ ] **Step 3: Add an integration test**

In `crates/cairn-cli/tests/cli.rs`, add (the `cairn(dir)` helper and `contains` import already exist):
```rust
#[test]
fn list_and_graph_subcommands() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir).args(["write", "a.md", "see [[b]]"]).assert().success();
    cairn(dir).args(["write", "b.md", "hi"]).assert().success();

    cairn(dir)
        .arg("list")
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("b.md"));
    cairn(dir)
        .arg("graph")
        .assert()
        .success()
        .stdout(contains("a.md -> b.md"));
}
```

- [ ] **Step 4: Run tests + lint**

Run: `cargo test -p cairn-cli`
Expected: all PASS (existing + new).
Then: `cargo clippy -p cairn-cli --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(cli): list and graph subcommands"
```

---

## Task 6: Daemon — prove ListNotes flows through HTTP

**Files:**
- Modify: `crates/cairn-daemon/tests/http.rs`

- [ ] **Step 1: Add an HTTP test for the new query**

In `crates/cairn-daemon/tests/http.rs`, append:
```rust
#[tokio::test]
async fn list_notes_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, _) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) =
        post_json(app, "/query", serde_json::json!({"type":"list_notes"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "notes");
    assert_eq!(body["notes"][0]["path"], "a.md");
}
```

- [ ] **Step 2: Run the daemon tests**

Run: `cargo test -p cairn-daemon --test http`
Expected: all PASS (existing 5 + new).

- [ ] **Step 3: Full workspace gate**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```
Expected: all green. Confirm the CLI works: `cargo run -p cairn-cli -- --cairn /tmp/cairn-demo init` then `... write a.md "x"` then `... list` and `... graph`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test(daemon): ListNotes flows through HTTP unchanged"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** §2 domain (stem/display_title/nodes/edges) → Task 1; §3 contract DTOs + Query/QueryResponse variants → Task 2; §4 app accessors → Task 3; §5 dispatcher arms → Task 4; §6 CLI → Task 5; §7 daemon flow-through test → Task 6. Title precedence + directed edges covered in Task 1/4 tests.
- **Type consistency:** `NotePath::stem`, `Note::display_title`, `Graph::nodes/edges`, `Engine::list_notes/graph`, `Query::ListNotes/GetGraph`, `QueryResponse::Notes{notes}/Graph{nodes,edges}`, `NoteSummary{path,title}`, `GraphEdge{from,to}` are used identically across Tasks 1–6.
- **Placeholder scan:** no TBD/TODO; every code step is complete.
- **No daemon lib change:** confirmed — only a test is added (Task 6); the new queries flow through `dispatch_query`.
```
