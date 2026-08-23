# Engine Auto-Commit & Versioning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move commit policy and message generation into the engine so every edit source (client API, external editor, collab) lands as one commit per sealed session with a deterministic, human-readable message; add named versions (git-tag bookmarks) and the contract seam the UI mirrors.

**Architecture:** Hexagonal, dependencies inward. New `Vcs` port methods (`pending_summary`, `commit_summary`, `name_version`, `named_versions`) implemented by `GitVcs` (git2). A pure message generator in `cairn-app`. `Engine::commit` takes `Option<&str>` and self-guards empty trees. The daemon replaces the watcher-only coalescer with a source-agnostic seal loop (idle 2 s / backstop 20 min / explicit seal). Contract changes are all additive/serde-defaulted.

**Tech Stack:** Rust 1.88 workspace, git2 0.21, serde, ts-rs 11 (bindings emitted by `cargo test -p cairn-contract`), thiserror at boundaries, `forbid(unsafe_code)`.

**Spec:** `docs/superpowers/specs/2026-08-22-engine-auto-commit-versioning-design.md`

## Global Constraints

- Verification commands: `just fmt`, `just lint` (clippy `-D warnings`), `just test` (nextest, workspace, all targets). Every task must leave all three green.
- Conventional commits, imperative, scoped (e.g. `feat(infra): …`). Commit at the end of every task.
- `#![forbid(unsafe_code)]` workspace-wide; thiserror at boundaries (`PortError`, `ServiceError`), no `anyhow` in library crates.
- Contract changes must be additive: existing JSON payloads (string `message`, `Revision` without `summary`/`name`) must keep deserializing. Doc comments on all public items (workspace lints require them).
- Word counting = `str::split_whitespace` token count (Unicode whitespace). Minus sign in messages is U+2212 `−` per spec.
- Timestamps never appear in commit subjects.
- Known flaky tests to ignore (pre-existing, not yours): `invoke_times_out_and_kills_plugin`, `cairn-sandbox-win::exec_and_pipe_stdout`.

---

### Task 1: E1 — diff-summary types (ports) + `GitVcs::pending_summary` / `commit_summary`

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (Vcs trait, ~line 257; new types near `Revision`, ~line 228)
- Modify: `crates/cairn-infra/src/git.rs`
- Modify: `crates/cairn-app/src/lib.rs:2367` (`CountingVcs` test double gains the new methods)
- Test: inline `#[cfg(test)]` in `crates/cairn-infra/src/git.rs`

**Interfaces:**
- Produces (later tasks rely on these exact names):
  - `cairn_ports::{ChangeOp, NoteChange, DiffSummary}`
  - `Vcs::pending_summary(&self) -> Result<DiffSummary, PortError>`
  - `Vcs::commit_summary(&self, revision: &str) -> Result<DiffSummary, PortError>`

- [ ] **Step 1: Add the port types and trait methods**

In `crates/cairn-ports/src/lib.rs`, next to `Revision`:

```rust
/// How a seal/commit changed one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeOp {
    /// File is new.
    Add,
    /// File content changed.
    Edit,
    /// File moved; `from` is the old relative path.
    Rename {
        /// Previous relative path.
        from: String,
    },
    /// File was removed.
    Delete,
}

/// One file's change within a [`DiffSummary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteChange {
    /// Relative path (new path for renames).
    pub path: String,
    /// Operation class.
    pub op: ChangeOp,
    /// Display title: frontmatter `title:` → first `# ` heading → file stem.
    /// Derived from the new content (old content for `Delete`).
    pub title: String,
    /// Nearest markdown heading at/above the first changed line of the new
    /// content. `None` for `Add`/`Delete` or when no heading precedes the change.
    pub heading: Option<String>,
    /// Unicode-whitespace-split tokens on added diff lines.
    pub words_added: u32,
    /// Unicode-whitespace-split tokens on removed diff lines.
    pub words_removed: u32,
}

/// What a commit (or the pending working tree) changed, in note terms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSummary {
    /// Per-file changes. Empty ⇒ the tree is byte-identical (nothing to commit).
    pub changes: Vec<NoteChange>,
    /// Total added words across `changes`.
    pub words_added: u32,
    /// Total removed words across `changes`.
    pub words_removed: u32,
}
```

In `trait Vcs` add:

```rust
    /// Summarize the working tree vs HEAD — what a seal would commit. Empty
    /// `changes` ⇒ nothing to commit. Untracked files count as `Add`; renames
    /// are detected.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a git failure.
    fn pending_summary(&self) -> Result<DiffSummary, PortError>;

    /// Summarize `revision` vs its first parent (vs the empty tree for the
    /// root commit).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revspec does not resolve;
    /// [`PortError::Adapter`] on a git failure.
    fn commit_summary(&self, revision: &str) -> Result<DiffSummary, PortError>;
```

- [ ] **Step 2: Satisfy the `CountingVcs` test double in `crates/cairn-app/src/lib.rs:2367`**

```rust
        fn pending_summary(&self) -> Result<cairn_ports::DiffSummary, PortError> {
            Ok(cairn_ports::DiffSummary::default())
        }
        fn commit_summary(&self, _revision: &str) -> Result<cairn_ports::DiffSummary, PortError> {
            Ok(cairn_ports::DiffSummary::default())
        }
```

(Adjust to the double's existing style; if its tests later need a dirty tree, they'll override — Task 4 revisits.)

- [ ] **Step 3: Write failing tests in `crates/cairn-infra/src/git.rs`**

```rust
    fn word_summary(vcs: &GitVcs) -> DiffSummary {
        vcs.pending_summary().unwrap()
    }

    #[test]
    fn pending_summary_empty_on_clean_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(word_summary(&vcs).changes.is_empty(), "fresh repo");
        fs::write(tmp.path().join("a.md"), "one two").unwrap();
        vcs.commit_all("c1").unwrap();
        assert!(word_summary(&vcs).changes.is_empty(), "clean after commit");
    }

    #[test]
    fn pending_summary_classifies_add_edit_delete_with_word_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "alpha beta\n").unwrap();
        let s = word_summary(&vcs);
        assert_eq!(s.changes.len(), 1);
        assert_eq!(s.changes[0].op, ChangeOp::Add);
        assert_eq!(s.changes[0].title, "a");
        assert_eq!((s.words_added, s.words_removed), (2, 0));

        vcs.commit_all("c1").unwrap();
        fs::write(tmp.path().join("a.md"), "alpha gamma delta\n").unwrap();
        let s = word_summary(&vcs);
        assert_eq!(s.changes[0].op, ChangeOp::Edit);
        // Line-level diff: the whole line is replaced (3 added, 2 removed words).
        assert_eq!((s.words_added, s.words_removed), (3, 2));

        vcs.commit_all("c2").unwrap();
        fs::remove_file(tmp.path().join("a.md")).unwrap();
        let s = word_summary(&vcs);
        assert_eq!(s.changes[0].op, ChangeOp::Delete);
        assert_eq!(s.changes[0].title, "a", "delete title from old blob/stem");
    }

    #[test]
    fn pending_summary_detects_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        // Enough identical content for git2 similarity detection.
        let body = "# Title\n\n".to_string() + &"stable content line here\n".repeat(20);
        fs::write(tmp.path().join("old.md"), &body).unwrap();
        vcs.commit_all("c1").unwrap();
        fs::rename(tmp.path().join("old.md"), tmp.path().join("new.md")).unwrap();
        let s = word_summary(&vcs);
        assert_eq!(s.changes.len(), 1);
        assert_eq!(
            s.changes[0].op,
            ChangeOp::Rename { from: "old.md".into() }
        );
        assert_eq!(s.changes[0].path, "new.md");
    }

    #[test]
    fn pending_summary_title_prefers_frontmatter_then_heading() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(
            tmp.path().join("a.md"),
            "---\ntitle: Q3 Roadmap\n---\n# Ignored\nbody\n",
        )
        .unwrap();
        fs::write(tmp.path().join("b.md"), "# Budget\nbody\n").unwrap();
        fs::write(tmp.path().join("c.md"), "no heading\n").unwrap();
        let s = vcs.pending_summary().unwrap();
        let title = |p: &str| {
            s.changes.iter().find(|c| c.path == p).unwrap().title.clone()
        };
        assert_eq!(title("a.md"), "Q3 Roadmap");
        assert_eq!(title("b.md"), "Budget");
        assert_eq!(title("c.md"), "c");
    }

    #[test]
    fn pending_summary_heading_is_nearest_above_first_change_on_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        let v1 = "# Doc\n\n## Goals\n\nold goals text\n\n## Risks\n\nrisk text\n";
        fs::write(tmp.path().join("a.md"), v1).unwrap();
        vcs.commit_all("c1").unwrap();
        let v2 = "# Doc\n\n## Goals\n\nnew goals text expanded a lot\n\n## Risks\n\nrisk text\n";
        fs::write(tmp.path().join("a.md"), v2).unwrap();
        let s = vcs.pending_summary().unwrap();
        assert_eq!(s.changes[0].heading.as_deref(), Some("Goals"));
        assert!(matches!(s.changes[0].op, ChangeOp::Edit));
    }

    #[test]
    fn pending_summary_non_md_counts_zero_words() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("img.png"), [0u8, 1, 2]).unwrap();
        let s = vcs.pending_summary().unwrap();
        assert_eq!(s.changes.len(), 1);
        assert_eq!(s.changes[0].title, "img");
        assert_eq!((s.words_added, s.words_removed), (0, 0));
    }

    #[test]
    fn commit_summary_diffs_against_first_parent_and_root_against_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "one two three\n").unwrap();
        let c1 = vcs.commit_all("c1").unwrap();
        fs::write(tmp.path().join("a.md"), "one two three four five\n").unwrap();
        let c2 = vcs.commit_all("c2").unwrap();

        let root = vcs.commit_summary(&c1).unwrap();
        assert_eq!(root.changes[0].op, ChangeOp::Add);
        assert_eq!(root.words_added, 3);

        let s2 = vcs.commit_summary(&c2).unwrap();
        assert_eq!(s2.changes[0].op, ChangeOp::Edit);
        assert_eq!((s2.words_added, s2.words_removed), (5, 3));

        assert!(matches!(
            vcs.commit_summary("nope"),
            Err(PortError::NotFound(_))
        ));
    }
```

Add `use cairn_ports::{ChangeOp, DiffSummary};` to the test imports.

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo nextest run -p cairn-infra pending_summary commit_summary`
Expected: compile error (methods not implemented) — that is the failure signal for trait additions.

- [ ] **Step 5: Implement in `GitVcs`**

Private helpers in `crates/cairn-infra/src/git.rs`:

```rust
/// Unicode-whitespace-split token count.
fn word_count(s: &str) -> u32 {
    u32::try_from(s.split_whitespace().count()).unwrap_or(u32::MAX)
}

/// Display title: frontmatter `title:` → first `# ` heading → file stem.
fn title_from_content(content: Option<&str>, path: &Path) -> String {
    if let Some(c) = content {
        if let Some(rest) = c.strip_prefix("---\n") {
            if let Some(end) = rest.find("\n---") {
                for line in rest[..end].lines() {
                    if let Some(t) = line.strip_prefix("title:") {
                        let t = t.trim();
                        if !t.is_empty() {
                            return t.to_string();
                        }
                    }
                }
            }
        }
        for line in c.lines() {
            if let Some(h) = line.strip_prefix("# ") {
                return h.trim().to_string();
            }
        }
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Nearest `#`-prefixed heading at/above 1-based `line` in `content`.
fn heading_above(content: &str, line: u32) -> Option<String> {
    content
        .lines()
        .take(line as usize)
        .filter(|l| l.starts_with('#'))
        .next_back()
        .map(|l| l.trim_start_matches('#').trim().to_string())
}
```

Core summarizer (shared by both methods). `read_new`/`read_old` fetch full file content by path (workdir/blobs), returning `None` for binary/absent:

```rust
fn summarize_diff(
    diff: &git2::Diff,
    read_new: &dyn Fn(&Path) -> Option<String>,
    read_old: &dyn Fn(&Path) -> Option<String>,
) -> Result<DiffSummary, git2::Error> {
    use std::cell::RefCell;
    struct Acc {
        path: PathBuf,
        op: ChangeOp,
        old_path: Option<PathBuf>,
        words_added: u32,
        words_removed: u32,
        first_changed_new_line: Option<u32>,
    }
    let files: RefCell<Vec<Acc>> = RefCell::new(Vec::new());
    diff.foreach(
        &mut |delta, _| {
            let op = match delta.status() {
                git2::Delta::Added | git2::Delta::Untracked => ChangeOp::Add,
                git2::Delta::Deleted => ChangeOp::Delete,
                git2::Delta::Renamed => ChangeOp::Rename {
                    from: delta
                        .old_file()
                        .path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                },
                _ => ChangeOp::Edit,
            };
            let path = match op {
                ChangeOp::Delete => delta.old_file().path(),
                _ => delta.new_file().path(),
            }
            .map(Path::to_path_buf)
            .unwrap_or_default();
            files.borrow_mut().push(Acc {
                path,
                op,
                old_path: delta.old_file().path().map(Path::to_path_buf),
                words_added: 0,
                words_removed: 0,
                first_changed_new_line: None,
            });
            true
        },
        None,
        None,
        Some(&mut |_, _, line| {
            let mut fs = files.borrow_mut();
            if let Some(acc) = fs.last_mut() {
                let is_md = acc.path.extension().is_some_and(|e| e == "md");
                match line.origin() {
                    '+' => {
                        if is_md {
                            acc.words_added +=
                                word_count(&String::from_utf8_lossy(line.content()));
                        }
                        if acc.first_changed_new_line.is_none() {
                            acc.first_changed_new_line = line.new_lineno();
                        }
                    }
                    '-' => {
                        if is_md {
                            acc.words_removed +=
                                word_count(&String::from_utf8_lossy(line.content()));
                        }
                        // A pure deletion still locates the change site.
                        if acc.first_changed_new_line.is_none() {
                            acc.first_changed_new_line = line.old_lineno();
                        }
                    }
                    _ => {}
                }
            }
            true
        }),
    )?;

    let mut summary = DiffSummary::default();
    for acc in files.into_inner() {
        let (content_for_title, heading) = match &acc.op {
            ChangeOp::Delete => (read_old(&acc.path), None),
            ChangeOp::Add => (read_new(&acc.path), None),
            _ => {
                let new = read_new(&acc.path);
                let heading = match (&new, acc.first_changed_new_line) {
                    (Some(c), Some(l)) => heading_above(c, l),
                    _ => None,
                };
                // Rename titles come from the new content too.
                let _ = &acc.old_path;
                (new, heading)
            }
        };
        let title = title_from_content(content_for_title.as_deref(), &acc.path);
        summary.words_added += acc.words_added;
        summary.words_removed += acc.words_removed;
        summary.changes.push(NoteChange {
            path: acc.path.to_string_lossy().into_owned(),
            op: acc.op,
            title,
            heading,
            words_added: acc.words_added,
            words_removed: acc.words_removed,
        });
    }
    Ok(summary)
}
```

Trait impls:

```rust
    fn pending_summary(&self) -> Result<DiffSummary, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = git2::DiffOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);
        let mut diff = repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
            .map_err(adapt)?;
        let mut find = git2::DiffFindOptions::new();
        find.renames(true);
        diff.find_similar(Some(&mut find)).map_err(adapt)?;
        let root = self.root.clone();
        let read_new = move |p: &Path| std::fs::read_to_string(root.join(p)).ok();
        let read_old = |p: &Path| {
            head_tree
                .as_ref()
                .and_then(|t| t.get_path(p).ok())
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_blob().ok())
                .map(|b| String::from_utf8_lossy(b.content()).into_owned())
        };
        summarize_diff(&diff, &read_new, &read_old).map_err(adapt)
    }

    fn commit_summary(&self, revision: &str) -> Result<DiffSummary, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let commit = repo
            .revparse_single(revision)
            .and_then(|o| o.peel_to_commit())
            .map_err(|_| PortError::NotFound(format!("revision {revision}")))?;
        let new_tree = commit.tree().map_err(adapt)?;
        let old_tree = match commit.parent(0) {
            Ok(p) => Some(p.tree().map_err(adapt)?),
            Err(_) => None, // root: diff against the empty tree
        };
        let mut diff = repo
            .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
            .map_err(adapt)?;
        let mut find = git2::DiffFindOptions::new();
        find.renames(true);
        diff.find_similar(Some(&mut find)).map_err(adapt)?;
        let blob_of = |tree: Option<&git2::Tree>, p: &Path| {
            tree.and_then(|t| t.get_path(p).ok())
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_blob().ok())
                .map(|b| String::from_utf8_lossy(b.content()).into_owned())
        };
        let read_new = |p: &Path| blob_of(Some(&new_tree), p);
        let read_old = |p: &Path| blob_of(old_tree.as_ref(), p);
        summarize_diff(&diff, &read_new, &read_old).map_err(adapt)
    }
```

Note: borrow-checker friction around the closures capturing `repo`/trees is expected — restructure locally (e.g. inline `blob_of` per closure) as needed; keep the signatures.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-infra -p cairn-app`
Expected: PASS (including all pre-existing git.rs tests).

- [ ] **Step 7: Full gate + commit**

Run: `just fmt && just lint && just test`

```bash
git add crates/cairn-ports crates/cairn-infra crates/cairn-app
git commit -m "feat(ports,infra): diff summary port — pending/commit summaries with word counts, op class, titles, headings"
```

---

### Task 2: E2 — pure commit-message generator (`cairn-app`)

**Files:**
- Create: `crates/cairn-app/src/commit_msg.rs`
- Modify: `crates/cairn-app/src/lib.rs` (add `pub mod commit_msg;` — top of file with the other mod/use decls)
- Test: inline in `crates/cairn-app/src/commit_msg.rs`

**Interfaces:**
- Consumes: `cairn_ports::{DiffSummary, NoteChange, ChangeOp}` (Task 1)
- Produces: `cairn_app::commit_msg::commit_message(summary: &DiffSummary) -> String`

- [ ] **Step 1: Write the failing tests**

Create `crates/cairn-app/src/commit_msg.rs`:

```rust
//! Deterministic commit-subject generation from a [`DiffSummary`]. Pure — no
//! I/O — so history stays reproducible from a known diff.

use cairn_ports::{ChangeOp, DiffSummary, NoteChange};

#[cfg(test)]
mod tests {
    use super::*;

    fn change(op: ChangeOp, title: &str, added: u32, removed: u32) -> NoteChange {
        NoteChange {
            path: format!("{title}.md"),
            op,
            title: title.into(),
            heading: None,
            words_added: added,
            words_removed: removed,
        }
    }

    fn summary(changes: Vec<NoteChange>) -> DiffSummary {
        let (a, r) = changes
            .iter()
            .fold((0, 0), |(a, r), c| (a + c.words_added, r + c.words_removed));
        DiffSummary { changes, words_added: a, words_removed: r }
    }

    #[test]
    fn single_edit_with_heading_and_both_counts() {
        let mut c = change(ChangeOp::Edit, "Q3 Roadmap", 112, 8);
        c.heading = Some("Goals".into());
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Edit \"Q3 Roadmap\" § Goals (+112/−8 words)"
        );
    }

    #[test]
    fn single_edit_elides_zero_sides() {
        let c = change(ChangeOp::Edit, "A", 112, 0);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\" (+112 words)");
        let c = change(ChangeOp::Edit, "A", 0, 8);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\" (−8 words)");
        let c = change(ChangeOp::Edit, "A", 0, 0);
        assert_eq!(commit_message(&summary(vec![c])), "Edit \"A\"");
    }

    #[test]
    fn single_add_and_delete() {
        let c = change(ChangeOp::Add, "Meeting 2026-08-22", 540, 0);
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Add \"Meeting 2026-08-22\" (+540 words)"
        );
        // Delete never shows counts.
        let c = change(ChangeOp::Delete, "Scratch", 0, 300);
        assert_eq!(commit_message(&summary(vec![c])), "Delete \"Scratch\"");
    }

    #[test]
    fn single_rename_shows_old_stem_and_counts_only_if_content_changed() {
        let mut c = change(ChangeOp::Rename { from: "old-name.md".into() }, "New Name", 0, 0);
        assert_eq!(
            commit_message(&summary(vec![c.clone()])),
            "Rename \"old-name\" → \"New Name\""
        );
        c.words_added = 4;
        assert_eq!(
            commit_message(&summary(vec![c])),
            "Rename \"old-name\" → \"New Name\" (+4 words)"
        );
    }

    #[test]
    fn multi_note_rollup_caps_titles_at_three() {
        let s = summary(vec![
            change(ChangeOp::Edit, "A", 1, 0),
            change(ChangeOp::Add, "B", 2, 0),
            change(ChangeOp::Edit, "C", 3, 0),
        ]);
        assert_eq!(commit_message(&s), "Update 3 notes: \"A\", \"B\", \"C\"");
        let s = summary(vec![
            change(ChangeOp::Edit, "A", 1, 0),
            change(ChangeOp::Edit, "B", 1, 0),
            change(ChangeOp::Edit, "C", 1, 0),
            change(ChangeOp::Edit, "D", 1, 0),
        ]);
        assert_eq!(commit_message(&s), "Update 4 notes: \"A\", \"B\", \"C\"…");
    }

    #[test]
    fn empty_summary_is_checkpoint() {
        assert_eq!(commit_message(&DiffSummary::default()), "Checkpoint");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p cairn-app commit_msg`
Expected: compile FAIL — `commit_message` not defined.

- [ ] **Step 3: Implement**

Above the tests in `commit_msg.rs`:

```rust
/// Render the counts suffix, eliding zero sides. Empty when both are zero.
fn counts(added: u32, removed: u32) -> String {
    match (added, removed) {
        (0, 0) => String::new(),
        (a, 0) => format!(" (+{a} words)"),
        (0, r) => format!(" (−{r} words)"),
        (a, r) => format!(" (+{a}/−{r} words)"),
    }
}

/// Deterministic commit subject for `summary`. Never includes timestamps.
#[must_use]
pub fn commit_message(summary: &DiffSummary) -> String {
    match summary.changes.as_slice() {
        [] => "Checkpoint".to_string(),
        [c] => single(c),
        many => {
            let titles: Vec<String> =
                many.iter().take(3).map(|c| format!("\"{}\"", c.title)).collect();
            let ellipsis = if many.len() > 3 { "…" } else { "" };
            format!("Update {} notes: {}{ellipsis}", many.len(), titles.join(", "))
        }
    }
}

fn single(c: &NoteChange) -> String {
    let heading = c
        .heading
        .as_deref()
        .map(|h| format!(" § {h}"))
        .unwrap_or_default();
    match &c.op {
        ChangeOp::Add => format!("Add \"{}\"{}", c.title, counts(c.words_added, 0)),
        ChangeOp::Edit => format!(
            "Edit \"{}\"{heading}{}",
            c.title,
            counts(c.words_added, c.words_removed)
        ),
        ChangeOp::Delete => format!("Delete \"{}\"", c.title),
        ChangeOp::Rename { from } => {
            let old = std::path::Path::new(from)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| from.clone());
            format!(
                "Rename \"{old}\" → \"{}\"{}",
                c.title,
                counts(c.words_added, c.words_removed)
            )
        }
    }
}
```

Add `pub mod commit_msg;` to `crates/cairn-app/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-app commit_msg`
Expected: PASS.

- [ ] **Step 5: Full gate + commit**

Run: `just fmt && just lint && just test`

```bash
git add crates/cairn-app
git commit -m "feat(app): deterministic commit-message generator from diff summaries"
```

---

### Task 3: E4 — named versions in the Vcs port + `GitVcs` (tags)

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (Vcs trait)
- Modify: `crates/cairn-infra/src/git.rs`
- Modify: `crates/cairn-app/src/lib.rs:2367` (`CountingVcs`: stub the two methods like Task 1)
- Test: inline in `crates/cairn-infra/src/git.rs`

**Interfaces:**
- Produces:
  - `Vcs::name_version(&mut self, revision: &str, name: &str) -> Result<(), PortError>`
  - `Vcs::named_versions(&self) -> Result<std::collections::HashMap<String, String>, PortError>` — key = **7-char short oid** (matches `Revision.id` for cheap joining), value = exact display name.

- [ ] **Step 1: Add trait methods**

```rust
    /// Create or replace the cairn name for `revision` (annotated tag
    /// `refs/tags/cairn/<slug>`; the tag message holds `name` exactly).
    /// Re-naming an already-named commit replaces its name.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revspec does not resolve;
    /// [`PortError::AlreadyExists`] if `name` already labels a different
    /// commit; [`PortError::Adapter`] on a git failure.
    fn name_version(&mut self, revision: &str, name: &str) -> Result<(), PortError>;

    /// All cairn names: 7-char short commit id → exact display name.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a git failure.
    fn named_versions(&self) -> Result<std::collections::HashMap<String, String>, PortError>;
```

Stub both in `CountingVcs` (`Ok(())` / `Ok(Default::default())`).

- [ ] **Step 2: Write failing tests in `crates/cairn-infra/src/git.rs`**

```rust
    #[test]
    fn name_version_round_trips_unicode_display_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "x").unwrap();
        let c1 = vcs.commit_all("c1").unwrap();
        vcs.name_version(&c1, "Avant la grande réorg ✨").unwrap();
        let names = vcs.named_versions().unwrap();
        assert_eq!(names.get(&c1).map(String::as_str), Some("Avant la grande réorg ✨"));
    }

    #[test]
    fn name_version_replaces_on_same_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "x").unwrap();
        let c1 = vcs.commit_all("c1").unwrap();
        vcs.name_version(&c1, "first").unwrap();
        vcs.name_version(&c1, "second").unwrap();
        let names = vcs.named_versions().unwrap();
        assert_eq!(names.len(), 1, "old tag removed");
        assert_eq!(names.get(&c1).map(String::as_str), Some("second"));
    }

    #[test]
    fn name_version_rejects_reuse_on_different_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "x").unwrap();
        let c1 = vcs.commit_all("c1").unwrap();
        fs::write(tmp.path().join("a.md"), "y").unwrap();
        let c2 = vcs.commit_all("c2").unwrap();
        vcs.name_version(&c1, "milestone").unwrap();
        let err = vcs.name_version(&c2, "milestone").unwrap_err();
        assert!(matches!(err, PortError::AlreadyExists(_)));
        // Same name on the SAME commit is an idempotent success.
        vcs.name_version(&c1, "milestone").unwrap();
    }

    #[test]
    fn name_version_unknown_revision_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(matches!(
            vcs.name_version("nope", "x"),
            Err(PortError::NotFound(_))
        ));
    }

    #[test]
    fn name_version_slug_collision_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "x").unwrap();
        let c1 = vcs.commit_all("c1").unwrap();
        fs::write(tmp.path().join("a.md"), "y").unwrap();
        let c2 = vcs.commit_all("c2").unwrap();
        // Different display names, same slug ("v1!" and "v1?" → "v1").
        vcs.name_version(&c1, "v1!").unwrap();
        vcs.name_version(&c2, "v1?").unwrap();
        let names = vcs.named_versions().unwrap();
        assert_eq!(names.get(&c1).map(String::as_str), Some("v1!"));
        assert_eq!(names.get(&c2).map(String::as_str), Some("v1?"));
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo nextest run -p cairn-infra name_version`
Expected: compile FAIL.

- [ ] **Step 4: Implement in `GitVcs`**

```rust
/// Lowercased, alphanumerics kept, everything else collapsed to single '-',
/// trimmed. Empty (all-symbol) slugs fall back to the commit id.
fn slugify(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { fallback.to_string() } else { out }
}
```

```rust
    fn name_version(&mut self, revision: &str, name: &str) -> Result<(), PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let commit = repo
            .revparse_single(revision)
            .and_then(|o| o.peel_to_commit())
            .map_err(|_| PortError::NotFound(format!("revision {revision}")))?;
        let short = commit.id().to_string()[..7].to_string();

        // Uniqueness of display names + replace-on-same-commit: walk cairn tags.
        let mut to_delete: Vec<String> = Vec::new();
        for r in repo.references_glob("refs/tags/cairn/*").map_err(adapt)? {
            let r = r.map_err(adapt)?;
            let Some(refname) = r.name().map(str::to_string) else { continue };
            let Ok(tag) = r.peel_to_tag() else { continue };
            let existing_name = tag.message().unwrap_or("").trim().to_string();
            let target = tag.target_id().to_string();
            if existing_name == name && !target.starts_with(&short) {
                return Err(PortError::AlreadyExists(format!(
                    "name \"{name}\" already labels commit {}",
                    &target[..7]
                )));
            }
            if target.starts_with(&short) {
                to_delete.push(refname); // replace this commit's old name
            }
        }
        for refname in to_delete {
            repo.find_reference(&refname)
                .and_then(|mut r| r.delete())
                .map_err(adapt)?;
        }

        // Unique ref: suffix -2, -3… on slug collisions with other commits.
        let base = slugify(name, &short);
        let mut tagname = format!("cairn/{base}");
        let mut n = 1;
        while repo
            .find_reference(&format!("refs/tags/{tagname}"))
            .is_ok()
        {
            n += 1;
            tagname = format!("cairn/{base}-{n}");
        }
        let sig = signature_from_config(&repo.config().map_err(adapt)?)?;
        repo.tag(&tagname, commit.as_object(), &sig, name, false)
            .map_err(adapt)?;
        Ok(())
    }

    fn named_versions(&self) -> Result<std::collections::HashMap<String, String>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut out = std::collections::HashMap::new();
        for r in repo.references_glob("refs/tags/cairn/*").map_err(adapt)? {
            let r = r.map_err(adapt)?;
            let Ok(tag) = r.peel_to_tag() else { continue };
            let name = tag.message().unwrap_or("").trim().to_string();
            if name.is_empty() {
                continue;
            }
            out.insert(tag.target_id().to_string()[..7].to_string(), name);
        }
        Ok(out)
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-infra -p cairn-app`
Expected: PASS.

- [ ] **Step 6: Full gate + commit**

Run: `just fmt && just lint && just test`

```bash
git add crates/cairn-ports crates/cairn-infra crates/cairn-app
git commit -m "feat(ports,infra): named versions as annotated cairn/* tags with slug refs"
```

---

### Task 4: Engine seam — `commit(Option<&str>) -> CommitOutcome`, `name_version`, enriched history

**Files:**
- Modify: `crates/cairn-app/src/lib.rs` (`Engine::commit` ~line 878; `note_history` / `vault_history` / `structural_revisions`; new types near `Event`)
- Modify: every `engine.commit(`/`eng.commit(` call site the compiler reports (they are all in-workspace tests plus `cairn-daemon/src/lib.rs`, `cairn-service` tests)
- Test: inline in `crates/cairn-app/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 1–3 (`pending_summary`, `commit_msg::commit_message`, `name_version`, `named_versions`, `commit_summary`).
- Produces:
  - `cairn_app::CommitOutcome { Committed(String), NothingToCommit }`
  - `Engine::commit(&mut self, message: Option<&str>, sink: &mut dyn EventSink) -> Result<CommitOutcome, PortError>`
  - `Engine::name_version(&mut self, commit: &str, name: &str) -> Result<(), PortError>`
  - `cairn_app::EnrichedRevision { revision: cairn_ports::Revision, summary: Option<cairn_ports::DiffSummary>, name: Option<String> }`
  - `Engine::note_history` / `vault_history` / `structural_revisions` now return `Vec<EnrichedRevision>` (same params as before)
  - Enrichment cap constant: `const SUMMARY_CAP: usize = 50;`

- [ ] **Step 1: Write failing tests (cairn-app)**

```rust
    #[test]
    fn commit_none_generates_message_and_skips_clean_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path()); // existing test helper
        let mut ev: Vec<Event> = Vec::new();
        // Clean tree: nothing to commit, no event.
        assert!(matches!(
            eng.commit(None, &mut ev).unwrap(),
            CommitOutcome::NothingToCommit
        ));
        assert!(ev.is_empty(), "no Committed event on a no-op seal");

        let p = cairn_domain::NotePath::new("roadmap.md").unwrap();
        eng.write_note(&p, "# Q3 Roadmap\n\none two three\n", &mut ev)
            .unwrap();
        let CommitOutcome::Committed(id) = eng.commit(None, &mut ev).unwrap() else {
            panic!("dirty tree must commit");
        };
        assert!(ev.contains(&Event::Committed(id.clone())));
        // The generated subject names the note. Word counts are raw
        // whitespace tokens, so the "#" heading marker counts: 6, not 5.
        let hist = eng.vault_history(None).unwrap();
        assert_eq!(hist[0].revision.message, "Add \"Q3 Roadmap\" (+6 words)");
        assert_eq!(hist[0].revision.id, id);
    }

    #[test]
    fn commit_some_keeps_caller_text_but_still_guards_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev: Vec<Event> = Vec::new();
        assert!(matches!(
            eng.commit(Some("explicit"), &mut ev).unwrap(),
            CommitOutcome::NothingToCommit
        ));
        let p = cairn_domain::NotePath::new("a.md").unwrap();
        eng.write_note(&p, "x", &mut ev).unwrap();
        let CommitOutcome::Committed(_) = eng.commit(Some("explicit"), &mut ev).unwrap() else {
            panic!()
        };
        assert_eq!(eng.vault_history(None).unwrap()[0].revision.message, "explicit");
    }

    #[test]
    fn history_rows_carry_summary_and_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev: Vec<Event> = Vec::new();
        let p = cairn_domain::NotePath::new("a.md").unwrap();
        eng.write_note(&p, "one two\n", &mut ev).unwrap();
        let CommitOutcome::Committed(c1) = eng.commit(None, &mut ev).unwrap() else {
            panic!()
        };
        eng.name_version(&c1, "Milestone").unwrap();

        let rows = eng.vault_history(None).unwrap();
        assert_eq!(rows[0].name.as_deref(), Some("Milestone"));
        let s = rows[0].summary.as_ref().expect("newest row enriched");
        assert_eq!(s.changes.len(), 1);
        assert_eq!(s.words_added, 2);

        // note_history and structural_revisions share the enrichment.
        let nh = eng.note_history(&p).unwrap();
        assert_eq!(nh[0].name.as_deref(), Some("Milestone"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p cairn-app commit_none commit_some history_rows_carry`
Expected: compile FAIL (`CommitOutcome` etc. undefined).

- [ ] **Step 3: Implement the engine changes**

In `crates/cairn-app/src/lib.rs`:

```rust
/// Result of a commit request: either a commit was created, or the tree was
/// byte-identical to HEAD and nothing happened. Empty commits are never made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// A commit was created; carries the short id.
    Committed(String),
    /// The working tree matched HEAD.
    NothingToCommit,
}

/// A history row plus engine-computed context: the change summary (newest
/// [`SUMMARY_CAP`] rows only) and the commit's cairn name, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrichedRevision {
    /// The underlying commit row.
    pub revision: cairn_ports::Revision,
    /// What the commit changed. `None` beyond the enrichment cap.
    pub summary: Option<cairn_ports::DiffSummary>,
    /// Named-version label from `refs/tags/cairn/*`.
    pub name: Option<String>,
}

/// Newest-rows cap for per-row diff summaries in unlimited history queries.
const SUMMARY_CAP: usize = 50;
```

Replace `Engine::commit`:

```rust
    /// Commit all changes. `message: None` ⇒ generate the subject from the
    /// pending diff. Never creates an empty commit.
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS fails.
    pub fn commit(
        &mut self,
        message: Option<&str>,
        sink: &mut dyn EventSink,
    ) -> Result<CommitOutcome, PortError> {
        let summary = self.vcs.pending_summary()?;
        if summary.changes.is_empty() {
            return Ok(CommitOutcome::NothingToCommit);
        }
        let msg = match message {
            Some(m) => m.to_string(),
            None => crate::commit_msg::commit_message(&summary),
        };
        let id = self.vcs.commit_all(&msg)?;
        sink.emit(Event::Committed(id.clone()));
        Ok(CommitOutcome::Committed(id))
    }

    /// Name (or rename) a committed version. See `Vcs::name_version`.
    ///
    /// # Errors
    /// [`PortError::NotFound`] for an unknown commit; [`PortError::AlreadyExists`]
    /// when the name labels a different commit.
    pub fn name_version(&mut self, commit: &str, name: &str) -> Result<(), PortError> {
        self.vcs.name_version(commit, name)
    }
```

Enrichment helper + rewire the three history methods (keep their names/params, change the return type to `Vec<EnrichedRevision>`):

```rust
    /// Join names and (capped) summaries onto raw history rows.
    fn enrich(
        &self,
        revs: Vec<cairn_ports::Revision>,
        cap: usize,
    ) -> Result<Vec<EnrichedRevision>, PortError> {
        let names = self.vcs.named_versions()?;
        revs.into_iter()
            .enumerate()
            .map(|(i, revision)| {
                let summary = if i < cap {
                    Some(self.vcs.commit_summary(&revision.id)?)
                } else {
                    None
                };
                let name = names.get(&revision.id).cloned();
                Ok(EnrichedRevision { revision, summary, name })
            })
            .collect()
    }
```

- `vault_history(limit)`: `let cap = limit.map_or(SUMMARY_CAP, |n| n as usize); self.enrich(self.vcs.vault_history(limit)?, cap)`
- `note_history(path)`: `self.enrich(self.vcs.history(path.as_str())?, SUMMARY_CAP)`
- `structural_revisions(limit)`: same cap rule as `vault_history` applied to its existing row collection.

- [ ] **Step 4: Sweep the compiler through the workspace**

Run: `cargo check --workspace --all-targets 2>&1 | head -80`

Fix every caller mechanically — no behavior changes beyond the seam:
- Test callers `eng.commit("msg", …)` → `eng.commit(Some("msg"), …)`; where a returned id is used: `let CommitOutcome::Committed(id) = … else { panic!("expected commit") };`
- `cairn-daemon/src/lib.rs` `commit_external_blocking`: `guard.commit(message, …)` → `guard.commit(Some(message), …)` and drop the now-redundant `has_uncommitted_changes` pre-check comment adjustments are Task 6's job — here only make it compile with identical behavior (`Ok(CommitOutcome::…) => {}` both arms).
- `run_collab_flush_pass`: `guard.commit(&msg, …)` → `guard.commit(Some(&msg), …)`, ignore the outcome as before.
- `cairn-service` mappers consuming the history methods now receive `EnrichedRevision`: map `r.revision.id` etc. and set the new wire fields in Task 5 — for THIS task, service still builds the old wire `Revision { id, message, timestamp_secs, author }` from `r.revision`.
- `cairn-mcp`: history consumers — same `r.revision.*` mechanical fix if it calls the engine directly.

Run: `cargo nextest run -p cairn-app`
Expected: the three new tests PASS; whole workspace compiles.

- [ ] **Step 5: Full gate + commit**

Run: `just fmt && just lint && just test`

```bash
git add -A crates
git commit -m "feat(app): engine-owned commit policy — optional message, empty-tree guard, named versions, enriched history"
```

---

### Task 5: C0 — contract seam + service dispatch + CLI (+ regenerated bindings)

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs` (`Command::Commit` :35, new `Command::NameVersion`, `CommandResponse` :229, `Revision` :497, new `ChangeSummary`)
- Modify: `crates/cairn-service/src/lib.rs` (`dispatch_command` Commit arm :191, history mappers :263-301)
- Modify: `crates/cairn-cli/src/main.rs` (Commit subcommand :130-133, dispatch arm :437-443)
- Modify: `crates/cairn-contract/bindings/*` (regenerated)
- Test: inline in contract + service

**Interfaces:**
- Consumes: Task 4 (`CommitOutcome`, `EnrichedRevision`, `Engine::name_version`).
- Produces (wire, mirrored by the UI track):
  - `Command::Commit { message: Option<String> }` — `None` ⇒ engine generates ("seal now")
  - `Command::NameVersion { commit: String, name: String }` → `CommandResponse::Done`
  - `CommandResponse::NothingToCommit` (tag `nothing_to_commit`)
  - `ChangeSummary { files_changed: u32, words_added: u32, words_removed: u32 }`
  - `Revision` + `summary: Option<ChangeSummary>` + `name: Option<String>` (both `#[serde(default)]`)

- [ ] **Step 1: Write failing contract serde tests**

In `crates/cairn-contract/src/lib.rs` tests:

```rust
    #[test]
    fn commit_message_is_optional_and_legacy_string_still_parses() {
        // Legacy payload (message as string) must keep deserializing.
        let legacy = r#"{"type":"commit","message":"hi"}"#;
        let c: Command = serde_json::from_str(legacy).unwrap();
        assert_eq!(c, Command::Commit { message: Some("hi".into()) });
        // Seal-now forms: explicit null and absent.
        let sealed: Command = serde_json::from_str(r#"{"type":"commit","message":null}"#).unwrap();
        assert_eq!(sealed, Command::Commit { message: None });
        let absent: Command = serde_json::from_str(r#"{"type":"commit"}"#).unwrap();
        assert_eq!(absent, Command::Commit { message: None });
    }

    #[test]
    fn name_version_command_and_nothing_to_commit_roundtrip() {
        let cmd = Command::NameVersion { commit: "ab12f3e".into(), name: "Milestone".into() };
        let j = serde_json::to_string(&cmd).unwrap();
        assert!(j.contains("\"type\":\"name_version\""));
        assert_eq!(serde_json::from_str::<Command>(&j).unwrap(), cmd);

        let r = CommandResponse::NothingToCommit;
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"nothing_to_commit\""));
        assert_eq!(serde_json::from_str::<CommandResponse>(&j).unwrap(), r);
    }

    #[test]
    fn revision_new_fields_default_and_roundtrip() {
        // Legacy payload without the new fields must parse.
        let legacy = r#"{"id":"ab12f3e","message":"m","timestamp_secs":1,"author":"a"}"#;
        let r: Revision = serde_json::from_str(legacy).unwrap();
        assert_eq!(r.summary, None);
        assert_eq!(r.name, None);
        // Enriched round-trip.
        let full = Revision {
            id: "ab12f3e".into(),
            message: "Edit \"A\" (+2 words)".into(),
            timestamp_secs: 1,
            author: "a".into(),
            summary: Some(ChangeSummary { files_changed: 1, words_added: 2, words_removed: 0 }),
            name: Some("Milestone".into()),
        };
        let j = serde_json::to_string(&full).unwrap();
        assert_eq!(serde_json::from_str::<Revision>(&j).unwrap(), full);
    }
```

- [ ] **Step 2: Run to verify failure, then implement the contract types**

Run: `cargo nextest run -p cairn-contract`
Expected: compile FAIL. Then:

```rust
    /// Commit all changes. With no message the engine generates one from the
    /// pending diff — this is the "seal now" gesture.
    Commit {
        /// Commit message; `None`/absent ⇒ engine-generated.
        #[serde(default)]
        message: Option<String>,
    },
    /// Label a commit as a named version (replaces the commit's prior name;
    /// reusing a name held by a different commit is invalid).
    NameVersion {
        /// Commit id (short or full) to label.
        commit: String,
        /// Display name, any string.
        name: String,
    },
```

`CommandResponse` gains (doc comment as in spec):

```rust
    /// Commit requested but the working tree matched HEAD; nothing was created.
    NothingToCommit,
```

New struct + `Revision` fields:

```rust
/// What a commit changed, in note terms (engine-computed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChangeSummary {
    /// Number of files touched.
    pub files_changed: u32,
    /// Unicode-whitespace-split words added.
    pub words_added: u32,
    /// Unicode-whitespace-split words removed.
    pub words_removed: u32,
}
```

On `Revision`:

```rust
    /// Change summary; `None` where the engine skipped computing it.
    #[serde(default)]
    pub summary: Option<ChangeSummary>,
    /// Named-version label, if this commit carries one.
    #[serde(default)]
    pub name: Option<String>,
```

- [ ] **Step 3: Wire the service (failing tests first)**

In `crates/cairn-service/src/lib.rs` tests:

```rust
    #[test]
    fn commit_none_seals_and_clean_tree_is_nothing_to_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        assert_eq!(
            dispatch_command(&mut eng, &Command::Commit { message: None }, &mut sink).unwrap(),
            CommandResponse::NothingToCommit
        );
        dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "a.md".into(), contents: "# A\n\nhi there\n".into() },
            &mut sink,
        )
        .unwrap();
        let resp =
            dispatch_command(&mut eng, &Command::Commit { message: None }, &mut sink).unwrap();
        assert!(matches!(resp, CommandResponse::Committed { .. }));
    }

    #[test]
    fn name_version_dispatch_and_history_carries_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "a.md".into(), contents: "one two".into() },
            &mut sink,
        )
        .unwrap();
        let CommandResponse::Committed { commit } =
            dispatch_command(&mut eng, &Command::Commit { message: None }, &mut sink).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            dispatch_command(
                &mut eng,
                &Command::NameVersion { commit: commit.clone(), name: "Milestone".into() },
                &mut sink,
            )
            .unwrap(),
            CommandResponse::Done
        );
        // Reuse on a different commit → InvalidRequest.
        dispatch_command(
            &mut eng,
            &Command::WriteNote { path: "a.md".into(), contents: "three".into() },
            &mut sink,
        )
        .unwrap();
        let CommandResponse::Committed { commit: c2 } =
            dispatch_command(&mut eng, &Command::Commit { message: None }, &mut sink).unwrap()
        else {
            panic!()
        };
        let err = dispatch_command(
            &mut eng,
            &Command::NameVersion { commit: c2, name: "Milestone".into() },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidRequest(_)));

        // History rows carry name + summary on the wire.
        let QueryResponse::History { revisions } =
            dispatch_query(&eng, &Query::VaultHistory { limit: None }).unwrap()
        else {
            panic!()
        };
        let named = revisions.iter().find(|r| r.id == commit).unwrap();
        assert_eq!(named.name.as_deref(), Some("Milestone"));
        let s = named.summary.as_ref().unwrap();
        assert_eq!(s.files_changed, 1);
        assert_eq!(s.words_added, 2);
    }
```

Implementation — dispatch arms:

```rust
        Command::Commit { message } => match engine.commit(message.as_deref(), sink)? {
            cairn_app::CommitOutcome::Committed(commit) => {
                Ok(CommandResponse::Committed { commit })
            }
            cairn_app::CommitOutcome::NothingToCommit => Ok(CommandResponse::NothingToCommit),
        },
        Command::NameVersion { commit, name } => {
            engine.name_version(commit, name)?;
            Ok(CommandResponse::Done)
        }
```

Shared history-row mapper (replaces the three inline `.map(|r| Revision {...})` blocks):

```rust
fn revision_to_wire(r: cairn_app::EnrichedRevision) -> Revision {
    Revision {
        id: r.revision.id,
        message: r.revision.message,
        timestamp_secs: r.revision.timestamp_secs,
        author: r.revision.author,
        summary: r.summary.map(|s| cairn_contract::ChangeSummary {
            files_changed: u32::try_from(s.changes.len()).unwrap_or(u32::MAX),
            words_added: s.words_added,
            words_removed: s.words_removed,
        }),
        name: r.name,
    }
}
```

Also update pre-existing service tests that construct `Command::Commit { message: "x".into() }` → `message: Some("x".into())`, and any asserting `Committed` after a no-change commit (empty commits no longer happen — adjust those tests to write first or expect `NothingToCommit`). The `cairn-infra` test `init_and_commit_a_file` asserting a second empty commit succeeds stays VALID — `commit_all` (the port primitive) still allows it; policy lives in `Engine::commit`.

- [ ] **Step 4: CLI**

`crates/cairn-cli/src/main.rs`:

```rust
    /// Commit all changes.
    Commit {
        /// Commit message (omit to let the engine generate one).
        message: Option<String>,
    },
```

```rust
        Command::Commit { message } => {
            let resp = dispatch_command(&mut engine, &WireCommand::Commit { message }, &mut events)
                .map_err(|e| e.to_string())?;
            match resp {
                CommandResponse::Committed { commit } => println!("committed {commit}"),
                CommandResponse::NothingToCommit => println!("nothing to commit"),
                _ => {}
            }
        }
```

Fix the CLI test at :758 (`needs_startup_reindex(&Command::Commit { … })`) for the new field shape.

- [ ] **Step 5: Regenerate bindings and verify**

Run: `cargo test -p cairn-contract`
Expected: PASS; `crates/cairn-contract/bindings/` now contains `ChangeSummary.ts` and updated `Command.ts`, `CommandResponse.ts`, `Revision.ts`. Inspect: `Revision.ts` must show `summary: ChangeSummary | null, name: string | null`; `Command.ts` must show `message: string | null` (with `#[serde(default)]` + `Option`, ts-rs 11 emits nullable — if it emits `string | undefined` adjust with `#[ts(optional = nullable)]` per ts-rs 11 docs and re-check).

- [ ] **Step 6: Full gate + commit**

Run: `just fmt && just lint && just test`

```bash
git add crates/cairn-contract crates/cairn-service crates/cairn-cli
git commit -m "feat(contract): optional commit message, NameVersion, NothingToCommit, enriched Revision (+ regenerated bindings)"
```

**Post-task note for the human:** this is the C0 publish point — hand `crates/cairn-contract/bindings/{Command,CommandResponse,Revision,ChangeSummary}.ts` to the UI track now.

---

### Task 6: E5 — config: `idle_seconds`, `backstop_minutes`, deprecated `quiet_period_ms`, default flip

**Files:**
- Modify: `crates/cairn-daemon/src/config.rs` (`SyncConfig` :30-64, tests :144-163)
- Modify: `crates/cairn-daemon/src/main.rs` (uses of `quiet_period_ms` :178-216 — switch to the accessor; behavior rewiring is Task 7)
- Test: inline in `config.rs`

**Interfaces:**
- Produces: `SyncConfig { auto_commit: bool /* default TRUE */, idle_seconds: Option<u64>, quiet_period_ms: Option<u64>, backstop_minutes: u64, confirm_grace_ms: u64 }` with accessors `SyncConfig::idle(&self) -> std::time::Duration` and `SyncConfig::backstop(&self) -> std::time::Duration`.

- [ ] **Step 1: Write failing tests (replace `sync_defaults_and_overrides`)**

```rust
    #[test]
    fn sync_defaults_on_with_idle_and_backstop() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.sync.auto_commit, "auto-commit ON by default");
        assert_eq!(c.sync.idle(), std::time::Duration::from_secs(2));
        assert_eq!(c.sync.backstop(), std::time::Duration::from_secs(20 * 60));
        assert_eq!(c.sync.confirm_grace_ms, 50);
    }

    #[test]
    fn sync_idle_seconds_overrides_and_wins_over_alias() {
        let c: Config =
            toml::from_str("[sync]\nidle_seconds = 5\nquiet_period_ms = 900").unwrap();
        assert_eq!(c.sync.idle(), std::time::Duration::from_secs(5));
        assert!(c.sync.quiet_period_ms.is_some(), "alias surfaced for the deprecation warning");
    }

    #[test]
    fn sync_quiet_period_ms_alias_still_honored() {
        let c: Config = toml::from_str("[sync]\nquiet_period_ms = 900").unwrap();
        assert_eq!(c.sync.idle(), std::time::Duration::from_millis(900));
        let c: Config = toml::from_str("[sync]\nauto_commit = false\nbackstop_minutes = 45").unwrap();
        assert!(!c.sync.auto_commit);
        assert_eq!(c.sync.backstop(), std::time::Duration::from_secs(45 * 60));
    }
```

Keep `sync_rejects_unknown_key` as-is (it must still pass).

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p cairn-daemon sync_`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
/// Settings for sealing editing sessions into commits (any edit source).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    /// Auto-commit sealed editing sessions. Default `true`.
    #[serde(default = "default_true_sync")]
    pub auto_commit: bool,
    /// Idle seconds with no further change before a session seals. Default 2.
    #[serde(default)]
    pub idle_seconds: Option<u64>,
    /// Deprecated alias for the idle window, in ms. `idle_seconds` wins.
    #[serde(default)]
    pub quiet_period_ms: Option<u64>,
    /// Backstop: a never-idle session seals after this many minutes. Default 20.
    #[serde(default = "default_backstop_minutes")]
    pub backstop_minutes: u64,
    /// Grace (ms) to wait and re-check before honoring a watcher `Removed`,
    /// absorbing the transient gap of a non-atomic / tmp-rename write. Default 50.
    #[serde(default = "default_confirm_grace_ms")]
    pub confirm_grace_ms: u64,
}

impl SyncConfig {
    /// The idle window: `idle_seconds` → `quiet_period_ms` (deprecated) → 2 s.
    #[must_use]
    pub fn idle(&self) -> std::time::Duration {
        if let Some(s) = self.idle_seconds {
            return std::time::Duration::from_secs(s);
        }
        if let Some(ms) = self.quiet_period_ms {
            return std::time::Duration::from_millis(ms);
        }
        std::time::Duration::from_secs(2)
    }

    /// The long-session backstop.
    #[must_use]
    pub fn backstop(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.backstop_minutes * 60)
    }
}

fn default_true_sync() -> bool {
    true
}
fn default_backstop_minutes() -> u64 {
    20
}
```

Update `Default for SyncConfig` accordingly (`auto_commit: true, idle_seconds: None, quiet_period_ms: None, backstop_minutes: 20, confirm_grace_ms: 50`). In `main.rs`, replace `config.sync.quiet_period_ms` reads with `config.sync.idle()` (both the watcher block and the collab debounce), keep behavior otherwise, and log once at startup:

```rust
    if config.sync.quiet_period_ms.is_some() {
        tracing::warn!("sync.quiet_period_ms is deprecated; use idle_seconds");
    }
```

- [ ] **Step 4: Run tests, full gate + commit**

Run: `cargo nextest run -p cairn-daemon && just fmt && just lint && just test`

```bash
git add crates/cairn-daemon
git commit -m "feat(daemon): sync config — idle_seconds/backstop_minutes, auto_commit on by default, quiet_period_ms deprecated"
```

---

### Task 7: E3 — seal loop in `cairn-service` (replaces `Coalescer`/`run_watch_loop_timeout`)

**Files:**
- Modify: `crates/cairn-service/src/lib.rs` (remove `Coalescer` :29-45 and `run_watch_loop_timeout` :51-80 and their tests :1154-1210; add `SealSignal`, `SealTimer`, `run_seal_loop`)
- Test: inline in `crates/cairn-service/src/lib.rs`

**Interfaces:**
- Consumes: nothing new (std only).
- Produces:
  - `cairn_service::SealSignal` (unit-variant enum: `Activity`)
  - `cairn_service::SealTimer::new(idle: Duration, backstop: Duration)`, `.on_activity(now: Instant)`, `.poll(now: Instant) -> SealPoll` where `pub enum SealPoll { Idle, WaitUntil(Instant), SealNow }`
  - `cairn_service::run_seal_loop(rx: &std::sync::mpsc::Receiver<SealSignal>, idle: Duration, backstop: Duration, on_seal: impl FnMut())` — blocking; flushes a pending session on disconnect.
- **Deletes** `run_watch_loop_timeout` (grep confirms `cairn-daemon/src/main.rs` is its only caller; Task 8 rewires it). `run_watch_loop` stays (CLI uses it).

- [ ] **Step 1: Write failing tests**

```rust
    #[test]
    fn seal_timer_idle_and_backstop_decisions() {
        use std::time::{Duration, Instant};
        let idle = Duration::from_secs(2);
        let backstop = Duration::from_secs(60);
        let t0 = Instant::now();
        let mut t = SealTimer::new(idle, backstop);
        assert_eq!(t.poll(t0), SealPoll::Idle, "no session, nothing to wait for");

        t.on_activity(t0);
        // Mid-session: next deadline is idle expiry.
        assert_eq!(t.poll(t0 + Duration::from_secs(1)), SealPoll::WaitUntil(t0 + idle));
        // Idle expiry seals and resets.
        assert_eq!(t.poll(t0 + idle), SealPoll::SealNow);
        assert_eq!(t.poll(t0 + idle), SealPoll::Idle, "sealing resets the session");

        // Never-idle session: activity every second until the backstop hits.
        // After 60 activities the last poll lands at t0+60s = start + backstop.
        let mut t = SealTimer::new(idle, backstop);
        let mut now = t0;
        for _ in 0..60 {
            t.on_activity(now);
            now += Duration::from_secs(1);
        }
        assert_eq!(t.poll(now), SealPoll::SealNow, "backstop seals a marathon session");
    }

    #[test]
    fn seal_loop_seals_after_quiet_and_flushes_on_disconnect() {
        use std::time::Duration;
        let (tx, rx) = std::sync::mpsc::channel();
        let sender = std::thread::spawn(move || {
            tx.send(SealSignal::Activity).unwrap();
            tx.send(SealSignal::Activity).unwrap();
            std::thread::sleep(Duration::from_millis(250));
            // Second burst, then disconnect before it goes idle.
            tx.send(SealSignal::Activity).unwrap();
            drop(tx);
        });
        let mut seals = 0;
        run_seal_loop(
            &rx,
            Duration::from_millis(60),
            Duration::from_secs(3600),
            || seals += 1,
        );
        sender.join().unwrap();
        assert_eq!(seals, 2, "one idle seal + one disconnect flush");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p cairn-service seal_`
Expected: compile FAIL.

- [ ] **Step 3: Implement (and delete the superseded pieces)**

```rust
/// A signal that an editing session saw activity, from any edit source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealSignal {
    /// A note changed (client write, external edit, collab flush…).
    Activity,
}

/// What the seal loop should do now. See [`SealTimer::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealPoll {
    /// No open session; wait for activity indefinitely.
    Idle,
    /// Session open; re-poll at this deadline unless activity arrives first.
    WaitUntil(std::time::Instant),
    /// Seal the session now (idle window or backstop elapsed).
    SealNow,
}

/// Decides when a burst of edits ("session") seals into one commit: after
/// `idle` with no activity, or `backstop` after the session opened even if it
/// never goes idle. Pure decision logic — the timing lives in [`run_seal_loop`].
#[derive(Debug)]
pub struct SealTimer {
    idle: std::time::Duration,
    backstop: std::time::Duration,
    session_start: Option<std::time::Instant>,
    last_activity: Option<std::time::Instant>,
}

impl SealTimer {
    /// A timer with the given idle window and never-idle backstop.
    #[must_use]
    pub fn new(idle: std::time::Duration, backstop: std::time::Duration) -> Self {
        Self { idle, backstop, session_start: None, last_activity: None }
    }

    /// Record activity at `now`, opening a session if none is open.
    pub fn on_activity(&mut self, now: std::time::Instant) {
        self.session_start.get_or_insert(now);
        self.last_activity = Some(now);
    }

    /// The action due at `now`. `SealNow` closes the session (state resets).
    pub fn poll(&mut self, now: std::time::Instant) -> SealPoll {
        let (Some(start), Some(last)) = (self.session_start, self.last_activity) else {
            return SealPoll::Idle;
        };
        let deadline = std::cmp::min(last + self.idle, start + self.backstop);
        if now >= deadline {
            self.session_start = None;
            self.last_activity = None;
            SealPoll::SealNow
        } else {
            SealPoll::WaitUntil(deadline)
        }
    }
}

/// Drive a [`SealTimer`] from a channel of activity signals, invoking `on_seal`
/// once per sealed session (and once on shutdown if a session is open).
/// Blocking — run on a dedicated thread.
pub fn run_seal_loop(
    rx: &std::sync::mpsc::Receiver<SealSignal>,
    idle: std::time::Duration,
    backstop: std::time::Duration,
    mut on_seal: impl FnMut(),
) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Instant;
    let mut timer = SealTimer::new(idle, backstop);
    loop {
        let action = timer.poll(Instant::now());
        let received = match action {
            SealPoll::SealNow => {
                on_seal();
                continue;
            }
            SealPoll::Idle => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            SealPoll::WaitUntil(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                rx.recv_timeout(wait)
            }
        };
        match received {
            Ok(SealSignal::Activity) => timer.on_activity(Instant::now()),
            Err(RecvTimeoutError::Timeout) => {} // next poll() decides
            Err(RecvTimeoutError::Disconnected) => {
                if !matches!(timer.poll(Instant::now()), SealPoll::Idle) {
                    on_seal();
                }
                break;
            }
        }
    }
}
```

Delete `Coalescer`, `run_watch_loop_timeout`, and their three tests (`coalescer_fires_once_per_dirty_burst`, `timeout_loop_flushes_pending_changes_on_disconnect`, `timeout_loop_commits_after_quiet_period`). Note: `cairn-daemon` still references `run_watch_loop_timeout` — the workspace will NOT compile until Task 8's first step; do Tasks 7 and 8 as one review unit if the executor needs a green tree per task, committing after Task 8's gate. (If you prefer a green tree at every commit: fold this task's commit into Task 8's.)

- [ ] **Step 4: Run the new tests**

Run: `cargo nextest run -p cairn-service seal_`
Expected: PASS (daemon may still fail to build — resolved next task).

- [ ] **Step 5: Commit together with Task 8** (see note above).

---

### Task 8: E3 — daemon wiring: every source marks activity; seals use generated messages

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (`AppState` fields/ctor; `run_command_blocking` :185-203; `commit_external_blocking` :279-296 → `seal_blocking`; `run_collab_flush_pass` commit block :355-365)
- Modify: `crates/cairn-daemon/src/main.rs` (watcher block :175-208; collab debounce :211-216)
- Test: Create `crates/cairn-daemon/tests/seal_integration.rs`

**Interfaces:**
- Consumes: Task 7 (`SealSignal`, `run_seal_loop`), Task 6 (`SyncConfig::idle/backstop`), Task 4 (`Engine::commit(None, …)`).
- Produces:
  - `AppState::with_sealer(self, tx: std::sync::mpsc::Sender<cairn_service::SealSignal>) -> Self`
  - `AppState::mark_activity(&self)` (no-op when no sealer is attached)
  - `AppState::seal_blocking(&self)` — commits the pending session with an engine-generated message; best-effort, logs failures.

- [ ] **Step 1: Write the failing integration test**

`crates/cairn-daemon/tests/seal_integration.rs`:

```rust
//! Multi-source coherence: client writes and external edits land as
//! one-commit-per-session with engine-generated messages (spec DoD).

use cairn_contract::{Command, Query, QueryResponse};

fn state(dir: &std::path::Path) -> cairn_daemon::AppState {
    let engine = cairn_startup::build_engine(dir).unwrap();
    cairn_daemon::AppState::new(engine)
}

#[test]
fn client_session_seals_with_generated_message() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    s.run_command_blocking(&Command::WriteNote {
        path: "roadmap.md".into(),
        contents: "# Q3 Roadmap\n\none two three\n".into(),
    })
    .unwrap();
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Add \"Q3 Roadmap\" (+6 words)");
    // Sealing again with no changes creates nothing.
    s.seal_blocking();
    let QueryResponse::History { revisions: again } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(again.len(), revisions.len(), "no empty commit on a clean seal");
}

#[test]
fn external_edit_session_seals_with_generated_message() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    // Simulate the watcher path: file appears on disk, change applied, sealed.
    std::fs::write(tmp.path().join("note.md"), "# Note\n\nalpha beta\n").unwrap();
    s.apply_change_blocking(&cairn_ports::FsChange::Changed(
        cairn_domain::NotePath::new("note.md").unwrap(),
    ));
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Add \"Note\" (+4 words)");
}

#[test]
fn multi_note_session_rolls_up() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    for (p, c) in [("a.md", "# A\nx"), ("b.md", "# B\ny")] {
        s.run_command_blocking(&Command::WriteNote { path: p.into(), contents: c.into() })
            .unwrap();
    }
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Update 2 notes: \"A\", \"B\"");
}
```

(Adjust helper visibility if `apply_change_blocking`/`run_query_blocking` names differ — use the daemon's existing public methods; they exist per `lib.rs`.) Add `cairn-startup`, `cairn-domain`, `cairn-ports`, `tempfile` to `[dev-dependencies]` of `cairn-daemon/Cargo.toml` if missing.

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p cairn-daemon seal_integration`
Expected: FAIL (`seal_blocking` missing; daemon may not compile after Task 7 — implementing this task fixes both).

- [ ] **Step 3: Implement the daemon wiring**

In `AppState`: add field `seal_tx: Option<std::sync::mpsc::Sender<cairn_service::SealSignal>>` (default `None`), builder `with_sealer`, and:

```rust
    /// Signal the seal loop that an editing session saw activity. No-op when
    /// auto-commit is off (no sealer attached).
    pub fn mark_activity(&self) {
        if let Some(tx) = &self.seal_tx {
            let _ = tx.send(cairn_service::SealSignal::Activity);
        }
    }

    /// Seal the pending editing session: commit everything uncommitted with an
    /// engine-generated message. No-ops on a clean tree. Best-effort: a failure
    /// is logged, not propagated. Blocking — call from a blocking context.
    pub fn seal_blocking(&self) {
        let mut guard = self.engine();
        let mut tap = EventTap { tx: self.events.clone(), collected: Vec::new() };
        match guard.commit(None, &mut tap) {
            Ok(_) => {}
            Err(e) => tracing::warn!("seal: auto-commit failed: {e}"),
        }
    }
```

Delete `commit_external_blocking` (superseded; grep for external callers first — `main.rs` only).

`run_command_blocking`: after a successful dispatch, mark activity for mutating commands that are not themselves seals:

```rust
        if result.is_ok()
            && !matches!(command, Command::Commit { .. } | Command::NameVersion { .. })
        {
            self.mark_activity();
        }
```

`run_collab_flush_pass`: replace the `has_uncommitted_changes`/`commit` block (:355-365) with `self.mark_activity();` — the seal loop commits the flushed write with a generated message. Keep `settle_flush` exactly as-is (baseline advance never depended on the commit succeeding).

`main.rs` watcher block — both branches now apply changes and mark activity; the seal loop is a separate thread:

```rust
    let (seal_tx, seal_rx) = std::sync::mpsc::channel();
    let state = /* existing builder chain */
        .with_sealer(seal_tx); // only when config.sync.auto_commit — see below
```

Concretely: build the channel before `AppState`; call `.with_sealer(seal_tx)` on the builder only if `config.sync.auto_commit` (otherwise drop `seal_tx` so the loop exits immediately and `mark_activity` is a no-op); then:

```rust
    if config.sync.auto_commit {
        let idle = config.sync.idle();
        let backstop = config.sync.backstop();
        tracing::info!(
            "seal: auto-committing sessions after {:?} idle ({:?} backstop)",
            idle, backstop
        );
        let sealer = state.clone();
        tokio::task::spawn_blocking(move || {
            cairn_service::run_seal_loop(&seal_rx, idle, backstop, || sealer.seal_blocking());
        });
    }
```

Watcher (replaces the `run_watch_loop_timeout` call — the plain loop is now the only variant):

```rust
    if !cli.no_watch {
        match NotifyWatcher.watch(&cli.cairn) {
            Ok(handle) => {
                let grace = Duration::from_millis(config.sync.confirm_grace_ms);
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    cairn_service::run_watch_loop(&handle, |change| {
                        watch_state.apply_change_confirmed_blocking(change, grace);
                        watch_state.mark_activity();
                    });
                });
                tracing::info!("watching {} for changes", cli.cairn.display());
            }
            Err(e) => tracing::warn!("file watcher disabled: {e}"),
        }
    }
```

Collab tick keeps its own debounce (`config.sync.idle()`), unchanged otherwise. Existing daemon tests referencing `commit_external_blocking` or the old messages ("cairn: sync external edits", "cairn: collab sync") must be updated to `seal_blocking` + generated-message assertions — grep both strings workspace-wide; zero occurrences must remain outside git history.

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p cairn-daemon -p cairn-service`
Expected: PASS, including the three integration tests.

- [ ] **Step 5: Full gate + commit (covers Tasks 7+8)**

Run: `just fmt && just lint && just test`

```bash
git add crates/cairn-service crates/cairn-daemon
git commit -m "feat(daemon,service): source-agnostic session sealing — idle/backstop seal loop, generated messages, auto-commit on"
```

---

### Task 9: Final sweep — spec DoD verification

**Files:**
- Modify: only what the sweep uncovers.

- [ ] **Step 1: Grep for leftovers**

Run: `grep -rn "cairn: sync external edits\|cairn: collab sync\|run_watch_loop_timeout\|Coalescer" crates`
Expected: no matches. Fix any stragglers.

- [ ] **Step 2: DoD checklist against the spec**

- No-op seal: covered by `client_session_seals_with_generated_message` (second seal). ✓
- Push decoupled: `grep -rn "push" crates/cairn-daemon/src crates/cairn-app/src` — confirm no commit path touches push (there is no push in the engine today; confirm it stayed that way). ✓
- NameVersion tags + `Revision.summary`/`name`: service test from Task 5. ✓
- Message generation from a known diff: Task 2 unit tests. ✓
- Seal/backstop/skip decisions: Task 7 `SealTimer` tests. ✓
- Multi-source coherence: Task 8 integration tests. ✓

- [ ] **Step 3: Full workspace gate**

Run: `just fmt && just lint && just test && cargo test --doc --workspace`
Expected: all green (modulo the two known-flaky tests listed in Global Constraints).

- [ ] **Step 4: Commit any sweep fixes**

```bash
git add -A crates
git commit -m "chore: final sweep for engine auto-commit — remove superseded messages and verify DoD"
```

(Skip the commit if the sweep changed nothing.)

---

## Self-review notes

- **Spec coverage:** C0 → Task 5; E1 → Task 1; E2 → Task 2; E3 → Tasks 7–8; E4 → Tasks 3–5; E5 → Task 6; testing section → distributed per task + Task 9. Manual web-app/CLI/external-editor DoD check remains a human step after merge (spec calls it out).
- **Green-tree caveat:** Task 7 deletes `run_watch_loop_timeout` while the daemon still calls it; Tasks 7+8 therefore share one commit/gate. All other task boundaries leave the workspace green.
- **Type consistency:** `EnrichedRevision.revision` is `cairn_ports::Revision` (4 original fields); wire `Revision` (contract) carries the two new fields — the mapper in Task 5 is the only place they meet. `named_versions` keys are 7-char short ids to match `Revision.id`.
