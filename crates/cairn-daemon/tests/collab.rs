//! `/collab` op-relay: two raw WS clients converge over the wire; a late
//! joiner is caught up by the snapshot; auth is enforced. See spec §8.

use cairn_app::Engine;
use cairn_contract::{CollabClientMsg, CollabServerMsg};
use cairn_daemon::{build_router, AppState};
use cairn_domain::{block::BlockKind, BlockDoc, BlockId, BlockOp};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_service::{block_op_from_wire, block_op_to_wire};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

const ORIGIN: &str = "http://localhost:5173";
const TOKEN: &str = "secret";

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve() -> std::net::SocketAddr {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine)
        .with_allowed_origins(vec![ORIGIN.to_string()])
        .with_token(TOKEN);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
        drop(tmp);
    });
    addr
}

/// Like `serve()`, but also hands back the `AppState` and the backing
/// `TempDir` so a test can drive the watcher/flush machinery directly (e.g.
/// simulate a foreign on-disk edit) while real WS clients are connected.
async fn serve_with_state() -> (
    std::net::SocketAddr,
    cairn_daemon::AppState,
    tempfile::TempDir,
) {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let state = AppState::new(engine)
        .with_allowed_origins(vec![ORIGIN.to_string()])
        .with_token(TOKEN);
    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, state, tmp)
}

fn req(
    addr: std::net::SocketAddr,
    origin: Option<&str>,
    token: Option<&str>,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let url = match token {
        Some(t) => format!("ws://{addr}/collab?token={t}"),
        None => format!("ws://{addr}/collab"),
    };
    let mut r = url.into_client_request().unwrap();
    if let Some(o) = origin {
        r.headers_mut().insert("origin", o.parse().unwrap());
    }
    r
}

async fn connect(addr: std::net::SocketAddr) -> Ws {
    tokio_tungstenite::connect_async(req(addr, Some(ORIGIN), Some(TOKEN)))
        .await
        .expect("handshake")
        .0
}

async fn send(ws: &mut Ws, msg: &CollabClientMsg) {
    ws.send(Message::Text(serde_json::to_string(msg).unwrap().into()))
        .await
        .unwrap();
}

async fn recv(ws: &mut Ws) -> CollabServerMsg {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

fn insert(replica: u64, text: &str) -> BlockOp {
    BlockOp::Insert {
        id: BlockId {
            replica,
            counter: 0,
        },
        after: None,
        lamport: 1,
        kind: BlockKind::Paragraph,
        text: text.into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_clients_converge_over_the_wire() {
    let addr = serve().await;
    let note = "n.md";

    let mut c1 = connect(addr).await;
    let mut c2 = connect(addr).await;
    send(
        &mut c1,
        &CollabClientMsg::Join {
            note: note.into(),
            replica: 1,
        },
    )
    .await;
    send(
        &mut c2,
        &CollabClientMsg::Join {
            note: note.into(),
            replica: 2,
        },
    )
    .await;
    // Each gets Joined + (empty) Snapshot.
    assert!(matches!(
        recv(&mut c1).await,
        CollabServerMsg::Joined { .. }
    ));
    assert!(matches!(
        recv(&mut c1).await,
        CollabServerMsg::Snapshot { .. }
    ));
    assert!(matches!(
        recv(&mut c2).await,
        CollabServerMsg::Joined { .. }
    ));
    assert!(matches!(
        recv(&mut c2).await,
        CollabServerMsg::Snapshot { .. }
    ));

    let op1 = insert(1, "from one");
    let op2 = insert(2, "from two");
    send(
        &mut c1,
        &CollabClientMsg::Op {
            note: note.into(),
            op: block_op_to_wire(op1.clone()),
        },
    )
    .await;
    send(
        &mut c2,
        &CollabClientMsg::Op {
            note: note.into(),
            op: block_op_to_wire(op2.clone()),
        },
    )
    .await;

    // Each client receives the OTHER's op (self-echo suppressed).
    let got1 = match recv(&mut c1).await {
        CollabServerMsg::Op { op, .. } => block_op_from_wire(op),
        other => panic!("expected Op, got {other:?}"),
    };
    let got2 = match recv(&mut c2).await {
        CollabServerMsg::Op { op, .. } => block_op_from_wire(op),
        other => panic!("expected Op, got {other:?}"),
    };

    // Reconstruct each replica and assert identical materialize.
    let mut d1 = BlockDoc::from_markdown(1, "");
    d1.merge(op1.clone());
    d1.merge(got1);
    let mut d2 = BlockDoc::from_markdown(2, "");
    d2.merge(op2.clone());
    d2.merge(got2);
    assert_eq!(d1.materialize(), d2.materialize());
    assert!(d1.materialize().contains("from one"));
    assert!(d1.materialize().contains("from two"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_joiner_is_caught_up_by_snapshot() {
    let addr = serve().await;
    let note = "n.md";

    let mut c1 = connect(addr).await;
    send(
        &mut c1,
        &CollabClientMsg::Join {
            note: note.into(),
            replica: 1,
        },
    )
    .await;
    let _ = recv(&mut c1).await; // Joined
    let _ = recv(&mut c1).await; // Snapshot (empty)

    let op1 = insert(1, "seeded");
    send(
        &mut c1,
        &CollabClientMsg::Op {
            note: note.into(),
            op: block_op_to_wire(op1.clone()),
        },
    )
    .await;
    // Let the daemon merge op1 into its replica before the late join.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut c2 = connect(addr).await;
    send(
        &mut c2,
        &CollabClientMsg::Join {
            note: note.into(),
            replica: 2,
        },
    )
    .await;
    assert!(matches!(
        recv(&mut c2).await,
        CollabServerMsg::Joined { .. }
    ));
    let snap = match recv(&mut c2).await {
        CollabServerMsg::Snapshot { ops, .. } => ops,
        other => panic!("expected Snapshot, got {other:?}"),
    };

    let mut d2 = BlockDoc::from_markdown(2, "");
    for op in snap {
        d2.merge(block_op_from_wire(op));
    }
    assert!(d2.materialize().contains("seeded"));
    // The joiner adopts the sender's live BlockId (shared identity over the
    // wire), not a fresh one — the whole point of state-as-ops catch-up.
    let ids = d2.block_ids_in_order();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        ids[0],
        BlockId {
            replica: 1,
            counter: 0
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collab_rejects_bad_token_and_origin() {
    let addr = serve().await;
    // Bad token -> 401.
    let err = tokio_tungstenite::connect_async(req(addr, Some(ORIGIN), Some("wrong")))
        .await
        .expect_err("bad token must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
        other => panic!("expected 401, got {other:?}"),
    }
    // Bad origin (valid token) -> 403.
    let err = tokio_tungstenite::connect_async(req(addr, Some("http://evil.example"), Some(TOKEN)))
        .await
        .expect_err("bad origin must be refused");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected 403, got {other:?}"),
    }
}

/// The DoD's headline proof (spec §8.2 / §13): a foreign editor writes the
/// note directly on disk while two peers hold a live `/collab` session on
/// it. The watcher defers the sessioned `Changed` to arbitration (marks the
/// session dirty instead of auto-committing), the next flush pass folds the
/// foreign bytes into the shared replica, and both peers see the resulting
/// `Op` over the wire — no lost work, both replicas converge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_disk_edit_mid_session_reaches_both_peers() {
    use cairn_ports::FsChange;
    let (addr, state, tmp) = serve_with_state().await;
    let note = "n.md";
    let path = cairn_domain::NotePath::new(note).unwrap();

    // Two peers join; each gets Joined + (empty) Snapshot. The note has never
    // been committed, so the session seeds empty (Seed { markdown: "", dirty:
    // false }); the fold against base="" inserts the foreign block.
    let mut c1 = connect(addr).await;
    let mut c2 = connect(addr).await;
    for (c, r) in [(&mut c1, 1u64), (&mut c2, 2u64)] {
        send(
            c,
            &CollabClientMsg::Join {
                note: note.into(),
                replica: r,
            },
        )
        .await;
        assert!(matches!(recv(c).await, CollabServerMsg::Joined { .. }));
        assert!(matches!(recv(c).await, CollabServerMsg::Snapshot { .. }));
    }

    // A foreign editor writes n.md directly, then the watcher fires.
    std::fs::write(tmp.path().join(note), "foreign para\n").unwrap();
    let s = state.clone();
    let p = path.clone();
    tokio::task::spawn_blocking(move || {
        s.apply_change_blocking(&FsChange::Changed(p.clone())); // arbitration -> dirty
        s.run_collab_flush_pass(std::time::Duration::ZERO); // fold + fan out
    })
    .await
    .unwrap();

    // Both peers receive the folded Insert over the wire. A single-block
    // foreign edit produces exactly one Op fanned to each peer, so one recv
    // per peer suffices (the 5s recv timeout is the only synchronization).
    let mut d1 = BlockDoc::from_markdown(1, "");
    let mut d2 = BlockDoc::from_markdown(2, "");
    for (c, d) in [(&mut c1, &mut d1), (&mut c2, &mut d2)] {
        match recv(c).await {
            CollabServerMsg::Op { op, .. } => d.merge(block_op_from_wire(op)),
            other => panic!("expected folded Op, got {other:?}"),
        }
    }
    assert_eq!(d1.materialize(), d2.materialize());
    assert!(d1.materialize().contains("foreign para"), "no lost work");
}
