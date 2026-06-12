//! Daemon settings loaded from a per-cairn `cairn.toml`. Minimal but
//! extensible — add sections as the daemon grows.

use std::path::Path;

use serde::Deserialize;

/// Daemon configuration.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// CORS settings.
    #[serde(default)]
    pub cors: CorsConfig,
    /// On-disk index settings.
    #[serde(default)]
    pub index: IndexConfig,
    /// Plugin host settings.
    #[serde(default)]
    pub plugins: PluginsConfig,
}

/// Plugin host settings.
#[derive(Debug, Default, Deserialize)]
pub struct PluginsConfig {
    /// Per-message plugin read timeout, in seconds. Unset → the host default
    /// (`cairn_infra::DEFAULT_PLUGIN_TIMEOUT`, 30s). A configured `0` is invalid
    /// (it would kill every plugin immediately) and is ignored with a warning by
    /// the daemon.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Plugin directory names the user trusts to spawn. Absent/empty ⇒ no plugin
    /// is spawned (default-deny). The name must match the plugin's directory
    /// under `<cairn>/.cairn/plugins/`. Each entry may be a bare string
    /// (legacy/shorthand) or a `[[plugins.trusted]]` table with optional `hash`.
    #[serde(default)]
    pub trusted: Vec<TrustedEntry>,
}

/// One entry in `[plugins].trusted`. Parsed untagged so both the legacy bare
/// string form (`trusted = ["name"]`) and the table form
/// (`[[plugins.trusted]] dir = "name" hash = "sha256:..."`) are accepted. A
/// bare string and a table with `hash` omitted both mean "trusted, unpinned".
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TrustedEntry {
    /// Legacy / shorthand: trust by directory name, no pin.
    Name(String),
    /// Table form: directory name plus an optional pinned content hash.
    Pinned {
        dir: String,
        #[serde(default)]
        hash: Option<String>,
    },
}

impl TrustedEntry {
    /// Reduce to `(dir_name, optional_pin_string)` for `TrustedPlugins`.
    pub fn normalize(&self) -> (String, Option<String>) {
        match self {
            TrustedEntry::Name(dir) => (dir.clone(), None),
            TrustedEntry::Pinned { dir, hash } => (dir.clone(), hash.clone()),
        }
    }
}

/// On-disk index persistence settings.
#[derive(Debug, Deserialize)]
pub struct IndexConfig {
    /// Persist the index under `<cairn>/.cairn/index` (default true).
    #[serde(default = "default_true")]
    pub persist: bool,
    /// Override the index directory (defaults to `<cairn>/.cairn/index`).
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            persist: true,
            path: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// CORS allowlist configuration.
#[derive(Debug, Default, Deserialize)]
pub struct CorsConfig {
    /// Allowed browser origins, e.g. `http://localhost:5173`.
    #[serde(default)]
    pub origins: Vec<String>,
}

impl Config {
    /// Load TOML config from `path`.
    ///
    /// # Errors
    /// Returns an error string if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Config, String> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| format!("read config {}: {e}", path.display()))?;
        toml::from_str(&s).map_err(|e| format!("parse config {}: {e}", path.display()))
    }

    /// Load `<cairn>/cairn.toml` if it exists, else the default (empty) config.
    ///
    /// # Errors
    /// Returns an error string if the file exists but cannot be read/parsed.
    pub fn load_default(cairn: &Path) -> Result<Config, String> {
        let path = cairn.join("cairn.toml");
        if path.exists() {
            Self::load(&path)
        } else {
            Ok(Config::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cors_origins() {
        let c: Config = toml::from_str("[cors]\norigins = [\"http://localhost:5173\"]").unwrap();
        assert_eq!(c.cors.origins, vec!["http://localhost:5173".to_string()]);
    }

    #[test]
    fn empty_or_sectionless_is_empty() {
        assert!(toml::from_str::<Config>("")
            .unwrap()
            .cors
            .origins
            .is_empty());
        assert!(toml::from_str::<Config>("[cors]\n")
            .unwrap()
            .cors
            .origins
            .is_empty());
    }

    #[test]
    fn load_default_absent_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Config::load_default(tmp.path())
            .unwrap()
            .cors
            .origins
            .is_empty());
    }

    #[test]
    fn load_reads_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("cairn.toml");
        std::fs::write(&p, "[cors]\norigins = [\"http://x\"]").unwrap();
        assert_eq!(
            Config::load(&p).unwrap().cors.origins,
            vec!["http://x".to_string()]
        );
    }

    #[test]
    fn index_persist_defaults_true() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.index.persist);
        let c: Config = toml::from_str("[index]\n").unwrap();
        assert!(c.index.persist);
    }

    #[test]
    fn index_persist_can_be_disabled() {
        let c: Config = toml::from_str("[index]\npersist = false").unwrap();
        assert!(!c.index.persist);
    }

    #[test]
    fn plugins_timeout_parses() {
        let c: Config = toml::from_str("[plugins]\ntimeout_secs = 60").unwrap();
        assert_eq!(c.plugins.timeout_secs, Some(60));
        // 0 parses as Some(0) (the daemon then guards it); it isn't swallowed.
        let z: Config = toml::from_str("[plugins]\ntimeout_secs = 0").unwrap();
        assert_eq!(z.plugins.timeout_secs, Some(0));
    }

    #[test]
    fn plugins_trusted_legacy_strings_parse() {
        let c: Config = toml::from_str("[plugins]\ntrusted = [\"a\", \"b\"]").unwrap();
        let entries: Vec<_> = c.plugins.trusted.iter().map(|e| e.normalize()).collect();
        assert_eq!(
            entries,
            vec![("a".to_string(), None), ("b".to_string(), None)]
        );
    }

    #[test]
    fn plugins_trusted_table_with_hash_parses() {
        let pin = format!("sha256:{}", "a".repeat(64));
        let toml = format!("[[plugins.trusted]]\ndir = \"a\"\nhash = \"{pin}\"\n");
        let c: Config = toml::from_str(&toml).unwrap();
        assert_eq!(
            c.plugins.trusted[0].normalize(),
            ("a".to_string(), Some(pin))
        );
    }

    #[test]
    fn plugins_trusted_table_without_hash_parses() {
        let c: Config = toml::from_str("[[plugins.trusted]]\ndir = \"a\"\n").unwrap();
        assert_eq!(c.plugins.trusted[0].normalize(), ("a".to_string(), None));
    }

    #[test]
    fn plugins_trusted_defaults_empty() {
        assert!(toml::from_str::<Config>("")
            .unwrap()
            .plugins
            .trusted
            .is_empty());
        assert!(toml::from_str::<Config>("[plugins]\ntimeout_secs = 5")
            .unwrap()
            .plugins
            .trusted
            .is_empty());
    }

    #[test]
    fn plugins_timeout_defaults_none() {
        assert_eq!(
            toml::from_str::<Config>("").unwrap().plugins.timeout_secs,
            None
        );
        assert_eq!(
            toml::from_str::<Config>("[plugins]\n")
                .unwrap()
                .plugins
                .timeout_secs,
            None
        );
    }
}
