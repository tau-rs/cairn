//! Port traits for Cairn. The application depends only on these; adapters
//! in `cairn-infra` (and future plugins) implement them.

use cairn_domain::{Note, NotePath};

/// Errors any port may surface to the application.
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// The requested note does not exist.
    #[error("note not found: {0}")]
    NotFound(String),
    /// An underlying adapter failed. Carries the original adapter error as a
    /// typed `#[source]` (see [`AdapterError`]) so callers can downcast to the
    /// cause instead of matching on a flattened message string.
    #[error(transparent)]
    Adapter(AdapterError),
    /// The target of a create/rename already exists.
    #[error("already exists: {0}")]
    AlreadyExists(String),
}

/// An adapter-layer failure. Preserves the original error as a typed `#[source]`
/// — a `git2::Error`, `std::io::Error`, a Tantivy error — so callers can
/// `downcast_ref` to the concrete cause (e.g. inspect an [`std::io::ErrorKind`]
/// or a `git2::ErrorCode`) rather than parsing the `Display` string. Its own
/// `Display` is the source error's message, so wrapping is transparent to any
/// caller that only formats the error.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AdapterError {
    message: String,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl AdapterError {
    /// Wrap a typed adapter error, preserving it as the recoverable `#[source]`.
    /// The `Display` is the wrapped error's message.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            message: source.to_string(),
            source: Some(Box::new(source)),
        }
    }

    /// A message-only adapter failure with no typed cause (e.g. a validation
    /// message constructed at the boundary, not a wrapped library error).
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }
}

impl From<String> for AdapterError {
    fn from(message: String) -> Self {
        Self::message(message)
    }
}

impl From<&str> for AdapterError {
    fn from(message: &str) -> Self {
        Self::message(message)
    }
}

/// Cheap file-change fingerprint: a note's last-modified time and byte length,
/// obtained without reading contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// Last modification time.
    pub modified: std::time::SystemTime,
    /// File length in bytes.
    pub len: u64,
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
    /// Move a note from `from` to `to`.
    ///
    /// # Errors
    /// `NotFound` if `from` is missing; `AlreadyExists` if `to` exists; `Adapter`
    /// on other failures.
    fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError>;
    /// List all note paths in the cairn.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn list(&self) -> Result<Vec<NotePath>, PortError>;
    /// Stat a note's change-fingerprint without reading its contents.
    ///
    /// # Errors
    /// `NotFound` if the note is missing; `Adapter` on other failures.
    fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError>;

    /// Read the persisted engine metadata blob (`<root>/.cairn/state.json`), if present.
    ///
    /// # Errors
    /// `Adapter` on a read/IO failure. A missing file is `Ok(None)`, not an error.
    fn read_meta(&self) -> Result<Option<String>, PortError>;

    /// Write the engine metadata blob, creating `<root>/.cairn/` if needed.
    ///
    /// # Errors
    /// `Adapter` on an IO failure.
    fn write_meta(&self, data: &str) -> Result<(), PortError>;

    /// Move a rejected metadata blob aside so it is not lost to a fresh write,
    /// renaming `<root>/.cairn/state.json` to `state.json.corrupt`. Best-effort
    /// diagnostics aid. Returns the destination path (for logging), or
    /// `Ok(None)` if there was nothing to move.
    ///
    /// # Errors
    /// `Adapter` on an IO failure during the rename.
    fn quarantine_meta(&self) -> Result<Option<String>, PortError>;
}

/// A single ranked search match.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The matching note.
    pub path: NotePath,
    /// Relevance score (BM25; higher is more relevant). Not normalized — use
    /// for relative ordering only.
    pub score: f32,
    /// A plain-text excerpt of the body around the best match. Empty if none.
    pub snippet: String,
    /// `(start, end)` byte ranges within `snippet` that matched, for UI
    /// highlighting. Half-open.
    pub highlights: Vec<(u32, u32)>,
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

/// One commit in a note's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Short commit id (7 chars).
    pub id: String,
    /// Commit summary (first line of the message).
    pub message: String,
    /// Commit time, seconds since the Unix epoch.
    pub timestamp_secs: i64,
    /// Author name.
    pub author: String,
}

/// Version control over the cairn directory.
pub trait Vcs {
    /// Stage all changes and create a commit with `message`. Returns the
    /// new commit's short id.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn commit_all(&mut self, message: &str) -> Result<String, PortError>;

    /// Commits that added/changed/removed `path`, newest first.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a git failure. An empty repo or a never-committed
    /// note yields `Ok(vec![])`.
    fn history(&self, path: &str) -> Result<Vec<Revision>, PortError>;

    /// The note's contents at `revision` (a git revspec: short/full hash, `HEAD~1`…).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the path doesn't exist at that revision;
    /// [`PortError::Adapter`] on a git failure (e.g. an unknown revision).
    fn show(&self, path: &str, revision: &str) -> Result<String, PortError>;
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

/// A loaded plugin and the commands it declared at handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Manifest id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Commands the plugin handles.
    pub commands: Vec<PluginCommand>,
}

/// A command a plugin can handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    /// Command id (used to invoke).
    pub id: String,
    /// Human title.
    pub title: String,
}

/// Operations a plugin may request of the host *during* an invoke. The host gates
/// each on a declared capability before calling through to the implementation
/// (the engine).
pub trait PluginCallbacks {
    /// Read a note's raw contents by path. Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note does not exist; [`PortError::Adapter`]
    /// on a storage failure.
    fn read_note(&mut self, path: &str) -> Result<String, PortError>;

    /// Create or overwrite a note. Gated on the `fs:write` capability. Emits
    /// change events through the host's sink.
    ///
    /// # Errors
    /// [`PortError`] on an invalid path or a storage failure.
    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError>;

    /// Ranked full-text search. Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError`] on an index failure.
    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError>;

    /// List all notes (for path + title). Gated on the `fs:read` capability.
    ///
    /// # Errors
    /// [`PortError`] on a storage failure.
    fn list_notes(&mut self) -> Result<Vec<Note>, PortError>;

    /// Delete a note. Gated on the `fs:write` capability. Emits a delete event
    /// through the host's sink.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the path is invalid or the note does not exist;
    /// [`PortError::Adapter`] on a storage failure.
    fn delete_note(&mut self, path: &str) -> Result<(), PortError>;
}

/// A cairn change the host may push to subscribed plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    /// A note was created or updated.
    NoteChanged(NotePath),
    /// A note was deleted.
    NoteDeleted(NotePath),
}

/// A single plugin's failure to handle a dispatched event. Event dispatch is
/// best-effort and has no caller-facing error channel, so a host returns these
/// for the engine to log rather than swallowing them silently (audit G4).
#[derive(Debug)]
pub struct EventDispatchError {
    /// The plugin whose handler failed.
    pub plugin: String,
    /// The underlying failure.
    pub error: PortError,
}

/// Hosts out-of-process plugins. Seam: [`NoopPluginHost`].
pub trait PluginHost: Send {
    /// The loaded plugins and their declared commands.
    fn plugins(&self) -> Vec<PluginInfo>;

    /// Invoke `command` on `plugin` with JSON `args`, returning its JSON result.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the plugin/command is unknown; [`PortError::Adapter`]
    /// on a transport or plugin-reported error.
    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError>;

    /// Deliver a cairn event to every loaded plugin that declared the `events`
    /// capability, servicing any host-callbacks each makes while handling it.
    /// Best-effort: returns any per-plugin handler failures for the caller to
    /// log (audit G4), rather than swallowing them. Default: no-op (a host that
    /// doesn't support events ignores them and reports no failures).
    fn dispatch_event(
        &mut self,
        _event: &PluginEvent,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Vec<EventDispatchError> {
        Vec::new()
    }
}

/// No-plugins seam — the engine's default host.
#[derive(Debug, Default)]
pub struct NoopPluginHost;

impl PluginHost for NoopPluginHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        Vec::new()
    }
    fn invoke(
        &mut self,
        plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_preserves_typed_source_kind() {
        // A typed adapter error (here an io::Error) must survive through
        // PortError::Adapter so callers can downcast to the cause and match on
        // kind ("lock held" vs "corrupt repo"), not just read a flattened string.
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "lock held");
        let err = PortError::Adapter(AdapterError::new(io));

        // Display is byte-identical to the old flattened message.
        assert_eq!(err.to_string(), "lock held");

        // ...but the typed cause is recoverable.
        let source = std::error::Error::source(&err).expect("typed source preserved");
        let io = source
            .downcast_ref::<std::io::Error>()
            .expect("io::Error kind recoverable");
        assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn adapter_message_only_has_no_source() {
        // A message-only adapter error (validation text, not a wrapped error)
        // carries no source and still displays its message.
        let err = PortError::Adapter(AdapterError::from("non-UTF-8 path".to_string()));
        assert_eq!(err.to_string(), "non-UTF-8 path");
        assert!(std::error::Error::source(&err).is_none());
    }
}
