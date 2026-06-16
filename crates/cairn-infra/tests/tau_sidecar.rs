//! Integration tests: TauSidecar over real `tau-stub` processes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cairn_infra::tau::config::TauConfig;
use cairn_infra::tau::process::{TauServe, Timeouts};
use cairn_infra::tau::supervisor::{TauChannel, TauSidecar};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink};

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tau-stub"))
}

fn cfg() -> TauConfig {
    TauConfig {
        bin: stub(),
        agent: "stub".into(),
        project: None,
    }
}

fn short() -> Timeouts {
    Timeouts {
        ready: Duration::from_secs(5),
        shutdown_grace: Duration::from_millis(200),
    }
}

#[derive(Default)]
struct Collect(Vec<AgentEvent>);
impl AgentSink for Collect {
    fn emit(&mut self, e: AgentEvent) {
        self.0.push(e);
    }
}

/// A spawner that builds a real TauServe from the stub in `mode`, counting spawns.
fn counting_spawner(
    mode: &'static str,
    count: Arc<AtomicUsize>,
) -> impl Fn(&TauConfig) -> Result<Box<dyn TauChannel>, cairn_ports::PortError> + Send + Sync {
    move |_cfg| {
        count.fetch_add(1, Ordering::SeqCst);
        let mut cmd = std::process::Command::new(stub());
        cmd.arg(mode);
        TauServe::spawn_command(cmd, short()).map(|s| Box::new(s) as Box<dyn TauChannel>)
    }
}

#[test]
fn sidecar_reuses_one_live_process() {
    let count = Arc::new(AtomicUsize::new(0));
    let sidecar = TauSidecar::with_spawner(cfg(), counting_spawner("ready-run", count.clone()));

    for _ in 0..3 {
        let mut sink = Collect::default();
        sidecar.answer("q", &mut sink).unwrap();
        assert!(sink.0.iter().any(|e| matches!(e, AgentEvent::Completed)));
    }
    assert_eq!(count.load(Ordering::SeqCst), 1, "one warm process reused");
}

#[test]
fn sidecar_respawns_after_a_crash() {
    let count = Arc::new(AtomicUsize::new(0));
    let sidecar = TauSidecar::with_spawner(cfg(), counting_spawner("die-after-run", count.clone()));

    // First request: spawns a process, runs successfully, and the stub then exits.
    let mut sink = Collect::default();
    sidecar.answer("q", &mut sink).unwrap();
    assert!(sink.0.iter().any(|e| matches!(e, AgentEvent::Completed)));

    // The stub exits cleanly right after the run. In production a crash is
    // observed on a *later* request (calls are seconds apart and serialized); here
    // we let the exit settle so the supervisor's liveness check deterministically
    // sees a dead process rather than racing the stub's exit. The respawn *logic*
    // itself is proven deterministically by the `dead_channel_triggers_respawn`
    // unit test in `supervisor.rs`; this test confirms it end-to-end over a real
    // process.
    std::thread::sleep(Duration::from_millis(300));

    // Second request: the dead process is detected and a fresh one is spawned.
    let mut sink = Collect::default();
    sidecar.answer("q", &mut sink).unwrap();
    assert!(sink.0.iter().any(|e| matches!(e, AgentEvent::Completed)));

    assert_eq!(count.load(Ordering::SeqCst), 2, "respawned after the crash");
}
