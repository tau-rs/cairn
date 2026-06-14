//! `AgentRuntime` over a one-shot `tau serve` subprocess.

use cairn_ports::{AgentRuntime, AgentSink, PortError};

use crate::tau::config::TauConfig;
use crate::tau::process::TauServe;

/// Runs each `answer` against a freshly-spawned `tau serve` (v1: one-shot, no
/// long-lived supervision — that lands with the daemon path in v1.1).
#[derive(Debug, Clone)]
pub struct TauServeRuntime {
    config: TauConfig,
}

impl TauServeRuntime {
    /// Build from a [`TauConfig`].
    pub fn new(config: TauConfig) -> Self {
        Self { config }
    }
}

impl AgentRuntime for TauServeRuntime {
    fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
        let mut serve = TauServe::spawn(&self.config)?;
        serve.run_streaming(&self.config.agent, prompt, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;

    #[test]
    fn answer_errs_when_binary_missing() {
        let rt = TauServeRuntime::new(TauConfig {
            bin: "/nonexistent/tau-xyz".into(),
            agent: "a".into(),
            project: None,
        });
        struct Noop;
        impl AgentSink for Noop {
            fn emit(&mut self, _e: AgentEvent) {}
        }
        assert!(rt.answer("hi", &mut Noop).is_err());
    }
}
