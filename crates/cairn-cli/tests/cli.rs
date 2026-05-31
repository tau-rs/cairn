use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn write_search_backlinks_commit_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let mut write_a = Command::cargo_bin("cairn").unwrap();
    write_a.args([
        "--cairn",
        dir.to_str().unwrap(),
        "write",
        "a.md",
        "links to [[b]]",
    ]);
    write_a.assert().success().stdout(contains("wrote a.md"));

    let mut write_b = Command::cargo_bin("cairn").unwrap();
    write_b.args([
        "--cairn",
        dir.to_str().unwrap(),
        "write",
        "b.md",
        "the target",
    ]);
    write_b.assert().success();

    let mut search = Command::cargo_bin("cairn").unwrap();
    search.args(["--cairn", dir.to_str().unwrap(), "search", "target"]);
    search.assert().success().stdout(contains("b.md"));

    let mut backlinks = Command::cargo_bin("cairn").unwrap();
    backlinks.args(["--cairn", dir.to_str().unwrap(), "backlinks", "b.md"]);
    backlinks.assert().success().stdout(contains("a.md"));

    let mut commit = Command::cargo_bin("cairn").unwrap();
    commit.args(["--cairn", dir.to_str().unwrap(), "commit", "first"]);
    commit.assert().success().stdout(contains("committed"));
}
