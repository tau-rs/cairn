//! Port traits for Cairn. The application depends only on these; adapters
//! in `cairn-infra` (and future plugins) implement them.

use cairn_domain::{Note, NotePath};

/// Errors any port may surface to the application.
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// The requested note does not exist.
    #[error("note not found: {0}")]
    NotFound(String),
    /// An underlying adapter failed.
    #[error("{0}")]
    Adapter(String),
}

/// Read/write access to note content in a cairn.
pub trait VaultStore {
    /// Read a note's raw contents.
    ///
    /// # Errors
    /// Returns [`PortError`] if the note is missing or the adapter fails.
    fn read(&self, path: &NotePath) -> Result<String, PortError>;
    /// Write (create or overwrite) a note's raw contents.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError>;
    /// Delete a note.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn delete(&mut self, path: &NotePath) -> Result<(), PortError>;
    /// List all note paths in the cairn.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn list(&self) -> Result<Vec<NotePath>, PortError>;
}

/// A search hit: a note path and a relevance-ordered position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The matching note.
    pub path: NotePath,
}

/// Full-text style search over note content.
pub trait SearchIndex {
    /// Replace the index contents with the given notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError>;
    /// Return notes matching `query` (substring match in the skeleton).
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError>;
}

/// Version control over the cairn directory.
pub trait Vcs {
    /// Stage all changes and create a commit with `message`. Returns the
    /// new commit's short id.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn commit_all(&mut self, message: &str) -> Result<String, PortError>;
}

/// Detects external changes to the cairn. Seam: `NoopWatcher` for now.
pub trait Watcher {
    /// Begin watching. The skeleton's `NoopWatcher` does nothing.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn start(&mut self) -> Result<(), PortError>;
}

/// Runs background/parallel work. Seam: `BlockingExecutor` runs inline.
pub trait Executor {
    /// Run a unit of work to completion.
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

/// Live collaboration session. Seam: `NoCollab`.
pub trait CollabSession {
    /// Whether a live session is active. Always false in the skeleton.
    fn is_active(&self) -> bool;
}

/// Agent runtime (tau). Seam: `NullRuntime`.
pub trait AgentRuntime {
    /// Run a named agent action over optional note context, returning text.
    ///
    /// # Errors
    /// Returns [`PortError`] if no runtime is configured or the action fails.
    fn run_action(&self, action: &str, context: Option<&str>) -> Result<String, PortError>;
}

/// No-op watcher seam.
#[derive(Debug, Default)]
pub struct NoopWatcher;
impl Watcher for NoopWatcher {
    fn start(&mut self) -> Result<(), PortError> {
        Ok(())
    }
}

/// Inline executor seam.
#[derive(Debug, Default)]
pub struct BlockingExecutor;
impl Executor for BlockingExecutor {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

/// No-collaboration seam.
#[derive(Debug, Default)]
pub struct NoCollab;
impl CollabSession for NoCollab {
    fn is_active(&self) -> bool {
        false
    }
}

/// Null agent runtime seam.
#[derive(Debug, Default)]
pub struct NullRuntime;
impl AgentRuntime for NullRuntime {
    fn run_action(&self, action: &str, _context: Option<&str>) -> Result<String, PortError> {
        Err(PortError::Adapter(format!(
            "no agent runtime configured (action '{action}' unavailable until tau is wired)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seams_have_expected_neutral_behavior() {
        assert!(!NoCollab.is_active());
        assert!(NoopWatcher.start().is_ok());
        assert!(NullRuntime.run_action("summarize", None).is_err());
    }
}
