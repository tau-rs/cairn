//! The transport-blind dispatcher: maps the wire contract to engine
//! use-cases and engine events to wire events. No I/O, no async.

use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    Command, CommandResponse, ContractError, Event as WireEvent, GraphEdge, NoteSummary,
    PluginCommandSummary, PluginSummary, Query, QueryResponse, Revision, SearchResult, TagCount,
};
use cairn_domain::NotePath;
use cairn_ports::{AdapterError, AgentRuntime, AgentSink, FsChange, PortError, WatchHandle};

/// Drain a watch handle until its sender drops, invoking `on_change` for each
/// debounced change. Blocking — run on a dedicated thread (CLI `watch`) or via
/// `tokio::task::spawn_blocking` (daemon).
///
/// The engine-apply + event-forwarding lives in the caller's `on_change`: the
/// daemon locks a shared engine per change while the CLI owns it, and output
/// differs — centralizing only the drain keeps this testable.
pub fn run_watch_loop(handle: &WatchHandle, mut on_change: impl FnMut(&FsChange)) {
    while let Ok(change) = handle.changes.recv() {
        on_change(&change);
    }
}

/// Tracks whether external changes have been seen since the last commit, so a
/// burst of changes coalesces into a single commit. Decision logic only — the
/// timing lives in [`run_watch_loop_timeout`].
#[derive(Debug, Default)]
struct Coalescer {
    dirty: bool,
}

impl Coalescer {
    /// Record that an external change was applied.
    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// On a quiet tick: returns whether a commit is due (changes were seen since
    /// the last commit), clearing the dirty flag.
    fn take_if_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

/// Like [`run_watch_loop`], but coalesces bursts: after `quiet` elapses with no
/// new change, `on_quiet` fires once (used to auto-commit externally-detected
/// edits). `on_quiet` also fires on shutdown if changes are pending. Blocking —
/// run on a dedicated thread.
pub fn run_watch_loop_timeout(
    handle: &WatchHandle,
    quiet: std::time::Duration,
    mut on_change: impl FnMut(&FsChange),
    mut on_quiet: impl FnMut(),
) {
    use std::sync::mpsc::RecvTimeoutError;
    let mut coalescer = Coalescer::default();
    loop {
        match handle.changes.recv_timeout(quiet) {
            Ok(change) => {
                on_change(&change);
                coalescer.mark_dirty();
            }
            // A full `quiet` window passed with no new change: flush if pending.
            Err(RecvTimeoutError::Timeout) => {
                if coalescer.take_if_dirty() {
                    on_quiet();
                }
            }
            // Shutdown: flush any pending changes, then stop.
            Err(RecvTimeoutError::Disconnected) => {
                if coalescer.take_if_dirty() {
                    on_quiet();
                }
                break;
            }
        }
    }
}

/// Errors surfaced when dispatching a contract request.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// A requested note/resource was missing.
    #[error("note not found: {0}")]
    NotFound(String),
    /// The request was malformed (e.g. an invalid note path).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// An internal/adapter failure. Carries the original adapter error as a
    /// typed `#[source]` (see [`AdapterError`]) so it stays downcastable when
    /// logged at the daemon edge; it is flattened to a string only at the wire
    /// boundary (`ContractError`), which never leaks internals to clients.
    #[error(transparent)]
    Internal(AdapterError),
}

impl From<PortError> for ServiceError {
    fn from(e: PortError) -> Self {
        match e {
            PortError::NotFound(s) => ServiceError::NotFound(s),
            PortError::AlreadyExists(s) => ServiceError::InvalidRequest(s),
            PortError::Adapter(a) => ServiceError::Internal(a),
        }
    }
}

impl From<ServiceError> for ContractError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::NotFound(what) => ContractError::NotFound { what },
            ServiceError::InvalidRequest(message) => ContractError::InvalidRequest { message },
            // Flatten to the message only here, at the wire boundary.
            ServiceError::Internal(a) => ContractError::Internal {
                message: a.to_string(),
            },
        }
    }
}

/// Convert an [`AppEvent`] to its wire representation [`WireEvent`].
///
/// This is a free function rather than a `From` impl because both types are
/// defined in external crates (`cairn-app` and `cairn-contract`), which
/// would violate Rust's orphan rule.
pub fn app_event_to_wire(e: AppEvent) -> WireEvent {
    match e {
        AppEvent::NoteChanged(p) => WireEvent::NoteChanged {
            path: p.as_str().to_string(),
        },
        AppEvent::NoteDeleted(p) => WireEvent::NoteDeleted {
            path: p.as_str().to_string(),
        },
        AppEvent::Committed(commit) => WireEvent::Committed { commit },
        AppEvent::Reindexed(n) => WireEvent::Reindexed {
            count: u32::try_from(n).unwrap_or(u32::MAX),
        },
    }
}

fn parse_path(raw: &str) -> Result<NotePath, ServiceError> {
    NotePath::new(raw).map_err(|e| ServiceError::InvalidRequest(e.to_string()))
}

/// Dispatch a mutating command, emitting produced events via `sink`.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
/// Callers serving a wire transport map the error via
/// [`ContractError::from`].
pub fn dispatch_command(
    engine: &mut Engine,
    command: &Command,
    sink: &mut dyn EventSink,
) -> Result<CommandResponse, ServiceError> {
    match command {
        Command::WriteNote { path, contents } => {
            let p = parse_path(path)?;
            engine.write_note(&p, contents, sink)?;
            Ok(CommandResponse::Done)
        }
        Command::DeleteNote { path } => {
            let p = parse_path(path)?;
            engine.delete_note(&p, sink)?;
            Ok(CommandResponse::Done)
        }
        Command::RenameNote { from, to } => {
            let from = parse_path(from)?;
            let to = parse_path(to)?;
            engine.rename_note(&from, &to, sink)?;
            Ok(CommandResponse::Done)
        }
        Command::Commit { message } => {
            let commit = engine.commit(message, sink)?;
            Ok(CommandResponse::Committed { commit })
        }
        Command::RestoreNote { path, revision } => {
            let p = parse_path(path)?;
            engine.restore_note(&p, revision, sink)?;
            Ok(CommandResponse::Done)
        }
        Command::InvokePluginCommand {
            plugin,
            command,
            args,
        } => {
            let result = engine.invoke_plugin_command(plugin, command, args, sink)?;
            Ok(CommandResponse::PluginResult { result })
        }
    }
}

/// Dispatch a read-only query.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
/// Callers serving a wire transport map the error via
/// [`ContractError::from`].
pub fn dispatch_query(engine: &Engine, query: &Query) -> Result<QueryResponse, ServiceError> {
    match query {
        Query::GetNote { path } => {
            let p = parse_path(path)?;
            let contents = engine.read_note(&p)?;
            Ok(QueryResponse::Note { contents })
        }
        Query::NoteHistory { path } => {
            let p = parse_path(path)?;
            let revisions = engine
                .note_history(&p)?
                .into_iter()
                .map(|r| Revision {
                    id: r.id,
                    message: r.message,
                    timestamp_secs: r.timestamp_secs,
                    author: r.author,
                })
                .collect();
            Ok(QueryResponse::History { revisions })
        }
        Query::NoteAt { path, revision } => {
            let p = parse_path(path)?;
            let contents = engine.note_at(&p, revision)?;
            Ok(QueryResponse::Note { contents })
        }
        Query::Search { query } => {
            let results = engine
                .search(query)?
                .into_iter()
                .map(|h| SearchResult {
                    path: h.path.as_str().to_string(),
                    score: h.score,
                    snippet: h.snippet,
                    highlights: h.highlights,
                })
                .collect();
            Ok(QueryResponse::SearchResults { results })
        }
        Query::GetBacklinks { path } => {
            let p = parse_path(path)?;
            let paths = engine
                .backlinks(&p)?
                .into_iter()
                .map(|np| np.as_str().to_string())
                .collect();
            Ok(QueryResponse::Paths { paths })
        }
        Query::ListNotes => {
            let notes = engine
                .list_notes()?
                .into_iter()
                .map(|n| NoteSummary {
                    path: n.path.as_str().to_string(),
                    title: n.display_title(),
                    tags: n.tags(),
                })
                .collect();
            Ok(QueryResponse::Notes { notes })
        }
        Query::GetGraph => {
            let graph = engine.graph()?;
            let nodes = graph
                .nodes()
                .into_iter()
                .map(|p| p.as_str().to_string())
                .collect();
            let edges = graph
                .edges()
                .into_iter()
                .map(|(from, to)| GraphEdge {
                    from: from.as_str().to_string(),
                    to: to.as_str().to_string(),
                })
                .collect();
            Ok(QueryResponse::Graph { nodes, edges })
        }
        Query::ListTags => {
            let tags = engine
                .list_tags()?
                .into_iter()
                .map(|(tag, count)| TagCount {
                    tag,
                    count: u32::try_from(count).unwrap_or(u32::MAX),
                })
                .collect();
            Ok(QueryResponse::Tags { tags })
        }
        Query::NotesByTag { tag } => {
            let paths = engine
                .notes_by_tag(tag)?
                .into_iter()
                .map(|p| p.as_str().to_string())
                .collect();
            Ok(QueryResponse::Paths { paths })
        }
        Query::ListPlugins => {
            let plugins = engine
                .list_plugins()
                .into_iter()
                .map(|p| PluginSummary {
                    id: p.id,
                    name: p.name,
                    version: p.version,
                    commands: p
                        .commands
                        .into_iter()
                        .map(|c| PluginCommandSummary {
                            id: c.id,
                            title: c.title,
                        })
                        .collect(),
                    contributions: p.contributions.into_iter().map(map_contribution).collect(),
                    capabilities: None,
                    ui_root: None,
                })
                .collect();
            Ok(QueryResponse::Plugins { plugins })
        }
    }
}

/// Build a note-grounded answer to `query`: search the cairn, read the top
/// `top_k` hits into context, prompt the agent, and stream the answer into
/// `sink`. Returns the cited note paths (the retrieval set), in rank order.
///
/// # Errors
/// [`ServiceError`] if a search/read dispatch fails or the runtime fails before
/// streaming begins.
pub fn augmented_answer(
    engine: &Engine,
    query: &str,
    runtime: &dyn AgentRuntime,
    sink: &mut dyn AgentSink,
    top_k: usize,
) -> Result<Vec<String>, ServiceError> {
    let (prompt, cited) = gather_answer_context(engine, query, top_k)?;
    runtime.answer(&prompt, sink)?;
    Ok(cited)
}

/// The engine-touching half of an answer: search the top `top_k` hits, read them
/// into context, and build the agent prompt. Returns `(prompt, cited_paths)`.
/// Pull this out so a transport can run it under the engine lock and then stream
/// the (long, lock-free) agent run separately.
///
/// # Errors
/// [`ServiceError`] if a search/read dispatch fails.
pub fn gather_answer_context(
    engine: &Engine,
    query: &str,
    top_k: usize,
) -> Result<(String, Vec<String>), ServiceError> {
    let cited: Vec<String> = match dispatch_query(
        engine,
        &Query::Search {
            query: query.to_string(),
        },
    )? {
        QueryResponse::SearchResults { results } => {
            results.into_iter().take(top_k).map(|r| r.path).collect()
        }
        _ => Vec::new(),
    };

    let mut context = String::new();
    for path in &cited {
        if let QueryResponse::Note { contents } =
            dispatch_query(engine, &Query::GetNote { path: path.clone() })?
        {
            context.push_str("## ");
            context.push_str(path);
            context.push('\n');
            context.push_str(&contents);
            context.push_str("\n\n");
        }
    }

    let prompt = build_answer_prompt(&context, query);
    Ok((prompt, cited))
}

/// Map a port [`AgentEvent`] to its wire [`AnswerEvent`]. `None` for kinds with
/// no wire form — `AgentEvent` is `#[non_exhaustive]`, so unknown upstream kinds
/// are skipped rather than panicking (mirroring the CLI's wildcard arm).
///
/// Does not produce [`AnswerEvent::Sources`]: it has no `AgentEvent` counterpart.
/// Callers emit that leading frame themselves from the `cited` paths returned by
/// [`gather_answer_context`].
#[must_use]
pub fn agent_event_to_wire(e: cairn_ports::AgentEvent) -> Option<cairn_contract::AnswerEvent> {
    use cairn_contract::AnswerEvent as W;
    use cairn_ports::AgentEvent as A;
    Some(match e {
        A::TextDelta(text) => W::TextDelta { text },
        A::ToolStarted { tool } => W::ToolStarted { tool },
        A::ToolCompleted { tool, ok } => W::ToolCompleted { tool, ok },
        A::TurnCompleted => W::TurnCompleted,
        A::Completed => W::Completed,
        A::Failed { message } => W::Failed { message },
        _ => return None,
    })
}

/// Assemble the agent prompt from retrieved `context` and the user `query`.
fn build_answer_prompt(context: &str, query: &str) -> String {
    if context.is_empty() {
        format!("Answer the question.\n\nQuestion: {query}")
    } else {
        format!(
            "Answer the question using the notes below. Cite note paths when relevant.\n\n\
             Notes:\n{context}\nQuestion: {query}"
        )
    }
}

fn map_contribution(
    c: cairn_plugin_protocol::PluginContribution,
) -> cairn_contract::PluginContribution {
    cairn_contract::PluginContribution {
        id: c.id,
        slot: map_slot(c.slot),
        widget: map_widget(c.widget),
        title: c.title,
        icon: c.icon.map(map_icon),
        order: c.order,
    }
}

fn map_slot(s: cairn_plugin_protocol::PluginSlot) -> cairn_contract::PluginSlot {
    match s {
        cairn_plugin_protocol::PluginSlot::SidebarSection => {
            cairn_contract::PluginSlot::SidebarSection
        }
        cairn_plugin_protocol::PluginSlot::TopbarAction => cairn_contract::PluginSlot::TopbarAction,
        cairn_plugin_protocol::PluginSlot::Command => cairn_contract::PluginSlot::Command,
    }
}

fn map_icon(i: cairn_plugin_protocol::PluginIcon) -> cairn_contract::PluginIcon {
    match i {
        cairn_plugin_protocol::PluginIcon::Tag => cairn_contract::PluginIcon::Tag,
        cairn_plugin_protocol::PluginIcon::Search => cairn_contract::PluginIcon::Search,
        cairn_plugin_protocol::PluginIcon::Note => cairn_contract::PluginIcon::Note,
        cairn_plugin_protocol::PluginIcon::Folder => cairn_contract::PluginIcon::Folder,
        cairn_plugin_protocol::PluginIcon::Link => cairn_contract::PluginIcon::Link,
        cairn_plugin_protocol::PluginIcon::Star => cairn_contract::PluginIcon::Star,
        cairn_plugin_protocol::PluginIcon::Info => cairn_contract::PluginIcon::Info,
        cairn_plugin_protocol::PluginIcon::Play => cairn_contract::PluginIcon::Play,
    }
}

fn map_list_item(li: cairn_plugin_protocol::PluginListItem) -> cairn_contract::PluginListItem {
    cairn_contract::PluginListItem {
        id: li.id,
        label: li.label,
        icon: li.icon.map(map_icon),
        command: li.command,
        args: li.args,
    }
}

fn map_widget(w: cairn_plugin_protocol::PluginWidget) -> cairn_contract::PluginWidget {
    match w {
        cairn_plugin_protocol::PluginWidget::Text { text, muted } => {
            cairn_contract::PluginWidget::Text { text, muted }
        }
        cairn_plugin_protocol::PluginWidget::Action {
            label,
            icon,
            command,
            args,
        } => cairn_contract::PluginWidget::Action {
            label,
            icon: icon.map(map_icon),
            command,
            args,
        },
        cairn_plugin_protocol::PluginWidget::List { items } => cairn_contract::PluginWidget::List {
            items: items.into_iter().map(map_list_item).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};

    fn engine(dir: &std::path::Path) -> Engine {
        Engine::new(
            LocalFsStore::open(dir).unwrap(),
            InMemoryIndex::default(),
            GitVcs::open_or_init(dir).unwrap(),
        )
    }

    #[test]
    fn write_commit_and_query_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();

        let resp = dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "the target [[b]]".into(),
            },
            &mut sink,
        )
        .unwrap();
        assert_eq!(resp, CommandResponse::Done);

        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "b.md".into(),
                contents: "second".into(),
            },
            &mut sink,
        )
        .unwrap();

        let got = dispatch_query(
            &eng,
            &Query::GetNote {
                path: "a.md".into(),
            },
        )
        .unwrap();
        assert_eq!(
            got,
            QueryResponse::Note {
                contents: "the target [[b]]".into()
            }
        );

        let search = dispatch_query(
            &eng,
            &Query::Search {
                query: "target".into(),
            },
        )
        .unwrap();
        match search {
            QueryResponse::SearchResults { results } => {
                assert!(results.iter().any(|r| r.path == "a.md"));
            }
            other => panic!("expected SearchResults, got {other:?}"),
        }

        let backlinks = dispatch_query(
            &eng,
            &Query::GetBacklinks {
                path: "b.md".into(),
            },
        )
        .unwrap();
        assert_eq!(
            backlinks,
            QueryResponse::Paths {
                paths: vec!["a.md".into()]
            }
        );

        let commit = dispatch_command(
            &mut eng,
            &Command::Commit {
                message: "first".into(),
            },
            &mut sink,
        )
        .unwrap();
        assert!(matches!(commit, CommandResponse::Committed { .. }));
    }

    #[test]
    fn missing_note_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let eng = engine(tmp.path());
        let err = dispatch_query(
            &eng,
            &Query::GetNote {
                path: "missing.md".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
        assert!(matches!(
            ContractError::from(err),
            ContractError::NotFound { .. }
        ));
    }

    #[test]
    fn invalid_path_is_invalid_request() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        let err = dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "../escape.md".into(),
                contents: "x".into(),
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidRequest(_)));
    }

    #[test]
    fn app_event_maps_to_wire_event() {
        let p = NotePath::new("a.md").unwrap();
        assert_eq!(
            app_event_to_wire(AppEvent::NoteChanged(p.clone())),
            WireEvent::NoteChanged {
                path: "a.md".into()
            }
        );
        assert_eq!(
            app_event_to_wire(AppEvent::Reindexed(3)),
            WireEvent::Reindexed { count: 3 }
        );
        assert_eq!(
            app_event_to_wire(AppEvent::NoteDeleted(p.clone())),
            WireEvent::NoteDeleted {
                path: "a.md".into()
            }
        );
        assert_eq!(
            app_event_to_wire(AppEvent::Committed("abc1234".into())),
            WireEvent::Committed {
                commit: "abc1234".into()
            }
        );
    }

    #[test]
    fn list_notes_and_graph_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "---\ntitle: Alpha\n---\nsee [[b]]".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "b.md".into(),
                contents: "hi".into(),
            },
            &mut sink,
        )
        .unwrap();

        match dispatch_query(&eng, &Query::ListNotes).unwrap() {
            QueryResponse::Notes { notes } => {
                assert_eq!(notes.len(), 2);
                assert!(notes.iter().any(|n| n.path == "a.md" && n.title == "Alpha"));
                assert!(notes.iter().any(|n| n.path == "b.md" && n.title == "b"));
            }
            other => panic!("expected Notes, got {other:?}"),
        }

        match dispatch_query(&eng, &Query::GetGraph).unwrap() {
            QueryResponse::Graph { nodes, edges } => {
                assert_eq!(nodes, vec!["a.md".to_string(), "b.md".to_string()]);
                assert_eq!(
                    edges,
                    vec![GraphEdge {
                        from: "a.md".into(),
                        to: "b.md".into()
                    }]
                );
            }
            other => panic!("expected Graph, got {other:?}"),
        }
    }

    #[test]
    fn tag_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "---\ntags: [rust, ideas]\n---\nx".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "b.md".into(),
                contents: "---\ntags: rust\n---\ny".into(),
            },
            &mut sink,
        )
        .unwrap();

        match dispatch_query(&eng, &Query::ListTags).unwrap() {
            QueryResponse::Tags { tags } => {
                assert_eq!(
                    tags,
                    vec![
                        TagCount {
                            tag: "ideas".into(),
                            count: 1
                        },
                        TagCount {
                            tag: "rust".into(),
                            count: 2
                        },
                    ]
                );
            }
            other => panic!("expected Tags, got {other:?}"),
        }

        match dispatch_query(&eng, &Query::NotesByTag { tag: "rust".into() }).unwrap() {
            QueryResponse::Paths { paths } => {
                assert_eq!(paths, vec!["a.md".to_string(), "b.md".to_string()])
            }
            other => panic!("expected Paths, got {other:?}"),
        }

        match dispatch_query(&eng, &Query::ListNotes).unwrap() {
            QueryResponse::Notes { notes } => {
                let a = notes.iter().find(|n| n.path == "a.md").unwrap();
                assert_eq!(a.tags, vec!["rust".to_string(), "ideas".to_string()]);
            }
            other => panic!("expected Notes, got {other:?}"),
        }
    }

    #[test]
    fn delete_dispatch_and_error_mappings() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();

        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "hi".into(),
            },
            &mut sink,
        )
        .unwrap();
        sink.clear();

        let resp = dispatch_command(
            &mut eng,
            &Command::DeleteNote {
                path: "a.md".into(),
            },
            &mut sink,
        )
        .unwrap();
        assert_eq!(resp, CommandResponse::Done);
        assert!(sink.contains(&AppEvent::NoteDeleted(NotePath::new("a.md").unwrap())));

        // ContractError mapping for the non-NotFound arms.
        assert!(matches!(
            ContractError::from(ServiceError::InvalidRequest("bad".into())),
            ContractError::InvalidRequest { .. }
        ));
        assert!(matches!(
            ContractError::from(ServiceError::Internal("boom".into())),
            ContractError::Internal { .. }
        ));
    }

    #[test]
    fn internal_error_preserves_typed_source_through_from_port_error() {
        // D3: an adapter's typed cause must survive PortError -> ServiceError so
        // it stays downcastable when logged at the daemon edge, not flattened.
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "lock held");
        let svc: ServiceError = PortError::Adapter(AdapterError::new(io)).into();

        let ServiceError::Internal(_) = &svc else {
            panic!("adapter failure must map to Internal");
        };
        let source = std::error::Error::source(&svc).expect("typed source preserved");
        let io = source
            .downcast_ref::<std::io::Error>()
            .expect("io::Error kind recoverable at the service edge");
        assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
        // Display still flattens for any string consumer.
        assert_eq!(svc.to_string(), "lock held");
    }

    #[test]
    fn list_plugins_empty_and_invoke_unknown_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        match dispatch_query(&eng, &Query::ListPlugins).unwrap() {
            QueryResponse::Plugins { plugins } => assert!(plugins.is_empty()),
            other => panic!("expected Plugins, got {other:?}"),
        }
        let mut sink: Vec<AppEvent> = Vec::new();
        let err = dispatch_command(
            &mut eng,
            &Command::InvokePluginCommand {
                plugin: "nope".into(),
                command: "x".into(),
                args: serde_json::Value::Null,
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[test]
    fn run_watch_loop_drains_until_sender_drops() {
        use cairn_ports::{FsChange, WatchHandle};
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = WatchHandle::new(rx, Box::new(()));
        tx.send(FsChange::Changed(NotePath::new("a.md").unwrap()))
            .unwrap();
        tx.send(FsChange::Removed(NotePath::new("b.md").unwrap()))
            .unwrap();
        drop(tx); // close the channel → loop ends

        let mut seen = Vec::new();
        run_watch_loop(&handle, |c| seen.push(c.clone()));
        assert_eq!(
            seen,
            vec![
                FsChange::Changed(NotePath::new("a.md").unwrap()),
                FsChange::Removed(NotePath::new("b.md").unwrap()),
            ]
        );
    }

    #[test]
    fn coalescer_fires_once_per_dirty_burst() {
        let mut c = Coalescer::default();
        assert!(!c.take_if_dirty(), "clean: nothing to commit");
        c.mark_dirty();
        c.mark_dirty();
        assert!(c.take_if_dirty(), "a dirty burst commits once");
        assert!(!c.take_if_dirty(), "already committed: clean again");
    }

    #[test]
    fn timeout_loop_flushes_pending_changes_on_disconnect() {
        use cairn_ports::{FsChange, WatchHandle};
        use std::time::Duration;
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = WatchHandle::new(rx, Box::new(()));
        tx.send(FsChange::Changed(NotePath::new("a.md").unwrap()))
            .unwrap();
        tx.send(FsChange::Changed(NotePath::new("b.md").unwrap()))
            .unwrap();
        drop(tx); // disconnect → drain both, then flush once

        let mut seen = Vec::new();
        let mut quiets = 0;
        run_watch_loop_timeout(
            &handle,
            Duration::from_millis(50),
            |c| seen.push(c.clone()),
            || quiets += 1,
        );
        assert_eq!(seen.len(), 2, "both changes applied");
        assert_eq!(
            quiets, 1,
            "pending changes flushed exactly once on shutdown"
        );
    }

    #[test]
    fn timeout_loop_commits_after_quiet_period() {
        use cairn_ports::{FsChange, WatchHandle};
        use std::time::Duration;
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = WatchHandle::new(rx, Box::new(()));
        // One change, then a long pause before disconnect: the quiet timeout must
        // fire a commit before the channel closes.
        let sender = std::thread::spawn(move || {
            tx.send(FsChange::Changed(NotePath::new("a.md").unwrap()))
                .unwrap();
            std::thread::sleep(Duration::from_millis(250));
            drop(tx);
        });
        let mut quiets = 0;
        run_watch_loop_timeout(&handle, Duration::from_millis(60), |_| {}, || quiets += 1);
        sender.join().unwrap();
        // One commit from the quiet timeout; the disconnect flush finds nothing
        // pending (already committed), so exactly one.
        assert_eq!(quiets, 1, "quiet period triggers exactly one commit");
    }

    #[test]
    fn history_show_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "v1".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::Commit {
                message: "v1".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "v2".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::Commit {
                message: "v2".into(),
            },
            &mut sink,
        )
        .unwrap();

        let revisions = match dispatch_query(
            &eng,
            &Query::NoteHistory {
                path: "a.md".into(),
            },
        )
        .unwrap()
        {
            QueryResponse::History { revisions } => revisions,
            other => panic!("expected History, got {other:?}"),
        };
        assert_eq!(revisions.len(), 2);
        let v1 = revisions[1].id.clone();

        // NoteAt returns the content at that revision (reuses the Note response).
        match dispatch_query(
            &eng,
            &Query::NoteAt {
                path: "a.md".into(),
                revision: v1.clone(),
            },
        )
        .unwrap()
        {
            QueryResponse::Note { contents } => assert_eq!(contents, "v1"),
            other => panic!("expected Note, got {other:?}"),
        }

        // RestoreNote writes v1 back.
        let mut sink2: Vec<AppEvent> = Vec::new();
        dispatch_command(
            &mut eng,
            &Command::RestoreNote {
                path: "a.md".into(),
                revision: v1,
            },
            &mut sink2,
        )
        .unwrap();
        match dispatch_query(
            &eng,
            &Query::GetNote {
                path: "a.md".into(),
            },
        )
        .unwrap()
        {
            QueryResponse::Note { contents } => assert_eq!(contents, "v1"),
            other => panic!("expected Note, got {other:?}"),
        }
    }

    #[test]
    fn rename_dispatch_moves_rewrites_and_maps_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();

        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "i am a".into(),
            },
            &mut sink,
        )
        .unwrap();
        dispatch_command(
            &mut eng,
            &Command::WriteNote {
                path: "b.md".into(),
                contents: "link to [[a]]".into(),
            },
            &mut sink,
        )
        .unwrap();
        sink.clear();

        // Success: move a.md -> c.md, rewriting [[a]] in b.md to [[c]].
        let resp = dispatch_command(
            &mut eng,
            &Command::RenameNote {
                from: "a.md".into(),
                to: "c.md".into(),
            },
            &mut sink,
        )
        .unwrap();
        assert_eq!(resp, CommandResponse::Done);
        assert_eq!(
            dispatch_query(
                &eng,
                &Query::GetNote {
                    path: "c.md".into()
                }
            )
            .unwrap(),
            QueryResponse::Note {
                contents: "i am a".into()
            }
        );
        assert_eq!(
            dispatch_query(
                &eng,
                &Query::GetNote {
                    path: "b.md".into()
                }
            )
            .unwrap(),
            QueryResponse::Note {
                contents: "link to [[c]]".into()
            }
        );

        // Target exists -> InvalidRequest (AlreadyExists mapped).
        let err = dispatch_command(
            &mut eng,
            &Command::RenameNote {
                from: "b.md".into(),
                to: "c.md".into(),
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidRequest(_)));

        // Missing source -> NotFound.
        let err = dispatch_command(
            &mut eng,
            &Command::RenameNote {
                from: "gone.md".into(),
                to: "z.md".into(),
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));

        // Invalid path -> InvalidRequest.
        let err = dispatch_command(
            &mut eng,
            &Command::RenameNote {
                from: "../escape.md".into(),
                to: "z.md".into(),
            },
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidRequest(_)));
    }

    #[test]
    fn list_plugins_maps_contributions_protocol_to_contract() {
        use cairn_ports::{PluginCallbacks, PluginHost, PluginInfo, PortError};

        struct FakeHost;

        impl PluginHost for FakeHost {
            fn plugins(&self) -> Vec<PluginInfo> {
                vec![PluginInfo {
                    id: "fake".into(),
                    name: "Fake".into(),
                    version: "0.1.0".into(),
                    commands: vec![],
                    contributions: vec![cairn_plugin_protocol::PluginContribution {
                        id: "fake.sidebar".into(),
                        slot: cairn_plugin_protocol::PluginSlot::SidebarSection,
                        widget: cairn_plugin_protocol::PluginWidget::Text {
                            text: "hello".into(),
                            muted: None,
                        },
                        title: Some("Fake Section".into()),
                        icon: None,
                        order: None,
                    }],
                }]
            }

            fn invoke(
                &mut self,
                _plugin: &str,
                _command: &str,
                _args: &serde_json::Value,
                _callbacks: &mut dyn PluginCallbacks,
            ) -> Result<serde_json::Value, PortError> {
                Ok(serde_json::Value::Null)
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_plugin_host(Box::new(FakeHost));

        match dispatch_query(&eng, &Query::ListPlugins).unwrap() {
            QueryResponse::Plugins { plugins } => {
                assert_eq!(plugins[0].contributions.len(), 1);
                assert_eq!(
                    plugins[0].contributions[0].slot,
                    cairn_contract::PluginSlot::SidebarSection
                );
            }
            other => panic!("expected Plugins, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod augmented_answer_tests {
    use super::*;
    use cairn_app::Event;
    use cairn_contract::Command;
    use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
    use std::cell::RefCell;

    struct RecordingRuntime {
        prompt: RefCell<String>,
    }
    impl AgentRuntime for RecordingRuntime {
        fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
            *self.prompt.borrow_mut() = prompt.to_string();
            sink.emit(AgentEvent::TextDelta("ok".into()));
            sink.emit(AgentEvent::Completed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    #[test]
    fn retrieves_context_builds_prompt_and_streams() {
        let dir = tempfile::tempdir().unwrap();
        let mut events: Vec<Event> = Vec::new();
        let mut engine = cairn_startup::build_engine(dir.path()).unwrap();
        dispatch_command(
            &mut engine,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "ownership moves by default".into(),
            },
            &mut events,
        )
        .unwrap();
        engine.reindex(&mut events).unwrap();

        let rt = RecordingRuntime {
            prompt: RefCell::new(String::new()),
        };
        let mut sink = VecSink::default();
        let cited = augmented_answer(&engine, "ownership", &rt, &mut sink, 5).unwrap();

        assert_eq!(cited, vec!["a.md".to_string()]);
        assert!(rt.prompt.borrow().contains("ownership moves by default"));
        assert!(rt.prompt.borrow().contains("Question: ownership"));
        assert_eq!(
            sink.0,
            vec![AgentEvent::TextDelta("ok".into()), AgentEvent::Completed]
        );
    }

    #[test]
    fn prompt_without_context_omits_notes_section() {
        let p = build_answer_prompt("", "what is x");
        assert!(!p.contains("Notes:"));
        assert!(p.contains("Question: what is x"));
    }

    #[test]
    fn gather_returns_prompt_with_context_and_cited_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut events: Vec<Event> = Vec::new();
        let mut engine = cairn_startup::build_engine(dir.path()).unwrap();
        dispatch_command(
            &mut engine,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "ownership moves by default".into(),
            },
            &mut events,
        )
        .unwrap();
        engine.reindex(&mut events).unwrap();

        let (prompt, cited) = gather_answer_context(&engine, "ownership", 5).unwrap();
        assert_eq!(cited, vec!["a.md".to_string()]);
        assert!(prompt.contains("ownership moves by default"));
        assert!(prompt.contains("Question: ownership"));
    }

    #[test]
    fn agent_event_maps_every_variant_to_wire() {
        use cairn_contract::AnswerEvent;
        assert_eq!(
            agent_event_to_wire(AgentEvent::TextDelta("hi".into())),
            Some(AnswerEvent::TextDelta { text: "hi".into() })
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::ToolStarted { tool: "g".into() }),
            Some(AnswerEvent::ToolStarted { tool: "g".into() })
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::ToolCompleted {
                tool: "g".into(),
                ok: false
            }),
            Some(AnswerEvent::ToolCompleted {
                tool: "g".into(),
                ok: false
            })
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::TurnCompleted),
            Some(AnswerEvent::TurnCompleted)
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::Completed),
            Some(AnswerEvent::Completed)
        );
        assert_eq!(
            agent_event_to_wire(AgentEvent::Failed {
                message: "x".into()
            }),
            Some(AnswerEvent::Failed {
                message: "x".into()
            })
        );
    }
}
