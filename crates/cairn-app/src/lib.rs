//! Application use-cases: orchestrate ports to fulfill commands and queries,
//! emitting domain events. No transport or serialization lives here.

use cairn_domain::{rewrite_link_target, Graph, GraphScope, Note, NotePath};
use cairn_ports::{
    AdapterError, AgentEvent, AgentRuntime, AgentSink, EventDispatchError, FileStamp, FsChange,
    InertSemanticIndex, NoopPluginHost, PluginCallbacks, PluginEvent, PluginHost, PluginInfo,
    PortError, Revision, SearchHit, SearchIndex, SemanticIndex, VaultStore, Vcs,
};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
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

/// What to compute suggestions over (the app-layer mirror of the wire `SuggestionScope`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One focus note.
    Note(NotePath),
    /// The whole vault.
    Vault,
}

/// A suggested non-explicit edge (the app-layer mirror of the wire `SuggestedEdge`).
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestedEdgeData {
    /// Source note.
    pub from: NotePath,
    /// Target note.
    pub to: NotePath,
    /// Cosine similarity, `0..1` — ranking only.
    pub weight: f32,
    /// Provenance (shared terms), or `None`.
    pub why: Option<String>,
}

/// Suggestions returned per focus note.
const SUGGEST_TOP_K: usize = 5;
/// Similarity below this is dropped as noise.
const SUGGEST_FLOOR: f32 = 0.1;
/// Max edges returned for a `Vault` scope.
const VAULT_EDGE_CAP: usize = 100;

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

/// A capacity-bounded LRU: most-recently-used at the back, evict from the front.
struct LruCache<K: Eq, V> {
    cap: usize,
    items: Vec<(K, V)>,
}
impl<K: Eq, V> LruCache<K, V> {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            items: Vec::new(),
        }
    }
    fn get(&mut self, key: &K) -> Option<&V> {
        let i = self.items.iter().position(|(k, _)| k == key)?;
        let item = self.items.remove(i);
        self.items.push(item);
        self.items.last().map(|(_, v)| v)
    }
    fn put(&mut self, key: K, val: V) {
        if let Some(i) = self.items.iter().position(|(k, _)| *k == key) {
            self.items.remove(i);
        } else if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push((key, val));
    }
}

/// A built graph plus per-note enrichment, cached whole (scope `Full`).
struct BuiltGraph {
    graph: Graph,
    meta: HashMap<NotePath, (String, Vec<String>, i64)>, // (title, tags, mtime_secs)
}

/// A wire-ready enriched graph node: identity plus display metadata.
#[derive(Debug, Clone)]
pub struct GraphNodeData {
    /// Relative note path.
    pub path: NotePath,
    /// Display title at the node's revision.
    pub title: String,
    /// Undirected degree within the returned (scoped) graph.
    pub degree: u32,
    /// Frontmatter tags at the node's revision.
    pub tags: Vec<String>,
    /// Last-modified, Unix seconds (i64 to allow historical/epoch math).
    pub mtime_secs: i64,
}

/// Flattened, scoped, enriched graph for the service layer.
pub struct GraphResult {
    /// Enriched nodes.
    pub nodes: Vec<GraphNodeData>,
    /// `(from, to)` link edges.
    pub edges: Vec<(NotePath, NotePath)>,
}

/// The enriched diff of two graphs.
pub struct GraphDeltaResult {
    /// Nodes in `to` not in `from` (enriched from `to`).
    pub nodes_added: Vec<GraphNodeData>,
    /// Nodes in `from` not in `to` (enriched from `from`).
    pub nodes_removed: Vec<GraphNodeData>,
    /// Nodes present in both revisions whose enriched metadata (title, degree,
    /// tags, or mtime) differs. Enriched from `to`. Sorted by path.
    pub nodes_changed: Vec<GraphNodeData>,
    /// Edges added.
    pub edges_added: Vec<(NotePath, NotePath)>,
    /// Edges removed.
    pub edges_removed: Vec<(NotePath, NotePath)>,
}

/// Flatten a built graph through a scope into wire-ready node/edge tuples.
fn scope_and_flatten(built: &BuiltGraph, scope: &GraphScope) -> GraphResult {
    let g = built.graph.scoped(scope);
    let nodes = g
        .nodes()
        .into_iter()
        .map(|p| {
            let (title, tags, mtime) = built.meta.get(p).cloned().unwrap_or_default();
            GraphNodeData {
                path: p.clone(),
                title,
                degree: g.degree(p),
                tags,
                mtime_secs: mtime,
            }
        })
        .collect();
    let edges = g
        .edges()
        .into_iter()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    GraphResult { nodes, edges }
}

/// Enrich a diff node: title/tags/mtime from `meta`, degree from `graph`
/// (the scoped graph the node belongs to). Defaults to empty title/tags / 0 time.
fn enrich(
    p: &NotePath,
    meta: &HashMap<NotePath, (String, Vec<String>, i64)>,
    graph: &Graph,
) -> GraphNodeData {
    let (title, tags, mtime) = meta.get(p).cloned().unwrap_or_default();
    GraphNodeData {
        path: p.clone(),
        title,
        degree: graph.degree(p),
        tags,
        mtime_secs: mtime,
    }
}

/// Unix seconds from a filesystem `SystemTime` (0 if before the epoch).
fn system_secs(t: std::time::SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The engine: owns the ports and runs use-cases.
///
/// The ports are held as boxed trait objects (like `plugins`), so the engine is
/// one concrete type rather than generic over its adapters. The daemon and CLI
/// pin a single concrete tuple anyway, and tests still substitute fakes through
/// `Engine::new`'s `impl Trait` parameters. The `+ Send` bound keeps `Engine:
/// Send` so the daemon can hold it behind `Arc<Mutex<…>>`.
pub struct Engine {
    store: Box<dyn VaultStore + Send>,
    index: Box<dyn SearchIndex + Send>,
    vcs: Box<dyn Vcs + Send>,
    memo: HashMap<NotePath, u64>,
    stamps: HashMap<NotePath, FileStamp>,
    notes_cache: RefCell<Option<HashMap<NotePath, Note>>>,
    graph_at_cache: RefCell<LruCache<String, Arc<BuiltGraph>>>,
    plugins: Box<dyn PluginHost>,
    runtime: Option<Arc<dyn AgentRuntime + Send + Sync>>,
    semantic: RefCell<Box<dyn SemanticIndex + Send>>,
    semantic_built: Cell<bool>,
}

impl Engine {
    /// Construct an engine from its ports. Generic at the constructor so callers
    /// pass concrete adapters (or test fakes); the ports are boxed internally.
    pub fn new(
        store: impl VaultStore + Send + 'static,
        index: impl SearchIndex + Send + 'static,
        vcs: impl Vcs + Send + 'static,
    ) -> Self {
        Self {
            store: Box::new(store),
            index: Box::new(index),
            vcs: Box::new(vcs),
            memo: HashMap::new(),
            stamps: HashMap::new(),
            notes_cache: RefCell::new(None),
            graph_at_cache: RefCell::new(LruCache::new(16)),
            plugins: Box::new(NoopPluginHost),
            runtime: None,
            semantic: RefCell::new(Box::new(InertSemanticIndex)),
            semantic_built: Cell::new(false),
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
                // A stale schema/hash regime is an expected migration: rebuild
                // quietly, exactly as before versioning.
                Err(StateRejection::Stale) => self.reconcile_cold(sink),
                Err(StateRejection::Corrupt(reason)) => {
                    // Preserve the corrupt blob (best-effort) so it is not lost,
                    // then warn before abandoning the warm-start path. A failed
                    // preservation is itself reported rather than swallowed.
                    let preserved = match self.store.quarantine_meta() {
                        Ok(dest) => dest,
                        Err(e) => {
                            eprintln!("warning: could not preserve rejected state.json: {e}");
                            None
                        }
                    };
                    eprintln!("{}", state_rejected_warning(&reason, preserved.as_deref()));
                    self.reconcile_cold(sink)
                }
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
        .map_err(|e| PortError::Adapter(AdapterError::new(e)))?;
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
                if self.semantic_built.get() {
                    self.semantic.get_mut().upsert(&note)?;
                }
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
            if self.semantic_built.get() {
                self.semantic.get_mut().remove(path)?;
            }
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
        if self.semantic_built.get() {
            self.semantic.get_mut().upsert(&note)?;
        }
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

    /// Whether the note currently exists on disk (a cheap `stat`, no read).
    /// Used by the daemon's confirm-before-delete: a watcher `Removed` fired
    /// mid-write may be a transient gap, so the daemon re-checks before deleting.
    #[must_use]
    pub fn exists_on_disk(&self, path: &NotePath) -> bool {
        self.store.stamp(path).is_ok()
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

    /// The HEAD link graph, scoped and enriched. `mtime_secs` is the filesystem
    /// mtime (`VaultStore::stamp`).
    ///
    /// # Errors
    /// [`PortError`] if a port fails.
    pub fn graph_view(&self, scope: &GraphScope) -> Result<GraphResult, PortError> {
        let (graph, per_note) = self.with_notes(|m| {
            let graph = Graph::build(m.values());
            let per_note: Vec<(NotePath, String, Vec<String>)> = m
                .iter()
                .map(|(p, n)| (p.clone(), n.display_title(), n.tags()))
                .collect();
            (graph, per_note)
        })?;
        let mut meta = HashMap::new();
        for (p, title, tags) in per_note {
            let mtime = self
                .store
                .stamp(&p)
                .map(|s| system_secs(s.modified))
                .unwrap_or(0);
            meta.insert(p, (title, tags, mtime));
        }
        Ok(scope_and_flatten(&BuiltGraph { graph, meta }, scope))
    }

    /// The link graph as of a past `revision`, scoped and enriched. Cached by
    /// resolved commit oid (immutable history → no invalidation).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the revision does not resolve; [`PortError`] on a port failure.
    pub fn graph_at(&self, revision: &str, scope: &GraphScope) -> Result<GraphResult, PortError> {
        let built = self.built_at(revision)?;
        Ok(scope_and_flatten(&built, scope))
    }

    /// The diff of the link graph between `from` (older) and `to` (newer).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if either revision does not resolve.
    pub fn graph_diff(
        &self,
        from: &str,
        to: &str,
        scope: &GraphScope,
    ) -> Result<GraphDeltaResult, PortError> {
        let a = self.built_at(from)?;
        let b = self.built_at(to)?;
        let a_scoped = a.graph.scoped(scope);
        let b_scoped = b.graph.scoped(scope);
        let delta = a_scoped.diff(&b_scoped);
        // Present-in-both nodes whose enrichment differs between revisions.
        // Intersecting sorted node sets keeps `nodes_changed` sorted by path,
        // mirroring the domain diff's added/removed ordering.
        let a_nodes: BTreeSet<&NotePath> = a_scoped.nodes().into_iter().collect();
        let b_nodes: BTreeSet<&NotePath> = b_scoped.nodes().into_iter().collect();
        let nodes_changed = a_nodes
            .intersection(&b_nodes)
            .filter_map(|p| {
                let before = enrich(p, &a.meta, &a_scoped);
                let after = enrich(p, &b.meta, &b_scoped);
                let changed = before.title != after.title
                    || before.degree != after.degree
                    || before.tags != after.tags
                    || before.mtime_secs != after.mtime_secs;
                changed.then_some(after)
            })
            .collect();
        Ok(GraphDeltaResult {
            nodes_added: delta
                .nodes_added
                .iter()
                .map(|p| enrich(p, &b.meta, &b_scoped))
                .collect(),
            nodes_removed: delta
                .nodes_removed
                .iter()
                .map(|p| enrich(p, &a.meta, &a_scoped))
                .collect(),
            nodes_changed,
            edges_added: delta.edges_added,
            edges_removed: delta.edges_removed,
        })
    }

    /// Resolve → cache-or-build the Full enriched graph as of `revision`.
    fn built_at(&self, revision: &str) -> Result<Arc<BuiltGraph>, PortError> {
        let oid = self.vcs.resolve(revision)?;
        if let Some(hit) = self.graph_at_cache.borrow_mut().get(&oid).cloned() {
            return Ok(hit);
        }
        let blobs = self.vcs.read_tree_at(&oid)?;
        let mut notes = Vec::with_capacity(blobs.len());
        let mut meta = HashMap::new();
        for b in &blobs {
            let Ok(path) = NotePath::new(&b.path) else {
                continue; // dotfiles / control paths are not notes
            };
            let note = Note::parse(path.clone(), &b.content);
            meta.insert(
                path.clone(),
                (note.display_title(), note.tags(), b.mtime_secs),
            );
            notes.push(note);
        }
        let built = Arc::new(BuiltGraph {
            graph: Graph::build(notes.iter()),
            meta,
        });
        self.graph_at_cache.borrow_mut().put(oid, built.clone());
        Ok(built)
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

    /// Ensure the semantic index is built from the current notes (lazy, once).
    fn ensure_semantic_built(&self) -> Result<(), PortError> {
        if self.semantic_built.get() {
            return Ok(());
        }
        let notes: Vec<Note> = self.with_notes(|m| m.values().cloned().collect())?;
        self.semantic.borrow_mut().reindex(&notes)?;
        self.semantic_built.set(true);
        Ok(())
    }

    /// Format `why` provenance from shared terms.
    fn why_from(shared: &[String]) -> Option<String> {
        if shared.is_empty() {
            None
        } else {
            Some(format!("shared: {}", shared.join(", ")))
        }
    }

    /// Suggested non-explicit edges within `scope`. Excludes self, already-linked
    /// pairs, and sub-floor similarities.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if a `Note` scope's path is unknown; [`PortError`]
    /// on a port failure.
    pub fn suggestions(&self, scope: &Scope) -> Result<Vec<SuggestedEdgeData>, PortError> {
        self.ensure_semantic_built()?;
        let graph = self.graph()?;
        match scope {
            Scope::Note(path) => {
                // Unknown focus → NotFound (mirrors read_note semantics).
                if !self.with_notes(|m| m.contains_key(path))? {
                    return Err(PortError::NotFound(path.as_str().to_string()));
                }
                let mut linked: HashSet<NotePath> = HashSet::new();
                linked.extend(graph.forward_links(path).iter().cloned());
                linked.extend(graph.backlinks(path).iter().cloned());
                let mut out = Vec::new();
                for s in self.semantic.borrow().neighbors(path, SUGGEST_TOP_K)? {
                    if s.score < SUGGEST_FLOOR || &s.path == path || linked.contains(&s.path) {
                        continue;
                    }
                    out.push(SuggestedEdgeData {
                        from: path.clone(),
                        to: s.path,
                        weight: s.score,
                        why: Self::why_from(&s.shared),
                    });
                }
                Ok(out)
            }
            Scope::Vault => {
                let paths: Vec<NotePath> = self.with_notes(|m| m.keys().cloned().collect())?;
                let mut seen: HashSet<(NotePath, NotePath)> = HashSet::new();
                let mut out: Vec<SuggestedEdgeData> = Vec::new();
                for focus in &paths {
                    let mut linked: HashSet<NotePath> = HashSet::new();
                    linked.extend(graph.forward_links(focus).iter().cloned());
                    linked.extend(graph.backlinks(focus).iter().cloned());
                    for s in self.semantic.borrow().neighbors(focus, SUGGEST_TOP_K)? {
                        if s.score < SUGGEST_FLOOR || &s.path == focus || linked.contains(&s.path) {
                            continue;
                        }
                        // Canonical undirected pair (from < to) for dedup.
                        let (from, to) = if focus < &s.path {
                            (focus.clone(), s.path.clone())
                        } else {
                            (s.path.clone(), focus.clone())
                        };
                        if !seen.insert((from.clone(), to.clone())) {
                            continue;
                        }
                        out.push(SuggestedEdgeData {
                            from,
                            to,
                            weight: s.score,
                            why: Self::why_from(&s.shared),
                        });
                    }
                }
                out.sort_by(|a, b| {
                    b.weight
                        .partial_cmp(&a.weight)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| (&a.from, &a.to).cmp(&(&b.from, &b.to)))
                });
                out.truncate(VAULT_EDGE_CAP);
                Ok(out)
            }
        }
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

    /// Whether the working tree has uncommitted changes. The daemon's auto-commit
    /// of external edits checks this to avoid creating an empty commit.
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter fails.
    pub fn has_uncommitted_changes(&self) -> Result<bool, PortError> {
        self.vcs.is_dirty()
    }

    /// A note's commit history (newest first).
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter fails.
    pub fn note_history(&self, path: &NotePath) -> Result<Vec<Revision>, PortError> {
        self.vcs.history(path.as_str())
    }

    /// The whole repository's commit history (newest first), capped at `limit`.
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter fails.
    pub fn vault_history(&self, limit: Option<u32>) -> Result<Vec<Revision>, PortError> {
        self.vcs.vault_history(limit)
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

    /// Inject the agent runtime backing the plugin `host/agent` callback.
    /// Absent by default; a plugin `agent` call then fails as "no runtime".
    pub fn set_runtime(&mut self, runtime: Arc<dyn AgentRuntime + Send + Sync>) {
        self.runtime = Some(runtime);
    }

    /// Replace the semantic index (the composition root injects the real one).
    /// Resets the lazy-build flag so the next `suggestions` call rebuilds it.
    pub fn set_semantic_index(&mut self, index: Box<dyn SemanticIndex + Send>) {
        *self.semantic.get_mut() = index;
        self.semantic_built.set(false);
    }

    /// Loaded plugins and their declared commands.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.plugins()
    }

    /// Invoke a plugin command, servicing any host-callbacks it makes mid-invoke.
    ///
    /// The host is moved out of `self` for the duration (see below). If the host
    /// *panics*, the unwind is caught and surfaced as [`PortError::Adapter`], and
    /// the host is restored — a panicking plugin must not unwind through the
    /// daemon's locked engine and poison the mutex (audit: mutex-poisoning DoS).
    ///
    /// # Errors
    /// Propagates [`PortError`] from the plugin host, or [`PortError::Adapter`]
    /// if the host panicked.
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
        // Catch a panicking host so it surfaces as an error instead of unwinding
        // through the daemon's locked engine (which would poison the mutex and
        // brick the daemon). `self`/`host` are only borrowed for the call, so
        // `AssertUnwindSafe` is sound: on panic the engine's `RefCell` borrow
        // guards unwind cleanly and `host` (owned here, not by the closure) is
        // restored below.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.invoke(plugin, command, args, &mut cb)
            // cb is dropped here, releasing the &mut self borrow
        }));
        self.plugins = host;
        result.unwrap_or_else(|_| Err(PortError::Adapter("plugin host panicked".into())))
    }

    /// Deliver a cairn event to subscribed plugins (best-effort). Event-handler
    /// callbacks route through the engine, and any events they emit go to `sink`.
    pub fn dispatch_plugin_event(&mut self, event: &PluginEvent, sink: &mut dyn EventSink) {
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        // Catch a panicking host (see `invoke_plugin_command`) so a plugin can't
        // poison the daemon's engine mutex via event dispatch. Best-effort: this
        // method has no error channel, so failures are logged and swallowed.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.dispatch_event(event, &mut cb)
        }));
        self.plugins = host;
        match outcome {
            // Per-plugin handler errors are reported here rather than dropped
            // (audit G4), uniformly for every host implementation.
            Ok(errors) => {
                for EventDispatchError { plugin, error } in errors {
                    tracing::warn!(plugin = %plugin, error = %error, "plugin event handler failed");
                }
            }
            Err(_) => tracing::error!(?event, "plugin host panicked handling event"),
        }
    }

    /// Render a note: read its raw content, then transform it through the loaded
    /// content processors (host -> plugin). Read-only — processors may make gated
    /// read callbacks but cannot write, so this emits no events and is
    /// side-effect-free. A panicking host is caught and surfaced as an error (as
    /// in `invoke_plugin_command`), and the host is restored.
    ///
    /// # Errors
    /// [`PortError`] if the note is missing, or [`PortError::Adapter`] if the host
    /// panicked. Individual processor failures are logged and skipped by the host
    /// (fail-soft), not surfaced here.
    pub fn render_note(&mut self, path: &NotePath) -> Result<String, PortError> {
        let raw = self.read_note(path)?; // raw read = the recursion floor
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        // Writes are denied during processing, so this sink is never touched.
        let mut discard: Vec<Event> = Vec::new();
        // `AssertUnwindSafe` is sound: `self`/`discard` are only borrowed for the
        // call, so `host` (owned here, not by the closure) is restored below on
        // panic, same as `invoke_plugin_command`.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks {
                engine: self,
                sink: &mut discard,
            };
            host.process_content(path.as_str(), &raw, &mut cb)
        }));
        self.plugins = host;
        result.unwrap_or_else(|_| Err(PortError::Adapter("plugin host panicked".into())))
    }
}

/// Bridges plugin host-callbacks to engine operations. Held only for the duration
/// of a single `invoke_plugin_command` or `dispatch_plugin_event`, while
/// `self.plugins` is a `NoopPluginHost`.
struct EngineCallbacks<'a> {
    engine: &'a mut Engine,
    sink: &'a mut dyn EventSink,
}

impl PluginCallbacks for EngineCallbacks<'_> {
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

    fn run_agent(&mut self, prompt: &str) -> Result<String, PortError> {
        let rt = self
            .engine
            .runtime
            .clone()
            .ok_or_else(|| PortError::Adapter("no agent runtime configured".into()))?;

        // Buffer the streamed run into one string; `host/agent` is request/
        // response, not streaming. A `Failed` event becomes an error; other
        // event kinds are ignored (AgentEvent is #[non_exhaustive]).
        struct Buf {
            text: String,
            failed: Option<String>,
        }
        impl AgentSink for Buf {
            fn emit(&mut self, event: AgentEvent) {
                match event {
                    AgentEvent::TextDelta(s) => self.text.push_str(&s),
                    AgentEvent::Failed { message } => self.failed = Some(message),
                    _ => {}
                }
            }
        }

        let mut buf = Buf {
            text: String::new(),
            failed: None,
        };
        rt.answer(prompt, &mut buf)?;
        if let Some(message) = buf.failed {
            return Err(PortError::Adapter(message.into()));
        }
        Ok(buf.text)
    }
}

type RestoredState = HashMap<NotePath, (u64, FileStamp)>;

/// Why a persisted `state.json` could not be restored.
#[derive(Debug)]
enum StateRejection {
    /// A routine, expected mismatch — the schema / hash regime changed, so the
    /// persisted hashes are stale. Nothing is wrong with the file; rebuild
    /// quietly rather than alarming the user or quarantining it.
    Stale,
    /// The blob is malformed or internally invalid. Preserve it for diagnosis
    /// and warn, rather than discarding a genuine problem silently.
    Corrupt(String),
}

/// The warning text emitted when a persisted `state.json` is rejected as
/// corrupt. Pure so it is unit-testable as captured output; the caller writes
/// it to stderr.
fn state_rejected_warning(reason: &str, preserved: Option<&str>) -> String {
    match preserved {
        Some(dest) => format!(
            "warning: persisted state.json rejected ({reason}); preserved at {dest}, rebuilding index"
        ),
        None => format!("warning: persisted state.json rejected ({reason}); rebuilding index"),
    }
}

fn parse_state(json: &str) -> Result<RestoredState, StateRejection> {
    let payload: StatePayload =
        serde_json::from_str(json).map_err(|e| StateRejection::Corrupt(e.to_string()))?;
    if payload.schema_version != STATE_SCHEMA_VERSION {
        return Err(StateRejection::Stale); // different/absent hash regime → reconcile_cold rebuilds
    }
    let mut map = HashMap::with_capacity(payload.entries.len());
    for e in payload.entries {
        let path = NotePath::new(&e.path).map_err(|err| {
            StateRejection::Corrupt(format!("invalid note path {}: {err}", e.path))
        })?;
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
    use cairn_infra::{GitVcs, InMemoryIndex, LexicalSemanticIndex, LocalFsStore, TantivyIndex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A SemanticIndex that records upsert/remove calls via a shared handle, so a
    /// test can inspect them after the index is boxed into the engine.
    #[derive(Clone, Default)]
    struct RecordingSemantic {
        upserts: Arc<Mutex<Vec<String>>>,
        removes: Arc<Mutex<Vec<String>>>,
    }
    impl cairn_ports::SemanticIndex for RecordingSemantic {
        fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
            self.upserts
                .lock()
                .unwrap()
                .push(note.path.as_str().to_string());
            Ok(())
        }
        fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
            self.removes.lock().unwrap().push(path.as_str().to_string());
            Ok(())
        }
        fn reindex(&mut self, _notes: &[Note]) -> Result<(), PortError> {
            Ok(())
        }
        fn neighbors(
            &self,
            _f: &NotePath,
            _k: usize,
        ) -> Result<Vec<cairn_ports::Similarity>, PortError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn writes_skip_semantic_index_until_built() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let rec = RecordingSemantic::default();
        let upserts = rec.upserts.clone(); // shared handle survives the move into the engine
        eng.set_semantic_index(Box::new(rec));

        let mut ev = Vec::new();
        // semantic_built is false (no suggestions() call yet) → the write must NOT upsert.
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership", &mut ev)
            .unwrap();
        assert!(
            upserts.lock().unwrap().is_empty(),
            "a write before the lazy build must not reach the semantic index"
        );
    }

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
        fn quarantine_meta(&self) -> Result<Option<String>, PortError> {
            self.inner.quarantine_meta()
        }
    }

    #[test]
    fn warning_message_names_reason_and_preserved_path() {
        // The emitted (stderr) warning text — asserted here as captured output
        // via the pure formatter that feeds eprintln!.
        let msg = state_rejected_warning(
            "expected value at line 1",
            Some("/v/.cairn/state.json.corrupt"),
        );
        assert!(msg.contains("state.json"));
        assert!(msg.contains("expected value at line 1"));
        assert!(msg.contains("/v/.cairn/state.json.corrupt"));

        let msg_none = state_rejected_warning("bad", None);
        assert!(msg_none.contains("bad"));
        assert!(msg_none.contains("rebuild"));
    }

    #[test]
    fn corrupt_state_is_preserved_and_rebuild_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
        // Plant a corrupt state.json the warm path cannot parse.
        store.write_meta("{ not valid json").unwrap();

        let mut eng = Engine::new(
            store,
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        let mut ev = Vec::new();
        eng.reconcile(&mut ev).unwrap(); // must not error; falls back to cold rebuild

        // The note got indexed by the cold rebuild.
        assert_eq!(ev, vec![Event::Reindexed(1)]);
        // The corrupt file was preserved, not silently dropped.
        let corrupt = tmp.path().join(".cairn").join("state.json.corrupt");
        assert_eq!(
            std::fs::read_to_string(&corrupt).unwrap(),
            "{ not valid json"
        );
        // A fresh, valid state.json was written by the rebuild.
        let fresh = tmp.path().join(".cairn").join("state.json");
        assert!(parse_state(&std::fs::read_to_string(&fresh).unwrap()).is_ok());
    }

    #[test]
    fn parse_state_returns_reason_on_bad_json() {
        match parse_state("{ not json").unwrap_err() {
            StateRejection::Corrupt(reason) => {
                assert!(!reason.is_empty(), "reason must be non-empty")
            }
            StateRejection::Stale => panic!("malformed JSON should be Corrupt, not Stale"),
        }
    }

    #[test]
    fn stale_schema_is_not_quarantined() {
        // A schema/hash-regime bump is an expected migration (every pre-versioning
        // state.json hits it on first upgrade): rebuild quietly, do NOT rename the
        // file to .corrupt — that is reserved for genuinely malformed state.
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
        // Valid JSON, but a future/legacy schema version the warm path won't trust.
        store
            .write_meta(&format!(
                "{{\"schema_version\":{},\"entries\":[]}}",
                STATE_SCHEMA_VERSION + 1
            ))
            .unwrap();

        let mut eng = Engine::new(
            store,
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();

        // No quarantine file left behind for a benign migration.
        assert!(!tmp
            .path()
            .join(".cairn")
            .join("state.json.corrupt")
            .exists());
        // A fresh, current-version state.json was written by the rebuild.
        let fresh = tmp.path().join(".cairn").join("state.json");
        assert!(parse_state(&std::fs::read_to_string(&fresh).unwrap()).is_ok());
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

    fn engine(dir: &std::path::Path) -> Engine {
        Engine::new(
            LocalFsStore::open(dir).unwrap(),
            InMemoryIndex::default(),
            GitVcs::open_or_init(dir).unwrap(),
        )
    }

    #[test]
    fn has_uncommitted_changes_reflects_working_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        assert!(!eng.has_uncommitted_changes().unwrap(), "fresh repo clean");
        eng.write_note(&NotePath::new("a.md").unwrap(), "hi", &mut events)
            .unwrap();
        assert!(eng.has_uncommitted_changes().unwrap(), "dirty after write");
        eng.commit("add a", &mut events).unwrap();
        assert!(
            !eng.has_uncommitted_changes().unwrap(),
            "clean after commit"
        );
    }

    #[test]
    fn exists_on_disk_reflects_the_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        assert!(!eng.exists_on_disk(&a), "absent before any write");
        eng.write_note(&a, "hi", &mut events).unwrap();
        assert!(eng.exists_on_disk(&a), "present after write");
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
                contributions: vec![],
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
    fn plugin_agent_callback_runs_engine_runtime() {
        use cairn_ports::{AgentEvent, AgentRuntime, AgentSink};

        // A host that, on invoke, asks the engine to run the agent and echoes it.
        struct AgentHost;
        impl PluginHost for AgentHost {
            fn plugins(&self) -> Vec<PluginInfo> {
                vec![PluginInfo {
                    id: "p".into(),
                    name: "P".into(),
                    version: "0".into(),
                    commands: Vec::new(),
                    contributions: vec![],
                }]
            }
            fn invoke(
                &mut self,
                _plugin: &str,
                _command: &str,
                _args: &serde_json::Value,
                callbacks: &mut dyn cairn_ports::PluginCallbacks,
            ) -> Result<serde_json::Value, PortError> {
                let answer = callbacks.run_agent("hello")?;
                Ok(serde_json::json!({ "answer": answer }))
            }
        }

        // Streams "Hel" + "lo" then completes; the host buffers it into "Hello".
        struct TwoChunk;
        impl AgentRuntime for TwoChunk {
            fn answer(&self, _prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
                sink.emit(AgentEvent::TextDelta("Hel".into()));
                sink.emit(AgentEvent::TextDelta("lo".into()));
                sink.emit(AgentEvent::Completed);
                Ok(())
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(AgentHost));
        let mut sink: Vec<Event> = Vec::new();

        // No runtime configured -> Adapter error.
        let denied = eng.invoke_plugin_command("p", "ask", &serde_json::Value::Null, &mut sink);
        assert!(
            matches!(denied, Err(PortError::Adapter(_))),
            "no runtime => Adapter, got {denied:?}"
        );

        // Runtime configured -> buffered answer.
        eng.set_runtime(Arc::new(TwoChunk));
        let out = eng
            .invoke_plugin_command("p", "ask", &serde_json::Value::Null, &mut sink)
            .unwrap();
        assert_eq!(out, serde_json::json!({ "answer": "Hello" }));
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
                contributions: vec![],
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
                contributions: vec![],
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
        ) -> Vec<EventDispatchError> {
            let _ = callbacks.write_note("seen.md", "seen");
            Vec::new()
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

    /// A host whose event handler fails — surfaces a per-plugin delivery error.
    struct FailingEventHost;
    impl PluginHost for FailingEventHost {
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
            _callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Vec<EventDispatchError> {
            vec![EventDispatchError {
                plugin: "broken-plugin".to_string(),
                error: PortError::Adapter(AdapterError::message("handler exploded")),
            }]
        }
    }

    #[test]
    #[tracing_test::traced_test]
    fn dispatch_event_reports_handler_errors() {
        // A handler error must be logged, not silently dropped (audit G4).
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(FailingEventHost));
        let mut events: Vec<Event> = Vec::new();
        eng.dispatch_plugin_event(
            &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
            &mut events,
        );
        assert!(logs_contain("plugin event handler failed"));
        assert!(logs_contain("broken-plugin"));
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

    /// A host whose invoke panics — simulates a buggy or malicious plugin host.
    struct PanickingHost;
    impl PluginHost for PanickingHost {
        fn plugins(&self) -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "boom".into(),
                name: "boom".into(),
                version: "0".into(),
                commands: Vec::new(),
                contributions: vec![],
            }]
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            _args: &serde_json::Value,
            _callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            panic!("plugin host panicked mid-invoke");
        }
    }

    #[test]
    fn plugin_panic_is_caught_and_engine_survives() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        eng.write_note(&a, "hello body", &mut events).unwrap();

        eng.set_plugin_host(Box::new(PanickingHost));
        let mut sink: Vec<Event> = Vec::new();
        // The panic must surface as an error, not unwind through the caller (which,
        // in the daemon, holds the engine mutex — an unwind would poison it).
        let res = eng.invoke_plugin_command("boom", "x", &serde_json::Value::Null, &mut sink);
        assert!(matches!(res, Err(PortError::Adapter(_))));

        // The engine is still usable afterward — its state was not corrupted.
        assert_eq!(eng.read_note(&a).unwrap(), "hello body");
    }

    /// A stub host whose process_content uppercases and appends the result of a
    /// read-only callback — proves render invokes processors and services reads.
    struct UpcaseHost;
    impl PluginHost for UpcaseHost {
        fn plugins(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        fn invoke(
            &mut self,
            _p: &str,
            _c: &str,
            _a: &serde_json::Value,
            _cb: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            unreachable!()
        }
        fn process_content(
            &mut self,
            _path: &str,
            content: &str,
            _cb: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<String, PortError> {
            Ok(content.to_uppercase())
        }
    }

    #[test]
    fn render_note_applies_processors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events: Vec<Event> = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hello", &mut events)
            .unwrap();
        eng.set_plugin_host(Box::new(UpcaseHost));
        let out = eng.render_note(&NotePath::new("a.md").unwrap()).unwrap();
        assert_eq!(out, "HELLO");
        // Raw read is unchanged (recursion floor / raw vs rendered).
        assert_eq!(
            eng.read_note(&NotePath::new("a.md").unwrap()).unwrap(),
            "hello"
        );
    }

    #[test]
    fn render_note_is_identity_with_noop_host() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events: Vec<Event> = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hello", &mut events)
            .unwrap();
        let out = eng.render_note(&NotePath::new("a.md").unwrap()).unwrap();
        assert_eq!(out, "hello"); // default NoopPluginHost::process_content is identity
    }

    /// A host whose process_content panics — simulates a buggy or malicious
    /// content processor.
    struct PanickingProcessHost;
    impl PluginHost for PanickingProcessHost {
        fn plugins(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        fn invoke(
            &mut self,
            _plugin: &str,
            _command: &str,
            _args: &serde_json::Value,
            _callbacks: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            unreachable!()
        }
        fn process_content(
            &mut self,
            _path: &str,
            _content: &str,
            _cb: &mut dyn cairn_ports::PluginCallbacks,
        ) -> Result<String, PortError> {
            panic!("plugin host panicked mid-process_content");
        }
    }

    #[test]
    fn render_note_panic_is_caught_and_engine_survives() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        eng.write_note(&a, "hello body", &mut events).unwrap();

        eng.set_plugin_host(Box::new(PanickingProcessHost));
        // The panic must surface as an error, not unwind through the caller (which,
        // in the daemon, holds the engine mutex — an unwind would poison it).
        let res = eng.render_note(&a);
        assert!(matches!(res, Err(PortError::Adapter(_))));

        // The engine is still usable afterward — its state was not corrupted.
        assert_eq!(eng.read_note(&a).unwrap(), "hello body");
        eng.set_plugin_host(Box::new(NoopPluginHost));
        assert_eq!(eng.render_note(&a).unwrap(), "hello body");
    }

    use std::sync::atomic::{AtomicUsize as Au, Ordering as Ord2};

    /// A `Vcs` that counts `read_tree_at` calls, delegating to an inner `GitVcs`.
    struct CountingVcs {
        inner: GitVcs,
        tree_reads: Arc<Au>,
    }
    impl Vcs for CountingVcs {
        fn commit_all(&mut self, m: &str) -> Result<String, PortError> {
            self.inner.commit_all(m)
        }
        fn history(&self, p: &str) -> Result<Vec<cairn_ports::Revision>, PortError> {
            self.inner.history(p)
        }
        fn vault_history(
            &self,
            limit: Option<u32>,
        ) -> Result<Vec<cairn_ports::Revision>, PortError> {
            self.inner.vault_history(limit)
        }
        fn show(&self, p: &str, r: &str) -> Result<String, PortError> {
            self.inner.show(p, r)
        }
        fn is_dirty(&self) -> Result<bool, PortError> {
            self.inner.is_dirty()
        }
        fn resolve(&self, r: &str) -> Result<String, PortError> {
            self.inner.resolve(r)
        }
        fn read_tree_at(&self, r: &str) -> Result<Vec<cairn_ports::HistoricalBlob>, PortError> {
            self.tree_reads.fetch_add(1, Ord2::SeqCst);
            self.inner.read_tree_at(r)
        }
    }

    #[test]
    fn graph_at_builds_historical_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "[[b]]", &mut ev).unwrap();
        eng.write_note(&b, "x", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();
        eng.write_note(&a, "no link now", &mut ev).unwrap();
        eng.commit("c2", &mut ev).unwrap();

        // As of c1: a -> b present.
        let at = eng.graph_at(&c1, &GraphScope::Full).unwrap();
        assert!(at
            .edges
            .iter()
            .any(|(x, y)| x.as_str() == "a.md" && y.as_str() == "b.md"));
        // At HEAD: the link is gone.
        let head = eng.graph_view(&GraphScope::Full).unwrap();
        assert!(!head
            .edges
            .iter()
            .any(|(x, y)| x.as_str() == "a.md" && y.as_str() == "b.md"));
        // Nodes are enriched with a title (stem fallback here).
        assert!(at
            .nodes
            .iter()
            .any(|n| n.path.as_str() == "a.md" && n.title == "a"));
        // Degree flows through: a->b link gives b.md degree >= 1.
        assert!(at
            .nodes
            .iter()
            .any(|n| n.path.as_str() == "b.md" && n.degree >= 1));
    }

    #[test]
    fn graph_at_caches_by_oid() {
        let tmp = tempfile::tempdir().unwrap();
        let reads = Arc::new(Au::new(0));
        let vcs = CountingVcs {
            inner: GitVcs::open_or_init(tmp.path()).unwrap(),
            tree_reads: reads.clone(),
        };
        let mut eng = Engine::new(
            LocalFsStore::open(tmp.path()).unwrap(),
            InMemoryIndex::default(),
            vcs,
        );
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "[[b]]", &mut ev)
            .unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();

        let _ = eng.graph_at(&c1, &GraphScope::Full).unwrap();
        let _ = eng.graph_at(&c1, &GraphScope::Full).unwrap(); // same oid
        let _ = eng
            .graph_at(
                &c1,
                &GraphScope::Focused {
                    path: NotePath::new("a.md").unwrap(),
                    depth: 0,
                },
            )
            .unwrap(); // different scope, same oid -> still cached
        assert_eq!(reads.load(Ord2::SeqCst), 1, "tree walked once per oid");
    }

    #[test]
    fn graph_diff_reports_added_link_between_revisions() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        let c = NotePath::new("c.md").unwrap();
        eng.write_note(&a, "[[b]]", &mut ev).unwrap();
        eng.write_note(&b, "x", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();
        eng.write_note(&c, "[[b]]", &mut ev).unwrap();
        let c2 = eng.commit("c2", &mut ev).unwrap();

        let d = eng.graph_diff(&c1, &c2, &GraphScope::Full).unwrap();
        assert!(d.nodes_added.iter().any(|n| n.path.as_str() == "c.md"));
        assert!(d
            .edges_added
            .iter()
            .any(|(x, y)| x.as_str() == "c.md" && y.as_str() == "b.md"));
        assert!(d.nodes_removed.is_empty());
    }

    #[test]
    fn graph_diff_reports_changed_nodes_between_revisions() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut ev = Vec::new();
        let b = NotePath::new("b.md").unwrap();
        let x = NotePath::new("x.md").unwrap();
        let gone = NotePath::new("gone.md").unwrap();
        // c1: anchor b, x titled "Old" linking b, plus a note that will be removed.
        eng.write_note(&b, "anchor", &mut ev).unwrap();
        eng.write_note(&x, "# Old\n[[b]]", &mut ev).unwrap();
        eng.write_note(&gone, "temp", &mut ev).unwrap();
        let c1 = eng.commit("c1", &mut ev).unwrap();
        // c2: x retitled "New" (same path, same links), a genuine add, gone removed.
        eng.write_note(&x, "# New\n[[b]]", &mut ev).unwrap();
        eng.write_note(&NotePath::new("added.md").unwrap(), "fresh", &mut ev)
            .unwrap();
        eng.delete_note(&gone, &mut ev).unwrap();
        let c2 = eng.commit("c2", &mut ev).unwrap();

        let d = eng.graph_diff(&c1, &c2, &GraphScope::Full).unwrap();

        // x kept its path but changed title: it is a *changed* node, and it
        // carries the new-revision title.
        assert!(
            d.nodes_changed
                .iter()
                .any(|n| n.path.as_str() == "x.md" && n.title == "New"),
            "x.md should be reported as changed with its new title"
        );
        // A changed node is neither added nor removed.
        assert!(!d.nodes_added.iter().any(|n| n.path.as_str() == "x.md"));
        assert!(!d.nodes_removed.iter().any(|n| n.path.as_str() == "x.md"));
        // Genuine add/remove still bucket correctly and are not misreported as changed.
        assert!(d.nodes_added.iter().any(|n| n.path.as_str() == "added.md"));
        assert!(d.nodes_removed.iter().any(|n| n.path.as_str() == "gone.md"));
        assert!(!d
            .nodes_changed
            .iter()
            .any(|n| n.path.as_str() == "added.md"));
        assert!(!d.nodes_changed.iter().any(|n| n.path.as_str() == "gone.md"));
    }

    fn lexical_engine(dir: &std::path::Path) -> Engine {
        let mut e = engine(dir);
        e.set_semantic_index(Box::new(LexicalSemanticIndex::new()));
        e
    }

    #[test]
    fn suggestions_exclude_self_and_already_linked() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        // a links to b explicitly; c is unlinked but topically identical to a.
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "rust ownership borrow [[b]]",
            &mut ev,
        )
        .unwrap();
        eng.write_note(
            &NotePath::new("b.md").unwrap(),
            "rust ownership borrow lifetime",
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
        // self never appears; already-linked b never appears; c (unlinked, related) does.
        assert!(s.iter().all(|e| e.to.as_str() != "a.md"));
        assert!(
            s.iter().all(|e| e.to.as_str() != "b.md"),
            "already-linked excluded"
        );
        assert!(
            s.iter().any(|e| e.to.as_str() == "c.md"),
            "unlinked related surfaced"
        );
        assert!(s.iter().all(|e| e.from.as_str() == "a.md"));
    }

    #[test]
    fn suggestions_below_floor_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "rust ownership borrow",
            &mut ev,
        )
        .unwrap();
        eng.write_note(
            &NotePath::new("z.md").unwrap(),
            "tomato basil pasta garlic",
            &mut ev,
        )
        .unwrap();
        let s = eng
            .suggestions(&Scope::Note(NotePath::new("a.md").unwrap()))
            .unwrap();
        assert!(
            s.iter().all(|e| e.to.as_str() != "z.md"),
            "unrelated note below floor excluded"
        );
    }

    #[test]
    fn vault_scope_dedups_pairs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "rust ownership borrow lifetime",
            &mut ev,
        )
        .unwrap();
        eng.write_note(
            &NotePath::new("b.md").unwrap(),
            "rust ownership borrow lifetime",
            &mut ev,
        )
        .unwrap();
        let s = eng.suggestions(&Scope::Vault).unwrap();
        // exactly one undirected pair (a,b), canonical from < to.
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].from.as_str(), "a.md");
        assert_eq!(s[0].to.as_str(), "b.md");
    }

    #[test]
    fn suggestions_lazy_build_then_stays_live() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(
            &NotePath::new("a.md").unwrap(),
            "rust ownership borrow",
            &mut ev,
        )
        .unwrap();
        // First call lazily builds from existing notes.
        let _ = eng
            .suggestions(&Scope::Note(NotePath::new("a.md").unwrap()))
            .unwrap();
        // A later write must be reflected (index is now live).
        eng.write_note(
            &NotePath::new("d.md").unwrap(),
            "rust ownership borrow lifetime",
            &mut ev,
        )
        .unwrap();
        let s = eng
            .suggestions(&Scope::Note(NotePath::new("a.md").unwrap()))
            .unwrap();
        assert!(
            s.iter().any(|e| e.to.as_str() == "d.md"),
            "post-build write surfaced"
        );
    }
}
