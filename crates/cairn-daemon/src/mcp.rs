//! The `/mcp` route: a Streamable-HTTP MCP endpoint over the cairn dispatcher.
//!
//! Tool calls map (via the pure `cairn-mcp` crate) to the existing
//! `Command`/`Query` contract and run through the same engine-lock discipline as
//! `/command` and `/query` — write tools reuse [`AppState::run_command_blocking`],
//! so their events reach `/events` subscribers and plugins for free. Tools are
//! request/response, so this mirrors the command/query handlers, not the SSE
//! `/ask` handler.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use cairn_contract::ContractError;
use cairn_mcp::{
    initialize_result, map_error, parse_tool_call, render_command_result, render_query_result,
    tools_list, McpRequest, McpResponse, RpcError, ToolDispatch, INTERNAL_ERROR, INVALID_PARAMS,
    METHOD_NOT_FOUND,
};
use serde_json::{json, Value};

use crate::AppState;

/// `POST /mcp`: handle one JSON-RPC request (`initialize`, `tools/list`,
/// `tools/call`). Notifications (`notifications/*`) get a `202` with no body.
pub(crate) async fn mcp_handler(
    State(state): State<AppState>,
    Json(req): Json<McpRequest>,
) -> Response {
    if req.method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }
    let id = req.id.clone();
    let resp = match req.method.as_str() {
        "initialize" => McpResponse::result(
            id,
            serde_json::to_value(initialize_result()).unwrap_or(Value::Null),
        ),
        "tools/list" => McpResponse::result(id, json!({ "tools": tools_list(state.mcp_write) })),
        "tools/call" => tools_call(&state, id, &req.params).await,
        other => McpResponse::error(
            id,
            RpcError {
                code: METHOD_NOT_FOUND,
                message: format!("unknown method: {other}"),
            },
        ),
    };
    Json(resp).into_response()
}

/// Resolve and run a `tools/call`, returning the JSON-RPC response. A tool-level
/// failure (note not found, invalid path) is a successful response whose result
/// is flagged `isError`; only malformed requests / unknown tools / worker panics
/// become JSON-RPC errors.
async fn tools_call(state: &AppState, id: Value, params: &Value) -> McpResponse {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return McpResponse::error(
            id,
            RpcError {
                code: INVALID_PARAMS,
                message: "tools/call requires a string `name`".to_string(),
            },
        );
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let dispatch = match parse_tool_call(name, &args) {
        Ok(d) => d,
        Err(e) => return McpResponse::error(id, e),
    };

    // Write tools are absent from `tools/list` in read-only mode; reject a direct
    // call too (defense in depth) as if the tool did not exist.
    if matches!(dispatch, ToolDispatch::Command(_)) && !state.mcp_write {
        return McpResponse::error(
            id,
            RpcError {
                code: METHOD_NOT_FOUND,
                message: format!(
                    "unknown tool: {name} (daemon is read-only; start with --mcp-write)"
                ),
            },
        );
    }

    // Engine work blocks; run it off the reactor under the engine lock, exactly
    // like the command/query handlers.
    let st = state.clone();
    let outcome = match dispatch {
        ToolDispatch::Query(q) => {
            tokio::task::spawn_blocking(move || {
                st.run_query_blocking(&q).map(|r| render_query_result(&r))
            })
            .await
        }
        ToolDispatch::Command(c) => {
            tokio::task::spawn_blocking(move || {
                st.run_command_blocking(&c)
                    .map(|r| render_command_result(&r))
            })
            .await
        }
    };

    match outcome {
        Ok(Ok(result)) => McpResponse::result(id, to_value(result)),
        // Tool-level failure → a result with isError, not a protocol error.
        Ok(Err(svc)) => McpResponse::result(id, to_value(map_error(&ContractError::from(svc)))),
        Err(join) => {
            tracing::error!(error = %join, "mcp: tool worker panicked");
            McpResponse::error(
                id,
                RpcError {
                    code: INTERNAL_ERROR,
                    message: "internal error".to_string(),
                },
            )
        }
    }
}

/// Serialize a tool result, falling back to a generic error result if it somehow
/// fails to serialize (it cannot in practice — all fields are plain JSON).
fn to_value(result: cairn_mcp::ToolResult) -> Value {
    serde_json::to_value(result).unwrap_or_else(|_| json!({"isError": true, "content": []}))
}
