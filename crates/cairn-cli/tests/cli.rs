use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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
fn reinit_reports_already_a_cairn() {
    // D9: a second `init` on an existing cairn must report it as a no-op,
    // distinct from the fresh-init success line, instead of silently claiming
    // it initialized again.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir)
        .arg("init")
        .assert()
        .success()
        .stdout(contains("initialized cairn"));
    cairn(dir)
        .arg("init")
        .assert()
        .success()
        .stdout(contains("already a cairn"))
        .stdout(contains("initialized cairn").not());
}

#[test]
fn short_query_search_prints_hint() {
    // D11: a sub-2-char query is rejected by the n-gram index; the CLI must
    // surface a hint (on stderr) rather than a bare empty result.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "alpha beta"])
        .assert()
        .success();
    cairn(dir)
        .args(["search", "a"])
        .assert()
        .success()
        .stderr(contains("minimum"));
    // A long-enough query still works and prints no hint.
    cairn(dir)
        .args(["search", "alpha"])
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stderr(contains("minimum").not());
}

#[test]
fn non_search_commands_work_without_a_startup_reindex() {
    // D2: `backlinks` and `list` read the lazy notes-cache directly, so they
    // must return correct results even though the CLI no longer builds the
    // search index on startup for them.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "links to [[b]]"])
        .assert()
        .success();
    cairn(dir)
        .args(["write", "b.md", "target"])
        .assert()
        .success();

    cairn(dir)
        .args(["backlinks", "b.md"])
        .assert()
        .success()
        .stdout(contains("a.md"));
    cairn(dir)
        .arg("list")
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("b.md"));
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
    // `watch` is also guarded — it must require an existing cairn.
    cairn(dir)
        .args(["watch"])
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
fn history_show_restore_subcommands() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();

    cairn(dir).args(["write", "a.md", "v1"]).assert().success();
    cairn(dir).args(["commit", "v1"]).assert().success();
    cairn(dir).args(["write", "a.md", "v2"]).assert().success();
    cairn(dir).args(["commit", "v2"]).assert().success();

    // `history` lists revisions newest-first as `<7-char-id>  <message>`.
    let history = cairn(dir).args(["history", "a.md"]).assert().success();
    let stdout = String::from_utf8(history.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("v2"),
        "newest line should be v2: {stdout}"
    );
    assert!(
        lines[1].contains("v1"),
        "oldest line should be v1: {stdout}"
    );

    // The older commit's short id is the first token of the last line.
    let old_id = lines[1].split_whitespace().next().unwrap();

    // `show` prints the note's contents at that past revision.
    cairn(dir)
        .args(["show", "a.md", old_id])
        .assert()
        .success()
        .stdout("v1");

    // `restore` writes that past version back as current, so `read` sees it.
    cairn(dir)
        .args(["restore", "a.md", old_id])
        .assert()
        .success();
    cairn(dir)
        .args(["read", "a.md"])
        .assert()
        .success()
        .stdout("v1");
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
