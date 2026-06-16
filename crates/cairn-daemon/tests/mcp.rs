//! Integration tests for the `/mcp` MCP route, driven in-process via `oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use http_body_util::BodyExt;
use tower::ServiceExt; // for `oneshot`

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn engine(dir: &std::path::Path) -> Engine {
    Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    )
}

/// POST a JSON-RPC body to `/mcp` with an optional bearer token, return
/// `(status, parsed body)`. A non-JSON body parses to `Null`.
async fn rpc(
    app: &axum::Router,
    body: serde_json::Value,
    auth: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json");
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    let resp = app
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn call(name: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}

#[tokio::test]
async fn initialize_advertises_protocol_and_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())));
    let (status, body) = rpc(
        &app,
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["result"]["protocolVersion"],
        cairn_mcp::PROTOCOL_VERSION
    );
    assert!(body["result"]["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn tools_list_gates_writes_on_mcp_write() {
    let tmp = tempfile::tempdir().unwrap();
    let list = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});

    let ro = build_router(AppState::new(engine(tmp.path())));
    let (_, body) = rpc(&ro, list.clone(), None).await;
    let names: Vec<String> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 8, "read-only mode lists 8 tools");
    assert!(!names.iter().any(|n| n == "write_note"));

    let rw = build_router(AppState::new(engine(tmp.path())).with_mcp_write(true));
    let (_, body) = rpc(&rw, list, None).await;
    assert_eq!(body["result"]["tools"].as_array().unwrap().len(), 12);
}

#[tokio::test]
async fn write_then_read_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())).with_mcp_write(true));

    let (status, body) = rpc(
        &app,
        call(
            "write_note",
            serde_json::json!({"path":"a.md","contents":"ownership"}),
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"]["isError"], false);

    let (_, body) = rpc(
        &app,
        call("read_note", serde_json::json!({"path":"a.md"})),
        None,
    )
    .await;
    assert_eq!(body["result"]["content"][0]["text"], "ownership");
}

#[tokio::test]
async fn write_tool_rejected_in_read_only_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path()))); // write disabled
    let (status, body) = rpc(
        &app,
        call(
            "write_note",
            serde_json::json!({"path":"a.md","contents":"x"}),
        ),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "transport ok; rejection is JSON-RPC level"
    );
    assert_eq!(body["error"]["code"], cairn_mcp::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn missing_note_is_a_tool_error_not_http_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())));
    let (status, body) = rpc(
        &app,
        call("read_note", serde_json::json!({"path":"nope.md"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"]["isError"], true);
}

#[tokio::test]
async fn mcp_requires_token_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())).with_token(TOKEN));
    let list = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});

    let (status, _) = rpc(&app, list.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no token rejected");

    let (status, _) = rpc(&app, list.clone(), Some("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "wrong token rejected");

    let (status, _) = rpc(&app, list, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK, "correct bearer accepted");
}

#[tokio::test]
async fn mcp_write_forwards_events_to_plugins() {
    use cairn_ports::{
        EventDispatchError, PluginCallbacks, PluginEvent, PluginHost, PluginInfo, PortError,
    };
    use std::sync::{Arc, Mutex};

    // Records every cairn event forwarded to plugins, proving an MCP-originated
    // write flows through the same EventTap as `/command`.
    struct RecordingHost(Arc<Mutex<Vec<PluginEvent>>>);
    impl PluginHost for RecordingHost {
        fn plugins(&self) -> Vec<PluginInfo> {
            Vec::new()
        }
        fn invoke(
            &mut self,
            plugin: &str,
            _command: &str,
            _args: &serde_json::Value,
            _callbacks: &mut dyn PluginCallbacks,
        ) -> Result<serde_json::Value, PortError> {
            Err(PortError::NotFound(format!("plugin {plugin}")))
        }
        fn dispatch_event(
            &mut self,
            event: &PluginEvent,
            _callbacks: &mut dyn PluginCallbacks,
        ) -> Vec<EventDispatchError> {
            self.0.lock().unwrap().push(event.clone());
            Vec::new()
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let mut eng = engine(tmp.path());
    let recorded = Arc::new(Mutex::new(Vec::new()));
    eng.set_plugin_host(Box::new(RecordingHost(recorded.clone())));
    let app = build_router(AppState::new(eng).with_mcp_write(true));

    let (status, _) = rpc(
        &app,
        call(
            "write_note",
            serde_json::json!({"path":"a.md","contents":"hi"}),
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let events = recorded.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PluginEvent::NoteChanged(p) if p.as_str() == "a.md")),
        "MCP write must forward NoteChanged(a.md), got {events:?}"
    );
}

#[tokio::test]
async fn mcp_accepts_token_in_query_for_headerless_clients() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())).with_token(TOKEN));
    let uri = format!("/mcp?token={TOKEN}");
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "query-param token accepted");
}
