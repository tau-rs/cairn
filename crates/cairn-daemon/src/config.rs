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
}
