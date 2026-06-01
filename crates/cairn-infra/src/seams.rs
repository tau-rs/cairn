//! Neutral seam adapters for ports whose real implementations are deferred
//! to later sub-projects. They let the engine compose and run today.

use cairn_ports::{AgentRuntime, CollabSession, Executor, PortError, Watcher};

/// No-op watcher seam.
#[derive(Debug, Default)]
pub struct NoopWatcher;
impl Watcher for NoopWatcher {
    fn start(&mut self) -> Result<(), PortError> {
        Ok(())
    }
}

/// Inline executor seam.
#[derive(Debug, Default)]
pub struct BlockingExecutor;
impl Executor for BlockingExecutor {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

/// No-collaboration seam.
#[derive(Debug, Default)]
pub struct NoCollab;
impl CollabSession for NoCollab {
    fn is_active(&self) -> bool {
        false
    }
}

/// Null agent runtime seam.
#[derive(Debug, Default)]
pub struct NullRuntime;
impl AgentRuntime for NullRuntime {
    fn run_action(&self, action: &str, _context: Option<&str>) -> Result<String, PortError> {
        Err(PortError::Adapter(format!(
            "no agent runtime configured (action '{action}' unavailable until tau is wired)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seams_have_expected_neutral_behavior() {
        assert!(!NoCollab.is_active());
        assert!(NoopWatcher.start().is_ok());
        assert!(NullRuntime.run_action("summarize", None).is_err());
    }
}
