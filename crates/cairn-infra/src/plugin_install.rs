//! Install a plugin from a git URL into `<cairn>/.cairn/plugins/<id>/`, routed
//! through the existing trust gate. A freshly added plugin lands UNTRUSTED: this
//! module never writes `cairn.toml`. Provenance (source URL, ref, resolved
//! commit, content hash) is recorded in a sibling `<id>.source.toml` file that
//! sits OUTSIDE the hashed tree and is advisory only — the daemon's authoritative
//! pin remains the user-edited `cairn.toml` value.

use std::collections::HashSet;
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
    /// The plugin's manifest `id` is not a single safe path component (empty,
    /// `.`/`..`, contains a path separator, or absolute) — refused to prevent
    /// writing outside `.cairn/plugins/`.
    #[error("plugin id {0:?} is not a valid single path component")]
    InvalidId(String),
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

/// Reject any plugin id that is not exactly one normal path component, so it
/// cannot escape `.cairn/plugins/` via `..`, an absolute path, or a separator.
fn validate_id(id: &str) -> Result<(), PluginInstallError> {
    let mut comps = Path::new(id).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(c)), None) if c == id => Ok(()),
        _ => Err(PluginInstallError::InvalidId(id.to_string())),
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
    validate_id(&id)?;

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
    validate_id(id)?;
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

/// Whether the remote's copy of the pinned ref moved past the recorded commit.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    /// Update check skipped (`fetch == false`).
    Skipped,
    /// Remote ref matches the recorded commit.
    UpToDate,
    /// Remote ref points at a different commit (the new commit id).
    Available(String),
    /// The remote could not be reached (never fatal).
    Unreachable,
}

/// One row of `cairn plugin list`.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledInfo {
    pub id: String,
    pub source: String,
    pub pinned_ref: String,
    pub pinned_commit: String,
    /// Whether `cairn.toml [plugins].trusted` lists this id (advisory peek).
    pub trusted: bool,
    pub update: UpdateStatus,
}

/// Best-effort read of the trusted directory names from `cairn.toml`. Lenient by
/// design — this only powers the advisory TRUSTED column, never a trust decision.
fn read_trusted_ids(cairn_root: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let Ok(raw) = std::fs::read_to_string(cairn_root.join("cairn.toml")) else {
        return set;
    };
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return set;
    };
    let trusted = value
        .get("plugins")
        .and_then(|p| p.get("trusted"))
        .and_then(|t| t.as_array());
    if let Some(entries) = trusted {
        for entry in entries {
            if let Some(name) = entry.as_str() {
                set.insert(name.to_string());
            } else if let Some(dir) = entry.get("dir").and_then(|d| d.as_str()) {
                set.insert(dir.to_string());
            }
        }
    }
    set
}

/// The commit id the remote currently advertises for `git_ref` (branch or tag).
fn remote_commit(url: &str, git_ref: &str) -> Result<String, git2::Error> {
    let mut remote = git2::Remote::create_detached(url)?;
    let mut cb = git2::RemoteCallbacks::new();
    cb.credentials(credentials_cb);
    remote.connect_auth(git2::Direction::Fetch, Some(cb), None)?;
    let branch = format!("refs/heads/{git_ref}");
    let tag = format!("refs/tags/{git_ref}");
    let list = remote.list()?;
    for head in list {
        if head.name() == branch || head.name() == tag {
            return Ok(head.oid().to_string());
        }
    }
    Err(git2::Error::from_str("ref not advertised by remote"))
}

/// List installed plugins with trust and (optionally) update status. Reads the
/// `<id>.source.toml` sidecars; with `fetch`, queries each remote best-effort.
///
/// # Errors
/// [`PluginInstallError`] only on a local IO or sidecar-parse failure; network
/// failures surface per-row as [`UpdateStatus::Unreachable`], never an error.
pub fn list(cairn_root: &Path, fetch: bool) -> Result<Vec<InstalledInfo>, PluginInstallError> {
    let plugins = plugins_dir(cairn_root);
    let trusted = read_trusted_ids(cairn_root);
    let mut out = Vec::new();
    if !plugins.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&plugins).map_err(io("reading plugins dir"))? {
        let entry = entry.map_err(io("reading plugins dir entry"))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".source.toml") else {
            continue;
        };
        // `list` is best-effort: one corrupt/partially-written sidecar must not
        // abort the whole listing, so skip it and keep going.
        let Ok(rec) = read_sidecar(&path) else {
            continue;
        };
        let update = if !fetch {
            UpdateStatus::Skipped
        } else {
            match remote_commit(&rec.source, &rec.git_ref) {
                Ok(remote) if remote == rec.commit => UpdateStatus::UpToDate,
                Ok(remote) => UpdateStatus::Available(remote),
                Err(_) => UpdateStatus::Unreachable,
            }
        };
        out.push(InstalledInfo {
            id: id.to_string(),
            source: rec.source,
            pinned_ref: rec.git_ref,
            pinned_commit: rec.commit,
            trusted: trusted.contains(id),
            update,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
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

    #[test]
    fn install_rejects_traversal_id() {
        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), "../escape", "x");
        let cairn = tempfile::tempdir().unwrap();

        let err = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, PluginInstallError::InvalidId(_)));

        // The escape target (a sibling of `.cairn/`) must never be created.
        let escape_target = cairn.path().join("escape");
        assert!(!escape_target.exists());
        // Nor should anything have been committed under plugins/.
        assert!(!cairn.path().join(".cairn/plugins/../escape").exists());
    }

    #[test]
    fn install_rejects_absolute_id() {
        let evil = std::env::temp_dir().join("cairn-evil-abs");
        let _ = std::fs::remove_dir_all(&evil); // in case a previous failed run left it
        let abs_id = evil.to_str().unwrap().to_string();

        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), &abs_id, "x");
        let cairn = tempfile::tempdir().unwrap();

        let err = install(cairn.path(), src.path().to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, PluginInstallError::InvalidId(_)));
        assert!(!evil.exists(), "absolute id must not be created on disk");
    }

    #[test]
    fn remove_rejects_traversal_id() {
        let cairn = tempfile::tempdir().unwrap();
        let err = remove(cairn.path(), "../../foo").unwrap_err();
        assert!(matches!(err, PluginInstallError::InvalidId(_)));
        assert!(!cairn.path().join("../foo").exists());
    }

    #[test]
    fn list_skips_malformed_sidecar() {
        let src = tempfile::tempdir().unwrap();
        init_fixture_repo(src.path(), "good", "x");
        let cairn = tempfile::tempdir().unwrap();
        install(cairn.path(), src.path().to_str().unwrap(), None).unwrap();

        std::fs::write(
            cairn.path().join(".cairn/plugins/bad.source.toml"),
            "not valid toml {{{",
        )
        .unwrap();

        let infos = list(cairn.path(), false).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "good");
    }

    #[test]
    fn list_reports_installed_and_trusted_offline() {
        let a = tempfile::tempdir().unwrap();
        init_fixture_repo(a.path(), "alpha", "x");
        let b = tempfile::tempdir().unwrap();
        init_fixture_repo(b.path(), "beta", "y");
        let cairn = tempfile::tempdir().unwrap();
        install(cairn.path(), a.path().to_str().unwrap(), None).unwrap();
        install(cairn.path(), b.path().to_str().unwrap(), None).unwrap();

        // Trust only "beta" via cairn.toml (read-only peek — list must never write it).
        std::fs::write(
            cairn.path().join("cairn.toml"),
            "[plugins]\ntrusted = [\"beta\"]\n",
        )
        .unwrap();

        let infos = list(cairn.path(), false).unwrap(); // offline
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].id, "alpha"); // sorted by id
        assert_eq!(infos[1].id, "beta");
        assert!(!infos[0].trusted);
        assert!(infos[1].trusted);
        assert!(matches!(infos[0].update, UpdateStatus::Skipped));
    }
}
