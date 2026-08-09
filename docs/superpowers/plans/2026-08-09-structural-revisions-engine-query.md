# StructuralRevisions Engine Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Query::StructuralRevisions { limit }` engine query that returns only the vault revisions that changed the link graph (a note node or a link edge added/removed), newest-first, to unblock cairn-web-ui Phase 2 of the vault-history timeline.

**Architecture:** Three layers, bottom-up. (1) A new `Vcs` port method `md_change_log` (git2, in `cairn-infra`) returns every commit newest-first flagged with whether it touched any `.md` file vs its first parent — the cheap tree-diff pre-filter. (2) `Engine::structural_revisions` (in `cairn-app`) walks that log, skips non-`.md` commits, and confirms a real graph change for the rest by comparing `built_at(child).graph != built_at(parent).graph` (domain `Graph` `Eq` = node set + edge set; the oid-keyed graph cache gives per-commit single-parse reuse). (3) The contract gains the `Query` variant reusing the existing `QueryResponse::History`, wired through `dispatch_query` and the daemon telemetry match.

**Tech Stack:** Rust, `git2`, `ts-rs` (TypeScript binding generation), `cargo nextest`.

## Global Constraints

- **Hexagonal boundary:** `git2` stays in `cairn-infra`; domain `Graph` (link extraction, stem resolution) stays out of `cairn-infra`. The graph-equality confirmation lives in `cairn-app`.
- **`forbid(unsafe_code)`** is in force; no `unsafe`.
- **Errors:** adapters return `PortError` via the existing `adapt()` helper; no `unwrap`/`panic` in non-test code.
- **Contract is ts-rs-generated + drift-checked.** Never hand-edit files under `crates/cairn-contract/bindings/`; regenerate them by running the codegen test and commit the result.
- **`Revision` is reused verbatim** — `Revision { id: String (7-char), message: String, timestamp_secs: i64, author: String }`. No new response type.
- **Ordering:** commits are newest-first via `git2::Sort::TIME | git2::Sort::TOPOLOGICAL`, matching `vault_history`/`history`.
- **Verification:** each task's tests via `cargo test -p <crate> <name>`; the final gate is `just ci` (fmt, lint/clippy `-D warnings`, test, doc-test, deny, locked-check).
- Work on branch `feat/structural-revisions-graph-query` (already created off `origin/main`; the design spec is already committed there).

---

### Task 1: `Vcs::md_change_log` port method + git2 adapter

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (add `MdCommit` struct near `HistoricalBlob`/`Revision`; add `md_change_log` to the `Vcs` trait)
- Modify: `crates/cairn-infra/src/git.rs` (add `tree_has_md_change` helper; implement `md_change_log` in `impl Vcs for GitVcs`; add tests)
- Modify: `crates/cairn-app/src/lib.rs` (add `md_change_log` to the `CountingVcs` test double so the trait stays satisfied)

**Interfaces:**
- Produces:
  ```rust
  // cairn-ports
  pub struct MdCommit {
      pub revision: Revision,      // the commit as a Revision (7-char id, summary, time, author)
      pub oid: String,             // full 40-hex commit oid
      pub parent: Option<String>,  // first-parent oid; None at the root commit
      pub md_changed: bool,        // any `.md` path differs vs the first parent (root: any `.md` in tree)
  }
  // cairn-ports Vcs trait
  fn md_change_log(&self) -> Result<Vec<MdCommit>, PortError>; // newest-first; empty repo -> Ok(vec![])
  ```

- [ ] **Step 1: Add `MdCommit` to `cairn-ports`**

In `crates/cairn-ports/src/lib.rs`, immediately after the `Revision` struct, add:

```rust
/// One commit's structural-candidacy signal for the graph time-view: the commit
/// as a [`Revision`], its full oid, its first-parent oid, and whether it changed
/// any `.md` path vs that parent. Returned newest-first by [`Vcs::md_change_log`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdCommit {
    /// The commit as a `Revision` (7-char id, summary, time, author).
    pub revision: Revision,
    /// The commit's full oid (40-hex), for graph-cache keying.
    pub oid: String,
    /// First-parent oid, or `None` for the root commit.
    pub parent: Option<String>,
    /// Whether any `.md` path differs between this commit and its first parent
    /// (for the root commit: whether the tree contains any `.md` blob). A
    /// `false` value means the commit cannot have changed the link graph.
    pub md_changed: bool,
}
```

- [ ] **Step 2: Declare the trait method**

In the `pub trait Vcs` block, after `read_tree_at`, add:

```rust
    /// Every commit newest-first, each flagged with whether it changed any `.md`
    /// path vs its first parent — the cheap pre-filter for the structural-graph
    /// time-view. Ordering matches [`Vcs::vault_history`].
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a git failure. An empty repo yields `Ok(vec![])`.
    fn md_change_log(&self) -> Result<Vec<MdCommit>, PortError>;
```

- [ ] **Step 3: Write the failing adapter tests**

In `crates/cairn-infra/src/git.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn md_change_log_flags_md_and_non_md_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("add a.md").unwrap(); // root: .md present -> structural candidate
        fs::write(tmp.path().join("config.txt"), "x").unwrap();
        vcs.commit_all("add config").unwrap(); // no .md touched
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("edit a.md").unwrap(); // .md touched

        let log = vcs.md_change_log().unwrap();
        assert_eq!(log.len(), 3);
        // Newest-first.
        assert_eq!(log[0].revision.message, "edit a.md");
        assert!(log[0].md_changed);
        assert_eq!(log[1].revision.message, "add config");
        assert!(!log[1].md_changed, "a commit touching no .md is not a candidate");
        assert_eq!(log[2].revision.message, "add a.md");
        assert!(log[2].md_changed, "root commit adding a .md is a candidate");
        // Shape: full oid, 7-char short id, parent linkage.
        assert_eq!(log[0].oid.len(), 40);
        assert_eq!(log[0].revision.id.len(), 7);
        assert_eq!(log[0].parent.as_deref(), Some(log[1].oid.as_str()));
        assert_eq!(log[2].parent, None, "root has no parent");
    }

    #[test]
    fn md_change_log_empty_for_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(vcs.md_change_log().unwrap().is_empty());
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p cairn-infra md_change_log`
Expected: FAIL to compile — `md_change_log` not implemented for `GitVcs` (and `CountingVcs`). This confirms the trait method is required everywhere.

- [ ] **Step 5: Implement the `.md`-diff helper and the method**

In `crates/cairn-infra/src/git.rs`, add near `commit_touched_path` (top of file):

```rust
/// Whether `commit`'s tree differs from `parent`'s in any `.md` path. For the
/// root commit (`parent` = `None`), whether the tree contains any `.md` blob.
fn tree_has_md_change(
    repo: &Repository,
    commit: &git2::Commit,
    parent: Option<&git2::Commit>,
) -> Result<bool, git2::Error> {
    let new_tree = commit.tree()?;
    let old_tree = match parent {
        Some(p) => Some(p.tree()?),
        None => None,
    };
    let diff = repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)?;
    for delta in diff.deltas() {
        let is_md = [delta.new_file().path(), delta.old_file().path()]
            .into_iter()
            .flatten()
            .any(|p| p.extension().is_some_and(|e| e == "md"));
        if is_md {
            return Ok(true);
        }
    }
    Ok(false)
}
```

Add `use cairn_ports::MdCommit;` to the existing `cairn_ports::{...}` import line at the top.

In `impl Vcs for GitVcs`, after `read_tree_at`, add:

```rust
    fn md_change_log(&self) -> Result<Vec<MdCommit>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut walk = repo.revwalk().map_err(adapt)?;
        // No HEAD (empty repo) -> no history.
        if walk.push_head().is_err() {
            return Ok(Vec::new());
        }
        // TOPOLOGICAL keeps children before parents (newest first), matching
        // vault_history even when commits share a timestamp.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
            .map_err(adapt)?;
        let mut out = Vec::new();
        for oid in walk {
            let oid = oid.map_err(adapt)?;
            let commit = repo.find_commit(oid).map_err(adapt)?;
            // First parent only: "what this commit changed on the mainline".
            let parent = commit.parent(0).ok();
            let md_changed = tree_has_md_change(&repo, &commit, parent.as_ref()).map_err(adapt)?;
            out.push(MdCommit {
                revision: Revision {
                    id: oid.to_string()[..7].to_string(),
                    message: commit.summary().ok().flatten().unwrap_or("").to_string(),
                    timestamp_secs: commit.time().seconds(),
                    author: commit.author().name().unwrap_or("").to_string(),
                },
                oid: oid.to_string(),
                parent: parent.map(|p| p.id().to_string()),
                md_changed,
            });
        }
        Ok(out)
    }
```

- [ ] **Step 6: Satisfy the `CountingVcs` test double**

In `crates/cairn-app/src/lib.rs`, inside `impl Vcs for CountingVcs`, add a delegating method (place it after `read_tree_at`):

```rust
        fn md_change_log(&self) -> Result<Vec<cairn_ports::MdCommit>, PortError> {
            self.inner.md_change_log()
        }
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p cairn-infra md_change_log`
Expected: PASS (2 tests). Also confirm the workspace still builds: `cargo build -p cairn-app`.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/git.rs crates/cairn-app/src/lib.rs
git commit -m "feat(ports): md_change_log — per-commit .md-change flag for structural graph queries"
```

---

### Task 2: `Engine::structural_revisions` in `cairn-app`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs` (add `structural_revisions` method near `vault_history`; add tests in `mod tests`)

**Interfaces:**
- Consumes: `Vcs::md_change_log() -> Result<Vec<MdCommit>, PortError>` (Task 1); the existing private `fn built_at(&self, revision: &str) -> Result<Arc<BuiltGraph>, PortError>` where `BuiltGraph { graph: Graph, .. }`; `cairn_domain::Graph: PartialEq + Default`.
- Produces:
  ```rust
  pub fn structural_revisions(&self, limit: Option<u32>) -> Result<Vec<Revision>, PortError>;
  // The `limit` most-recent revisions whose graph differs from their first
  // parent's; newest-first. Returns ports `Revision` (like vault_history).
  ```

- [ ] **Step 1: Write the failing tests**

In `crates/cairn-app/src/lib.rs`, inside `mod tests`, add. These use the existing `engine(dir)` helper (real `GitVcs`), `eng.write_note`, `eng.delete_note`, `eng.commit`:

```rust
    #[test]
    fn structural_revisions_excludes_text_only_edits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "hello", &mut ev).unwrap();
        eng.write_note(&b, "world", &mut ev).unwrap();
        eng.commit("c1 create a,b", &mut ev).unwrap(); // nodes added -> structural
        eng.write_note(&a, "hello, more prose but no links", &mut ev).unwrap();
        eng.commit("c2 text edit", &mut ev).unwrap(); // text only -> NOT structural

        let revs = eng.structural_revisions(None).unwrap();
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].message, "c1 create a,b");
    }

    #[test]
    fn structural_revisions_includes_link_and_node_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "x", &mut ev).unwrap();
        eng.write_note(&b, "y", &mut ev).unwrap();
        eng.commit("c1 create", &mut ev).unwrap(); // nodes added
        eng.write_note(&a, "[[b]]", &mut ev).unwrap();
        eng.commit("c2 add link", &mut ev).unwrap(); // edge a->b added
        eng.delete_note(&b, &mut ev).unwrap();
        eng.commit("c3 remove b", &mut ev).unwrap(); // node b + edge removed

        let revs = eng.structural_revisions(None).unwrap();
        let msgs: Vec<&str> = revs.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(msgs, vec!["c3 remove b", "c2 add link", "c1 create"]); // newest-first
    }

    #[test]
    fn structural_revisions_skips_non_md_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        eng.write_note(&a, "x", &mut ev).unwrap();
        eng.commit("c1 create a", &mut ev).unwrap(); // structural
        std::fs::write(tmp.path().join("assets.bin"), b"blob").unwrap();
        eng.commit("c2 add asset", &mut ev).unwrap(); // no .md touched -> skipped

        let revs = eng.structural_revisions(None).unwrap();
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].message, "c1 create a");
    }

    #[test]
    fn structural_revisions_caps_at_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        for i in 0..3 {
            let p = NotePath::new(&format!("n{i}.md")).unwrap();
            eng.write_note(&p, "x", &mut ev).unwrap();
            eng.commit(&format!("c{i}"), &mut ev).unwrap(); // each adds a node -> structural
        }
        let revs = eng.structural_revisions(Some(2)).unwrap();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].message, "c2"); // newest two
        assert_eq!(revs[1].message, "c1");
    }

    #[test]
    fn structural_revisions_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let eng = engine(tmp.path());
        assert!(eng.structural_revisions(None).unwrap().is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-app structural_revisions`
Expected: FAIL — `no method named structural_revisions`.

- [ ] **Step 3: Implement `structural_revisions`**

In `crates/cairn-app/src/lib.rs`, add the method immediately after `vault_history` (same `impl Engine` block). `Graph` is already imported (used by `built_at`):

```rust
    /// Vault revisions that changed the link graph — a note node or a link edge
    /// added or removed — newest-first, capped at `limit`. Metadata-only and
    /// body-text edits that add/remove no link are excluded.
    ///
    /// Walks the `.md`-change log newest→oldest, skips commits that touched no
    /// `.md` (they cannot change the graph), and confirms the rest by comparing
    /// the commit's built graph against its first parent's. `built_at` caches by
    /// oid, so consecutive commits parse each tree at most once.
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter or a tree read fails.
    pub fn structural_revisions(&self, limit: Option<u32>) -> Result<Vec<Revision>, PortError> {
        let cap = limit.map(|n| n as usize);
        let mut out = Vec::new();
        for c in self.vcs.md_change_log()? {
            if cap.is_some_and(|n| out.len() >= n) {
                break;
            }
            if !c.md_changed {
                continue; // a commit touching no .md cannot change the graph
            }
            let child = self.built_at(&c.oid)?;
            let parent_graph = match &c.parent {
                Some(p) => self.built_at(p)?.graph.clone(),
                None => Graph::default(), // root: compare against the empty graph
            };
            if child.graph != parent_graph {
                out.push(c.revision);
            }
        }
        Ok(out)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-app structural_revisions`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): structural_revisions — vault revisions that changed the link graph"
```

---

### Task 3: Contract variant + binding regen + dispatch wiring

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (add `Query::StructuralRevisions { limit }`)
- Regenerate: `crates/cairn-contract/bindings/Query.ts` (via the codegen test — do NOT hand-edit)
- Modify: `crates/cairn-service/src/lib.rs` (add the `dispatch_query` arm; add a service test)
- Modify: `crates/cairn-daemon/src/lib.rs` (add the `query_kind` telemetry arm)

**Interfaces:**
- Consumes: `Engine::structural_revisions(limit) -> Result<Vec<Revision>, PortError>` (Task 2); existing `QueryResponse::History { revisions: Vec<Revision> }`.
- Produces: `Query::StructuralRevisions { limit: Option<u32> }` → `QueryResponse::History`.

- [ ] **Step 1: Add the `Query` variant**

In `crates/cairn-contract/src/lib.rs`, in `pub enum Query`, after the `VaultHistory { .. }` variant, add:

```rust
    /// Vault revisions that changed the link graph — a note node or a link edge
    /// added/removed — newest first, capped at `limit`. Metadata-only edits
    /// (title, tags, body text with no link change) are excluded. The response
    /// is a `QueryResponse::History`, like `VaultHistory`.
    StructuralRevisions {
        /// Max structural revisions to return; `None` returns all.
        limit: Option<u32>,
    },
```

- [ ] **Step 2: Write the failing service dispatch test**

In `crates/cairn-service/src/lib.rs`, inside `mod tests`, add (mirrors the existing `dispatch_query` tests; `eng`/engine setup follows the surrounding tests — reuse the same helper the neighbouring `dispatch_query` tests use):

```rust
    #[test]
    fn structural_revisions_dispatches_to_history() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path()); // same helper the other dispatch tests use
        let mut ev = Vec::new();
        let a = cairn_domain::NotePath::new("a.md").unwrap();
        eng.write_note(&a, "hello", &mut ev).unwrap();
        eng.commit("c1", &mut ev).unwrap(); // node added -> structural

        match dispatch_query(&eng, &Query::StructuralRevisions { limit: None }).unwrap() {
            QueryResponse::History { revisions } => {
                assert_eq!(revisions.len(), 1);
                assert_eq!(revisions[0].message, "c1");
            }
            other => panic!("expected History, got {other:?}"),
        }
    }
```

> If the service test module has no local `engine(dir)` helper, construct the `Engine` inline exactly as `crates/cairn-app/src/lib.rs`'s `engine()` does: `Engine::new(LocalFsStore::open(dir).unwrap(), InMemoryIndex::default(), GitVcs::open_or_init(dir).unwrap())`, importing those from `cairn_app`/`cairn_infra` as the surrounding tests do.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cairn-service structural_revisions_dispatches_to_history`
Expected: FAIL to compile — `dispatch_query` match is non-exhaustive (missing `StructuralRevisions`).

- [ ] **Step 4: Add the `dispatch_query` arm**

In `crates/cairn-service/src/lib.rs`, in `dispatch_query`, after the `Query::VaultHistory { limit } => { .. }` arm, add:

```rust
        Query::StructuralRevisions { limit } => {
            let revisions = engine
                .structural_revisions(*limit)?
                .into_iter()
                .map(|r| Revision {
                    id: r.id,
                    message: r.message,
                    timestamp_secs: r.timestamp_secs,
                    author: r.author,
                })
                .collect();
            Ok(QueryResponse::History { revisions })
        }
```

- [ ] **Step 5: Add the daemon `query_kind` arm**

In `crates/cairn-daemon/src/lib.rs`, in `fn query_kind`, after `Query::VaultHistory { .. } => "vault_history",`, add:

```rust
        Query::StructuralRevisions { .. } => "structural_revisions",
```

- [ ] **Step 6: Regenerate the TypeScript bindings**

Run: `cargo test -p cairn-contract exports_typescript_bindings`
This runs `Query::export_all()`, rewriting `crates/cairn-contract/bindings/Query.ts` to include the new `StructuralRevisions` member. Confirm the diff:

Run: `git diff --stat crates/cairn-contract/bindings/`
Expected: `Query.ts` changed (one new union member), nothing else.

- [ ] **Step 7: Run the service test + verify exhaustiveness**

Run: `cargo test -p cairn-service structural_revisions_dispatches_to_history`
Expected: PASS. Then `cargo build --workspace` — expected: clean (daemon `query_kind` now exhaustive).

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/bindings/Query.ts \
        crates/cairn-service/src/lib.rs crates/cairn-daemon/src/lib.rs
git commit -m "feat(contract): StructuralRevisions query — wired through dispatch + bindings"
```

---

### Task 4: Full gate

- [ ] **Step 1: Run the complete local gate**

Run: `just ci`
Expected: PASS — fmt, clippy (`-D warnings`), `cargo nextest run --workspace --all-targets`, doc-tests, cargo-deny, locked-check all green. Fix any fmt/clippy nits inline (e.g. run `just fmt` if formatting drifted) and re-run.

- [ ] **Step 2: Push and open the engine PR**

```bash
git push -u origin feat/structural-revisions-graph-query
gh pr create --base main --title "feat: StructuralRevisions query — vault revisions that changed the link graph" \
  --body "Adds Query::StructuralRevisions { limit } → QueryResponse::History, returning only vault revisions that changed the link graph (node/edge added/removed). Cheap: git tree-diff skips commits touching no .md; the rest are confirmed by comparing built graphs (oid-cached, single-parse). Unblocks cairn-web-ui Phase 2 of the vault-history timeline (PR #122). Spec: docs/superpowers/specs/2026-08-09-structural-revisions-engine-query-design.md"
```

---

## Downstream (separate PRs — NOT part of this plan)

1. **cairn-web-ui contract sync PR** — after this PR merges: bump the six `cairn-*` git revs in `web/../src-tauri/Cargo.toml` to the new engine commit; regenerate/vendor the contract so `Query.ts` gains `StructuralRevisions`.
2. **cairn-web-ui UI PR** — add a structural mode to `loadVaultTimeline` (`web/src/store/store.ts`) and thin the markers in `web/src/components/graph/timelineDensity.ts` and `TemporalScrubber.tsx`. **Also blocked on PR #122 merging** (still open as of 2026-08-09).
```
```

## Self-review

**Spec coverage:** Query variant reusing `History` (Task 3) ✓ · algorithm tree-diff skip (Task 1) + graph-equality confirm + limit + reuse (Task 2) ✓ · root/merge/empty edge cases (Task 1 helper + Task 2 root branch) ✓ · hexagonal split ports/infra/app (Tasks 1–2) ✓ · change surface contract/ports/infra/app/service/daemon (Tasks 1–3) ✓ · tests at infra/app/service (Tasks 1–3) ✓ · bindings regen (Task 3 Step 6) ✓.

**Type consistency:** `MdCommit`/`md_change_log`/`structural_revisions` signatures are identical across producing and consuming tasks; `Revision` mapping in Task 3 matches the `VaultHistory` arm verbatim.
