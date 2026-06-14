//! Neutral seam adapters for ports whose real implementations are deferred
//! to later sub-projects. They let the engine compose and run today.

use cairn_ports::{
    AgentRuntime, AgentSink, CollabSession, Executor, FsChange, PortError, WatchHandle, Watcher,
};

/// No-op watcher seam.
#[derive(Debug, Default)]
pub struct NoopWatcher;
impl Watcher for NoopWatcher {
    fn watch(&self, _root: &std::path::Path) -> Result<WatchHandle, PortError> {
        // Park the sender in the keepalive so the receiver never yields and
        // never disconnects: a no-op watcher reports no changes, ever.
        let (tx, rx) = std::sync::mpsc::channel::<FsChange>();
        Ok(WatchHandle::new(rx, Box::new(tx)))
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
    fn answer(&self, _prompt: &str, _sink: &mut dyn AgentSink) -> Result<(), PortError> {
        Err(PortError::Adapter(
            "no agent runtime configured (set TAU_BIN to enable `cairn ask`)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;

    #[test]
    fn seams_have_expected_neutral_behavior() {
        assert!(!NoCollab.is_active());
        // Times out (not Disconnected): the parked sender keeps the channel
        // open, so the no-op watcher never yields and never disconnects.
        let handle = NoopWatcher.watch(std::path::Path::new(".")).unwrap();
        assert_eq!(
            handle
                .changes
                .recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );
        // Collect into a Vec sink; NullRuntime errors before emitting anything.
        struct NoopSink;
        impl AgentSink for NoopSink {
            fn emit(&mut self, _e: AgentEvent) {}
        }
        assert!(NullRuntime.answer("summarize this", &mut NoopSink).is_err());
    }
}
