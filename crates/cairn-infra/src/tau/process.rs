//! Owns one `tau serve` subprocess and the client speaking to it.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use cairn_ports::{AdapterError, AgentSink, PortError};

use crate::tau::client::ServeClient;
use crate::tau::config::TauConfig;

/// Max wait for tau's stderr readiness line before kill + error.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// Max wait after closing stdin before SIGKILL on drop.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Per-process timeouts. `Default` uses [`READY_TIMEOUT`] / [`SHUTDOWN_GRACE`];
/// tests inject short values.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Readiness-line read bound.
    pub ready: Duration,
    /// Grace period between stdin-close and SIGKILL.
    pub shutdown_grace: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            ready: READY_TIMEOUT,
            shutdown_grace: SHUTDOWN_GRACE,
        }
    }
}

/// How a [`TauServe`] stopped: it exited on its own within the grace window, or
/// it had to be killed.
#[derive(Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// Exited cleanly after stdin close, within the grace window.
    Graceful,
    /// Still running after the grace window; SIGKILLed.
    Killed,
}

/// A live `tau serve` process plus its serve-mode client. Gracefully shut down
/// on drop. `client` is an `Option` so shutdown can drop it (closing stdin) while
/// still owning `child` to wait/kill.
pub struct TauServe {
    child: Child,
    client: Option<ServeClient<BufReader<ChildStdout>, ChildStdin>>,
    shutdown_grace: Duration,
}

fn missing(what: &str) -> PortError {
    PortError::Adapter(AdapterError::message(format!("tau serve: {what}")))
}

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

impl TauServe {
    /// Spawn `tau serve` from a [`TauConfig`] with default timeouts.
    pub fn spawn(cfg: &TauConfig) -> Result<Self, PortError> {
        Self::spawn_with(cfg, Timeouts::default())
    }

    /// Spawn `tau serve` from a [`TauConfig`] with explicit timeouts.
    pub fn spawn_with(cfg: &TauConfig, timeouts: Timeouts) -> Result<Self, PortError> {
        let mut cmd = Command::new(&cfg.bin);
        cmd.arg("serve").arg("--ready-on-stderr");
        if let Some(project) = &cfg.project {
            cmd.arg("--project").arg(project);
        }
        Self::spawn_command(cmd, timeouts)
    }

    /// Spawn from a pre-built command (stdio is configured here), wait for the
    /// readiness line under [`Timeouts::ready`], and handshake. Lets callers (and
    /// tests) customize the command.
    pub fn spawn_command(mut cmd: Command, timeouts: Timeouts) -> Result<Self, PortError> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(adapt)?;

        // Any failure after the process exists must reap it: `std::process::Child`
        // has no killing `Drop`, so a bare `?` here would orphan the process.
        match Self::connect(&mut child, timeouts.ready) {
            Ok(client) => Ok(Self {
                child,
                client: Some(client),
                shutdown_grace: timeouts.shutdown_grace,
            }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }

    /// Wait for the readiness line (bounded by `ready_timeout`), take the stdio
    /// pipes, and handshake. The std pipe read cannot be interrupted directly, so
    /// a short thread performs it and we bound the wait with `recv_timeout`.
    fn connect(
        child: &mut Child,
        ready_timeout: Duration,
    ) -> Result<ServeClient<BufReader<ChildStdout>, ChildStdin>, PortError> {
        let stderr = child.stderr.take().ok_or_else(|| missing("no stderr"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut err = BufReader::new(stderr);
            let mut line = String::new();
            // Send the byte count (0 = EOF) or the io error.
            let _ = tx.send(err.read_line(&mut line));
        });
        match rx.recv_timeout(ready_timeout) {
            Ok(Ok(0)) => return Err(missing("exited before signalling ready")),
            Ok(Ok(_)) => {} // any non-empty readiness line counts
            Ok(Err(e)) => return Err(adapt(e)),
            Err(RecvTimeoutError::Timeout) => {
                return Err(missing("timed out waiting for readiness"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(missing("readiness reader disconnected"))
            }
        }

        let stdin = child.stdin.take().ok_or_else(|| missing("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| missing("no stdout"))?;
        let mut client = ServeClient::new(BufReader::new(stdout), stdin);
        client.handshake()?;
        Ok(client)
    }

    /// Run `agent` over `prompt`, streaming into `sink`.
    pub fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError> {
        self.client
            .as_mut()
            .ok_or_else(|| missing("client closed"))?
            .run_streaming(agent, prompt, sink)
    }

    /// True while the child is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Graceful shutdown: close stdin (EOF; `tau serve` exits on it, like
    /// plugins), poll for a clean exit up to the grace window, then SIGKILL +
    /// reap if still running. Idempotent: a second call returns `Graceful`
    /// quickly because the child is already reaped.
    pub fn shutdown(&mut self) -> ShutdownOutcome {
        // Dropping the client drops its `ChildStdin`, closing the pipe → EOF.
        self.client = None;
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return ShutdownOutcome::Graceful,
                Ok(None) => {
                    if start.elapsed() >= self.shutdown_grace {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                // A signal can interrupt `waitpid`; retry rather than kill, so a
                // signal arriving mid-shutdown does not skip the grace window.
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        ShutdownOutcome::Killed
    }
}

impl std::fmt::Debug for TauServe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauServe")
            .field("shutdown_grace", &self.shutdown_grace)
            .field("client_present", &self.client.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for TauServe {
    fn drop(&mut self) {
        // Graceful: close stdin → wait-with-grace → kill. Outcome ignored on drop.
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;

    #[test]
    fn spawn_fails_for_missing_binary() {
        let cfg = TauConfig {
            bin: "/nonexistent/tau-xyz".into(),
            agent: "a".into(),
            project: None,
        };
        assert!(TauServe::spawn(&cfg).is_err());
    }

    #[test]
    fn live_run_streams_when_tau_present() {
        // Self-skips unless a real tau is configured (CI stays hermetic).
        let Some(cfg) = TauConfig::from_env() else {
            eprintln!("skip: TAU_BIN unset");
            return;
        };
        let mut serve = TauServe::spawn(&cfg).expect("spawn tau serve");
        #[derive(Default)]
        struct Collect(Vec<AgentEvent>);
        impl AgentSink for Collect {
            fn emit(&mut self, e: AgentEvent) {
                self.0.push(e);
            }
        }
        let mut sink = Collect::default();
        serve
            .run_streaming(&cfg.agent, "say hello", &mut sink)
            .expect("run");
        assert!(sink
            .0
            .iter()
            .any(|e| matches!(e, AgentEvent::Completed | AgentEvent::Failed { .. })));
    }
}
