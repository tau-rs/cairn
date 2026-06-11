//! Application use-cases: orchestrate ports to fulfill commands and queries,
//! emitting domain events. No transport or serialization lives here.

use cairn_domain::{rewrite_link_target, Graph, Note, NotePath};
use cairn_ports::{
    FileStamp, FsChange, NoopPluginHost, PluginCallbacks, PluginEvent, PluginHost, PluginInfo,
    PortError, Revision, SearchHit, SearchIndex, VaultStore, Vcs,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, UNIX_EPOCH};

/// Schema version of `.cairn/state.json`. Tags the hash regime: bump this
/// whenever `Note::content_hash`'s algorithm changes so stale persisted hashes
/// are rebuilt (cold) rather than silently trusted.
const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct StateEntry {
    path: String,
    hash: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    len: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StatePayload {
    #[serde(default)]
    schema_version: u32,
    entries: Vec<StateEntry>,
}

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
    stamps: HashMap<NotePath, FileStamp>,
    notes_cache: RefCell<Option<HashMap<NotePath, Note>>>,
    plugins: Box<dyn PluginHost>,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V> {
    /// Construct an engine from its ports.
    pub fn new(store: S, index: I, vcs: V) -> Self {
        Self {
            store,
            index,
            vcs,
            memo: HashMap::new(),
            stamps: HashMap::new(),
            notes_cache: RefCell::new(None),
            plugins: Box::new(NoopPluginHost),
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

    /// Ensure the parsed-note cache is populated (reading the vault once if
    /// empty), then run `f` over it.
    fn with_notes<R>(&self, f: impl FnOnce(&HashMap<NotePath, Note>) -> R) -> Result<R, PortError> {
        if self.notes_cache.borrow().is_none() {
            let map: HashMap<NotePath, Note> = self
                .load_all_notes()?
                .into_iter()
                .map(|n| (n.path.clone(), n))
                .collect();
            *self.notes_cache.borrow_mut() = Some(map);
        }
        let guard = self.notes_cache.borrow();
        Ok(f(guard.as_ref().expect("cache populated above")))
    }

    fn rebuild(&mut self) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        self.memo = notes
            .iter()
            .map(|n| (n.path.clone(), n.content_hash()))
            .collect();
        let mut stamps = HashMap::with_capacity(notes.len());
        for n in &notes {
            stamps.insert(n.path.clone(), self.store.stamp(&n.path)?);
        }
        self.stamps = stamps;
        Ok(())
    }

    /// Rebuild the index and the content-hash memo from the store (startup /
    /// full rescan). Emits [`Event::Reindexed`].
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        self.rebuild()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    /// Startup reconcile against a persisted index: load `state.json`, seed memo
    /// and stamps, then stat each current note and re-index only what changed,
    /// removing notes gone from disk. Saves the refreshed state, emits a single
    /// [`Event::Reindexed`], and falls back to a full rebuild if state is absent
    /// or invalid.
    ///
    /// # Errors
    /// Returns [`PortError`] if a port operation fails.
    pub fn reconcile(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        match self.store.read_meta()? {
            Some(json) => match parse_state(&json) {
                Ok(restored) => self.reconcile_warm(restored, sink),
                Err(()) => self.reconcile_cold(sink),
            },
            None => self.reconcile_cold(sink),
        }
    }

    fn reconcile_cold(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        self.rebuild()?;
        self.save_state()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    fn reconcile_warm(
        &mut self,
        restored: RestoredState,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.memo = restored.iter().map(|(p, (h, _))| (p.clone(), *h)).collect();
        self.stamps = restored.iter().map(|(p, (_, s))| (p.clone(), *s)).collect();

        let current = self.store.list()?;
        let current_set: HashSet<&NotePath> = current.iter().collect();
        let removed: Vec<NotePath> = restored
            .keys()
            .filter(|p| !current_set.contains(*p))
            .cloned()
            .collect();
        for p in removed {
            self.index.remove(&p)?;
            self.memo.remove(&p);
            self.stamps.remove(&p);
        }

        for path in current {
            let stamp = self.store.stamp(&path)?;
            if self.stamps.get(&path) == Some(&stamp) {
                continue; // unchanged on disk → trust the persisted index
            }
            let raw = self.store.read(&path)?;
            let note = Note::parse(path.clone(), &raw);
            let hash = note.content_hash();
            self.index.upsert(&note)?;
            self.memo.insert(path.clone(), hash);
            self.stamps.insert(path, stamp);
        }

        self.save_state()?;
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    fn save_state(&self) -> Result<(), PortError> {
        let mut entries = Vec::with_capacity(self.stamps.len());
        for (path, stamp) in &self.stamps {
            let hash = self.memo.get(path).copied().unwrap_or(0);
            let dur = stamp
                .modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            entries.push(StateEntry {
                path: path.as_str().to_string(),
                hash,
                mtime_secs: dur.as_secs(),
                mtime_nanos: dur.subsec_nanos(),
                len: stamp.len,
            });
        }
        let json = serde_json::to_string(&StatePayload {
            schema_version: STATE_SCHEMA_VERSION,
            entries,
        })
        .map_err(|e| PortError::Adapter(e.to_string()))?;
        self.store.write_meta(&json)
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
                // Stat-guard: skip the read entirely when the file's (mtime,len)
                // is unchanged (a spurious/duplicate watcher event).
                let stamp = match self.store.stamp(path) {
                    Ok(s) => s,
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                if self.stamps.get(path) == Some(&stamp) {
                    return Ok(());
                }
                let raw = match self.store.read(path) {
                    Ok(raw) => raw,
                    Err(PortError::NotFound(_)) => return self.apply_removal(path, sink),
                    Err(e) => return Err(e),
                };
                let note = Note::parse(path.clone(), &raw);
                let hash = note.content_hash();
                // Record the new stamp even if content reverted, so the next
                // unchanged event short-circuits.
                self.stamps.insert(path.clone(), stamp);
                if let Some(map) = self.notes_cache.get_mut() {
                    map.insert(path.clone(), note.clone());
                }
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
        // Drop the stamp unconditionally: a note seen by the stat-guard but
        // never indexed (no memo entry) would otherwise leak its stamp here.
        self.stamps.remove(path);
        if let Some(map) = self.notes_cache.get_mut() {
            map.remove(path);
        }
        if self.memo.contains_key(path) {
            // Fallible op first, then the infallible memo drop, so index and
            // memo stay consistent if a future index adapter's remove fails.
            self.index.remove(path)?;
            self.memo.remove(path);
            sink.emit(Event::NoteDeleted(path.clone()));
            sink.emit(Event::Reindexed(self.memo.len()));
        }
        Ok(())
    }

    /// Index a note whose new `contents` we just wrote ourselves. Unlike
    /// [`Engine::apply_change`], this does NOT stat-guard: a same-length
    /// self-write can share the previous `(mtime, len)` on coarse-resolution
    /// filesystems (e.g. Windows), and a command write must never be skipped.
    /// Still deduped by content hash. Records the fresh stamp so a later
    /// external event on this path stat-guards correctly.
    fn apply_write(
        &mut self,
        path: &NotePath,
        contents: &str,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        let note = Note::parse(path.clone(), contents);
        let hash = note.content_hash();
        self.stamps.insert(path.clone(), self.store.stamp(path)?);
        if let Some(map) = self.notes_cache.get_mut() {
            map.insert(path.clone(), note.clone());
        }
        if self.memo.get(path) == Some(&hash) {
            return Ok(());
        }
        self.index.upsert(&note)?;
        self.memo.insert(path.clone(), hash);
        sink.emit(Event::NoteChanged(path.clone()));
        sink.emit(Event::Reindexed(self.memo.len()));
        Ok(())
    }

    /// Create or overwrite a note; emits via the memo diff.
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
        self.apply_write(path, contents, sink)
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

    /// Rename or move a note, link-aware: moves the file, then rewrites
    /// `[[wikilinks]]` that pointed at the old stem to the new stem in every
    /// note. Emits `NoteDeleted(from)` + `NoteChanged(to)` (+ a `NoteChanged`
    /// per rewritten note, + `Reindexed`s), all via [`Engine::apply_change`].
    ///
    /// A pure directory move (same stem) does not rewrite links. The rewrite
    /// loop includes the moved note itself, so a self-link is fixed too.
    ///
    /// # Errors
    /// Propagates [`PortError`] from the store (`NotFound` if `from` is missing,
    /// `AlreadyExists` if `to` exists, `Adapter` otherwise).
    pub fn rename_note(
        &mut self,
        from: &NotePath,
        to: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.rename(from, to)?;
        self.apply_change(&FsChange::Removed(from.clone()), sink)?;
        self.apply_change(&FsChange::Changed(to.clone()), sink)?;

        let old_stem = from.stem();
        let new_stem = to.stem();
        if old_stem != new_stem {
            for path in self.store.list()? {
                let raw = self.store.read(&path)?;
                let rewritten = rewrite_link_target(&raw, old_stem, new_stem);
                if rewritten != raw {
                    self.store.write(&path, &rewritten)?;
                    // A link rewrite is often the same length (e.g. `[[a]]`->`[[c]]`);
                    // index the known content directly so the stat-guard can't skip
                    // it on a coarse-mtime filesystem.
                    self.apply_write(&path, &rewritten, sink)?;
                }
            }
        }
        Ok(())
    }

    /// Search note content.
    ///
    /// # Errors
    /// Returns [`PortError`] if the index fails.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.index.search(query)
    }

    /// Backlinks for a note, computed from the cached notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
        self.with_notes(|m| Graph::build(m.values()).backlinks(path).to_vec())
    }

    /// All parsed notes in the cairn (from the cache).
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn list_notes(&self) -> Result<Vec<Note>, PortError> {
        self.with_notes(|m| m.values().cloned().collect())
    }

    /// The link graph derived from the cached notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn graph(&self) -> Result<Graph, PortError> {
        self.with_notes(|m| Graph::build(m.values()))
    }

    /// All tags across the cairn with note counts, sorted by tag.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn list_tags(&self) -> Result<Vec<(String, usize)>, PortError> {
        self.with_notes(|m| {
            let mut counts: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for note in m.values() {
                for tag in note.tags() {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            }
            counts.into_iter().collect()
        })
    }

    /// Notes carrying `tag`, sorted by path.
    ///
    /// # Errors
    /// Returns [`PortError`] if the cache must be populated and a port fails.
    pub fn notes_by_tag(&self, tag: &str) -> Result<Vec<NotePath>, PortError> {
        self.with_notes(|m| {
            let mut out: Vec<NotePath> = m
                .values()
                .filter(|n| n.tags().iter().any(|t| t == tag))
                .map(|n| n.path.clone())
                .collect();
            out.sort();
            out
        })
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

    /// A note's commit history (newest first).
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter fails.
    pub fn note_history(&self, path: &NotePath) -> Result<Vec<Revision>, PortError> {
        self.vcs.history(path.as_str())
    }

    /// A note's contents at a past revision.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note didn't exist at that revision;
    /// [`PortError`] on a VCS failure.
    pub fn note_at(&self, path: &NotePath, revision: &str) -> Result<String, PortError> {
        self.vcs.show(path.as_str(), revision)
    }

    /// Restore a note to a past revision: write that revision's contents as the
    /// current note (a pending change to commit later). Emits `NoteChanged`.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note didn't exist at that revision;
    /// [`PortError`] on a VCS or storage failure.
    pub fn restore_note(
        &mut self,
        path: &NotePath,
        revision: &str,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        let contents = self.vcs.show(path.as_str(), revision)?;
        self.write_note(path, &contents, sink)
    }

    /// Replace the plugin host (the composition root injects the real one).
    pub fn set_plugin_host(&mut self, host: Box<dyn PluginHost>) {
        self.plugins = host;
    }

    /// Loaded plugins and their declared commands.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.plugins()
    }

    /// Invoke a plugin command, servicing any host-callbacks it makes mid-invoke.
    ///
    /// The host is moved out of `self` for the duration (see below). If the host
    /// *panics*, `self.plugins` is left as a [`NoopPluginHost`] rather than
    /// restored — accepted, since a panicking host already implies a poisoned
    /// engine (no `catch_unwind` guard).
    ///
    /// # Errors
    /// Propagates [`PortError`] from the plugin host.
    pub fn invoke_plugin_command(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        sink: &mut dyn EventSink,
    ) -> Result<serde_json::Value, PortError> {
        // Move the real host into a local so `self.plugins` no longer aliases it;
        // the callbacks handler can then borrow the rest of `self` (the store) to
        // service host-callbacks the plugin sends mid-invoke.
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        let result = {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.invoke(plugin, command, args, &mut cb)
            // cb is dropped here, releasing the &mut self borrow
        };
        self.plugins = host;
        result
    }

    /// Deliver a cairn event to subscribed plugins (best-effort). Event-handler
    /// callbacks route through the engine, and any events they emit go to `sink`.
    pub fn dispatch_plugin_event(&mut self, event: &PluginEvent, sink: &mut dyn EventSink) {
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.dispatch_event(event, &mut cb);
        }
        self.plugins = host;
    }
}

/// Bridges plugin host-callbacks to engine operations. Held only for the duration
/// of a single `invoke_plugin_command` or `dispatch_plugin_event`, while
/// `self.plugins` is a `NoopPluginHost`.
struct EngineCallbacks<'a, S, I, V> {
    engine: &'a mut Engine<S, I, V>,
    sink: &'a mut dyn EventSink,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> PluginCallbacks for EngineCallbacks<'_, S, I, V> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        self.engine.read_note(&np)
    }

    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        // Routes through the engine write path: persists, updates the note cache,
        // and emits NoteChanged/Reindexed through the sink.
        self.engine.write_note(&np, contents, self.sink)
    }

    fn delete_note(&mut self, path: &str) -> Result<(), PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        // Routes through the engine delete path: removes the note + caches and
        // emits NoteDeleted through the sink.
        self.engine.delete_note(&np, self.sink)
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.engine.search(query)
    }

    fn list_notes(&mut self) -> Result<Vec<Note>, PortError> {
        self.engine.list_notes()
    }
}

type RestoredState = HashMap<NotePath, (u64, FileStamp)>;

fn parse_state(json: &str) -> Result<RestoredState, ()> {
    let payload: StatePayload = serde_json::from_str(json).map_err(|_| ())?;
    if payload.schema_version != STATE_SCHEMA_VERSION {
        return Err(()); // different/absent hash regime → reconcile_cold rebuilds
    }
    let mut map = HashMap::with_capacity(payload.entries.len());
    for e in payload.entries {
        let path = NotePath::new(&e.path).map_err(|_| ())?;
        let modified = UNIX_EPOCH + Duration::new(e.mtime_secs, e.mtime_nanos);
        map.insert(
            path,
            (
                e.hash,
                FileStamp {
                    modified,
                    len: e.len,
                },
            ),
        );
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore, TantivyIndex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A `VaultStore` that counts `read` calls, delegating everything else to
    /// an inner `LocalFsStore`.
    struct CountingStore {
        inner: LocalFsStore,
        reads: Arc<AtomicUsize>,
    }
    impl VaultStore for CountingStore {
        fn read(&self, path: &NotePath) -> Result<String, PortError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.read(path)
        }
        fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError> {
            self.inner.write(path, contents)
        }
        fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
            self.inner.delete(path)
        }
        fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError> {
            self.inner.rename(from, to)
        }
        fn list(&self) -> Result<Vec<NotePath>, PortError> {
            self.inner.list()
        }
        fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError> {
            self.inner.stamp(path)
        }
        fn read_meta(&self) -> Result<Option<String>, PortError> {
            self.inner.read_meta()
        }
        fn write_meta(&self, data: &str) -> Result<(), PortError> {
            self.inner.write_meta(data)
        }
    }

    #[test]
    fn stat_guard_skips_read_when_stamp_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let store = CountingStore {
            inner: LocalFsStore::open(tmp.path()).unwrap(),
            reads: reads.clone(),
        };
        let mut eng = Engine::new(
            store,
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );

        std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
        let mut ev = Vec::new();
        eng.reindex(&mut ev).unwrap(); // reads a.md once, seeds stamp
        let before = reads.load(Ordering::SeqCst);

        // Unchanged file: the stat-guard must skip the read AND emit nothing.
        let a = NotePath::new("a.md").unwrap();
        let mut e2 = Vec::new();
        eng.apply_change(&FsChange::Changed(a), &mut e2).unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            before,
            "stat-guard must skip the read"
        );
        assert!(e2.is_empty());
    }

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
            eng.search("target")
                .unwrap()
                .iter()
                .map(|h| &h.path)
                .collect::<Vec<_>>(),
            vec![&b]
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
    fn list_tags_and_notes_by_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "---\ntags: [rust, ideas]\n---\nx",
            &mut ev,
        )
        .unwrap();
        eng.write_note(
            &NotePath::new("b.md").unwrap(),
            "---\ntags: rust\n---\ny",
            &mut ev,
        )
        .unwrap();

        assert_eq!(
            eng.list_tags().unwrap(),
            vec![("ideas".to_string(), 1), ("rust".to_string(), 2)]
        );
        assert_eq!(
            eng.notes_by_tag("rust").unwrap(),
            vec![
                NotePath::new("a.md").unwrap(),
                NotePath::new("b.md").unwrap()
            ]
        );
        assert!(eng.notes_by_tag("missing").unwrap().is_empty());
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

    #[test]
    fn rename_moves_file_and_rewrites_links() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        let c = NotePath::new("c.md").unwrap();
        eng.write_note(&a, "i am a", &mut ev).unwrap();
        eng.write_note(&b, "link to [[a]] here", &mut ev).unwrap();

        let mut ev2 = Vec::new();
        eng.rename_note(&a, &c, &mut ev2).unwrap();

        // file moved
        assert!(matches!(eng.read_note(&a), Err(PortError::NotFound(_))));
        assert_eq!(eng.read_note(&c).unwrap(), "i am a");
        // link in b rewritten a -> c (stems)
        assert_eq!(eng.read_note(&b).unwrap(), "link to [[c]] here");
        // events: move (delete a + change c) then the rewrite of b
        assert!(ev2.contains(&Event::NoteDeleted(a.clone())));
        assert!(ev2.contains(&Event::NoteChanged(c.clone())));
        assert!(ev2.contains(&Event::NoteChanged(b.clone())));
    }

    #[test]
    fn pure_directory_move_keeps_stem_and_does_not_rewrite() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let moved = NotePath::new("dir/a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "x", &mut ev).unwrap();
        eng.write_note(&b, "see [[a]]", &mut ev).unwrap();

        let mut ev2 = Vec::new();
        eng.rename_note(&a, &moved, &mut ev2).unwrap();

        assert_eq!(eng.read_note(&moved).unwrap(), "x");
        // same stem "a" -> link NOT rewritten
        assert_eq!(eng.read_note(&b).unwrap(), "see [[a]]");
        assert!(!ev2.contains(&Event::NoteChanged(b.clone())));
    }

    #[test]
    fn rename_onto_existing_note_is_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "a", &mut ev).unwrap();
        eng.write_note(&b, "b", &mut ev).unwrap();
        assert!(matches!(
            eng.rename_note(&a, &b, &mut Vec::new()),
            Err(PortError::AlreadyExists(_))
        ));
    }

    #[test]
    fn reconcile_cold_builds_and_writes_state() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "ownership rules").unwrap();
        let mut eng = Engine::new(
            LocalFsStore::open(tmp.path()).unwrap(),
            TantivyIndex::open_at(&tmp.path().join(".cairn/index")).unwrap(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        assert!(eng
            .search("ownership")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
        // state.json was written — assert via a fresh store reading the same dir.
        let store = LocalFsStore::open(tmp.path()).unwrap();
        assert!(store.read_meta().unwrap().is_some());
    }

    #[test]
    fn note_cache_serves_queries_and_stays_live() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "see [[b]]").unwrap();
        std::fs::write(tmp.path().join("b.md"), "hi").unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );

        assert_eq!(eng.list_notes().unwrap().len(), 2);
        let after_first = reads.load(Ordering::SeqCst);
        assert!(after_first >= 2);

        assert_eq!(eng.graph().unwrap().edges().len(), 1);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            after_first,
            "cache hit: no re-read"
        );

        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("c.md").unwrap(), "from c to [[b]]", &mut ev)
            .unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 3);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            after_first,
            "write kept cache live"
        );

        eng.delete_note(&NotePath::new("a.md").unwrap(), &mut ev)
            .unwrap();
        assert_eq!(eng.list_notes().unwrap().len(), 2);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            after_first,
            "delete kept cache live"
        );
    }

    #[test]
    fn reindex_does_not_invalidate_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "x").unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.list_notes().unwrap();
        let base = reads.load(Ordering::SeqCst);
        eng.reindex(&mut Vec::new()).unwrap();
        let after_reindex = reads.load(Ordering::SeqCst);
        assert!(after_reindex > base, "reindex reads for the index");
        eng.list_notes().unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            after_reindex,
            "reindex did not invalidate the cache"
        );
    }

    /// A stub host whose invoke calls back into the engine via the callbacks
    /// handler — exercises the mem::replace re-entrancy in invoke_plugin_command.
    struct CallbackEcho;
    impl PluginHost for CallbackEcho {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "cb".into(),
                name: "cb".into(),
                version: "0".into(),
                commands: Vec::new(),
            }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            let contents = callbacks.read_note(path)?;
            Ok(serde_json::json!({ "contents": contents }))
        }
    }

    #[test]
    fn invoke_services_read_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hello body", &mut events)
            .unwrap();
        eng.set_plugin_host(Box::new(CallbackEcho));
        let mut sink: Vec<Event> = Vec::new();
        let out = eng
            .invoke_plugin_command(
                "cb",
                "readit",
                &serde_json::json!({ "path": "a.md" }),
                &mut sink,
            )
            .unwrap();
        assert_eq!(out["contents"], "hello body");
    }

    #[test]
    fn default_plugin_host_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        assert!(eng.list_plugins().is_empty());
        let mut sink: Vec<Event> = Vec::new();
        let err = eng
            .invoke_plugin_command("nope", "x", &serde_json::Value::Null, &mut sink)
            .unwrap_err();
        assert!(matches!(err, PortError::NotFound(_)));
    }

    /// A stub host whose invoke deletes a note via the callbacks handler —
    /// exercises delete event emission through invoke_plugin_command.
    struct CallbackDeleter;
    impl PluginHost for CallbackDeleter {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "d".into(),
                name: "d".into(),
                version: "0".into(),
                commands: Vec::new(),
            }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            callbacks.delete_note(path)?;
            Ok(serde_json::json!({ "deleted": true }))
        }
    }

    #[test]
    fn delete_callback_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events: Vec<Event> = Vec::new();
        eng.write_note(&NotePath::new("x.md").unwrap(), "body", &mut events)
            .unwrap();
        events.clear();
        eng.set_plugin_host(Box::new(CallbackDeleter));
        let out = eng
            .invoke_plugin_command(
                "d",
                "del",
                &serde_json::json!({ "path": "x.md" }),
                &mut events,
            )
            .unwrap();
        assert_eq!(out, serde_json::json!({ "deleted": true }));
        assert!(events.contains(&Event::NoteDeleted(NotePath::new("x.md").unwrap())));
        assert!(eng.read_note(&NotePath::new("x.md").unwrap()).is_err());
    }

    /// A stub host whose invoke writes a note via the callbacks handler —
    /// exercises sink threading through invoke_plugin_command.
    struct CallbackWriter;
    impl PluginHost for CallbackWriter {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "w".into(),
                name: "w".into(),
                version: "0".into(),
                commands: Vec::new(),
            }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            args: &serde_json::Value,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            let path = args["path"].as_str().unwrap_or_default();
            let contents = args["contents"].as_str().unwrap_or_default();
            callbacks.write_note(path, contents)?;
            Ok(serde_json::json!({ "written": true }))
        }
    }

    #[test]
    fn write_callback_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(CallbackWriter));
        let mut sink: Vec<Event> = Vec::new();
        let out = eng
            .invoke_plugin_command(
                "w",
                "write",
                &serde_json::json!({ "path": "x.md", "contents": "body text" }),
                &mut sink,
            )
            .unwrap();
        assert_eq!(out, serde_json::json!({ "written": true }));
        assert!(sink.contains(&Event::NoteChanged(NotePath::new("x.md").unwrap())));
        assert_eq!(
            eng.read_note(&NotePath::new("x.md").unwrap()).unwrap(),
            "body text"
        );
    }

    #[test]
    fn reconcile_warm_skips_unchanged_and_catches_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let idx_dir = tmp.path().join(".cairn/index");
        std::fs::write(tmp.path().join("a.md"), "alpha body").unwrap();
        std::fs::write(tmp.path().join("b.md"), "beta body").unwrap();
        // c.md is left untouched between runs — it must NOT be re-read.
        std::fs::write(tmp.path().join("c.md"), "gamma body").unwrap();

        {
            let mut eng = Engine::new(
                LocalFsStore::open(tmp.path()).unwrap(),
                TantivyIndex::open_at(&idx_dir).unwrap(),
                GitVcs::open_or_init(tmp.path()).unwrap(),
            );
            eng.reconcile(&mut Vec::new()).unwrap();
        }

        std::fs::write(tmp.path().join("a.md"), "alpha CHANGED body").unwrap();
        std::fs::remove_file(tmp.path().join("b.md")).unwrap();

        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            TantivyIndex::open_at(&idx_dir).unwrap(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        // Only the changed a.md is re-read; the unchanged c.md is skipped via
        // the stamp, and the deleted b.md is removed without a read.
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "only the changed note is re-read; unchanged c.md is skipped"
        );
        assert!(eng
            .search("CHANGED")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));
        assert!(eng.search("beta").unwrap().is_empty());
        // The unchanged note survived (trusted from the persisted index).
        assert!(eng
            .search("gamma")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "c.md"));
    }

    #[test]
    fn parse_state_rejects_mismatched_schema_version() {
        // A payload from a different (future) hash regime must not seed memo.
        let json = serde_json::json!({
            "schema_version": STATE_SCHEMA_VERSION + 1,
            "entries": []
        })
        .to_string();
        assert!(parse_state(&json).is_err());
    }

    #[test]
    fn parse_state_rejects_legacy_state_without_version() {
        // Pre-versioning state.json (no schema_version field) is rebuilt, not trusted.
        let json = r#"{"entries":[]}"#;
        assert!(parse_state(json).is_err());
    }

    #[test]
    fn save_state_round_trips_through_parse_state() {
        // save_state's serialized field names must match what parse_state reads:
        // a serde rename of `schema_version`/`entries` would slip past the
        // hand-built-JSON tests but break real persistence.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "alpha body").unwrap();
        let mut eng = engine(tmp.path());
        eng.reconcile(&mut Vec::new()).unwrap();

        let store = LocalFsStore::open(tmp.path()).unwrap();
        let raw = store.read_meta().unwrap().unwrap();
        let restored = parse_state(&raw).expect("save_state output must parse back");
        assert!(restored.contains_key(&NotePath::new("a.md").unwrap()));
    }

    #[test]
    fn parse_state_accepts_current_version() {
        let json = serde_json::json!({
            "schema_version": STATE_SCHEMA_VERSION,
            "entries": []
        })
        .to_string();
        assert!(parse_state(&json).is_ok());
    }

    #[test]
    fn stale_state_json_triggers_full_rebuild() {
        // End-to-end: a state.json from a different regime must rebuild the
        // index (re-read every note) rather than warm-start off stale hashes.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "alpha body").unwrap();
        std::fs::write(tmp.path().join("b.md"), "beta body").unwrap();

        // First run writes a current-version state.json.
        {
            let mut eng = engine(tmp.path());
            eng.reconcile(&mut Vec::new()).unwrap();
        }

        // Rewrite state.json with a bumped schema_version (simulated future regime).
        let store = LocalFsStore::open(tmp.path()).unwrap();
        let raw = store.read_meta().unwrap().unwrap();
        let mut payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
        payload["schema_version"] =
            serde_json::json!(payload["schema_version"].as_u64().unwrap() + 1);
        store.write_meta(&payload.to_string()).unwrap();

        // Reconcile again with a read-counting store: a rebuild re-reads both notes.
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "stale schema_version forces a full rebuild that re-reads every note"
        );
    }

    /// A stub host whose dispatch_event writes a marker note via the callbacks —
    /// exercises Engine::dispatch_plugin_event + handler callbacks.
    struct EventWriter;
    impl PluginHost for EventWriter {
        fn plugins(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        fn invoke(
            &mut self,
            plugin: &str,
            _command: &str,
            _args: &serde_json::Value,
            _callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            Err(PortError::NotFound(format!("plugin {plugin}")))
        }
        fn dispatch_event(
            &mut self,
            _event: &cairn_ports::PluginEvent,
            callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) {
            let _ = callbacks.write_note("seen.md", "seen");
        }
    }

    #[test]
    fn dispatch_event_runs_handler_with_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(EventWriter));
        let mut events: Vec<Event> = Vec::new();
        eng.dispatch_plugin_event(
            &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
            &mut events,
        );
        assert_eq!(
            eng.read_note(&NotePath::new("seen.md").unwrap()).unwrap(),
            "seen"
        );
        assert!(events.contains(&Event::NoteChanged(NotePath::new("seen.md").unwrap())));
    }

    #[test]
    fn restore_writes_old_content_and_emits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let a = NotePath::new("a.md").unwrap();
        let mut events = Vec::new();
        eng.write_note(&a, "v1", &mut events).unwrap();
        eng.commit("v1", &mut events).unwrap();
        eng.write_note(&a, "v2", &mut events).unwrap();
        eng.commit("v2", &mut events).unwrap();

        let hist = eng.note_history(&a).unwrap();
        assert_eq!(hist.len(), 2);
        let v1_rev = hist[1].id.clone(); // oldest = v1
        assert_eq!(eng.note_at(&a, &v1_rev).unwrap(), "v1");

        events.clear();
        eng.restore_note(&a, &v1_rev, &mut events).unwrap();
        assert_eq!(eng.read_note(&a).unwrap(), "v1");
        assert!(events.contains(&Event::NoteChanged(a.clone())));
    }
}
