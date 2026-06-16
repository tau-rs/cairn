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
    /// Rename or move a note (link-aware).
    RenameNote {
        /// Current relative path.
        from: String,
        /// New relative path (may be in a different directory).
        to: String,
    },
    /// Commit all changes with a message.
    Commit {
        /// Commit message.
        message: String,
    },
    /// Restore a note to a past revision (writes that version as current).
    RestoreNote {
        /// Relative note path.
        path: String,
        /// A git revspec to restore from.
        revision: String,
    },
    /// Invoke a command exposed by a loaded plugin.
    InvokePluginCommand {
        /// Plugin id.
        plugin: String,
        /// Command id.
        command: String,
        /// Arbitrary JSON arguments.
        args: serde_json::Value,
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
    /// List loaded plugins and their commands.
    ListPlugins,
    /// A note's commit history (newest first).
    NoteHistory {
        /// Relative note path.
        path: String,
    },
    /// A note's contents at a past revision.
    NoteAt {
        /// Relative note path.
        path: String,
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
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

/// A streaming, note-grounded question. Its own shape — not a `Command` (no
/// mutation) and not a `Query` (no single response).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct AskRequest {
    /// The question to answer.
    pub query: String,
    /// How many top search hits to ground the answer in. `None` ⇒ 5.
    pub top_k: Option<usize>,
}

/// One increment of an answer stream — cairn's own closed wire vocabulary,
/// mirroring `cairn_ports::AgentEvent` plus a leading `Sources` frame. Struct
/// variants are required: `#[serde(tag = "type")]` cannot tag a newtype variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerEvent {
    /// The cited notes grounding the answer; emitted first.
    Sources {
        /// Relative note paths, in rank order.
        paths: Vec<String>,
    },
    /// A chunk of answer text.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// The agent began a tool call.
    ToolStarted {
        /// Tool name.
        tool: String,
    },
    /// A tool call finished; `ok` is false if it reported an error.
    ToolCompleted {
        /// Tool name.
        tool: String,
        /// Whether the call succeeded.
        ok: bool,
    },
    /// One agent turn completed; a run may span several.
    TurnCompleted,
    /// The run finished successfully.
    Completed,
    /// The run failed; `message` is human-readable.
    Failed {
        /// Failure detail.
        message: String,
    },
}

/// Result of a successful command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// Result of a plugin command (arbitrary JSON).
    PluginResult {
        /// The command's JSON output.
        result: serde_json::Value,
    },
}

/// An icon a plugin may reference by name. Closed set — never a string/URL/SVG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PluginIcon {
    Tag,
    Search,
    Note,
    Folder,
    Link,
    Star,
    Info,
    Play,
}

/// A named shell slot a contribution targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum PluginSlot {
    #[serde(rename = "sidebar.section")]
    SidebarSection,
    #[serde(rename = "topbar.action")]
    TopbarAction,
    #[serde(rename = "command")]
    Command,
}

/// A capability a Tier-3 (sandboxed-iframe) plugin may request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum PluginCapability {
    #[serde(rename = "activeNote.read")]
    ActiveNoteRead,
    #[serde(rename = "activeNote.write")]
    ActiveNoteWrite,
    #[serde(rename = "notes.read")]
    NotesRead,
    #[serde(rename = "notes.search")]
    NotesSearch,
    #[serde(rename = "command.invoke")]
    CommandInvoke,
}

/// One row inside a `list` widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginListItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

/// A host-renderable widget. Closed vocabulary; first cut: text / action / list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginWidget {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        muted: Option<bool>,
    },
    Action {
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        icon: Option<PluginIcon>,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    List {
        items: Vec<PluginListItem>,
    },
    Iframe {
        html: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
}

/// One placement of one widget into one slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginContribution {
    pub id: String,
    pub slot: PluginSlot,
    pub widget: PluginWidget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
}

/// A loaded plugin and its commands (response to `ListPlugins`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginSummary {
    /// Manifest id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Declared commands.
    pub commands: Vec<PluginCommandSummary>,
    /// UI contributions (Tier-2). Empty for plugins that declare none.
    #[serde(default)]
    pub contributions: Vec<PluginContribution>,
    /// Capabilities a Tier-3 plugin requests. None for plugins that declare none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<PluginCapability>>,
}

/// A command a plugin handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginCommandSummary {
    /// Command id.
    pub id: String,
    /// Human title.
    pub title: String,
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

/// One ranked search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchResult {
    /// Relative note path.
    pub path: String,
    /// Relevance score (relative ordering only).
    pub score: f32,
    /// Plain-text excerpt around the match.
    pub snippet: String,
    /// `(start, end)` byte ranges within `snippet` that matched.
    pub highlights: Vec<(u32, u32)>,
}

/// One commit in a note's history (response element of `NoteHistory`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Revision {
    /// Short commit id.
    pub id: String,
    /// Commit summary (first line).
    pub message: String,
    /// Commit time, seconds since the Unix epoch.
    pub timestamp_secs: i64,
    /// Author name.
    pub author: String,
}

/// Result of a successful query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// Ranked search results (response to `Search`).
    SearchResults {
        /// Best match first.
        results: Vec<SearchResult>,
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
    /// Loaded plugins (response to `ListPlugins`).
    Plugins {
        /// One per loaded plugin.
        plugins: Vec<PluginSummary>,
    },
    /// A note's commit history (response to `NoteHistory`).
    History {
        /// One per commit, newest first.
        revisions: Vec<Revision>,
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
    fn rename_note_command_roundtrips_with_snake_case_tag() {
        let cmd = Command::RenameNote {
            from: "a.md".into(),
            to: "dir/b.md".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"rename_note\""));
        assert!(json.contains("\"from\":\"a.md\""));
        assert!(json.contains("\"to\":\"dir/b.md\""));
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), cmd);
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
    fn search_results_roundtrip() {
        let r = QueryResponse::SearchResults {
            results: vec![SearchResult {
                path: "a.md".into(),
                score: 1.5,
                snippet: "hello target".into(),
                highlights: vec![(6, 12)],
            }],
        };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"search_results\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), r);
    }

    #[test]
    fn plugin_command_and_response_roundtrip() {
        let cmd = Command::InvokePluginCommand {
            plugin: "p".into(),
            command: "echo".into(),
            args: serde_json::json!({"x": 1}),
        };
        let j = serde_json::to_string(&cmd).unwrap();
        assert!(j.contains("\"type\":\"invoke_plugin_command\""));
        assert_eq!(serde_json::from_str::<Command>(&j).unwrap(), cmd);

        let resp = QueryResponse::Plugins {
            plugins: vec![PluginSummary {
                id: "p".into(),
                name: "P".into(),
                version: "0.1.0".into(),
                commands: vec![PluginCommandSummary {
                    id: "echo".into(),
                    title: "Echo".into(),
                }],
                contributions: vec![],
                capabilities: None,
            }],
        };
        let j = serde_json::to_string(&resp).unwrap();
        assert!(j.contains("\"type\":\"plugins\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), resp);
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

    #[test]
    fn plugin_value_arrays_match_enums() {
        use serde_json::to_value;
        // Each enum variant's serialized string must appear in the .ts arrays below.
        let slots = [
            PluginSlot::SidebarSection,
            PluginSlot::TopbarAction,
            PluginSlot::Command,
        ];
        let slot_strs: Vec<String> = slots
            .iter()
            .map(|s| to_value(s).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(slot_strs, ["sidebar.section", "topbar.action", "command"]);

        let icons = [
            PluginIcon::Tag,
            PluginIcon::Search,
            PluginIcon::Note,
            PluginIcon::Folder,
            PluginIcon::Link,
            PluginIcon::Star,
            PluginIcon::Info,
            PluginIcon::Play,
        ];
        let icon_strs: Vec<String> = icons
            .iter()
            .map(|s| to_value(s).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            icon_strs,
            ["tag", "search", "note", "folder", "link", "star", "info", "play"]
        );

        // Widget kinds are the serde `tag` discriminants:
        let kinds: Vec<String> = [
            to_value(PluginWidget::Text {
                text: "x".into(),
                muted: None,
            })
            .unwrap(),
            to_value(PluginWidget::Action {
                label: "x".into(),
                icon: None,
                command: "c".into(),
                args: None,
            })
            .unwrap(),
            to_value(PluginWidget::List { items: vec![] }).unwrap(),
        ]
        .iter()
        .map(|v| v["kind"].as_str().unwrap().to_string())
        .collect();
        assert_eq!(kinds, ["text", "action", "list"]);

        // Capability wire strings (the contract): dotted, exact, ordered.
        let caps = [
            PluginCapability::ActiveNoteRead,
            PluginCapability::ActiveNoteWrite,
            PluginCapability::NotesRead,
            PluginCapability::NotesSearch,
            PluginCapability::CommandInvoke,
        ];
        let cap_strs: Vec<String> = caps
            .iter()
            .map(|c| to_value(c).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            cap_strs,
            [
                "activeNote.read",
                "activeNote.write",
                "notes.read",
                "notes.search",
                "command.invoke"
            ]
        );

        // The Iframe widget kind serializes to "iframe".
        let iframe_kind = to_value(PluginWidget::Iframe {
            html: "<p>x</p>".into(),
            height: None,
        })
        .unwrap();
        assert_eq!(iframe_kind["kind"].as_str().unwrap(), "iframe");

        // A present height serializes as a bare number (not stringified/wrapped).
        let iframe_sized = to_value(PluginWidget::Iframe {
            html: "<p>x</p>".into(),
            height: Some(240),
        })
        .unwrap();
        assert_eq!(iframe_sized["height"].as_u64().unwrap(), 240);
    }

    #[test]
    fn plugin_summary_capabilities_round_trip() {
        // Tier-2 payloads (no `capabilities` key) must still deserialize.
        let legacy = r#"{"id":"p","name":"P","version":"1","commands":[],"contributions":[]}"#;
        let s: PluginSummary = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.capabilities, None);

        // Round-trip with capabilities present.
        let s2 = PluginSummary {
            id: "p".into(),
            name: "P".into(),
            version: "1".into(),
            commands: vec![],
            contributions: vec![],
            capabilities: Some(vec![PluginCapability::NotesRead]),
        };
        let j = serde_json::to_string(&s2).unwrap();
        assert!(j.contains("\"capabilities\":[\"notes.read\"]"));
        assert_eq!(serde_json::from_str::<PluginSummary>(&j).unwrap(), s2);
    }
}

#[cfg(test)]
mod ask_wire_format {
    use super::{AnswerEvent, AskRequest};

    #[test]
    fn answer_event_tags_match_the_track04_mock() {
        let cases = [
            (
                serde_json::to_value(AnswerEvent::Sources {
                    paths: vec!["a.md".into()],
                })
                .unwrap(),
                "sources",
            ),
            (
                serde_json::to_value(AnswerEvent::TextDelta { text: "hi".into() }).unwrap(),
                "text_delta",
            ),
            (
                serde_json::to_value(AnswerEvent::ToolStarted {
                    tool: "grep".into(),
                })
                .unwrap(),
                "tool_started",
            ),
            (
                serde_json::to_value(AnswerEvent::ToolCompleted {
                    tool: "grep".into(),
                    ok: true,
                })
                .unwrap(),
                "tool_completed",
            ),
            (
                serde_json::to_value(AnswerEvent::TurnCompleted).unwrap(),
                "turn_completed",
            ),
            (
                serde_json::to_value(AnswerEvent::Completed).unwrap(),
                "completed",
            ),
            (
                serde_json::to_value(AnswerEvent::Failed {
                    message: "boom".into(),
                })
                .unwrap(),
                "failed",
            ),
        ];
        for (json, tag) in cases {
            assert_eq!(json["type"], tag, "wire tag drift for {tag}");
        }
        let delta = serde_json::to_value(AnswerEvent::TextDelta { text: "x".into() }).unwrap();
        assert_eq!(delta["text"], "x");
    }

    #[test]
    fn ask_request_top_k_is_optional() {
        let r: AskRequest = serde_json::from_str(r#"{"query":"q"}"#).unwrap();
        assert_eq!(r.query, "q");
        assert_eq!(r.top_k, None);
    }
}
