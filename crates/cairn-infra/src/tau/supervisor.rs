//! `TauSidecar`: a long-lived, daemon-supervised `tau serve` behind the
//! `AgentRuntime` port. Serializes concurrent answers, restarts on death, and
//! throttles respawn crash-loops with [`Backoff`].

use std::sync::Mutex;
use std::time::Duration;

use cairn_ports::{AgentRuntime, AgentSink, PortError};

use crate::tau::config::TauConfig;
use crate::tau::process::TauServe;

/// Lower bound of the respawn backoff (delay after the first failure).
pub const BACKOFF_BASE: Duration = Duration::from_millis(100);
/// Upper bound of the respawn backoff.
pub const BACKOFF_CAP: Duration = Duration::from_secs(5);

/// Crash-loop guard: tracks consecutive spawn failures and the delay to wait
/// before the next respawn. Pure — no clock; the caller does the sleeping.
#[derive(Debug, Default)]
struct Backoff {
    failures: u32,
}

impl Backoff {
    /// Delay to wait before the next spawn attempt: zero with no prior failure,
    /// then `BACKOFF_BASE * 2^(failures-1)` capped at `BACKOFF_CAP`.
    fn delay_before_retry(&self) -> Duration {
        if self.failures == 0 {
            return Duration::ZERO;
        }
        let base_ms = BACKOFF_BASE.as_millis() as u64;
        let cap_ms = BACKOFF_CAP.as_millis() as u64;
        // `checked_shl` only guards the shift amount, not value overflow, so build
        // the doubling factor with it and then `saturating_mul` — a large factor
        // saturates to `u64::MAX` instead of wrapping (which could yield 0ms and
        // erase the guard), and `.min(cap_ms)` bounds it.
        let factor = 1u64.checked_shl(self.failures - 1).unwrap_or(u64::MAX);
        let ms = base_ms.saturating_mul(factor).min(cap_ms);
        Duration::from_millis(ms)
    }

    fn record_success(&mut self) {
        self.failures = 0;
    }

    fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

/// The supervisor's view of a serve connection. Implemented by [`TauServe`]
/// (production) and by an in-memory fake in tests, so the supervision state
/// machine is testable without a subprocess.
pub trait TauChannel: Send {
    /// True while the underlying process is still running.
    fn is_alive(&mut self) -> bool;
    /// Run `agent` over `prompt`, streaming into `sink`.
    fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError>;
}

impl TauChannel for TauServe {
    fn is_alive(&mut self) -> bool {
        TauServe::is_alive(self)
    }
    fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError> {
        TauServe::run_streaming(self, agent, prompt, sink)
    }
}

type Spawn = Box<dyn Fn(&TauConfig) -> Result<Box<dyn TauChannel>, PortError> + Send + Sync>;

struct State {
    conn: Option<Box<dyn TauChannel>>,
    backoff: Backoff,
}

/// A long-lived, daemon-supervised `tau serve` behind the `AgentRuntime` port.
/// One process, reused across requests; concurrent `answer` calls serialize on
/// the `Mutex`; a dead process is respawned (with backoff) on the next request.
pub struct TauSidecar {
    config: TauConfig,
    spawn: Spawn,
    state: Mutex<State>,
}

impl TauSidecar {
    /// Build a sidecar that spawns a real supervised `tau serve` lazily on first
    /// use. The process is not started here.
    pub fn new(config: TauConfig) -> Self {
        Self::with_spawner(config, |cfg| {
            TauServe::spawn(cfg).map(|s| Box::new(s) as Box<dyn TauChannel>)
        })
    }

    /// Build with a custom spawner (tests inject an in-memory `TauChannel`).
    /// Not part of the stable API — the seam exists for tests, including the
    /// out-of-crate integration tests that cannot use `#[cfg(test)]` visibility.
    #[doc(hidden)]
    pub fn with_spawner(
        config: TauConfig,
        spawn: impl Fn(&TauConfig) -> Result<Box<dyn TauChannel>, PortError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            config,
            spawn: Box::new(spawn),
            state: Mutex::new(State {
                conn: None,
                backoff: Backoff::default(),
            }),
        }
    }

    /// Ensure `state.conn` holds a live connection, respawning (with backoff) if
    /// it is absent or dead. Returns a mutable reference to the live connection.
    fn ensure_alive<'a>(
        &self,
        state: &'a mut State,
    ) -> Result<&'a mut Box<dyn TauChannel>, PortError> {
        let need_spawn = match &mut state.conn {
            Some(conn) => !conn.is_alive(),
            None => true,
        };
        if need_spawn {
            state.conn = None; // drop the dead one (runs its graceful shutdown)
            let delay = state.backoff.delay_before_retry();
            if !delay.is_zero() {
                // NOTE: this sleeps while the caller still holds the state lock, so
                // concurrent answers stall for the backoff. That is acceptable under
                // the serialize-one-process model (there is no parallelism to lose);
                // a multiplexed design would need the sleep outside the lock.
                std::thread::sleep(delay);
            }
            match (self.spawn)(&self.config) {
                Ok(conn) => {
                    state.backoff.record_success();
                    state.conn = Some(conn);
                }
                Err(e) => {
                    state.backoff.record_failure();
                    return Err(e);
                }
            }
        }
        Ok(state
            .conn
            .as_mut()
            .expect("conn present after ensure_alive"))
    }
}

impl AgentRuntime for TauSidecar {
    fn answer(&self, prompt: &str, sink: &mut dyn AgentSink) -> Result<(), PortError> {
        // The lock serializes concurrent answers against the one process.
        // Recover from poisoning rather than propagating (mirrors the daemon).
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let result = {
            let conn = self.ensure_alive(&mut state)?;
            conn.run_streaming(&self.config.agent, prompt, sink)
        };
        if result.is_err() {
            // Transport failure: drop the connection so the next call respawns.
            state.conn = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::AgentEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn backoff_schedule_grows_and_caps() {
        let mut b = Backoff::default();
        assert_eq!(b.delay_before_retry(), Duration::ZERO); // no failure yet
        b.record_failure();
        assert_eq!(b.delay_before_retry(), Duration::from_millis(100));
        b.record_failure();
        assert_eq!(b.delay_before_retry(), Duration::from_millis(200));
        b.record_failure();
        assert_eq!(b.delay_before_retry(), Duration::from_millis(400));
        for _ in 0..40 {
            b.record_failure();
        }
        assert_eq!(b.delay_before_retry(), BACKOFF_CAP, "saturates at the cap");
    }

    #[test]
    fn backoff_resets_on_success() {
        let mut b = Backoff::default();
        b.record_failure();
        b.record_failure();
        b.record_success();
        assert_eq!(b.delay_before_retry(), Duration::ZERO);
    }

    #[test]
    fn backoff_stays_capped_at_extreme_failure_counts() {
        // Regression: at high counts the doubling factor overflows u64; a naive
        // `value << shift` wraps (e.g. to 0ms at failures 63/64), erasing the
        // guard. The delay must never exceed the cap and must hold AT the cap once
        // reached, across the whole wrap window and beyond.
        let mut b = Backoff::default();
        for i in 1..=70 {
            b.record_failure(); // failures = i
            let d = b.delay_before_retry();
            assert!(d <= BACKOFF_CAP, "failures={i}: {d:?} exceeds cap");
            if i >= 7 {
                // 100ms * 2^6 = 6.4s > 5s cap, so from here it is pinned at the cap.
                assert_eq!(d, BACKOFF_CAP, "failures={i}: must stay at the cap");
            }
        }
    }

    fn cfg() -> TauConfig {
        TauConfig {
            bin: "unused".into(),
            agent: "a".into(),
            project: None,
        }
    }

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    /// A scripted in-memory channel. `alive` controls `is_alive`; each run emits
    /// Completed and records concurrency via `active`/`max_active`.
    struct FakeChannel {
        alive: bool,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        runs: Arc<AtomicUsize>,
    }
    impl TauChannel for FakeChannel {
        fn is_alive(&mut self) -> bool {
            self.alive
        }
        fn run_streaming(
            &mut self,
            _agent: &str,
            _prompt: &str,
            sink: &mut dyn AgentSink,
        ) -> Result<(), PortError> {
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            self.runs.fetch_add(1, Ordering::SeqCst);
            sink.emit(AgentEvent::Completed);
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn answers_serialize_against_one_process() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicUsize::new(0));
        let (a, m, r) = (active.clone(), max_active.clone(), runs.clone());
        let sidecar = Arc::new(TauSidecar::with_spawner(cfg(), move |_| {
            Ok(Box::new(FakeChannel {
                alive: true,
                active: a.clone(),
                max_active: m.clone(),
                runs: r.clone(),
            }) as Box<dyn TauChannel>)
        }));

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let s = sidecar.clone();
                std::thread::spawn(move || {
                    let mut sink = VecSink::default();
                    s.answer("q", &mut sink).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 1, "runs never overlap");
        assert_eq!(runs.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn spawn_failure_surfaces_as_err() {
        let sidecar = TauSidecar::with_spawner(cfg(), |_| Err(PortError::Adapter("boom".into())));
        let mut sink = VecSink::default();
        assert!(sidecar.answer("q", &mut sink).is_err());
    }

    /// A channel that always reports dead, so `ensure_alive` respawns it on the
    /// next call. Runs still succeed (emit Completed) so `answer` returns Ok.
    struct DeadChannel;
    impl TauChannel for DeadChannel {
        fn is_alive(&mut self) -> bool {
            false
        }
        fn run_streaming(
            &mut self,
            _agent: &str,
            _prompt: &str,
            sink: &mut dyn AgentSink,
        ) -> Result<(), PortError> {
            sink.emit(AgentEvent::Completed);
            Ok(())
        }
    }

    #[test]
    fn dead_channel_triggers_respawn() {
        let spawns = Arc::new(AtomicUsize::new(0));
        let s = spawns.clone();
        let sidecar = TauSidecar::with_spawner(cfg(), move |_| {
            s.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(DeadChannel) as Box<dyn TauChannel>)
        });

        let mut sink = VecSink::default();
        sidecar.answer("q", &mut sink).unwrap(); // conn was None -> spawn #1
        sidecar.answer("q", &mut sink).unwrap(); // conn dead -> respawn #2
        assert_eq!(
            spawns.load(Ordering::SeqCst),
            2,
            "dead conn forces a respawn"
        );
    }
}
