use cairn_domain::NotePath;
use cairn_infra::NotifyWatcher;
use cairn_ports::{FsChange, Watcher};
use std::time::Duration;

fn drain_for(rx: &std::sync::mpsc::Receiver<FsChange>, want: &FsChange) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(c) if &c == want => return true,
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn reports_md_create() {
    let tmp = tempfile::tempdir().unwrap();
    let handle = NotifyWatcher.watch(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join(".git/config"), "x").unwrap();
    std::fs::write(tmp.path().join("note.txt"), "x").unwrap();
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    assert!(drain_for(
        &handle.changes,
        &FsChange::Changed(NotePath::new("a.md").unwrap())
    ));
}

#[test]
fn reports_md_removal() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
    let handle = NotifyWatcher.watch(tmp.path()).unwrap();
    std::fs::remove_file(tmp.path().join("a.md")).unwrap();
    assert!(drain_for(
        &handle.changes,
        &FsChange::Removed(NotePath::new("a.md").unwrap())
    ));
}
