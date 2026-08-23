//! Multi-source coherence: client writes and external edits land as
//! one-commit-per-session with engine-generated messages (spec DoD).

use cairn_contract::{Command, Query, QueryResponse};

fn state(dir: &std::path::Path) -> cairn_daemon::AppState {
    let engine = cairn_startup::build_engine(dir).unwrap();
    cairn_daemon::AppState::new(engine)
}

#[test]
fn client_session_seals_with_generated_message() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    s.run_command_blocking(&Command::WriteNote {
        path: "roadmap.md".into(),
        contents: "# Q3 Roadmap\n\none two three\n".into(),
    })
    .unwrap();
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Add \"Q3 Roadmap\" (+6 words)");
    // Sealing again with no changes creates nothing.
    s.seal_blocking();
    let QueryResponse::History { revisions: again } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        again.len(),
        revisions.len(),
        "no empty commit on a clean seal"
    );
}

#[test]
fn external_edit_session_seals_with_generated_message() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    // Simulate the watcher path: file appears on disk, change applied, sealed.
    std::fs::write(tmp.path().join("note.md"), "# Note\n\nalpha beta\n").unwrap();
    s.apply_change_blocking(&cairn_ports::FsChange::Changed(
        cairn_domain::NotePath::new("note.md").unwrap(),
    ));
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Add \"Note\" (+4 words)");
}

#[test]
fn multi_note_session_rolls_up() {
    let tmp = tempfile::tempdir().unwrap();
    let s = state(tmp.path());
    for (p, c) in [("a.md", "# A\nx"), ("b.md", "# B\ny")] {
        s.run_command_blocking(&Command::WriteNote {
            path: p.into(),
            contents: c.into(),
        })
        .unwrap();
    }
    s.seal_blocking();
    let QueryResponse::History { revisions } = s
        .run_query_blocking(&Query::VaultHistory { limit: None })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions[0].message, "Update 2 notes: \"A\", \"B\"");
}
