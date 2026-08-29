//! Session-sealing settings, read from a per-cairn `cairn.toml`.
//!
//! The seal *mechanism* — `SealTimer` / `run_seal_loop` — lives in
//! `cairn-service` and takes plain [`Duration`](std::time::Duration)s, so any
//! transport can drive it. The *numbers* it is driven with live here, in the
//! adapter layer that already owns on-disk and environment configuration, so
//! every transport reads one schema from one file. They deliberately do **not**
//! live in `cairn-daemon`: that is a leaf binary crate pulling the whole HTTP
//! stack, so a non-daemon shell (the Tauri desktop app) could only reach them by
//! depending on the daemon or by re-declaring the schema and letting it drift.

use std::path::Path;

use serde::Deserialize;

/// Settings for sealing editing sessions into commits (any edit source).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    /// Auto-commit sealed editing sessions. Default `true`.
    #[serde(default = "default_true_sync")]
    pub auto_commit: bool,
    /// Idle seconds with no further change before a session seals. Default 2.
    #[serde(default)]
    pub idle_seconds: Option<u64>,
    /// Deprecated alias for the idle window, in ms. `idle_seconds` wins.
    #[serde(default)]
    pub quiet_period_ms: Option<u64>,
    /// Backstop: a never-idle session seals after this many minutes. Default 20.
    #[serde(default = "default_backstop_minutes")]
    pub backstop_minutes: u64,
    /// Grace (ms) to wait and re-check before honoring a watcher `Removed`,
    /// absorbing the transient gap of a non-atomic / tmp-rename write. Default 50.
    #[serde(default = "default_confirm_grace_ms")]
    pub confirm_grace_ms: u64,
}

/// The `[sync]` section of a `cairn.toml`, parsed on its own.
///
/// Unknown *sections* are ignored (no `deny_unknown_fields` here) so a caller
/// that owns only sealing can read the same file the daemon does, without
/// having to know about `[cors]`, `[index]` or `[plugins]`. The `[sync]` table
/// itself is still strict — a typo'd key is an error, not a silent default.
#[derive(Debug, Default, Deserialize)]
struct SyncSection {
    #[serde(default)]
    sync: SyncConfig,
}

impl SyncConfig {
    /// The idle window: `idle_seconds` → `quiet_period_ms` (deprecated) → 2 s.
    #[must_use]
    pub fn idle(&self) -> std::time::Duration {
        if let Some(s) = self.idle_seconds {
            return std::time::Duration::from_secs(s);
        }
        if let Some(ms) = self.quiet_period_ms {
            return std::time::Duration::from_millis(ms);
        }
        std::time::Duration::from_secs(2)
    }

    /// The long-session backstop.
    #[must_use]
    pub fn backstop(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.backstop_minutes * 60)
    }

    /// Read the `[sync]` section of `<cairn>/cairn.toml`. Absent file ⇒ defaults.
    ///
    /// # Errors
    /// Returns an error string if the file exists but cannot be read or parsed.
    pub fn load(cairn: &Path) -> Result<Self, String> {
        let path = cairn.join("cairn.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(&path)
            .map_err(|e| format!("read config {}: {e}", path.display()))?;
        let parsed: SyncSection =
            toml::from_str(&s).map_err(|e| format!("parse config {}: {e}", path.display()))?;
        Ok(parsed.sync)
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            auto_commit: true,
            idle_seconds: None,
            quiet_period_ms: None,
            backstop_minutes: 20,
            confirm_grace_ms: 50,
        }
    }
}

fn default_true_sync() -> bool {
    true
}

fn default_backstop_minutes() -> u64 {
    20
}

fn default_confirm_grace_ms() -> u64 {
    50
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a whole `cairn.toml` body the way [`SyncConfig::load`] does.
    fn sync_from(body: &str) -> Result<SyncConfig, String> {
        toml::from_str::<SyncSection>(body)
            .map(|s| s.sync)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn defaults_on_with_idle_and_backstop() {
        let c = sync_from("").unwrap();
        assert!(c.auto_commit, "auto-commit ON by default");
        assert_eq!(c.idle(), std::time::Duration::from_secs(2));
        assert_eq!(c.backstop(), std::time::Duration::from_secs(20 * 60));
        assert_eq!(c.confirm_grace_ms, 50);
    }

    #[test]
    fn idle_seconds_overrides_and_wins_over_alias() {
        let c = sync_from("[sync]\nidle_seconds = 5\nquiet_period_ms = 900").unwrap();
        assert_eq!(c.idle(), std::time::Duration::from_secs(5));
        assert!(
            c.quiet_period_ms.is_some(),
            "alias surfaced for the deprecation warning"
        );
    }

    #[test]
    fn quiet_period_ms_alias_still_honored() {
        let c = sync_from("[sync]\nquiet_period_ms = 900").unwrap();
        assert_eq!(c.idle(), std::time::Duration::from_millis(900));
        let c = sync_from("[sync]\nauto_commit = false\nbackstop_minutes = 45").unwrap();
        assert!(!c.auto_commit);
        assert_eq!(c.backstop(), std::time::Duration::from_secs(45 * 60));
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(sync_from("[sync]\nauto_comit = true").is_err());
    }

    #[test]
    fn load_absent_file_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        let c = SyncConfig::load(tmp.path()).unwrap();
        assert!(c.auto_commit);
        assert_eq!(c.idle(), std::time::Duration::from_secs(2));
    }

    #[test]
    fn load_reads_sync_past_sections_it_does_not_own() {
        // The desktop shell owns sealing and nothing else, but reads the same
        // file the daemon writes its CORS/index/plugin settings into. Those
        // sections must not make the parse fail.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[cors]\norigins = [\"http://localhost:5173\"]\n\
             [index]\npersist = false\n\
             [plugins]\ntrusted = [\"a\"]\n\
             [sync]\nidle_seconds = 10\nbackstop_minutes = 45\n",
        )
        .unwrap();
        let c = SyncConfig::load(tmp.path()).unwrap();
        assert_eq!(c.idle(), std::time::Duration::from_secs(10));
        assert_eq!(c.backstop(), std::time::Duration::from_secs(45 * 60));
    }

    #[test]
    fn load_without_a_sync_section_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("cairn.toml"), "[cors]\norigins = []\n").unwrap();
        let c = SyncConfig::load(tmp.path()).unwrap();
        assert!(c.auto_commit);
        assert_eq!(c.backstop(), std::time::Duration::from_secs(20 * 60));
    }

    #[test]
    fn load_surfaces_a_typo_in_the_sync_table() {
        // Strict inside `[sync]`: a user who typed `auto_comit = false` and got
        // silent auto-commit would have no way to tell the setting was ignored.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[sync]\nauto_comit = false\n",
        )
        .unwrap();
        assert!(SyncConfig::load(tmp.path()).is_err());
    }
}
