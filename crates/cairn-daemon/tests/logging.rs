use axum::body::Body;
use axum::http::Request;
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use tower::ServiceExt; // for `oneshot`

fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    AppState::new(engine)
}

#[tokio::test]
#[tracing_test::traced_test]
async fn command_request_emits_span_with_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    // The per-request span carries the command kind and a completion event.
    assert!(logs_contain("request completed"));
    assert!(logs_contain("command=\"write_note\""));
}
