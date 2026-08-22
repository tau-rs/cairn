//! A `Vcs` adapter over a local git repository using `git2`.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use cairn_ports::{
    AdapterError, ChangeOp, DiffSummary, HistoricalBlob, MdCommit, NoteChange, PortError, Revision,
    Vcs,
};
use git2::{Repository, Signature};

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

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
        .last()
        .map(|l| l.trim_start_matches('#').trim().to_string())
}

/// Shared diff → [`DiffSummary`] reducer. `read_new`/`read_old` fetch full
/// file content by path (workdir/blobs), returning `None` for binary/absent.
fn summarize_diff(
    diff: &git2::Diff,
    read_new: &dyn Fn(&Path) -> Option<String>,
    read_old: &dyn Fn(&Path) -> Option<String>,
) -> Result<DiffSummary, git2::Error> {
    use std::cell::RefCell;
    struct Acc {
        path: PathBuf,
        op: ChangeOp,
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
                words_added: 0,
                words_removed: 0,
                first_changed_new_line: None,
            });
            true
        },
        None,
        Some(&mut |_, _| true),
        Some(&mut |_, _, line| {
            let mut fs = files.borrow_mut();
            if let Some(acc) = fs.last_mut() {
                let is_md = acc.path.extension().is_some_and(|e| e == "md");
                match line.origin() {
                    '+' => {
                        if is_md {
                            acc.words_added += word_count(&String::from_utf8_lossy(line.content()));
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

/// Whether `commit` added/changed/removed the blob at `path` (vs its parents).
fn commit_touched_path(commit: &git2::Commit, path: &Path) -> Result<bool, git2::Error> {
    let cur = commit.tree()?.get_path(path).ok().map(|e| e.id());
    if commit.parent_count() == 0 {
        return Ok(cur.is_some()); // root commit: touched iff the path exists
    }
    for i in 0..commit.parent_count() {
        let parent = commit.parent(i)?;
        let prev = parent.tree()?.get_path(path).ok().map(|e| e.id());
        if prev != cur {
            return Ok(true);
        }
    }
    Ok(false)
}

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

/// The commit identity for cairn-made commits: the repo's configured git
/// identity (`user.name`/`user.email`, as merged from local/global/system
/// config) when both are set, else a `Cairn` default so a commit can still be
/// made in a repo with no identity configured. Mirrors [`Repository::signature`]
/// but takes the [`git2::Config`] directly so the fallback is testable without
/// depending on the ambient global git identity.
fn signature_from_config(config: &git2::Config) -> Result<Signature<'static>, PortError> {
    match (
        config.get_string("user.name"),
        config.get_string("user.email"),
    ) {
        (Ok(name), Ok(email)) if !name.is_empty() && !email.is_empty() => {
            Signature::now(&name, &email).map_err(adapt)
        }
        _ => Signature::now("Cairn", "cairn@localhost").map_err(adapt),
    }
}

/// Operates on the git repository rooted at `root`.
#[derive(Debug)]
pub struct GitVcs {
    root: PathBuf,
}

impl GitVcs {
    /// Open an existing repository, or initialize one if absent.
    ///
    /// # Errors
    /// Returns [`PortError`] if the repository cannot be opened or created.
    pub fn open_or_init(root: impl AsRef<Path>) -> Result<Self, PortError> {
        let root = root.as_ref().to_path_buf();
        match Repository::open(&root) {
            Ok(_) => {}
            Err(e) if e.code() == git2::ErrorCode::NotFound => {
                Repository::init(&root).map_err(adapt)?;
            }
            Err(e) => return Err(adapt(e)),
        }
        Ok(Self { root })
    }
}

impl Vcs for GitVcs {
    fn commit_all(&mut self, message: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut index = repo.index().map_err(adapt)?;
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(adapt)?;
        index.write().map_err(adapt)?;
        let tree_id = index.write_tree().map_err(adapt)?;
        let tree = repo.find_tree(tree_id).map_err(adapt)?;
        let sig = signature_from_config(&repo.config().map_err(adapt)?)?;

        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .map_err(adapt)?;
        Ok(oid.to_string()[..7].to_string())
    }

    fn is_dirty(&self) -> Result<bool, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut opts = git2::StatusOptions::new();
        opts.include_ignored(false).include_untracked(true);
        let statuses = repo.statuses(Some(&mut opts)).map_err(adapt)?;
        Ok(!statuses.is_empty())
    }

    fn history(&self, path: &str) -> Result<Vec<Revision>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut walk = repo.revwalk().map_err(adapt)?;
        // No HEAD (empty repo) -> no history.
        if walk.push_head().is_err() {
            return Ok(Vec::new());
        }
        // TOPOLOGICAL ensures a child is emitted before its parents (newest
        // first) even when commits share the same timestamp, which is common
        // for note edits made within the same second.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
            .map_err(adapt)?;
        let p = Path::new(path);
        let mut revs = Vec::new();
        for oid in walk {
            let oid = oid.map_err(adapt)?;
            let commit = repo.find_commit(oid).map_err(adapt)?;
            if commit_touched_path(&commit, p).map_err(adapt)? {
                revs.push(Revision {
                    id: oid.to_string()[..7].to_string(),
                    // git2 0.21 made `summary()` fallible (surfaces UTF-8
                    // errors); treat both a UTF-8 error and a missing
                    // summary as an empty message, as before.
                    message: commit.summary().ok().flatten().unwrap_or("").to_string(),
                    timestamp_secs: commit.time().seconds(),
                    author: commit.author().name().unwrap_or("").to_string(),
                });
            }
        }
        Ok(revs)
    }

    fn vault_history(&self, limit: Option<u32>) -> Result<Vec<Revision>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut walk = repo.revwalk().map_err(adapt)?;
        // No HEAD (empty repo) -> no history.
        if walk.push_head().is_err() {
            return Ok(Vec::new());
        }
        // TOPOLOGICAL keeps children before parents (newest first) even when
        // commits share a timestamp — matches `history`'s ordering.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
            .map_err(adapt)?;
        let cap = limit.map(|n| n as usize);
        let mut revs = Vec::new();
        for oid in walk {
            if cap.is_some_and(|n| revs.len() >= n) {
                break;
            }
            let oid = oid.map_err(adapt)?;
            let commit = repo.find_commit(oid).map_err(adapt)?;
            revs.push(Revision {
                id: oid.to_string()[..7].to_string(),
                message: commit.summary().ok().flatten().unwrap_or("").to_string(),
                timestamp_secs: commit.time().seconds(),
                author: commit.author().name().unwrap_or("").to_string(),
            });
        }
        Ok(revs)
    }

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
                if let Ok(name) = entry.name() {
                    if name.ends_with(".md") {
                        if let Ok(blob) = entry.to_object(&repo).and_then(|o| o.peel_to_blob()) {
                            paths.push(format!("{dir}{name}"));
                            contents.push(String::from_utf8_lossy(blob.content()).into_owned());
                        }
                    }
                }
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(adapt)?;

        // 2. One backward revwalk: newest touching-commit time per path.
        let snapshot_secs = commit.time().seconds();
        let mut mtimes: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        if !paths.is_empty() {
            let mut remaining: std::collections::HashSet<String> = paths.iter().cloned().collect();
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
                HistoricalBlob {
                    path,
                    content,
                    mtime_secs,
                }
            })
            .collect())
    }

    fn show(&self, path: &str, revision: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let obj = repo.revparse_single(revision).map_err(adapt)?;
        let tree = obj.peel_to_commit().map_err(adapt)?.tree().map_err(adapt)?;
        let entry = tree
            .get_path(Path::new(path))
            .map_err(|_| PortError::NotFound(format!("{path} at {revision}")))?;
        let blob = entry
            .to_object(&repo)
            .map_err(adapt)?
            .peel_to_blob()
            .map_err(|_| PortError::NotFound(format!("{path} at {revision} is not a file")))?;
        Ok(String::from_utf8_lossy(blob.content()).into_owned())
    }

    fn walk_md_changes(
        &self,
        visit: &mut dyn FnMut(MdCommit) -> Result<ControlFlow<()>, PortError>,
    ) -> Result<(), PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut walk = repo.revwalk().map_err(adapt)?;
        // No HEAD (empty repo) -> no history.
        if walk.push_head().is_err() {
            return Ok(());
        }
        // TOPOLOGICAL keeps children before parents (newest first), matching
        // vault_history even when commits share a timestamp.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
            .map_err(adapt)?;
        for oid in walk {
            let oid = oid.map_err(adapt)?;
            let commit = repo.find_commit(oid).map_err(adapt)?;
            // First parent only: "what this commit changed on the mainline".
            let parent = commit.parent(0).ok();
            let md_changed = tree_has_md_change(&repo, &commit, parent.as_ref()).map_err(adapt)?;
            let mc = MdCommit {
                revision: Revision {
                    id: oid.to_string()[..7].to_string(),
                    message: commit.summary().ok().flatten().unwrap_or("").to_string(),
                    timestamp_secs: commit.time().seconds(),
                    author: commit.author().name().unwrap_or("").to_string(),
                },
                oid: oid.to_string(),
                parent: parent.map(|p| p.id().to_string()),
                md_changed,
            };
            // The caller decides when it has seen enough; stopping here means the
            // revwalk never loads the remaining (older) commits.
            if visit(mc)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    fn pending_summary(&self) -> Result<DiffSummary, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = git2::DiffOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        let mut diff = repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
            .map_err(adapt)?;
        let mut find = git2::DiffFindOptions::new();
        find.renames(true).for_untracked(true);
        diff.find_similar(Some(&mut find)).map_err(adapt)?;
        let root = self.root.clone();
        let read_new = |p: &Path| std::fs::read_to_string(root.join(p)).ok();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::{ChangeOp, DiffSummary};
    use std::fs;

    #[test]
    fn delete_then_commit_empties_tree() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        vcs.commit_all("add a.md").unwrap();

        // Removing the file on disk and committing must stage the removal.
        fs::remove_file(tmp.path().join("a.md")).unwrap();
        vcs.commit_all("remove a.md").unwrap();

        let repo = Repository::open(tmp.path()).unwrap();
        let tree = repo.head().unwrap().peel_to_tree().unwrap();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn is_dirty_tracks_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(!vcs.is_dirty().unwrap(), "fresh repo is clean");
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        assert!(vcs.is_dirty().unwrap(), "untracked file is dirty");
        vcs.commit_all("add a").unwrap();
        assert!(!vcs.is_dirty().unwrap(), "clean after commit");
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        assert!(vcs.is_dirty().unwrap(), "modified tracked file is dirty");
    }

    #[test]
    fn history_lists_commits_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("add a v1").unwrap();
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("update a v2").unwrap();
        // An unrelated note's commit must NOT appear in a.md's history.
        fs::write(tmp.path().join("b.md"), "b").unwrap();
        vcs.commit_all("add b").unwrap();

        let hist = vcs.history("a.md").unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].message, "update a v2"); // newest first
        assert_eq!(hist[1].message, "add a v1");
        assert_eq!(hist[0].id.len(), 7);
    }

    #[test]
    fn history_empty_for_uncommitted() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        // No commits at all.
        assert!(vcs.history("a.md").unwrap().is_empty());
        // A file present but never committed (still no commits in the repo).
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        assert!(vcs.history("a.md").unwrap().is_empty());
    }

    #[test]
    fn vault_history_lists_all_commits_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("add a").unwrap();
        fs::write(tmp.path().join("b.md"), "b").unwrap();
        vcs.commit_all("add b").unwrap();
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("update a").unwrap();

        // Unlike history(path), vault_history spans EVERY commit regardless of path.
        let all = vcs.vault_history(None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].message, "update a"); // newest first
        assert_eq!(all[1].message, "add b");
        assert_eq!(all[2].message, "add a");
        assert_eq!(all[0].id.len(), 7);

        // limit truncates to the newest N.
        let recent = vcs.vault_history(Some(2)).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "update a");
        assert_eq!(recent[1].message, "add b");
    }

    #[test]
    fn vault_history_empty_for_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(vcs.vault_history(None).unwrap().is_empty());
    }

    #[test]
    fn show_returns_content_at_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("v1").unwrap();
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("v2").unwrap();

        let hist = vcs.history("a.md").unwrap();
        let old = hist[1].id.clone(); // the v1 commit
        assert_eq!(vcs.show("a.md", &old).unwrap(), "v1");
        assert_eq!(vcs.show("a.md", "HEAD").unwrap(), "v2");
        // Unknown path at a revision -> NotFound.
        assert!(matches!(
            vcs.show("nope.md", "HEAD"),
            Err(PortError::NotFound(_))
        ));
    }

    #[test]
    fn commit_uses_configured_git_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        // A repo-local identity must override the `Cairn` default so cairn-made
        // commits are attributed to the user.
        let repo = Repository::open(tmp.path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Ada Lovelace").unwrap();
        cfg.set_str("user.email", "ada@example.com").unwrap();

        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        vcs.commit_all("first").unwrap();

        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        // git2 0.21 made signature accessors return `Result<&str, Error>`.
        assert_eq!(commit.author().name().unwrap(), "Ada Lovelace");
        assert_eq!(commit.author().email().unwrap(), "ada@example.com");
        assert_eq!(commit.committer().name().unwrap(), "Ada Lovelace");
        assert_eq!(commit.committer().email().unwrap(), "ada@example.com");
    }

    #[test]
    fn signature_falls_back_to_cairn_default_when_unset() {
        // An empty config (no `user.name`/`user.email` anywhere) must yield the
        // `Cairn` default so a commit can still be made. Tested against an
        // isolated `Config` to avoid depending on the ambient global identity.
        let empty = git2::Config::new().unwrap();
        let sig = signature_from_config(&empty).unwrap();
        assert_eq!(sig.name().unwrap(), "Cairn");
        assert_eq!(sig.email().unwrap(), "cairn@localhost");
    }

    #[test]
    fn init_and_commit_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        let id = vcs.commit_all("first").unwrap();
        assert_eq!(id.len(), 7);
        // A second commit with no changes still succeeds (empty commit).
        let id2 = vcs.commit_all("second").unwrap();
        assert_eq!(id2.len(), 7);
    }

    #[test]
    fn resolve_returns_full_oid_and_notfound_on_bad_revspec() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("v1").unwrap();
        let oid = vcs.resolve("HEAD").unwrap();
        assert_eq!(oid.len(), 40);
        assert!(matches!(
            vcs.resolve("no-such-rev"),
            Err(PortError::NotFound(_))
        ));
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
    fn read_tree_at_mtime_uses_earlier_commit_for_file_unchanged_at_rev() {
        use git2::{Signature, Time};
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        let repo = Repository::open(tmp.path()).unwrap();

        // Commit the current working tree with an explicit committer time.
        let commit_at = |secs: i64, msg: &str| -> git2::Oid {
            let mut index = repo.index().unwrap();
            index
                .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = Signature::new("T", "t@e", &Time::new(secs, 0)).unwrap();
            let parent = repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .and_then(|o| repo.find_commit(o).ok());
            let parents: Vec<&git2::Commit> = parent.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
                .unwrap()
        };

        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        commit_at(1_000, "c1 add a");
        // a.md is NOT modified in c2; only b.md is added.
        fs::write(tmp.path().join("b.md"), "new").unwrap();
        let c2 = commit_at(2_000, "c2 add b");

        let blobs = vcs.read_tree_at(&c2.to_string()).unwrap();
        let a = blobs.iter().find(|b| b.path == "a.md").unwrap();
        let b = blobs.iter().find(|b| b.path == "b.md").unwrap();
        // a.md's last touch is c1 (1000), NOT the snapshot commit c2 (2000).
        assert_eq!(
            a.mtime_secs, 1_000,
            "unchanged file keeps its earlier touch time"
        );
        assert_eq!(b.mtime_secs, 2_000, "new file gets the c2 time");
        assert!(a.mtime_secs < b.mtime_secs);
    }

    #[test]
    fn read_tree_at_notfound_on_bad_revspec() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(matches!(
            vcs.read_tree_at("nope"),
            Err(PortError::NotFound(_))
        ));
    }

    /// Drain `walk_md_changes` into a `Vec`, visiting every commit.
    fn collect_md_changes(vcs: &GitVcs) -> Vec<MdCommit> {
        let mut out = Vec::new();
        vcs.walk_md_changes(&mut |c| {
            out.push(c);
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .unwrap();
        out
    }

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

        let log = collect_md_changes(&vcs);
        assert_eq!(log.len(), 3);
        // Newest-first.
        assert_eq!(log[0].revision.message, "edit a.md");
        assert!(log[0].md_changed);
        assert_eq!(log[1].revision.message, "add config");
        assert!(
            !log[1].md_changed,
            "a commit touching no .md is not a candidate"
        );
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
        assert!(collect_md_changes(&vcs).is_empty());
    }

    #[test]
    fn walk_md_changes_stops_on_break() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        for i in 0..5 {
            fs::write(tmp.path().join(format!("n{i}.md")), "x").unwrap();
            vcs.commit_all(&format!("c{i}")).unwrap();
        }

        // Break on the very first visit: the revwalk must not load any further
        // commit once the visitor asks to stop.
        let mut visited = 0;
        vcs.walk_md_changes(&mut |_c| {
            visited += 1;
            Ok(std::ops::ControlFlow::Break(()))
        })
        .unwrap();
        assert_eq!(visited, 1, "walk halts on the first Break");
    }

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
            ChangeOp::Rename {
                from: "old.md".into()
            }
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
            s.changes
                .iter()
                .find(|c| c.path == p)
                .unwrap()
                .title
                .clone()
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
}
