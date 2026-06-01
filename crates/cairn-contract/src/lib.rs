//! The transport-blind contract: serializable Command / Query / Event DTOs
//! with generated TypeScript bindings. This is the surface a UI consumes.
//!
//! These DTOs are intentionally independent of `cairn-domain` types so the
//! wire format can stay stable while the domain evolves.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A request that mutates the cairn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Create or overwrite a note.
    WriteNote {
        /// Relative note path.
        path: String,
        /// Full markdown contents.
        contents: String,
    },
    /// Delete a note.
    DeleteNote {
        /// Relative note path.
        path: String,
    },
    /// Commit all changes with a message.
    Commit {
        /// Commit message.
        message: String,
    },
}

/// A read-only request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    /// Read a note's contents.
    GetNote {
        /// Relative note path.
        path: String,
    },
    /// Search note content.
    Search {
        /// Query string.
        query: String,
    },
    /// List the notes that link to a note.
    GetBacklinks {
        /// Relative note path.
        path: String,
    },
    /// List every note with a display title.
    ListNotes,
    /// Fetch the full link graph.
    GetGraph,
    /// List all tags with note counts.
    ListTags,
    /// List the notes carrying a tag.
    NotesByTag {
        /// The tag to filter by.
        tag: String,
    },
}

/// A push event emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A note was created or updated.
    NoteChanged {
        /// Relative note path.
        path: String,
    },
    /// A note was deleted.
    NoteDeleted {
        /// Relative note path.
        path: String,
    },
    /// The cairn was committed.
    Committed {
        /// Short commit id.
        commit: String,
    },
    /// The index finished rebuilding.
    Reindexed {
        /// Number of notes indexed.
        count: u32,
    },
}

/// Result of a successful command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandResponse {
    /// A simple command (write or delete) succeeded.
    Done,
    /// A commit was created.
    Committed {
        /// Short commit id.
        commit: String,
    },
}

/// A note's path and display title, for list views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NoteSummary {
    /// Relative note path.
    pub path: String,
    /// Display title (frontmatter title, first heading, or filename).
    pub title: String,
    /// Frontmatter tags of the note.
    pub tags: Vec<String>,
}

/// A tag and how many notes carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TagCount {
    /// The tag.
    pub tag: String,
    /// Number of notes carrying it.
    pub count: u32,
}

/// A directed link edge between two notes, by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphEdge {
    /// Source note path.
    pub from: String,
    /// Target note path.
    pub to: String,
}

/// Result of a successful query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryResponse {
    /// A note's contents.
    Note {
        /// Full markdown contents.
        contents: String,
    },
    /// A list of note paths (used by search and backlinks).
    Paths {
        /// Relative note paths.
        paths: Vec<String>,
    },
    /// Note summaries (response to `ListNotes`).
    Notes {
        /// One per note.
        notes: Vec<NoteSummary>,
    },
    /// The link graph (response to `GetGraph`).
    Graph {
        /// All note paths.
        nodes: Vec<String>,
        /// Directed link edges.
        edges: Vec<GraphEdge>,
    },
    /// All tags with counts (response to `ListTags`).
    Tags {
        /// One per distinct tag, sorted by tag.
        tags: Vec<TagCount>,
    },
}

/// A typed error returned across the contract boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContractError {
    /// The requested resource does not exist.
    NotFound {
        /// What was missing (e.g. a note path).
        what: String,
    },
    /// The request was malformed or invalid.
    InvalidRequest {
        /// Human-readable reason.
        message: String,
    },
    /// An internal failure occurred.
    Internal {
        /// Human-readable detail.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serializes_with_type_tag() {
        let cmd = Command::WriteNote {
            path: "a.md".into(),
            contents: "hi".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"write_note\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn response_and_error_tags_are_snake_case() {
        let r = CommandResponse::Committed {
            commit: "abc1234".into(),
        };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"committed\""));
        assert_eq!(serde_json::from_str::<CommandResponse>(&j).unwrap(), r);

        let e = ContractError::NotFound {
            what: "a.md".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"type\":\"not_found\""));
        assert_eq!(serde_json::from_str::<ContractError>(&j).unwrap(), e);
    }

    #[test]
    fn list_and_graph_responses_roundtrip() {
        let n = QueryResponse::Notes {
            notes: vec![NoteSummary {
                path: "a.md".into(),
                title: "Alpha".into(),
                tags: vec!["rust".into()],
            }],
        };
        let j = serde_json::to_string(&n).unwrap();
        assert!(j.contains("\"type\":\"notes\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), n);

        let g = QueryResponse::Graph {
            nodes: vec!["a.md".into(), "b.md".into()],
            edges: vec![GraphEdge {
                from: "a.md".into(),
                to: "b.md".into(),
            }],
        };
        let j = serde_json::to_string(&g).unwrap();
        assert!(j.contains("\"type\":\"graph\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), g);

        assert_eq!(
            serde_json::to_string(&Query::ListNotes).unwrap(),
            "{\"type\":\"list_notes\"}"
        );
        assert_eq!(
            serde_json::from_str::<Query>("{\"type\":\"get_graph\"}").unwrap(),
            Query::GetGraph
        );
    }

    #[test]
    fn tag_query_and_response_roundtrip() {
        let r = QueryResponse::Tags {
            tags: vec![TagCount {
                tag: "rust".into(),
                count: 2,
            }],
        };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"tags\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), r);

        assert_eq!(
            serde_json::to_string(&Query::ListTags).unwrap(),
            "{\"type\":\"list_tags\"}"
        );
        let q = Query::NotesByTag { tag: "rust".into() };
        let j = serde_json::to_string(&q).unwrap();
        assert!(j.contains("\"type\":\"notes_by_tag\""));
        assert_eq!(serde_json::from_str::<Query>(&j).unwrap(), q);
    }
}
