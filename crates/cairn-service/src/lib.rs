//! The transport-blind dispatcher: maps the wire contract to engine
//! use-cases and engine events to wire events. No I/O, no async.

use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    Command, CommandResponse, ContractError, Event as WireEvent, GraphEdge, NoteSummary, Query,
    QueryResponse,
};
use cairn_domain::NotePath;
use cairn_ports::{PortError, SearchIndex, VaultStore, Vcs};

/// Errors surfaced when dispatching a contract request.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// A requested note/resource was missing.
    #[error("note not found: {0}")]
    NotFound(String),
    /// The request was malformed (e.g. an invalid note path).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// An internal/adapter failure.
    #[error("{0}")]
    Internal(String),
}

impl From<PortError> for ServiceError {
    fn from(e: PortError) -> Self {
        match e {
            PortError::NotFound(s) => ServiceError::NotFound(s),
            PortError::Adapter(s) => ServiceError::Internal(s),
        }
    }
}

impl From<ServiceError> for ContractError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::NotFound(what) => ContractError::NotFound { what },
            ServiceError::InvalidRequest(message) => ContractError::InvalidRequest { message },
            ServiceError::Internal(message) => ContractError::Internal { message },
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
pub fn dispatch_command<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &mut Engine<S, I, V>,
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
        Command::Commit { message } => {
            let commit = engine.commit(message, sink)?;
            Ok(CommandResponse::Committed { commit })
        }
    }
}

/// Dispatch a read-only query.
///
/// # Errors
/// Returns [`ServiceError`] on invalid input or engine failure.
/// Callers serving a wire transport map the error via
/// [`ContractError::from`].
pub fn dispatch_query<S: VaultStore, I: SearchIndex, V: Vcs>(
    engine: &Engine<S, I, V>,
    query: &Query,
) -> Result<QueryResponse, ServiceError> {
    match query {
        Query::GetNote { path } => {
            let p = parse_path(path)?;
            let contents = engine.read_note(&p)?;
            Ok(QueryResponse::Note { contents })
        }
        Query::Search { query } => {
            let paths = engine
                .search(query)?
                .into_iter()
                .map(|hit| hit.path.as_str().to_string())
                .collect();
            Ok(QueryResponse::Paths { paths })
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
        assert_eq!(
            search,
            QueryResponse::Paths {
                paths: vec!["a.md".into()]
            }
        );

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
}
