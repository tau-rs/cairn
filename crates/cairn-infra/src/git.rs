//! A `Vcs` adapter over a local git repository using `git2`.

use std::path::{Path, PathBuf};

use cairn_ports::{PortError, Vcs};
use git2::{Repository, Signature};

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
        if Repository::open(&root).is_err() {
            Repository::init(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        Ok(Self { root })
    }
}

impl Vcs for GitVcs {
    fn commit_all(&mut self, message: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(|e| PortError::Adapter(e.to_string()))?;
        let mut index = repo.index().map_err(|e| PortError::Adapter(e.to_string()))?;
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        index.write().map_err(|e| PortError::Adapter(e.to_string()))?;
        let tree_id = index.write_tree().map_err(|e| PortError::Adapter(e.to_string()))?;
        let tree = repo.find_tree(tree_id).map_err(|e| PortError::Adapter(e.to_string()))?;
        let sig = Signature::now("Cairn", "cairn@localhost")
            .map_err(|e| PortError::Adapter(e.to_string()))?;

        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(oid.to_string()[..7].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn init_and_commit_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        let id = vcs.commit_all("first").unwrap();
        assert_eq!(id.len(), 7);
        let id2 = vcs.commit_all("second").unwrap();
        assert_eq!(id2.len(), 7);
    }
}
