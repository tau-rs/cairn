//! Install a plugin from a git URL into `<cairn>/.cairn/plugins/<id>/`, routed
//! through the existing trust gate. A freshly added plugin lands UNTRUSTED: this
//! module never writes `cairn.toml`. Provenance (source URL, ref, resolved
//! commit, content hash) is recorded in a sibling `<id>.source.toml` file that
//! sits OUTSIDE the hashed tree and is advisory only — the daemon's authoritative
//! pin remains the user-edited `cairn.toml` value.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::PinnedHash;
use cairn_plugin_protocol::Manifest;

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
fn sidecar_path(plugins: &Path, id: &str) -> PathBuf {
    plugins.join(format!("{id}.source.toml"))
}

fn read_sidecar(path: &Path) -> Result<SourceRecord, PluginInstallError> {
    let raw = std::fs::read_to_string(path).map_err(io("reading sidecar"))?;
    toml::from_str(&raw).map_err(|e| PluginInstallError::SidecarInvalid {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

fn write_sidecar(path: &Path, rec: &SourceRecord) -> Result<(), PluginInstallError> {
    let body = toml::to_string(rec).map_err(|e| PluginInstallError::Io {
        context: "serializing sidecar".to_string(),
        source: std::io::Error::other(e),
    })?;
    std::fs::write(path, body).map_err(io("writing sidecar"))
}

/// Credentials callback: try ssh-agent for ssh URLs, else default. Public HTTPS
/// invokes no callback; private HTTPS falls through to a clear clone error.
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

/// `<cairn>/.cairn/plugins`.
fn plugins_dir(cairn_root: &Path) -> PathBuf {
    cairn_root.join(".cairn").join("plugins")
}

/// Removes a staging directory on drop, so an early `?` return leaves no
/// half-written tree. After a successful rename the path is gone → drop is a
/// harmless no-op (removing a missing path is ignored).
struct CleanupDir<'a>(&'a Path);
impl Drop for CleanupDir<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

/// The result of a successful [`install`].
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub id: String,
    pub git_ref: String,
    pub commit: String,
    pub hash: String,
    /// `true` if this replaced an existing same-source install (update).
    pub updated: bool,
}

/// Clone `url` at `git_ref` (default: remote HEAD) into
/// `<cairn>/.cairn/plugins/<id>/`, where `<id>` is the manifest's `id`. Writes a
/// provenance sidecar; never touches `cairn.toml` (the plugin lands UNTRUSTED).
///
/// # Errors
/// [`PluginInstallError`] on clone/checkout failure, a missing/invalid manifest,
/// an id collision with a different source, or a rejected content tree.
pub fn install(
    cairn_root: &Path,
    url: &str,
    git_ref: Option<&str>,
) -> Result<Installed, PluginInstallError> {
    let plugins = plugins_dir(cairn_root);
    std::fs::create_dir_all(&plugins).map_err(io("creating plugins dir"))?;

    // Stage inside the plugins dir so the final rename is same-filesystem.
    let incoming = plugins.join(".incoming");
    if incoming.exists() {
        std::fs::remove_dir_all(&incoming).map_err(io("clearing stale staging dir"))?;
    }
    let _guard = CleanupDir(&incoming);

    let (resolved_ref, commit) = clone_at_ref(url, git_ref, &incoming)?;

    // Learn the id from the manifest — it is the trust anchor and must equal the
    // directory name (the daemon rejects a mismatch), which holds by construction.
    let manifest_path = incoming.join("manifest.toml");
    if !manifest_path.exists() {
        return Err(PluginInstallError::ManifestMissing {
            url: url.to_string(),
        });
    }
    let raw = std::fs::read_to_string(&manifest_path).map_err(io("reading manifest"))?;
    let manifest: Manifest =
        toml::from_str(&raw).map_err(|e| PluginInstallError::ManifestInvalid(e.to_string()))?;
    let id = manifest.id.clone();

    let dest = plugins.join(&id);
    let sidecar = sidecar_path(&plugins, &id);

    // Collision: an existing dest must be a same-source update, else hard error.
    let updated = if dest.exists() {
        match read_sidecar(&sidecar) {
            Ok(rec) if rec.source == url => true,
            Ok(rec) => {
                return Err(PluginInstallError::IdConflict {
                    id,
                    existing_source: rec.source,
                })
            }
            Err(_) => {
                return Err(PluginInstallError::IdConflict {
                    id,
                    existing_source: "an unknown source".to_string(),
                })
            }
        }
    } else {
        false
    };

    // Hash the staged content (refuses symlink / non-regular / non-utf8).
    let hash = PinnedHash::of_dir(&incoming)
        .map_err(|e| PluginInstallError::ContentRejected(e.to_string()))?
        .to_string();

    // Commit the staged tree into place.
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(io("removing previous plugin dir"))?;
    }
    std::fs::rename(&incoming, &dest).map_err(io("moving plugin into place"))?;

    write_sidecar(
        &sidecar,
        &SourceRecord {
            source: url.to_string(),
            git_ref: resolved_ref.clone(),
            commit: commit.clone(),
            hash: hash.clone(),
        },
    )?;

    Ok(Installed {
        id,
        git_ref: resolved_ref,
        commit,
        hash,
        updated,
    })
}

/// Delete an installed plugin's directory and its provenance sidecar. Cannot
/// revoke trust — that lives in the user-owned `cairn.toml` (the CLI reminds).
///
/// # Errors
/// [`PluginInstallError::NotInstalled`] if no directory exists for `id`; `Io` on
/// a filesystem failure.
pub fn remove(cairn_root: &Path, id: &str) -> Result<(), PluginInstallError> {
    let plugins = plugins_dir(cairn_root);
    let dest = plugins.join(id);
    if !dest.exists() {
        return Err(PluginInstallError::NotInstalled(id.to_string()));
    }
    std::fs::remove_dir_all(&dest).map_err(io("removing plugin dir"))?;
    let sidecar = sidecar_path(&plugins, id);
    if sidecar.exists() {
        std::fs::remove_file(&sidecar).map_err(io("removing sidecar"))?;
    }
    Ok(())
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

    #[test]
    fn install_is_named_from_manifest_and_writes_sidecar() {
        let src = tempfile::tempdir().unwrap();
        let head = init_fixture_repo(src.path(), "notes-linter", "hello");
        let cairn = tempfile::tempdir().unwrap();

        let installed = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();

        assert_eq!(installed.id, "notes-linter"); // id from manifest, not the URL
        assert_eq!(installed.commit, head);
        assert!(!installed.updated);

        let dest = cairn.path().join(".cairn/plugins/notes-linter");
        assert!(dest.join("manifest.toml").exists());
        assert!(!dest.join(".git").exists());

        // Sidecar records provenance and the hash the daemon will pin against.
        let rec = read_sidecar(&sidecar_path(dest.parent().unwrap(), "notes-linter")).unwrap();
        assert_eq!(rec.source, src.path().to_str().unwrap());
        assert_eq!(rec.commit, head);
        assert_eq!(rec.hash, installed.hash);
        assert_eq!(rec.hash, PinnedHash::of_dir(&dest).unwrap().to_string());
    }

    #[test]
    fn readd_same_source_is_update_with_new_hash() {
        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), "p", "v1");
        let cairn = tempfile::tempdir().unwrap();
        let url = src.path().to_str().unwrap();

        let first = install(cairn.path(), url, None).unwrap();
        // Change the plugin content and re-add from the same source.
        commit_file(src.path(), "data.txt", "v2");
        let second = install(cairn.path(), url, None).unwrap();

        assert!(!first.updated);
        assert!(second.updated);
        assert_ne!(first.hash, second.hash); // drift the daemon will refuse until re-pinned
    }

    #[test]
    fn readd_different_source_same_id_errors() {
        let a = tempfile::tempdir().unwrap();
        init_fixture_repo(a.path(), "p", "a");
        let b = tempfile::tempdir().unwrap();
        init_fixture_repo(b.path(), "p", "b"); // same manifest id, different repo
        let cairn = tempfile::tempdir().unwrap();

        install(cairn.path(), a.path().to_str().unwrap(), None).unwrap();
        let err = install(cairn.path(), b.path().to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, PluginInstallError::IdConflict { .. }));
    }

    #[test]
    fn manifest_missing_errors_and_leaves_no_staging() {
        let src = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(src.path()).unwrap();
        std::fs::write(src.path().join("data.txt"), "x").unwrap();
        commit_all(&repo, "no manifest");
        let cairn = tempfile::tempdir().unwrap();

        let err = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, PluginInstallError::ManifestMissing { .. }));
        assert!(!cairn.path().join(".cairn/plugins/.incoming").exists());
    }

    #[test]
    fn remove_deletes_dir_and_sidecar() {
        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), "p", "x");
        let cairn = tempfile::tempdir().unwrap();
        install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();

        remove(cairn.path(), "p").unwrap();
        assert!(!cairn.path().join(".cairn/plugins/p").exists());
        assert!(!cairn.path().join(".cairn/plugins/p.source.toml").exists());

        assert!(matches!(
            remove(cairn.path(), "p").unwrap_err(),
            PluginInstallError::NotInstalled(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_repo_is_rejected_and_rolls_back() {
        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), "p", "x");
        std::os::unix::fs::symlink("data.txt", src.path().join("link.txt")).unwrap();
        commit_file(src.path(), ".gitkeep", ""); // commit including the symlink
        let cairn = tempfile::tempdir().unwrap();

        let err = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, PluginInstallError::ContentRejected(_)));
        assert!(!cairn.path().join(".cairn/plugins/p").exists());
        assert!(!cairn.path().join(".cairn/plugins/.incoming").exists());
    }
}
