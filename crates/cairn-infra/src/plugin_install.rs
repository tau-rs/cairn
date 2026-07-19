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
// Only exercised by this module's tests until Task 2 wires up clone/install
// callers; remove this allow once a non-test caller lands.
#[allow(dead_code)]
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
