//! Configuration for reaching tau. v1 reads it from the environment; the daemon
//! `[tau]` TOML section + sidecar supervision land with the web panel (v1.1).

use std::path::PathBuf;

/// How to launch and address tau.
#[derive(Debug, Clone)]
pub struct TauConfig {
    /// Path to the `tau` binary.
    pub bin: PathBuf,
    /// Agent id to invoke.
    pub agent: String,
    /// tau project directory; `None` lets tau use its default.
    pub project: Option<PathBuf>,
}

impl TauConfig {
    /// Build from a lookup function. `None` if `TAU_BIN` is unset (tau disabled).
    /// `TAU_AGENT` defaults to `"default"`; `TAU_PROJECT` is optional.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let bin = get("TAU_BIN")?;
        Some(Self {
            bin: PathBuf::from(bin),
            agent: get("TAU_AGENT").unwrap_or_else(|| "default".to_string()),
            project: get("TAU_PROJECT").map(PathBuf::from),
        })
    }

    /// Build from the process environment.
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn none_without_bin() {
        assert!(TauConfig::from_lookup(lookup(&[("TAU_AGENT", "x")])).is_none());
    }

    #[test]
    fn defaults_agent_when_only_bin_set() {
        let cfg = TauConfig::from_lookup(lookup(&[("TAU_BIN", "/usr/bin/tau")])).unwrap();
        assert_eq!(cfg.bin, PathBuf::from("/usr/bin/tau"));
        assert_eq!(cfg.agent, "default");
        assert!(cfg.project.is_none());
    }

    #[test]
    fn reads_all_fields() {
        let cfg = TauConfig::from_lookup(lookup(&[
            ("TAU_BIN", "/t"),
            ("TAU_AGENT", "answerer"),
            ("TAU_PROJECT", "/proj"),
        ]))
        .unwrap();
        assert_eq!(cfg.agent, "answerer");
        assert_eq!(cfg.project, Some(PathBuf::from("/proj")));
    }
}
