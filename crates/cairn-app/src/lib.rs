//! Application use-cases: orchestrate ports to fulfill commands and queries,
//! emitting domain events. No transport or serialization lives here.

use cairn_domain::{Graph, Note, NotePath};
use cairn_ports::{PortError, SearchHit, SearchIndex, VaultStore, Vcs};

/// A domain event emitted as a side effect of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A note was created or updated.
    NoteChanged(NotePath),
    /// A note was deleted.
    NoteDeleted(NotePath),
    /// The cairn was committed; carries the short commit id.
    Committed(String),
    /// The index finished rebuilding; carries note count.
    Reindexed(usize),
}

/// Collects events emitted during a use-case.
pub trait EventSink {
    /// Record an event.
    fn emit(&mut self, event: Event);
}

impl EventSink for Vec<Event> {
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

/// The engine: owns the ports and runs use-cases.
pub struct Engine<S, I, V> {
    store: S,
    index: I,
    vcs: V,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V> {
    /// Construct an engine from its ports.
    pub fn new(store: S, index: I, vcs: V) -> Self {
        Self { store, index, vcs }
    }

    fn load_all_notes(&self) -> Result<Vec<Note>, PortError> {
        // NOTE: loads and parses every note on each call; acceptable while the
        // index is in-memory and reindex is full.
        let paths = self.store.list()?;
        let mut notes = Vec::with_capacity(paths.len());
        for path in paths {
            let raw = self.store.read(&path)?;
            notes.push(Note::parse(path, &raw));
        }
        Ok(notes)
    }

    /// Rebuild the search index from the current store contents.
    ///
    /// Intentionally public: callers such as the CLI on startup (or a future
    /// full-rescan command) invoke it to sync the index with notes changed
    /// outside the engine. Emits [`Event::Reindexed`].
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        sink.emit(Event::Reindexed(notes.len()));
        Ok(())
    }

    /// Create or overwrite a note and refresh the index.
    ///
    /// `NoteChanged` is emitted after a successful write, before the index is
    /// rebuilt; if the subsequent reindex fails, that event has already been
    /// emitted. This is acceptable for the walking skeleton.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn write_note(
        &mut self,
        path: &NotePath,
        contents: &str,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.write(path, contents)?;
        sink.emit(Event::NoteChanged(path.clone()));
        self.reindex(sink)
    }

    /// Read a note's raw contents.
    ///
    /// # Errors
    /// Returns [`PortError`] if the note is missing or a port fails.
    pub fn read_note(&self, path: &NotePath) -> Result<String, PortError> {
        self.store.read(path)
    }

    /// Delete a note and refresh the index.
    ///
    /// `NoteDeleted` is emitted after a successful delete, before the index is
    /// rebuilt (same tradeoff as [`Engine::write_note`]).
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn delete_note(
        &mut self,
        path: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.delete(path)?;
        sink.emit(Event::NoteDeleted(path.clone()));
        self.reindex(sink)
    }

    /// Search note content.
    ///
    /// # Errors
    /// Returns [`PortError`] if the index fails.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.index.search(query)
    }

    /// Backlinks for a note, computed from the current store contents.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
        let notes = self.load_all_notes()?;
        let graph = Graph::build(&notes);
        Ok(graph.backlinks(path).to_vec())
    }

    /// All parsed notes in the cairn.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn list_notes(&self) -> Result<Vec<Note>, PortError> {
        self.load_all_notes()
    }

    /// The link graph derived from the current notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn graph(&self) -> Result<Graph, PortError> {
        Ok(Graph::build(&self.load_all_notes()?))
    }

    /// Commit all changes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS fails.
    pub fn commit(&mut self, message: &str, sink: &mut dyn EventSink) -> Result<String, PortError> {
        let id = self.vcs.commit_all(message)?;
        sink.emit(Event::Committed(id.clone()));
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};

    fn engine(dir: &std::path::Path) -> Engine<LocalFsStore, InMemoryIndex, GitVcs> {
        Engine::new(
            LocalFsStore::open(dir).unwrap(),
            InMemoryIndex::default(),
            GitVcs::open_or_init(dir).unwrap(),
        )
    }

    #[test]
    fn write_then_search_and_backlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();

        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "I link to [[b]]", &mut events).unwrap();
        eng.write_note(&b, "target note", &mut events).unwrap();

        assert_eq!(
            events,
            vec![
                Event::NoteChanged(a.clone()),
                Event::Reindexed(1),
                Event::NoteChanged(b.clone()),
                Event::Reindexed(2),
            ]
        );

        assert_eq!(
            eng.search("target").unwrap(),
            vec![SearchHit { path: b.clone() }]
        );
        assert_eq!(eng.backlinks(&b).unwrap(), vec![a]);
    }

    #[test]
    fn delete_removes_from_search_and_backlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();

        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "I link to [[b]]", &mut events).unwrap();
        eng.write_note(&b, "target note", &mut events).unwrap();

        eng.delete_note(&b, &mut events).unwrap();

        assert!(events.contains(&Event::NoteDeleted(b.clone())));
        assert!(eng.search("target").unwrap().is_empty());
        // a still links to [[b]], but b no longer exists so it resolves to nothing.
        assert!(eng.backlinks(&b).unwrap().is_empty());
    }

    #[test]
    fn commit_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hi", &mut events)
            .unwrap();
        let id = eng.commit("first", &mut events).unwrap();
        assert!(events.contains(&Event::Committed(id)));
    }

    #[test]
    fn list_notes_and_graph_expose_engine_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "see [[b]]", &mut events)
            .unwrap();
        eng.write_note(&NotePath::new("b.md").unwrap(), "hi", &mut events)
            .unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 2);
        assert_eq!(eng.graph().unwrap().edges().len(), 1);
    }
}
