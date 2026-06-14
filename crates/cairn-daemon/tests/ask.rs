use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
use http_body_util::BodyExt; // for `.collect()`
use tower::ServiceExt; // for `oneshot`

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn engine(dir: &std::path::Path) -> Engine {
    Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    )
}

/// A runtime that ignores the prompt and emits a fixed, scripted run.
struct StubRuntime;
impl AgentRuntime for StubRuntime {
    fn answer(&self, _prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
        sink.emit(AgentEvent::TextDelta("hello".into()));
        sink.emit(AgentEvent::Completed);
        Ok(())
    }
}

fn ask_request(auth: Option<&str>, query: &str) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/ask")
        .header("content-type", "application/json");
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    b.body(Body::from(
        serde_json::json!({ "query": query }).to_string(),
    ))
    .unwrap()
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn ask_streams_sources_then_text_then_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_router(
        AppState::new(engine(tmp.path()))
            .with_token(TOKEN)
            .with_runtime(Arc::new(StubRuntime)),
    );
    let resp = app
        .oneshot(ask_request(Some(TOKEN), "anything"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let sources = body.find("\"type\":\"sources\"").expect("sources frame");
    let delta = body
        .find("\"type\":\"text_delta\"")
        .expect("text_delta frame");
    let completed = body
        .find("\"type\":\"completed\"")
        .expect("completed frame");
    assert!(
        sources < delta && delta < completed,
        "frames out of order:\n{body}"
    );
    assert!(
        body.contains("\"text\":\"hello\""),
        "missing delta text:\n{body}"
    );
}
