use assert_cmd::Command;
use predicates::str::contains;

/// Build a local git repo that looks like a plugin, return its path.
fn fixture_plugin(dir: &std::path::Path, id: &str) {
    let repo = git2::Repository::init(dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!("id=\"{id}\"\nname=\"X\"\nversion=\"0.1.0\"\n[engine]\ncommand=\"bin\"\n"),
    )
    .unwrap();
    let mut idx = repo.index().unwrap();
    idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    idx.write().unwrap();
    let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("t", "t@e").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
}

#[test]
fn plugin_add_lands_untrusted_and_prints_snippet() {
    let cairn = tempfile::tempdir().unwrap();
    Command::cargo_bin("cairn")
        .unwrap()
        .args(["--cairn", cairn.path().to_str().unwrap(), "init"])
        .assert()
        .success();

    let src = tempfile::tempdir().unwrap();
    fixture_plugin(src.path(), "demo");

    Command::cargo_bin("cairn")
        .unwrap()
        .args([
            "--cairn",
            cairn.path().to_str().unwrap(),
            "plugin",
            "add",
            src.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("UNTRUSTED"))
        .stdout(contains("[[plugins.trusted]]"))
        .stdout(contains("dir  = \"demo\""));

    assert!(cairn
        .path()
        .join(".cairn/plugins/demo/manifest.toml")
        .exists());

    // list (offline) shows it as untrusted; remove reminds about cairn.toml.
    Command::cargo_bin("cairn")
        .unwrap()
        .args([
            "--cairn",
            cairn.path().to_str().unwrap(),
            "plugin",
            "list",
            "--offline",
        ])
        .assert()
        .success()
        .stdout(contains("demo"))
        .stdout(contains("no"));

    Command::cargo_bin("cairn")
        .unwrap()
        .args([
            "--cairn",
            cairn.path().to_str().unwrap(),
            "plugin",
            "remove",
            "demo",
        ])
        .assert()
        .success()
        .stdout(contains("revoke trust"));
}
