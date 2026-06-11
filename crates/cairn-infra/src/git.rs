//! A `Vcs` adapter over a local git repository using `git2`.

use std::path::{Path, PathBuf};

use cairn_ports::{AdapterError, PortError, Revision, Vcs};
use git2::{Repository, Signature};

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
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
        let sig = Signature::now("Cairn", "cairn@localhost").map_err(adapt)?;

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
                    message: commit.summary().unwrap_or("").to_string(),
                    timestamp_secs: commit.time().seconds(),
                    author: commit.author().name().unwrap_or("").to_string(),
                });
            }
        }
        Ok(revs)
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
