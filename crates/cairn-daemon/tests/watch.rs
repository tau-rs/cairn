use cairn_app::Engine;
use cairn_daemon::{build_router, AppState};
use cairn_domain::NotePath;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::FsChange;
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_change_pushes_event_then_dedups() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine);
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Simulate an external edit: write the file on disk, then notify the engine.
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    let a = NotePath::new("a.md").unwrap();
    state.apply_change_blocking(&FsChange::Changed(a.clone()));

    // A real change emits `note_changed` plus a trailing `reindexed` frame;
    // read until we see `note_changed`, draining the `reindexed` that follows.
    let mut saw_note_changed = false;
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out")
            .expect("stream ended")
            .expect("ws error");
        let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
        match json["type"].as_str() {
            Some("note_changed") => {
                assert_eq!(json["path"], "a.md");
                saw_note_changed = true;
            }
            // Trailing `reindexed` from the same change: drain and stop.
            Some("reindexed") if saw_note_changed => break,
            Some("reindexed") => {}
            other => panic!("unexpected event before note_changed: {other:?}"),
        }
    }

    // Same content again -> memo dedups -> no further frame within the window.
    state.apply_change_blocking(&FsChange::Changed(a));
    assert!(tokio::time::timeout(Duration::from_millis(800), ws.next())
        .await
        .is_err());
}
