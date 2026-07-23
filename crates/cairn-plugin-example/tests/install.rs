use std::path::Path;
use std::process::Command;

use cairn_infra::plugin_install::install;
use cairn_infra::{ProcessPluginHost, TrustedPlugins};
use cairn_ports::{PluginHost, Sandbox, SandboxCapabilities, SandboxError};

/// Test double: spawns the command verbatim (no OS jail). Mirrors tests/host.rs.
struct PermissiveSandbox;
impl Sandbox for PermissiveSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        _dir: &Path,
        cmd: &Path,
        args: &[String],
        _caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
        let mut c = Command::new(cmd);
        c.args(args);
        Ok(c)
    }
}

/// A git repo whose manifest declares `id = "example"` and points its engine
/// command at the already-built example binary. Returns nothing; `install`
/// reads the manifest id.
fn fixture_example_repo(dir: &Path) {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let repo = git2::Repository::init(dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!("id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n[engine]\ncommand='{bin}'\n"),
    )
    .unwrap();
    commit_all(&repo, "init");
}

fn commit_all(repo: &git2::Repository, msg: &str) {
    let mut idx = repo.index().unwrap();
    idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    idx.write().unwrap();
    let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("t", "t@e").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parent_refs)
        .unwrap();
}

fn plugins_dir(cairn: &Path) -> std::path::PathBuf {
    cairn.join(".cairn").join("plugins")
}

#[test]
fn install_lands_untrusted_then_approves_and_spawns() {
    let src = tempfile::tempdir().unwrap();
    fixture_example_repo(src.path());
    let cairn = tempfile::tempdir().unwrap();

    let installed = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();

    // Untrusted: default-deny → the host spawns nothing.
    let host = ProcessPluginHost::load(
        &plugins_dir(cairn.path()),
        &TrustedPlugins::none(),
        &PermissiveSandbox,
    )
    .unwrap();
    assert!(host.plugins().is_empty(), "untrusted plugin must not spawn");

    // Approve with the exact installed hash → the host spawns it.
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(installed.hash.clone()))])
            .unwrap();
    let host =
        ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox).unwrap();
    assert!(host.plugins().iter().any(|p| p.id == "example"));
}

#[test]
fn readd_new_commit_forces_reapproval() {
    let src = tempfile::tempdir().unwrap();
    fixture_example_repo(src.path());
    let cairn = tempfile::tempdir().unwrap();

    let first = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();

    // Change plugin content and re-add from the same source (an update).
    std::fs::write(src.path().join("README.md"), "changed").unwrap();
    {
        let repo = git2::Repository::open(src.path()).unwrap();
        commit_all(&repo, "update");
    }
    let second = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();
    assert_ne!(first.hash, second.hash);

    // The old pin no longer matches the new content → the host refuses to spawn.
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(first.hash.clone()))]).unwrap();
    let host =
        ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox).unwrap();
    assert!(
        host.plugins().is_empty(),
        "drifted plugin must not spawn on the old pin"
    );

    // Re-pinning to the new hash restores spawn.
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(second.hash.clone()))]).unwrap();
    let host =
        ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox).unwrap();
    assert!(host.plugins().iter().any(|p| p.id == "example"));
}
