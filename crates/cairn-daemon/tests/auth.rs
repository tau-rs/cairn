use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use tower::ServiceExt; // for `oneshot`

// A realistic 64-hex token, matching what the daemon generates at startup.
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn engine(dir: &std::path::Path) -> Engine<LocalFsStore, TantivyIndex, GitVcs> {
    Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    )
}

fn app(dir: &std::path::Path) -> axum::Router {
    build_router(AppState::new(engine(dir)).with_token(TOKEN))
}

fn write_command(auth: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/command")
        .header("content-type", "application/json");
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    b.body(Body::from(
        serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}).to_string(),
    ))
    .unwrap()
}

#[tokio::test]
async fn no_token_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path()).oneshot(write_command(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn correct_token_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path())
        .oneshot(write_command(Some(TOKEN)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn wrong_token_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app(tmp.path())
        .oneshot(write_command(Some("nope")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn malformed_auth_header_is_401() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/command")
        .header("content-type", "application/json")
        .header("authorization", format!("Basic {TOKEN}"))
        .body(Body::from(
            serde_json::json!({"type":"write_note","path":"a.md","contents":"hi"}).to_string(),
        ))
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_also_requires_token() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"type":"list_notes"}).to_string(),
        ))
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn no_token_configured_serves_without_auth() {
    // The in-process/library default (`AppState::new`, no `with_token`) disables
    // the gate: a `/command` with no Authorization header still succeeds.
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(engine(tmp.path())));
    let resp = app.oneshot(write_command(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_is_open_without_token() {
    let tmp = tempfile::tempdir().unwrap();
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app(tmp.path()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
