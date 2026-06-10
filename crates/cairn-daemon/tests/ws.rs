use cairn_app::Engine;
use cairn_contract::Command;
use cairn_daemon::{build_router, AppState};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use futures_util::StreamExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;

/// Spawn a daemon whose WS allowlist is exactly `origins`; return its address.
async fn serve_with_origins(origins: Vec<String>) -> std::net::SocketAddr {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine).with_allowed_origins(origins);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
        drop(tmp); // hold the tempdir until the server stops
    });
    addr
}

/// Build a `/events` WS handshake request, optionally carrying an `Origin`.
fn ws_request(
    addr: std::net::SocketAddr,
    origin: Option<&str>,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut req = format!("ws://{addr}/events").into_client_request().unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    req
}

// Multi-threaded runtime: the test triggers a blocking engine call directly
// while the server task runs concurrently, so they must not share one thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_command_pushes_event_over_websocket() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine).with_allowed_origins(vec!["http://localhost:5173".to_string()]);
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        // Ignore the error on teardown when the test drops the socket.
        let _ = axum::serve(listener, app).await;
    });

    // Connect the WS first so the broadcast subscription exists.
    let (mut ws, _resp) = tokio_tungstenite::connect_async(ws_request(addr, Some("http://localhost:5173")))
        .await
        .unwrap();
    // Give the server task a moment to register the subscriber.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Trigger a command on the same state -> publishes events to the channel.
    // Run the blocking dispatch off the async worker thread.
    let trigger = state.clone();
    tokio::task::spawn_blocking(move || {
        trigger
            .run_command_blocking(&Command::WriteNote {
                path: "a.md".into(),
                contents: "hi".into(),
            })
            .unwrap();
    })
    .await
    .unwrap();

    // The first event should be note_changed for a.md.
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for event")
        .expect("websocket stream ended")
        .expect("websocket error");
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["type"], "note_changed");
    assert_eq!(json["path"], "a.md");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_origin_is_rejected() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let err = tokio_tungstenite::connect_async(ws_request(addr, Some("http://evil.example")))
        .await
        .expect_err("foreign origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected HTTP 403, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_origin_is_rejected() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let err = tokio_tungstenite::connect_async(ws_request(addr, None))
        .await
        .expect_err("missing origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected HTTP 403, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permitted_origin_upgrades() {
    let addr = serve_with_origins(vec!["http://localhost:5173".to_string()]).await;
    let (_ws, resp) = tokio_tungstenite::connect_async(ws_request(addr, Some("http://localhost:5173")))
        .await
        .expect("permitted origin must upgrade");
    assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
}
