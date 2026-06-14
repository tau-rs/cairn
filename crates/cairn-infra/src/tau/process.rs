//! Owns one `tau serve` subprocess and the client speaking to it.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use cairn_ports::{AdapterError, AgentSink, PortError};

use crate::tau::client::ServeClient;
use crate::tau::config::TauConfig;

/// A live `tau serve` process plus its serve-mode client. Killed on drop.
pub struct TauServe {
    child: Child,
    client: ServeClient<BufReader<ChildStdout>, ChildStdin>,
}

fn missing(what: &str) -> PortError {
    PortError::Adapter(AdapterError::message(format!("tau serve: {what}")))
}

impl TauServe {
    /// Spawn `tau serve`, wait for its readiness line on stderr, and handshake.
    pub fn spawn(cfg: &TauConfig) -> Result<Self, PortError> {
        let mut cmd = Command::new(&cfg.bin);
        cmd.arg("serve").arg("--ready-on-stderr");
        if let Some(project) = &cfg.project {
            cmd.arg("--project").arg(project);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| PortError::Adapter(AdapterError::new(e)))?;

        // Any failure after the process exists must reap it: `std::process::Child`
        // has no killing `Drop`, so a bare `?` here would orphan `tau serve`.
        match Self::connect(&mut child) {
            Ok(client) => Ok(Self { child, client }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }

    /// Read the readiness line, take the stdio pipes, and handshake. Borrows the
    /// child so [`spawn`](Self::spawn) can reap it if any step fails.
    fn connect(
        child: &mut Child,
    ) -> Result<ServeClient<BufReader<ChildStdout>, ChildStdin>, PortError> {
        // Block until tau writes its readiness marker to stderr (the
        // `--ready-on-stderr` flag keeps it off the NDJSON stdout channel). Any
        // non-empty line counts as the marker.
        // TODO(v1.1): bound this read with a timeout so a started-but-silent tau
        // cannot hang `spawn` forever (matters once the daemon supervises it).
        let stderr = child.stderr.take().ok_or_else(|| missing("no stderr"))?;
        let mut err = BufReader::new(stderr);
        let mut line = String::new();
        if err
            .read_line(&mut line)
            .map_err(|e| PortError::Adapter(AdapterError::new(e)))?
            == 0
        {
            return Err(missing("exited before signalling ready"));
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
        self.client.run_streaming(agent, prompt, sink)
    }
}

impl Drop for TauServe {
    fn drop(&mut self) {
        // v1: immediate SIGKILL + reap — a one-shot `tau serve` holds no unflushed
        // state. TODO(v1.1): graceful shutdown (close stdin → wait-with-grace →
        // kill) per the spec, once the daemon owns a long-lived sidecar.
        let _ = self.child.kill();
        let _ = self.child.wait();
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
