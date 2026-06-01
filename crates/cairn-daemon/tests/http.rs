use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use http_body_util::BodyExt;
use tower::ServiceExt; // for `oneshot`

fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
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
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).expect("response body was not valid JSON");
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
    assert_eq!(body["type"], "search_results");
    assert_eq!(body["results"][0]["path"], "a.md");
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

#[tokio::test]
async fn health_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn invalid_path_command_is_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));
    let (status, body) = post_json(
        app,
        "/command",
        serde_json::json!({"type":"write_note","path":"../escape.md","contents":"x"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], "invalid_request");
}

#[tokio::test]
async fn malformed_json_is_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("content-type", "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn list_notes_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, _) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(app, "/query", serde_json::json!({"type":"list_notes"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "notes");
    assert_eq!(body["notes"][0]["path"], "a.md");
}

#[tokio::test]
async fn list_tags_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, _) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"---\ntags: [rust]\n---\nx"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(app, "/query", serde_json::json!({"type":"list_tags"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "tags");
    assert_eq!(body["tags"][0]["tag"], "rust");
    assert_eq!(body["tags"][0]["count"], 1);
}

#[tokio::test]
async fn rename_note_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let (status, _) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"a.md","contents":"i am a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"write_note","path":"b.md","contents":"link to [[a]]"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Rename a.md -> c.md: 200 + {"type":"done"}.
    let (status, body) = post_json(
        app.clone(),
        "/command",
        serde_json::json!({"type":"rename_note","from":"a.md","to":"c.md"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "done");

    // The moved note is readable at its new path; the link in b.md was rewritten.
    let (status, body) = post_json(
        app.clone(),
        "/query",
        serde_json::json!({"type":"get_note","path":"c.md"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["contents"], "i am a");
    let (status, body) = post_json(
        app,
        "/query",
        serde_json::json!({"type":"get_note","path":"b.md"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["contents"], "link to [[c]]");
}
