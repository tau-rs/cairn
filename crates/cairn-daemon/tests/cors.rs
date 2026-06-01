use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, cors_layer, AppState};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use tower::ServiceExt; // for `oneshot`

fn app(dir: &std::path::Path, origins: &[String]) -> axum::Router {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        InMemoryIndex::default(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    build_router(AppState::new(engine)).layer(cors_layer(origins))
}

#[tokio::test]
async fn allowed_origin_is_reflected() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query")
                .header("content-type", "application/json")
                .header("origin", "http://localhost:5173")
                .body(Body::from("{\"type\":\"list_notes\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:5173"
    );
}

#[tokio::test]
async fn disallowed_origin_gets_no_allow_header() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/query")
                .header("content-type", "application/json")
                .header("origin", "http://evil.example")
                .body(Body::from("{\"type\":\"list_notes\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn preflight_options_returns_allow_headers() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["http://localhost:5173".to_string()])
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/command")
                .header("origin", "http://localhost:5173")
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:5173"
    );
    assert!(resp.headers().get("access-control-allow-methods").is_some());
}

#[tokio::test]
async fn empty_allowlist_denies_any_origin() {
    // The deny-by-default guarantee: no origin is allowed when the list is empty.
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &[])
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("origin", "http://anything.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn wildcard_origin_does_not_panic_and_denies() {
    // A `*` in the allowlist would panic tower-http's AllowOrigin::list; we filter
    // it out, so the daemon treats it as no allowed origin (deny), not allow-all.
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path(), &["*".to_string()])
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("origin", "http://anything.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}
