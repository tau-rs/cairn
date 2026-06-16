use cairn_app::Engine;
use cairn_contract::{Query, QueryResponse};
use cairn_daemon::{build_router, AppState};
use cairn_domain::NotePath;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::{FsChange, Vcs};
use cairn_service::ServiceError;
use futures_util::StreamExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// A daemon `AppState` over a real on-disk cairn (so the git working tree and
/// store reflect files written in the test).
fn disk_state(dir: &std::path::Path) -> AppState {
    AppState::new(Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    ))
}

fn changed(path: &str) -> FsChange {
    FsChange::Changed(NotePath::new(path).unwrap())
}

fn removed(path: &str) -> FsChange {
    FsChange::Removed(NotePath::new(path).unwrap())
}

fn read(state: &AppState, path: &str) -> Result<String, ServiceError> {
    match state.run_query_blocking(&Query::GetNote { path: path.into() })? {
        QueryResponse::Note { contents } => Ok(contents),
        other => panic!("expected Note, got {other:?}"),
    }
}

#[test]
fn confirm_before_delete_absorbs_a_transient_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let state = disk_state(tmp.path());
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    state.apply_change_blocking(&changed("a.md"));

    // Remove the file but recreate it inside the grace window (a tmp-rename /
    // non-atomic write looks like this to the watcher).
    std::fs::remove_file(tmp.path().join("a.md")).unwrap();
    let dir = tmp.path().to_path_buf();
    let recreator = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("a.md"), "hello again").unwrap();
    });

    state.apply_change_confirmed_blocking(&removed("a.md"), Duration::from_millis(150));
    recreator.join().unwrap();

    // The note survived (re-routed to Changed), with the new content indexed.
    assert_eq!(read(&state, "a.md").unwrap(), "hello again");
}

#[test]
fn confirm_before_delete_honors_a_real_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let state = disk_state(tmp.path());
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    state.apply_change_blocking(&changed("a.md"));

    std::fs::remove_file(tmp.path().join("a.md")).unwrap();
    state.apply_change_confirmed_blocking(&removed("a.md"), Duration::from_millis(40));

    // Genuinely gone after the grace: the delete is honored.
    assert!(matches!(
        read(&state, "a.md"),
        Err(ServiceError::NotFound(_))
    ));
}

#[test]
fn auto_commit_commits_dirty_tree_and_skips_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let state = disk_state(tmp.path());
    std::fs::write(tmp.path().join("a.md"), "hi").unwrap();
    state.apply_change_blocking(&changed("a.md"));

    state.commit_external_blocking("cairn: sync external edits");

    let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
    assert_eq!(vcs.history("a.md").unwrap().len(), 1, "one sync commit");
    assert!(
        !vcs.is_dirty().unwrap(),
        "working tree clean after auto-commit"
    );

    // A second call on a clean tree must not create an empty commit.
    state.commit_external_blocking("cairn: sync external edits");
    assert_eq!(
        vcs.history("a.md").unwrap().len(),
        1,
        "no empty commit on a clean tree"
    );
}

#[test]
fn native_rename_reindexes_but_does_not_rewrite_links() {
    let tmp = tempfile::tempdir().unwrap();
    let state = disk_state(tmp.path());
    std::fs::write(tmp.path().join("a.md"), "i am a").unwrap();
    std::fs::write(tmp.path().join("b.md"), "see [[a]]").unwrap();
    state.apply_change_blocking(&changed("a.md"));
    state.apply_change_blocking(&changed("b.md"));

    // Native `mv a.md c.md`: the watcher reports Removed(a) + Changed(c).
    std::fs::rename(tmp.path().join("a.md"), tmp.path().join("c.md")).unwrap();
    state.apply_change_blocking(&removed("a.md"));
    state.apply_change_blocking(&changed("c.md"));

    // Index stays correct: the note moved.
    assert_eq!(read(&state, "c.md").unwrap(), "i am a");
    assert!(matches!(
        read(&state, "a.md"),
        Err(ServiceError::NotFound(_))
    ));

    // Documented limitation: a native rename does NOT rewrite wikilinks; use the
    // rename tool/command for link-preserving moves.
    assert_eq!(
        read(&state, "b.md").unwrap(),
        "see [[a]]",
        "native rename leaves [[a]] unrewritten"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_change_pushes_event_then_dedups() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state =
        AppState::new(engine).with_allowed_origins(vec!["http://localhost:5173".to_string()]);
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // /events now requires an allowlisted Origin (audit S2); send a permitted one.
    let mut req = format!("ws://{addr}/events").into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", "http://localhost:5173".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
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
