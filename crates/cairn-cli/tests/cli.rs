use assert_cmd::Command;
use predicates::str::contains;

/// A `cairn` invocation pre-pointed at `dir` via `--cairn`.
fn cairn(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("cairn").unwrap();
    cmd.args(["--cairn", dir.to_str().unwrap()]);
    cmd
}

#[test]
fn write_search_backlinks_commit_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    cairn(dir)
        .arg("init")
        .assert()
        .success()
        .stdout(contains("initialized cairn"));

    cairn(dir)
        .args(["write", "a.md", "links to [[b]]"])
        .assert()
        .success()
        .stdout(contains("wrote a.md"));
    cairn(dir)
        .args(["write", "b.md", "the target"])
        .assert()
        .success()
        .stdout(contains("wrote b.md"));
    cairn(dir)
        .args(["search", "target"])
        .assert()
        .success()
        .stdout(contains("b.md"));
    cairn(dir)
        .args(["backlinks", "b.md"])
        .assert()
        .success()
        .stdout(contains("a.md"));
    cairn(dir)
        .args(["commit", "first"])
        .assert()
        .success()
        .stdout(contains("committed"));
}

#[test]
fn read_existing_note_prints_contents() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "hello body"])
        .assert()
        .success();
    cairn(dir)
        .args(["read", "a.md"])
        .assert()
        .success()
        .stdout(contains("hello body"));
}

#[test]
fn read_missing_note_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["read", "missing.md"])
        .assert()
        .failure()
        .stderr(contains("error:"));
}

#[test]
fn commands_require_an_initialized_cairn() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // Without `init`, a non-init command must fail rather than silently init.
    cairn(dir)
        .args(["search", "x"])
        .assert()
        .failure()
        .stderr(contains("not a cairn"));
    // And it must NOT have created a .git directory.
    assert!(!dir.join(".git").exists());
}

#[test]
fn list_and_graph_subcommands() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "see [[b]]"])
        .assert()
        .success();
    cairn(dir).args(["write", "b.md", "hi"]).assert().success();

    cairn(dir)
        .arg("list")
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("b.md"));
    cairn(dir)
        .arg("graph")
        .assert()
        .success()
        .stdout(contains("a.md -> b.md"));
}

#[test]
fn rename_moves_note_and_rewrites_links() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "i am a"])
        .assert()
        .success();
    cairn(dir)
        .args(["write", "b.md", "link to [[a]] here"])
        .assert()
        .success();

    cairn(dir)
        .args(["rename", "a.md", "c.md"])
        .assert()
        .success()
        .stdout(contains("renamed a.md -> c.md"));

    // Old path gone, new path present with the same content.
    cairn(dir)
        .args(["read", "a.md"])
        .assert()
        .failure()
        .stderr(contains("error:"));
    cairn(dir)
        .args(["read", "c.md"])
        .assert()
        .success()
        .stdout(contains("i am a"));
    // The link in b.md was rewritten a -> c.
    cairn(dir)
        .args(["read", "b.md"])
        .assert()
        .success()
        .stdout(contains("link to [[c]] here"));
}

#[test]
fn search_prints_path_and_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "the borrow checker enforces ownership"])
        .assert()
        .success();
    cairn(dir)
        .args(["search", "ownership"])
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("ownership"));
}

#[test]
fn tags_and_tagged_subcommands() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "--", "---\ntags: [rust, ideas]\n---\nx"])
        .assert()
        .success();
    cairn(dir)
        .args(["write", "b.md", "--", "---\ntags: rust\n---\ny"])
        .assert()
        .success();

    cairn(dir)
        .arg("tags")
        .assert()
        .success()
        .stdout(contains("rust\t2"))
        .stdout(contains("ideas\t1"));
    cairn(dir)
        .args(["tagged", "rust"])
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("b.md"));
}
