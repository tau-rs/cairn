//! Transport-blind Model Context Protocol (MCP) surface for cairn: the wire
//! types and the mapping from MCP tool calls to the existing
//! `cairn-contract` `Command`/`Query` values. No transport, no engine, no I/O —
//! the daemon owns those. This crate is to MCP what `cairn-service` is to the
//! contract: a pure mapper.
//!
//! Hand-rolled (JSON-RPC 2.0, MCP-style), mirroring `cairn-plugin-protocol`.

use cairn_contract::{Command, CommandResponse, ContractError, Query, QueryResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The MCP protocol version this server speaks (Streamable HTTP).
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// JSON-RPC envelope version.
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC error code: the method (or tool) does not exist.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC error code: the params (or tool arguments) were invalid.
pub const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC error code: an internal server error.
pub const INTERNAL_ERROR: i64 = -32603;

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RpcError {
    /// JSON-RPC error code.
    pub code: i64,
    /// Human-readable detail.
    pub message: String,
}

impl RpcError {
    /// An unknown tool/method.
    fn method_not_found(what: &str) -> Self {
        Self {
            code: METHOD_NOT_FOUND,
            message: format!("unknown tool: {what}"),
        }
    }

    /// Malformed tool arguments.
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: message.into(),
        }
    }
}

/// An inbound JSON-RPC request envelope. `id` is echoed verbatim (it may be a
/// string, number, or null per JSON-RPC); `params` defaults to null.
#[derive(Debug, Clone, Deserialize)]
pub struct McpRequest {
    /// JSON-RPC version, expected `"2.0"`.
    #[serde(default)]
    pub jsonrpc: String,
    /// Request id, echoed in the response. Absent for notifications.
    #[serde(default)]
    pub id: Value,
    /// Method name (`initialize`, `tools/list`, `tools/call`).
    pub method: String,
    /// Method parameters.
    #[serde(default)]
    pub params: Value,
}

/// An outbound JSON-RPC response envelope: exactly one of `result`/`error`.
#[derive(Debug, Clone, Serialize)]
pub struct McpResponse {
    /// JSON-RPC version, always `"2.0"`.
    pub jsonrpc: &'static str,
    /// Echoes the request id.
    pub id: Value,
    /// Success payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Failure payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl McpResponse {
    /// A success response carrying `result`.
    #[must_use]
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    /// An error response carrying a JSON-RPC error object.
    #[must_use]
    pub fn error(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// The `initialize` result: the protocol version, advertised capabilities, and
/// server identity.
#[derive(Debug, Clone, Serialize)]
pub struct InitializeResult {
    /// Protocol version this server speaks.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'static str,
    /// Advertised capabilities (v1: tools only).
    pub capabilities: Value,
    /// Server name + version.
    #[serde(rename = "serverInfo")]
    pub server_info: Value,
}

/// Build the `initialize` result for this server.
#[must_use]
pub fn initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: PROTOCOL_VERSION,
        capabilities: json!({ "tools": { "listChanged": false } }),
        server_info: json!({ "name": "cairn", "version": env!("CARGO_PKG_VERSION") }),
    }
}

/// One tool advertised in `tools/list` and invoked via `tools/call`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDef {
    /// Tool name the agent calls.
    pub name: String,
    /// Agent-facing description.
    pub description: String,
    /// JSON Schema for the tool's arguments.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// What a parsed `tools/call` resolves to: an engine command or query, ready for
/// the daemon to run through the existing dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolDispatch {
    /// A mutating command.
    Command(Command),
    /// A read-only query.
    Query(Query),
}

/// Build a tool definition from a name, description, and JSON-Schema object.
fn tool(name: &str, description: &str, input_schema: Value) -> ToolDef {
    ToolDef {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    }
}

/// A JSON-Schema object taking no arguments.
fn no_args() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// A JSON-Schema object requiring the named string properties.
fn string_args(props: &[(&str, &str)]) -> Value {
    let properties: serde_json::Map<String, Value> = props
        .iter()
        .map(|(name, desc)| {
            (
                (*name).to_string(),
                json!({ "type": "string", "description": desc }),
            )
        })
        .collect();
    let required: Vec<&str> = props.iter().map(|(name, _)| *name).collect();
    json!({ "type": "object", "properties": properties, "required": required })
}

/// The catalog of tools, gated by whether write tools are enabled. Read tools
/// are always present; write tools only when `write_enabled` (default-deny).
#[must_use]
pub fn tools_list(write_enabled: bool) -> Vec<ToolDef> {
    let mut tools = vec![
        tool(
            "read_note",
            "Read a note's full markdown contents by relative path.",
            string_args(&[("path", "Relative note path, e.g. rust/ownership.md")]),
        ),
        tool(
            "search_notes",
            "Ranked full-text search across all notes. Returns matching paths with snippets.",
            string_args(&[("query", "Search query string")]),
        ),
        tool(
            "backlinks",
            "List the notes that link to a given note.",
            string_args(&[("path", "Relative note path to find backlinks for")]),
        ),
        tool(
            "list_notes",
            "List every note with its display title and tags.",
            no_args(),
        ),
        tool(
            "graph",
            "Fetch the full link graph: all note paths (nodes) and directed link edges.",
            no_args(),
        ),
        tool(
            "list_tags",
            "List all tags with the number of notes carrying each.",
            no_args(),
        ),
        tool(
            "notes_by_tag",
            "List the notes carrying a given tag.",
            string_args(&[("tag", "The tag to filter by")]),
        ),
        tool(
            "note_history",
            "A note's commit history (newest first).",
            string_args(&[("path", "Relative note path")]),
        ),
    ];

    if write_enabled {
        tools.extend([
            tool(
                "write_note",
                "Create or overwrite a note. Updates the search index and link graph.",
                string_args(&[
                    ("path", "Relative note path"),
                    ("contents", "Full markdown contents"),
                ]),
            ),
            tool(
                "rename_note",
                "Rename or move a note, rewriting wikilinks that point to it.",
                string_args(&[
                    ("from", "Current relative path"),
                    ("to", "New relative path"),
                ]),
            ),
            tool(
                "delete_note",
                "Delete a note.",
                string_args(&[("path", "Relative note path")]),
            ),
            tool(
                "commit",
                "Commit all pending changes to git with a message.",
                string_args(&[("message", "Commit message")]),
            ),
        ]);
    }

    tools
}

/// One content block in a tool result. v1 emits only text blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    /// A plain-text block (the canonical, always-present result form).
    Text {
        /// The text payload.
        text: String,
    },
}

/// The result of a `tools/call`: human/agent-readable content plus, for
/// structured tools, a typed `structuredContent` payload. `is_error` marks a
/// tool-level failure (note not found, invalid path) — distinct from a JSON-RPC
/// protocol error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolResult {
    /// Content blocks; always carries at least one text block.
    pub content: Vec<Content>,
    /// Whether the tool reported an error.
    #[serde(rename = "isError")]
    pub is_error: bool,
    /// Optional typed payload for programmatic clients.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

impl ToolResult {
    /// A successful text-only result.
    fn text(body: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text { text: body.into() }],
            is_error: false,
            structured_content: None,
        }
    }

    /// Attach a typed payload.
    fn with_structured(mut self, value: Value) -> Self {
        self.structured_content = Some(value);
        self
    }
}

/// Render a successful query response as a tool result. Structured responses
/// (search, lists, graph, tags, history) also attach a typed `structuredContent`
/// payload alongside the canonical text block.
#[must_use]
pub fn render_query_result(resp: &QueryResponse) -> ToolResult {
    match resp {
        QueryResponse::Note { contents } => ToolResult::text(contents.clone()),
        QueryResponse::Paths { paths } => ToolResult::text(if paths.is_empty() {
            "(none)".to_string()
        } else {
            paths.join("\n")
        })
        .with_structured(json!({ "paths": paths })),
        QueryResponse::SearchResults { results } => {
            let text = results
                .iter()
                .map(|r| format!("{} ({:.3})\n  {}", r.path, r.score, r.snippet))
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult::text(if text.is_empty() {
                "(no matches)".into()
            } else {
                text
            })
            .with_structured(json!({ "results": results }))
        }
        QueryResponse::Notes { notes } => {
            let text = notes
                .iter()
                .map(|n| format!("{} — {}", n.path, n.title))
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult::text(text).with_structured(json!({ "notes": notes }))
        }
        QueryResponse::Graph { nodes, edges } => {
            ToolResult::text(format!("{} notes, {} links", nodes.len(), edges.len()))
                .with_structured(json!({ "nodes": nodes, "edges": edges }))
        }
        QueryResponse::Tags { tags } => {
            let text = tags
                .iter()
                .map(|t| format!("{} ({})", t.tag, t.count))
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult::text(text).with_structured(json!({ "tags": tags }))
        }
        QueryResponse::History { revisions } => {
            let text = revisions
                .iter()
                .map(|r| format!("{} {}", r.id, r.message))
                .collect::<Vec<_>>()
                .join("\n");
            ToolResult::text(text).with_structured(json!({ "revisions": revisions }))
        }
        QueryResponse::Plugins { plugins } => {
            ToolResult::text(format!("{} plugins", plugins.len()))
                .with_structured(json!({ "plugins": plugins }))
        }
    }
}

/// Render a successful command response as a tool result.
#[must_use]
pub fn render_command_result(resp: &CommandResponse) -> ToolResult {
    match resp {
        CommandResponse::Done => ToolResult::text("done"),
        CommandResponse::Committed { commit } => ToolResult::text(format!("committed {commit}")),
        CommandResponse::PluginResult { result } => {
            ToolResult::text(result.to_string()).with_structured(result.clone())
        }
    }
}

/// Render a contract error as a tool-level error result (`isError: true`).
#[must_use]
pub fn map_error(err: &ContractError) -> ToolResult {
    let message = match err {
        ContractError::NotFound { what } => format!("not found: {what}"),
        ContractError::InvalidRequest { message } => format!("invalid request: {message}"),
        ContractError::Internal { message } => format!("internal error: {message}"),
    };
    ToolResult {
        content: vec![Content::Text { text: message }],
        is_error: true,
        structured_content: None,
    }
}

/// Extract a required string argument, erroring if absent or not a string.
fn arg(args: &Value, key: &str) -> Result<String, RpcError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RpcError::invalid_params(format!("missing or non-string argument: {key}")))
}

/// Resolve a `tools/call` (`name` + JSON `arguments`) to an engine dispatch.
///
/// # Errors
/// [`RpcError`] with [`METHOD_NOT_FOUND`] for an unknown tool, or
/// [`INVALID_PARAMS`] for missing/ill-typed arguments.
pub fn parse_tool_call(name: &str, args: &Value) -> Result<ToolDispatch, RpcError> {
    use ToolDispatch::{Command as C, Query as Q};
    Ok(match name {
        "read_note" => Q(Query::GetNote {
            path: arg(args, "path")?,
        }),
        "search_notes" => Q(Query::Search {
            query: arg(args, "query")?,
        }),
        "backlinks" => Q(Query::GetBacklinks {
            path: arg(args, "path")?,
        }),
        "list_notes" => Q(Query::ListNotes),
        "graph" => Q(Query::GetGraph),
        "list_tags" => Q(Query::ListTags),
        "notes_by_tag" => Q(Query::NotesByTag {
            tag: arg(args, "tag")?,
        }),
        "note_history" => Q(Query::NoteHistory {
            path: arg(args, "path")?,
        }),
        "write_note" => C(Command::WriteNote {
            path: arg(args, "path")?,
            contents: arg(args, "contents")?,
        }),
        "rename_note" => C(Command::RenameNote {
            from: arg(args, "from")?,
            to: arg(args, "to")?,
        }),
        "delete_note" => C(Command::DeleteNote {
            path: arg(args, "path")?,
        }),
        "commit" => C(Command::Commit {
            message: arg(args, "message")?,
        }),
        other => return Err(RpcError::method_not_found(other)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRITE_TOOLS: [&str; 4] = ["write_note", "rename_note", "delete_note", "commit"];

    #[test]
    fn tools_list_gates_write_tools() {
        let read_only = tools_list(false);
        assert_eq!(
            read_only.len(),
            8,
            "read-only mode exposes the 8 read tools"
        );
        for w in WRITE_TOOLS {
            assert!(
                !read_only.iter().any(|t| t.name == w),
                "read-only mode must not expose write tool {w}"
            );
        }

        let full = tools_list(true);
        assert_eq!(full.len(), 12, "write mode exposes all 12 tools");
        for w in WRITE_TOOLS {
            assert!(
                full.iter().any(|t| t.name == w),
                "write mode must expose write tool {w}"
            );
        }
    }

    #[test]
    fn parse_maps_read_tools_to_queries() {
        assert_eq!(
            parse_tool_call("read_note", &json!({ "path": "a.md" })).unwrap(),
            ToolDispatch::Query(Query::GetNote {
                path: "a.md".into()
            })
        );
        assert_eq!(
            parse_tool_call("search_notes", &json!({ "query": "rust" })).unwrap(),
            ToolDispatch::Query(Query::Search {
                query: "rust".into()
            })
        );
        assert_eq!(
            parse_tool_call("backlinks", &json!({ "path": "b.md" })).unwrap(),
            ToolDispatch::Query(Query::GetBacklinks {
                path: "b.md".into()
            })
        );
        assert_eq!(
            parse_tool_call("list_notes", &json!({})).unwrap(),
            ToolDispatch::Query(Query::ListNotes)
        );
        assert_eq!(
            parse_tool_call("graph", &json!({})).unwrap(),
            ToolDispatch::Query(Query::GetGraph)
        );
        assert_eq!(
            parse_tool_call("list_tags", &json!({})).unwrap(),
            ToolDispatch::Query(Query::ListTags)
        );
        assert_eq!(
            parse_tool_call("notes_by_tag", &json!({ "tag": "ideas" })).unwrap(),
            ToolDispatch::Query(Query::NotesByTag {
                tag: "ideas".into()
            })
        );
        assert_eq!(
            parse_tool_call("note_history", &json!({ "path": "a.md" })).unwrap(),
            ToolDispatch::Query(Query::NoteHistory {
                path: "a.md".into()
            })
        );
    }

    #[test]
    fn parse_maps_write_tools_to_commands() {
        assert_eq!(
            parse_tool_call("write_note", &json!({ "path": "a.md", "contents": "hi" })).unwrap(),
            ToolDispatch::Command(Command::WriteNote {
                path: "a.md".into(),
                contents: "hi".into()
            })
        );
        assert_eq!(
            parse_tool_call("rename_note", &json!({ "from": "a.md", "to": "b.md" })).unwrap(),
            ToolDispatch::Command(Command::RenameNote {
                from: "a.md".into(),
                to: "b.md".into()
            })
        );
        assert_eq!(
            parse_tool_call("delete_note", &json!({ "path": "a.md" })).unwrap(),
            ToolDispatch::Command(Command::DeleteNote {
                path: "a.md".into()
            })
        );
        assert_eq!(
            parse_tool_call("commit", &json!({ "message": "wip" })).unwrap(),
            ToolDispatch::Command(Command::Commit {
                message: "wip".into()
            })
        );
    }

    #[test]
    fn parse_rejects_unknown_tool() {
        let err = parse_tool_call("frobnicate", &json!({})).unwrap_err();
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[test]
    fn parse_rejects_missing_required_argument() {
        let err = parse_tool_call("read_note", &json!({})).unwrap_err();
        assert_eq!(err.code, INVALID_PARAMS);
    }

    /// The single text block of a successful result.
    fn text_of(r: &ToolResult) -> &str {
        assert!(!r.is_error, "expected a success result");
        match r.content.as_slice() {
            [Content::Text { text }] => text,
            other => panic!("expected one text block, got {other:?}"),
        }
    }

    #[test]
    fn render_note_result_carries_contents() {
        let r = render_query_result(&QueryResponse::Note {
            contents: "ownership moves".into(),
        });
        assert!(text_of(&r).contains("ownership moves"));
    }

    #[test]
    fn render_search_result_lists_paths_and_attaches_structured() {
        use cairn_contract::SearchResult;
        let r = render_query_result(&QueryResponse::SearchResults {
            results: vec![SearchResult {
                path: "a.md".into(),
                score: 1.0,
                snippet: "hit".into(),
                highlights: vec![],
            }],
        });
        assert!(text_of(&r).contains("a.md"));
        assert!(
            r.structured_content.is_some(),
            "search is a structured tool"
        );
    }

    #[test]
    fn render_command_committed_carries_commit_id() {
        let r = render_command_result(&CommandResponse::Committed {
            commit: "abc1234".into(),
        });
        assert!(text_of(&r).contains("abc1234"));
    }

    #[test]
    fn render_command_done_is_success() {
        let r = render_command_result(&CommandResponse::Done);
        assert!(!r.is_error);
    }

    #[test]
    fn initialize_result_advertises_version_and_tools() {
        let v = serde_json::to_value(initialize_result()).unwrap();
        assert_eq!(v["protocolVersion"], PROTOCOL_VERSION);
        assert!(
            v["capabilities"]["tools"].is_object(),
            "tools capability advertised"
        );
        assert_eq!(v["serverInfo"]["name"], "cairn");
    }

    #[test]
    fn mcp_request_defaults_params_and_id() {
        let req: McpRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"tools/list"}"#).unwrap();
        assert_eq!(req.method, "tools/list");
        assert!(req.params.is_null());
        assert!(req.id.is_null());
    }

    #[test]
    fn mcp_response_serializes_exactly_one_of_result_or_error() {
        let ok = serde_json::to_value(McpResponse::result(json!(1), json!({"k": "v"}))).unwrap();
        assert_eq!(ok["jsonrpc"], "2.0");
        assert_eq!(ok["id"], 1);
        assert_eq!(ok["result"]["k"], "v");
        assert!(ok.get("error").is_none(), "success omits error");

        let err = serde_json::to_value(McpResponse::error(
            json!("abc"),
            RpcError::method_not_found("x"),
        ))
        .unwrap();
        assert_eq!(err["id"], "abc");
        assert_eq!(err["error"]["code"], METHOD_NOT_FOUND);
        assert!(err.get("result").is_none(), "error omits result");
    }

    #[test]
    fn map_error_is_flagged_and_carries_message() {
        let r = map_error(&ContractError::NotFound {
            what: "missing.md".into(),
        });
        assert!(r.is_error, "contract errors map to isError: true");
        match r.content.as_slice() {
            [Content::Text { text }] => assert!(text.contains("missing.md")),
            other => panic!("expected one text block, got {other:?}"),
        }
    }
}
