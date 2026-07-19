//! Composition-root helpers shared by the `cairn` CLI and `cairn-daemon`:
//! detecting an existing cairn and constructing the engine from concrete
//! adapters. Lives outside `cairn-app` so the inner hexagon never depends on
//! `cairn-infra`; this crate is where the concrete adapters are wired.

use std::path::Path;

use cairn_app::Engine;
use cairn_infra::{GitVcs, LexicalSemanticIndex, LocalFsStore, TantivyIndex};

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
    let mut engine = Engine::new(store, index, vcs);
    engine.set_semantic_index(semantic_index());
    Ok(engine)
}

#[cfg(not(feature = "neural"))]
fn semantic_index() -> Box<dyn cairn_ports::SemanticIndex + Send> {
    Box::new(LexicalSemanticIndex::new())
}

/// Neural when weights load; otherwise warn and fall back to lexical so the
/// engine always builds (neural is an opt-in upgrade, never a hard dependency).
#[cfg(feature = "neural")]
fn semantic_index() -> Box<dyn cairn_ports::SemanticIndex + Send> {
    let path = cairn_infra::minilm_weights_path();
    match cairn_infra::NeuralSemanticIndex::open(&path) {
        Ok(ix) => Box::new(ix),
        Err(e) => {
            tracing::warn!(%e, path = %path.display(), "neural weights unavailable; using lexical");
            Box::new(LexicalSemanticIndex::new())
        }
    }
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

    #[test]
    fn build_engine_wires_semantic_suggestions() {
        use cairn_app::Scope;
        use cairn_domain::NotePath;
        let tmp = tempfile::tempdir().unwrap();
        GitVcs::open_or_init(tmp.path()).unwrap();
        let mut eng = build_engine(tmp.path()).unwrap();
        let mut ev = Vec::new();
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "rust ownership borrow",
            &mut ev,
        )
        .unwrap();
        eng.write_note(
            &NotePath::new("c.md").unwrap(),
            "rust ownership borrow lifetime",
            &mut ev,
        )
        .unwrap();
        let s = eng
            .suggestions(&Scope::Note(NotePath::new("a.md").unwrap()))
            .unwrap();
        assert!(
            s.iter().any(|e| e.to.as_str() == "c.md"),
            "real adapter wired by build_engine"
        );
    }
}
