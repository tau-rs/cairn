//! Install a plugin from a git URL into `<cairn>/.cairn/plugins/<id>/`, routed
//! through the existing trust gate. A freshly added plugin lands UNTRUSTED: this
//! module never writes `cairn.toml`. Provenance (source URL, ref, resolved
//! commit, content hash) is recorded in a sibling `<id>.source.toml` file that
//! sits OUTSIDE the hashed tree and is advisory only — the daemon's authoritative
//! pin remains the user-edited `cairn.toml` value.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Provenance recorded beside an installed plugin dir. `deny_unknown_fields` so a
/// typo (e.g. `hsah`) fails loudly instead of silently dropping a field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// The git URL the plugin was cloned from.
    pub source: String,
    /// The branch/tag/rev requested at add time (the TOML key is `ref`).
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// The exact commit that ref resolved to (40 hex).
    pub commit: String,
    /// `PinnedHash` of the content tree at install time.
    pub hash: String,
}

/// Why a plugin install/list/remove failed.
#[derive(Debug, thiserror::Error)]
pub enum PluginInstallError {
    /// git clone / checkout / ls-remote failure (network, auth, missing ref).
    #[error("clone {url} failed: {source}")]
    Clone {
        url: String,
        #[source]
        source: git2::Error,
    },
    /// The cloned repo has no `manifest.toml` at its root.
    #[error("{url} has no manifest.toml")]
    ManifestMissing { url: String },
    /// The cloned `manifest.toml` did not parse.
    #[error("invalid manifest.toml: {0}")]
    ManifestInvalid(String),
    /// A stored `<id>.source.toml` did not parse.
    #[error("invalid sidecar {path}: {message}")]
    SidecarInvalid { path: String, message: String },
    /// `<id>` is already installed from a different source.
    #[error("plugin {id:?} already installed from {existing_source}; remove it first")]
    IdConflict { id: String, existing_source: String },
    /// `PinnedHash::of_dir` refused the tree (symlink / non-regular / non-utf8).
    #[error("plugin content rejected: {0}")]
    ContentRejected(String),
    /// `remove`/target id has no installed directory.
    #[error("no plugin installed with id {0:?}")]
    NotInstalled(String),
    /// A filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

/// Build a closure mapping an `io::Error` to `PluginInstallError::Io` with context.
fn io(context: &'static str) -> impl Fn(std::io::Error) -> PluginInstallError {
    move |source| PluginInstallError::Io {
        context: context.to_string(),
        source,
    }
}

/// The `<plugins>/<id>.source.toml` provenance path (sibling of the plugin dir,
/// so writing it never perturbs the content hash).
#[allow(dead_code)] // see io() above
fn sidecar_path(plugins: &Path, id: &str) -> PathBuf {
    plugins.join(format!("{id}.source.toml"))
}

#[allow(dead_code)] // see io() above
fn read_sidecar(path: &Path) -> Result<SourceRecord, PluginInstallError> {
    let raw = std::fs::read_to_string(path).map_err(io("reading sidecar"))?;
    toml::from_str(&raw).map_err(|e| PluginInstallError::SidecarInvalid {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

#[allow(dead_code)] // see io() above
fn write_sidecar(path: &Path, rec: &SourceRecord) -> Result<(), PluginInstallError> {
    let body = toml::to_string(rec).map_err(|e| PluginInstallError::Io {
        context: "serializing sidecar".to_string(),
        source: std::io::Error::other(e),
    })?;
    std::fs::write(path, body).map_err(io("writing sidecar"))
}

/// Credentials callback: try ssh-agent for ssh URLs, else default. Public HTTPS
/// invokes no callback; private HTTPS falls through to a clear clone error.
// Only exercised via clone_at_ref's RemoteCallbacks until Task 3 wires up the
// `plugin add` command that actually calls clone_at_ref over the network.
#[allow(dead_code)] // until Task 3
fn credentials_cb(
    _url: &str,
    username: Option<&str>,
    allowed: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    if allowed.contains(git2::CredentialType::SSH_KEY) {
        git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
    } else {
        git2::Cred::default()
    }
}

/// Clone `url`, check out `git_ref` (or the remote default branch), strip `.git`,
/// and return `(resolved_ref, commit_id)`. Leaves a content-only tree at `into`.
#[allow(dead_code)] // until Task 3
fn clone_at_ref(
    url: &str,
    git_ref: Option<&str>,
    into: &Path,
) -> Result<(String, String), PluginInstallError> {
    let err = |source: git2::Error| PluginInstallError::Clone {
        url: url.to_string(),
        source,
    };

    let mut cb = git2::RemoteCallbacks::new();
    cb.credentials(credentials_cb);
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(cb);
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fo);
    let repo = builder.clone(url, into).map_err(err)?;

    // Resolve the requested ref (or HEAD) to a commit.
    let (object, resolved_ref) = match git_ref {
        Some(r) => (repo.revparse_single(r).map_err(err)?, r.to_string()),
        None => {
            let head = repo.head().map_err(err)?;
            let name = head.shorthand().unwrap_or("HEAD").to_string();
            (head.peel(git2::ObjectType::Any).map_err(err)?, name)
        }
    };
    let commit = object.peel_to_commit().map_err(err)?;
    let commit_id = commit.id().to_string();

    // Materialize that commit's tree and detach HEAD onto it.
    repo.checkout_tree(commit.as_object(), None).map_err(err)?;
    repo.set_head_detached(commit.id()).map_err(err)?;

    // Drop borrows of `repo` (git2 types have significant `Drop` impls, so NLL
    // won't shorten their scope for us) before dropping `repo` itself, then
    // remove `.git` (releases file handles on Windows too).
    drop(commit);
    drop(object);
    drop(repo);
    std::fs::remove_dir_all(into.join(".git")).map_err(io("stripping .git"))?;
    Ok((resolved_ref, commit_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = sidecar_path(tmp.path(), "bar");
        let rec = SourceRecord {
            source: "https://example.com/bar".to_string(),
            git_ref: "v1.2.0".to_string(),
            commit: "a".repeat(40),
            hash: format!("sha256:{}", "b".repeat(64)),
        };
        write_sidecar(&path, &rec).unwrap();
        assert_eq!(read_sidecar(&path).unwrap(), rec);
    }

    #[test]
    fn sidecar_rejects_unknown_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = sidecar_path(tmp.path(), "bar");
        std::fs::write(
            &path,
            "source=\"u\"\nref=\"r\"\ncommit=\"c\"\nhash=\"h\"\nbogus=\"x\"\n",
        )
        .unwrap();
        assert!(read_sidecar(&path).is_err());
    }

    /// Create a git repo at `dir` with a manifest and one data file; return HEAD id.
    /// `Signature::now` reads the wall clock — fine in tests (not in workflow scripts).
    fn init_fixture_repo(dir: &Path, manifest_id: &str, contents: &str) -> String {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "id=\"{manifest_id}\"\nname=\"X\"\nversion=\"0.1.0\"\n[engine]\ncommand=\"bin\"\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("data.txt"), contents).unwrap();
        commit_all(&repo, "init")
    }

    /// Add/replace a file in an existing fixture repo and commit; return new HEAD id.
    // Reused by Tasks 3-4's tests; unused by Task 2's own test.
    #[allow(dead_code)] // until Task 3
    fn commit_file(dir: &Path, name: &str, contents: &str) -> String {
        let repo = git2::Repository::open(dir).unwrap();
        std::fs::write(dir.join(name), contents).unwrap();
        commit_all(&repo, "update")
    }

    fn commit_all(repo: &git2::Repository, msg: &str) -> String {
        let mut idx = repo.index().unwrap();
        idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        idx.write().unwrap();
        let tree_id = idx.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("t", "t@e").unwrap();
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .into_iter()
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parent_refs)
            .unwrap()
            .to_string()
    }

    #[test]
    fn clone_at_ref_strips_git_and_resolves_head() {
        let src = tempfile::tempdir().unwrap();
        let head = init_fixture_repo(src.path(), "p", "hello");

        let dest = tempfile::tempdir().unwrap();
        let into = dest.path().join("out");
        let (git_ref, commit) = clone_at_ref(src.path().to_str().unwrap(), None, &into).unwrap();

        assert_eq!(commit, head);
        assert!(!git_ref.is_empty()); // default branch name (master/main)
        assert!(into.join("manifest.toml").exists());
        assert!(into.join("data.txt").exists());
        assert!(!into.join(".git").exists(), ".git must be stripped");
    }
}
