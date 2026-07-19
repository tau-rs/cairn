# Plugin install-from-git-URL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `cairn plugin add <git-url>` / `list` / `remove`, cloning a plugin into `<cairn>/.cairn/plugins/<id>/` routed through the existing default-deny trust gate and pinned-contents-on-drift model.

**Architecture:** A new `cairn-infra::plugin_install` module owns the git-clone → strip-`.git` → hash → provenance-sidecar mechanics behind three functions (`install`/`list`/`remove`) and a `thiserror` boundary type. The CLI gains a `plugin` subcommand that calls them *before* building the engine (pure fs/git, no reindex). `add` never writes `cairn.toml`; approval stays a manual paste, so re-adding at a new ref changes the content hash and trips the daemon's existing drift refusal, forcing re-approval for free.

**Tech Stack:** Rust, `git2` (clone/ls-remote), `sha2` via the existing `PinnedHash`, `toml`/`serde` for the sidecar, `clap` for the CLI, `thiserror` at the boundary.

## Global Constraints

- MSRV 1.88; edition from workspace.
- `unsafe_code = "forbid"` (workspace lint) — no `unsafe` anywhere.
- `thiserror` at the crate boundary (`PluginInstallError`); errors surfaced to the CLI as `String` via `to_string()`, matching existing arms.
- No new external dependencies: `git2`, `sha2`, `toml`, `serde`, `thiserror` are all workspace deps already. `thiserror` must be added to `cairn-infra/Cargo.toml` (it is transitively present, so `Cargo.lock` should not change; run `git add Cargo.lock` if it does).
- Merge queue enabled: branch off `main` → PR → `gh pr merge --auto --squash`. No manual rebase / local-merge. (Work is already on branch `plugin-add-from-git-url`.)
- Definition of done: `cargo test --workspace` + `cargo clippy --workspace --all-targets --locked` + `cargo fmt --check` all green.
- Provenance sidecar is **advisory only** — never a trust input. The daemon keeps comparing live content against the user-edited `cairn.toml` pin.

---

### Task 1: Module scaffold + provenance sidecar type

**Files:**
- Create: `crates/cairn-infra/src/plugin_install.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (add `pub mod plugin_install;`)
- Modify: `crates/cairn-infra/Cargo.toml` (add `thiserror`)
- Test: inline `#[cfg(test)] mod tests` in `plugin_install.rs`

**Interfaces:**
- Produces: `pub struct SourceRecord { pub source: String, pub git_ref: String (toml key "ref"), pub commit: String, pub hash: String }`; `pub enum PluginInstallError`; module-private `read_sidecar(&Path) -> Result<SourceRecord, PluginInstallError>`, `write_sidecar(&Path, &SourceRecord) -> Result<(), PluginInstallError>`, `sidecar_path(plugins: &Path, id: &str) -> PathBuf`, and the `io(context)` error-mapper helper.

- [ ] **Step 1: Add `thiserror` to `cairn-infra/Cargo.toml`**

In `crates/cairn-infra/Cargo.toml`, under `[dependencies]`, add after the `sha2` line:

```toml
thiserror = { workspace = true }
```

- [ ] **Step 2: Create the module file with the error type, sidecar type, and helpers**

Create `crates/cairn-infra/src/plugin_install.rs`:

```rust
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
}
```

- [ ] **Step 3: Wire the module into the crate**

In `crates/cairn-infra/src/lib.rs`, add alongside the other `pub mod` lines (after `pub mod notify_watcher;`):

```rust
pub mod plugin_install;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_install`
Expected: PASS (`sidecar_roundtrips`, `sidecar_rejects_unknown_field`).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs crates/cairn-infra/src/lib.rs crates/cairn-infra/Cargo.toml Cargo.lock
git commit -m "feat(plugin): sidecar provenance type + install error boundary"
```

---

### Task 2: `clone_at_ref` — clone, checkout ref, strip `.git`

**Files:**
- Modify: `crates/cairn-infra/src/plugin_install.rs`
- Test: inline `tests` module (same file)

**Interfaces:**
- Consumes: `PluginInstallError` (Task 1).
- Produces: module-private `clone_at_ref(url: &str, git_ref: Option<&str>, into: &Path) -> Result<(String, String), PluginInstallError>` returning `(resolved_ref, commit_id)` and leaving a **content-only** tree (no `.git`) at `into`. Also the shared `credentials_cb` closure builder. Test helper `init_fixture_repo(dir, manifest_id, contents) -> String` (returns HEAD commit id) and `commit_file(dir, name, contents) -> String`, reused by Tasks 3–4.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `plugin_install.rs`:

```rust
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
    let (git_ref, commit) =
        clone_at_ref(src.path().to_str().unwrap(), None, &into).unwrap();

    assert_eq!(commit, head);
    assert!(!git_ref.is_empty()); // default branch name (master/main)
    assert!(into.join("manifest.toml").exists());
    assert!(into.join("data.txt").exists());
    assert!(!into.join(".git").exists(), ".git must be stripped");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra clone_at_ref_strips_git_and_resolves_head`
Expected: FAIL to compile — `clone_at_ref` not defined.

- [ ] **Step 3: Implement `clone_at_ref`**

Add to `plugin_install.rs` (module body, above the `tests` module):

```rust
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

    // Drop the repo handle before removing `.git` (releases file handles on Windows).
    drop(repo);
    std::fs::remove_dir_all(into.join(".git")).map_err(io("stripping .git"))?;
    Ok((resolved_ref, commit_id))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-infra clone_at_ref_strips_git_and_resolves_head`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs
git commit -m "feat(plugin): clone at ref + strip .git into content-only tree"
```

---

### Task 3: `install` — fresh install happy path

**Files:**
- Modify: `crates/cairn-infra/src/plugin_install.rs`
- Test: inline `tests` module

**Interfaces:**
- Consumes: `clone_at_ref`, `read_sidecar`/`write_sidecar`/`sidecar_path`, `PinnedHash::of_dir`, `Manifest`, the `io` mapper (Tasks 1–2).
- Produces: `pub struct Installed { pub id: String, pub git_ref: String, pub commit: String, pub hash: String, pub updated: bool }` and `pub fn install(cairn_root: &Path, url: &str, git_ref: Option<&str>) -> Result<Installed, PluginInstallError>`. Module-private `plugins_dir(cairn_root) -> PathBuf` and `CleanupDir` staging guard.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
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
    let rec = read_sidecar(&sidecar_path(&dest.parent().unwrap(), "notes-linter")).unwrap();
    assert_eq!(rec.source, src.path().to_str().unwrap());
    assert_eq!(rec.commit, head);
    assert_eq!(rec.hash, installed.hash);
    assert_eq!(rec.hash, PinnedHash::of_dir(&dest).unwrap().to_string());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra install_is_named_from_manifest_and_writes_sidecar`
Expected: FAIL to compile — `install` not defined.

- [ ] **Step 3: Implement `install`, `Installed`, `plugins_dir`, and the staging guard**

Add to `plugin_install.rs` (module body):

```rust
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-infra install_is_named_from_manifest_and_writes_sidecar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs
git commit -m "feat(plugin): install() fresh-install happy path"
```

---

### Task 4: `install` — collision, same-source update, rollback

**Files:**
- Modify: `crates/cairn-infra/src/plugin_install.rs`
- Test: inline `tests` module

**Interfaces:**
- Consumes: `install`, `init_fixture_repo`, `commit_file` (Tasks 2–3). No new public API — this task only adds tests that exercise `install`'s existing branches; add code only if a test exposes a gap.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run tests to verify status**

Run: `cargo test -p cairn-infra install`
Expected: all four new tests PASS (they exercise branches implemented in Task 3). If any fails, fix `install` minimally so its behavior matches the test, then re-run.

Note on `symlink_in_repo_is_rejected_and_rolls_back`: `commit_file` re-adds all files including the symlink, and `git checkout` recreates it in the working tree, so `PinnedHash::of_dir` refuses it. The `CleanupDir` guard removes `.incoming` on the early return.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs
git commit -m "test(plugin): install collision, update, and rollback paths"
```

---

### Task 5: `remove`

**Files:**
- Modify: `crates/cairn-infra/src/plugin_install.rs`
- Test: inline `tests` module

**Interfaces:**
- Consumes: `plugins_dir`, `sidecar_path`, `install` (Tasks 1–3).
- Produces: `pub fn remove(cairn_root: &Path, id: &str) -> Result<(), PluginInstallError>`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra remove_deletes_dir_and_sidecar`
Expected: FAIL to compile — `remove` not defined.

- [ ] **Step 3: Implement `remove`**

Add to `plugin_install.rs` (module body):

```rust
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-infra remove_deletes_dir_and_sidecar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs
git commit -m "feat(plugin): remove() deletes plugin dir + sidecar"
```

---

### Task 6: `list`

**Files:**
- Modify: `crates/cairn-infra/src/plugin_install.rs`
- Test: inline `tests` module

**Interfaces:**
- Consumes: `plugins_dir`, `read_sidecar`, `credentials_cb`, `install` (Tasks 1–3).
- Produces: `pub enum UpdateStatus { Skipped, UpToDate, Available(String), Unreachable }`; `pub struct InstalledInfo { pub id: String, pub source: String, pub pinned_ref: String, pub pinned_commit: String, pub trusted: bool, pub update: UpdateStatus }`; `pub fn list(cairn_root: &Path, fetch: bool) -> Result<Vec<InstalledInfo>, PluginInstallError>`. Module-private `read_trusted_ids(cairn_root) -> HashSet<String>` and `remote_commit(url, git_ref) -> Result<String, git2::Error>`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra list_reports_installed_and_trusted_offline`
Expected: FAIL to compile — `list`/`InstalledInfo`/`UpdateStatus` not defined.

- [ ] **Step 3: Implement `list` and helpers**

Add `use std::collections::HashSet;` to the top of `plugin_install.rs` (with the other `use` lines), then add to the module body:

```rust
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
    let Ok(value) = raw.parse::<toml::Value>() else {
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
        let rec = read_sidecar(&path)?;
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-infra list_reports_installed_and_trusted_offline`
Expected: PASS.

- [ ] **Step 5: Run clippy + fmt on the crate**

Run: `cargo clippy -p cairn-infra --all-targets --locked` then `cargo fmt --check`
Expected: no warnings, no diff.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-infra/src/plugin_install.rs
git commit -m "feat(plugin): list() with trust peek + best-effort update check"
```

---

### Task 7: CLI `plugin` subcommand

**Files:**
- Modify: `crates/cairn-cli/src/main.rs`
- Test: create `crates/cairn-cli/tests/plugin.rs`

**Interfaces:**
- Consumes: `cairn_infra::plugin_install::{install, list, remove, Installed, InstalledInfo, UpdateStatus}` (Tasks 3, 5, 6).
- Produces: `Command::Plugin { action: PluginAction }` clap arm; `PluginAction { Add { url, r#ref }, List { offline }, Remove { id } }`; module-private `run_plugin(root: &Path, action: &PluginAction) -> Result<(), String>`.

- [ ] **Step 1: Write the failing test**

Create `crates/cairn-cli/tests/plugin.rs`:

```rust
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

    assert!(cairn.path().join(".cairn/plugins/demo/manifest.toml").exists());

    // list (offline) shows it as untrusted; remove reminds about cairn.toml.
    Command::cargo_bin("cairn")
        .unwrap()
        .args(["--cairn", cairn.path().to_str().unwrap(), "plugin", "list", "--offline"])
        .assert()
        .success()
        .stdout(contains("demo"))
        .stdout(contains("no"));

    Command::cargo_bin("cairn")
        .unwrap()
        .args(["--cairn", cairn.path().to_str().unwrap(), "plugin", "remove", "demo"])
        .assert()
        .success()
        .stdout(contains("revoke trust"));
}
```

Add `git2 = { workspace = true }` to `crates/cairn-cli/Cargo.toml` under `[dev-dependencies]` (needed only by this test to build fixtures).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-cli --test plugin`
Expected: FAIL to compile — no `plugin` subcommand.

- [ ] **Step 3: Add the clap arm and `PluginAction`**

In `crates/cairn-cli/src/main.rs`, add a variant at the end of `enum Command` (after `Restore { .. }`):

```rust
    /// Manage plugins installed from a git URL.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
```

Add this enum immediately after the `enum Command { .. }` block:

```rust
#[derive(Subcommand)]
enum PluginAction {
    /// Install a plugin from a git URL. It lands UNTRUSTED — approve it in
    /// cairn.toml before it will run.
    Add {
        /// Git URL (https or ssh).
        url: String,
        /// Branch, tag, or commit to check out (default: the remote's HEAD).
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// List installed plugins with trust and update status.
    List {
        /// Skip the network check for available updates.
        #[arg(long)]
        offline: bool,
    },
    /// Remove an installed plugin's files (does not revoke trust).
    Remove {
        /// Plugin id (its directory name under .cairn/plugins).
        id: String,
    },
}
```

- [ ] **Step 4: Short-circuit plugin commands before `build_engine` and add `run_plugin`**

In `run()`, immediately after the `ensure_cairn(&root)` block and before `let mut engine = build_engine(&root)...`, insert:

```rust
    // Plugin management is pure fs/git — it needs the cairn root but neither the
    // engine nor the startup reindex.
    if let Command::Plugin { action } = &cli.command {
        return run_plugin(&root, action);
    }
```

Add `run_plugin` and a small printer as free functions (e.g. after `fn run()`):

```rust
fn run_plugin(root: &Path, action: &PluginAction) -> Result<(), String> {
    use cairn_infra::plugin_install;
    match action {
        PluginAction::Add { url, r#ref } => {
            let installed = plugin_install::install(root, url, r#ref.as_deref())
                .map_err(|e| e.to_string())?;
            let verb = if installed.updated { "updated" } else { "cloned" };
            let short = installed.commit.get(..7).unwrap_or(&installed.commit);
            println!(
                "{verb} {} @ {} ({short}) -> .cairn/plugins/{}/",
                installed.id, installed.git_ref, installed.id
            );
            println!("UNTRUSTED - will not run until you approve. Add to cairn.toml:\n");
            println!("  [[plugins.trusted]]");
            println!("  dir  = \"{}\"", installed.id);
            println!("  hash = \"{}\"", installed.hash);
        }
        PluginAction::List { offline } => {
            let infos = plugin_install::list(root, !offline).map_err(|e| e.to_string())?;
            print_plugin_list(&infos);
        }
        PluginAction::Remove { id } => {
            plugin_install::remove(root, id).map_err(|e| e.to_string())?;
            println!("removed .cairn/plugins/{id}/ and {id}.source.toml");
            println!(
                "note: if \"{id}\" is still in cairn.toml [plugins].trusted, \
                 remove that line to revoke trust."
            );
        }
    }
    Ok(())
}

fn print_plugin_list(infos: &[cairn_infra::plugin_install::InstalledInfo]) {
    use cairn_infra::plugin_install::UpdateStatus;
    if infos.is_empty() {
        println!("no plugins installed");
        return;
    }
    println!("ID\tSOURCE\tPINNED\tTRUSTED\tUPDATE");
    for i in infos {
        let update = match &i.update {
            UpdateStatus::Skipped => "-".to_string(),
            UpdateStatus::UpToDate => "up to date".to_string(),
            UpdateStatus::Available(c) => format!("{} available", c.get(..7).unwrap_or(c)),
            UpdateStatus::Unreachable => "unreachable".to_string(),
        };
        let trusted = if i.trusted { "yes" } else { "no" };
        println!("{}\t{}\t{}\t{}\t{}", i.id, i.source, i.pinned_ref, trusted, update);
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p cairn-cli --test plugin`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-cli/src/main.rs crates/cairn-cli/tests/plugin.rs crates/cairn-cli/Cargo.toml Cargo.lock
git commit -m "feat(cli): plugin add/list/remove subcommands"
```

---

### Task 8: Integration — install → untrusted → approve → spawn, and drift forces re-approval

**Files:**
- Create: `crates/cairn-plugin-example/tests/install.rs`
- Test: this file (spawns the real example binary via the existing host harness)

**Interfaces:**
- Consumes: `cairn_infra::plugin_install::install`, `cairn_infra::{ProcessPluginHost, TrustedPlugins}`, the example binary via `env!("CARGO_BIN_EXE_cairn-plugin-example")`, and the `PermissiveSandbox` pattern mirrored from `tests/host.rs`.

- [ ] **Step 1: Write the failing tests**

Create `crates/cairn-plugin-example/tests/install.rs`:

```rust
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
    let host = ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox)
        .unwrap();
    assert!(host.plugins().iter().any(|p| p == "example"));
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
    let host = ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox)
        .unwrap();
    assert!(host.plugins().is_empty(), "drifted plugin must not spawn on the old pin");

    // Re-pinning to the new hash restores spawn.
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(second.hash.clone()))]).unwrap();
    let host = ProcessPluginHost::load(&plugins_dir(cairn.path()), &trusted, &PermissiveSandbox)
        .unwrap();
    assert!(host.plugins().iter().any(|p| p == "example"));
}
```

Add `git2 = { workspace = true }` to `crates/cairn-plugin-example/Cargo.toml` under `[dev-dependencies]` (needed to build fixture repos).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test install`
Expected: FAIL to compile — `git2` dev-dep and/or the tests are new. After adding the dep it should compile; if `host.plugins()` returns something other than `Vec<String>`, adjust the `iter().any(|p| p == "example")` predicate to match its item type (check `tests/host.rs` for the exact shape).

- [ ] **Step 3: Confirm `plugins()` shape and finalize**

Open `crates/cairn-plugin-example/tests/host.rs` and confirm how existing tests read `host.plugins()` (item type and comparison). Match that usage exactly in the two new tests. No production code changes are expected — these tests exercise `install` (Task 3) and the existing host gate together.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-plugin-example --test install`
Expected: PASS (both tests). These are the DoD acceptance tests: install → untrusted → approve → spawn, and drift-on-re-add → refusal until re-pin.

- [ ] **Step 5: Full workspace gate**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets --locked
cargo fmt --check
```
Expected: all green. (The pre-existing flaky `invoke_times_out_and_kills_plugin` in this sandbox is unrelated — see project memory.)

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-plugin-example/tests/install.rs crates/cairn-plugin-example/Cargo.toml Cargo.lock
git commit -m "test(plugin): install->untrusted->approve->spawn + drift reapproval"
```

---

## Self-Review

**Spec coverage:**
- Sidecar provenance (source/ref/commit/hash), `deny_unknown_fields` → Task 1. ✔
- Clone over https/ssh, strip `.git`, resolve commit → Task 2. ✔
- id from manifest, sidecar written, never touches `cairn.toml` → Task 3. ✔
- Same-source update, id conflict hard-error, manifest-missing, symlink rollback → Task 4. ✔
- `remove` dir + sidecar, `NotInstalled` → Task 5. ✔
- `list` with trusted peek + best-effort update check + `--offline` → Task 6. ✔
- CLI `add`/`list`/`remove`, untrusted banner + paste snippet, before `build_engine` → Task 7. ✔
- install → untrusted → approve → spawn; drift forces re-approval (fixture wraps example binary) → Task 8. ✔
- DoD gate (`test`/`clippy --locked`/`fmt --check`) → Task 8 Step 5. ✔

**Placeholder scan:** No TBD/TODO; every code step has complete code and an exact command with expected result. Two tasks (4, 8) note a verify-and-adjust step against real behavior rather than a placeholder — Task 4 confirms the branches already implemented in Task 3, Task 8 matches `host.plugins()`'s exact item shape.

**Type consistency:** `install`/`list`/`remove` signatures, `Installed`, `InstalledInfo`, `UpdateStatus`, `SourceRecord` (with `#[serde(rename = "ref")] git_ref`), and `PluginInstallError` variants are used identically across Tasks 1, 3, 5, 6, 7, 8. The sidecar filename `<id>.source.toml` (via `sidecar_path`) is consistent in Tasks 1, 3, 5, 6. The staging dir `.incoming` under the plugins dir is consistent in Tasks 3 and 4's assertions.

## Out of scope (fast-follows, not in this plan)

- Token / credential-helper auth for private HTTPS.
- A distinct `update` verb or `--all` bulk update.
- Cairn writing/rewriting `cairn.toml` (an interactive `plugin trust` command).
- Incremental `git fetch` updates (kept as fresh re-clone).
