//! Application use-cases: orchestrate ports to fulfill commands and queries,
//! emitting domain events. No transport or serialization lives here.

use cairn_domain::{Graph, Note, NotePath};
use cairn_ports::{FsChange, PortError, SearchHit, SearchIndex, VaultStore, Vcs};
use std::collections::HashMap;

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
    memo: HashMap<NotePath, u64>,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V> {
    /// Construct an engine from its ports.
    pub fn new(store: S, index: I, vcs: V) -> Self {
        Self {
            store,
            index,
            vcs,
            memo: HashMap::new(),
        }
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

    /// Rebuild the index and the content-hash memo from the store (startup /
    /// full rescan). Emits [`Event::Reindexed`].
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        self.memo = notes
            .iter()
            .map(|n| (n.path.clone(), n.content_hash()))
            .collect();
        sink.emit(Event::Reindexed(notes.len()));
        Ok(())
    }

    /// Apply a single filesystem change, deduped via the content-hash memo.
    /// The single source of change-events: emits only when content actually
    /// differs from what is indexed.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn apply_change(
        &mut self,
        change: &FsChange,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        match change {
            FsChange::Changed(path) => {
                let raw = match self.store.read(path) {
                    Ok(raw) => raw,
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                let note = Note::parse(path.clone(), &raw);
                let hash = note.content_hash();
                if self.memo.get(path) == Some(&hash) {
                    return Ok(());
                }
                self.index.upsert(&note)?;
                self.memo.insert(path.clone(), hash);
                sink.emit(Event::NoteChanged(path.clone()));
                sink.emit(Event::Reindexed(self.memo.len()));
                Ok(())
            }
            FsChange::Removed(path) => self.apply_removal(path, sink),
        }
    }

    fn apply_removal(
        &mut self,
        path: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        if self.memo.remove(path).is_some() {
            self.index.remove(path)?;
            sink.emit(Event::NoteDeleted(path.clone()));
            sink.emit(Event::Reindexed(self.memo.len()));
        }
        Ok(())
    }

    /// Create or overwrite a note; emits via the memo diff (see
    /// [`Engine::apply_change`]).
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
        self.apply_change(&FsChange::Changed(path.clone()), sink)
    }

    /// Read a note's raw contents.
    ///
    /// # Errors
    /// Returns [`PortError`] if the note is missing or a port fails.
    pub fn read_note(&self, path: &NotePath) -> Result<String, PortError> {
        self.store.read(path)
    }

    /// Delete a note; emits via the memo diff (see [`Engine::apply_change`]).
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn delete_note(
        &mut self,
        path: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.delete(path)?;
        self.apply_change(&FsChange::Removed(path.clone()), sink)
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
    fn apply_change_dedups_self_writes_and_emits_on_real_change() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let a = NotePath::new("a.md").unwrap();

        let mut e1 = Vec::new();
        eng.write_note(&a, "hello", &mut e1).unwrap();
        assert_eq!(e1, vec![Event::NoteChanged(a.clone()), Event::Reindexed(1)]);

        // Echo: same content already on disk -> nothing emitted.
        let mut e2 = Vec::new();
        eng.apply_change(&FsChange::Changed(a.clone()), &mut e2)
            .unwrap();
        assert!(e2.is_empty());

        // Real external change -> emits again.
        std::fs::write(tmp.path().join("a.md"), "changed").unwrap();
        let mut e3 = Vec::new();
        eng.apply_change(&FsChange::Changed(a.clone()), &mut e3)
            .unwrap();
        assert_eq!(e3, vec![Event::NoteChanged(a.clone()), Event::Reindexed(1)]);

        // Removal -> NoteDeleted; removing again -> nothing.
        std::fs::remove_file(tmp.path().join("a.md")).unwrap();
        let mut e4 = Vec::new();
        eng.apply_change(&FsChange::Removed(a.clone()), &mut e4)
            .unwrap();
        assert_eq!(e4, vec![Event::NoteDeleted(a.clone()), Event::Reindexed(0)]);
        let mut e5 = Vec::new();
        eng.apply_change(&FsChange::Removed(a.clone()), &mut e5)
            .unwrap();
        assert!(e5.is_empty());
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
