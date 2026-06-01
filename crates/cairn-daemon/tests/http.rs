use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use http_body_util::BodyExt;
use tower::ServiceExt; // for `oneshot`

fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    AppState::new(engine)
}

async fn post_json(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn write_then_search_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, body) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hello target"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "done");

    let (status, body) = post_json(
        app.clone(),
        "/query",
        serde_json::json!({"type":"search","query":"target"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "paths");
    assert_eq!(body["paths"][0], "a.md");
}

#[tokio::test]
async fn missing_note_query_is_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));
    let (status, body) = post_json(
        app,
        "/query",
        serde_json::json!({"type":"get_note","path":"missing.md"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], "not_found");
}
