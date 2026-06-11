//! Composition-root helpers shared by the `cairn` CLI and `cairn-daemon`:
//! detecting an existing cairn and constructing the engine from concrete
//! adapters. Lives outside `cairn-app` so the inner hexagon never depends on
//! `cairn-infra`; this crate is where the concrete adapters are wired.

use std::path::Path;

use cairn_app::Engine;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};

/// Failures starting up against a cairn directory.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// `root` is not an initialized cairn (no `.git`).
    #[error("not a cairn at {path} (run `cairn --cairn {path} init` first)")]
    NotACairn {
        /// The display path that was checked.
        path: String,
    },
    /// A concrete adapter failed to open.
    #[error("{0}")]
    Build(String),
}

/// True if `root` looks like an initialized cairn. `.git` is a directory in a
/// normal repo but a file in worktrees/submodules, so test existence, not type.
#[must_use]
pub fn is_cairn(root: &Path) -> bool {
    root.join(".git").exists()
}

/// Error unless `root` is an existing cairn. Only `cairn init` may create one,
/// so callers gate every other command on this rather than silently
/// `git init`-ing in the user's directory.
///
/// # Errors
/// [`StartupError::NotACairn`] if `root` has no `.git`.
pub fn ensure_cairn(root: &Path) -> Result<(), StartupError> {
    if is_cairn(root) {
        Ok(())
    } else {
        Err(StartupError::NotACairn {
            path: root.display().to_string(),
        })
    }
}

/// Build an engine from a cairn `root` with an ephemeral in-memory index
/// (store + git + Tantivy). The daemon's persistent path constructs its engine
/// separately with an on-disk index.
///
/// # Errors
/// [`StartupError::Build`] if any adapter fails to open.
pub fn build_engine(root: &Path) -> Result<Engine, StartupError> {
    let store = LocalFsStore::open(root).map_err(|e| StartupError::Build(e.to_string()))?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| StartupError::Build(e.to_string()))?;
    let index = TantivyIndex::in_memory().map_err(|e| StartupError::Build(e.to_string()))?;
    Ok(Engine::new(store, index, vcs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_cairn_distinguishes_initialized_and_not() {
        let tmp = tempfile::tempdir().unwrap();
        // A bare directory is not a cairn.
        assert!(!is_cairn(tmp.path()));
        let err = ensure_cairn(tmp.path()).unwrap_err();
        assert!(matches!(err, StartupError::NotACairn { .. }));
        assert!(err.to_string().contains("not a cairn"));

        // After a git init, it is — and the engine builds against it.
        GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(is_cairn(tmp.path()));
        ensure_cairn(tmp.path()).unwrap();
        build_engine(tmp.path()).unwrap();
    }
}
