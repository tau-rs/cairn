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

/// A search hit. Currently just the matching note's path; results are
/// ordered by path, not by relevance (ranking arrives with a real index).
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
    /// Insert or replace a single note in the index.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
    /// Remove a single note from the index.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
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

/// A change to a note detected on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    /// A note was created or modified.
    Changed(NotePath),
    /// A note was removed.
    Removed(NotePath),
}

/// Owns the OS watcher and delivers debounced changes; dropping it stops
/// watching.
pub struct WatchHandle {
    /// Debounced note changes.
    pub changes: std::sync::mpsc::Receiver<FsChange>,
    // Keeps the underlying OS watcher alive for the handle's lifetime.
    _keepalive: Box<dyn Send>,
}

impl WatchHandle {
    /// Build a handle from a change receiver and an opaque keepalive (the
    /// adapter's live watcher).
    #[must_use]
    pub fn new(changes: std::sync::mpsc::Receiver<FsChange>, keepalive: Box<dyn Send>) -> Self {
        Self {
            changes,
            _keepalive: keepalive,
        }
    }
}

/// Detects external changes to the cairn.
pub trait Watcher {
    /// Begin watching `root`; returns a handle delivering debounced changes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the OS watcher cannot be created.
    fn watch(&self, root: &std::path::Path) -> Result<WatchHandle, PortError>;
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
