# Daemon-supervised tau sidecar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote the daemon's one-shot `tau serve` adapter to a single long-lived, supervised process shared across `POST /ask` requests, with a readiness-read timeout, graceful shutdown, restart-on-use with backoff, and serialized concurrency.

**Architecture:** Improve the shared `TauServe` process primitive (bounded readiness read; graceful-shutdown `Drop`; `is_alive`). Add a `TauSidecar` supervisor behind the unchanged `AgentRuntime` port that owns one `TauServe` behind a `Mutex`, respawns on death, and serializes runs. The daemon swaps `TauServeRuntime` → `TauSidecar`; the CLI keeps the one-shot. Pure core (`cairn-domain`/`cairn-service`) is untouched.

**Tech Stack:** Rust, `std::process` / `std::sync::mpsc` / `std::thread` (no new deps), `serde_json` (already a dep), the existing `ServeClient`/`wire` serve-mode client.

**Spec:** `docs/superpowers/specs/2026-06-16-tau-daemon-sidecar-design.md`
**ADR:** `docs/decisions/0011-tau-sidecar-supervision.md`

---

## File Structure

| Path | Responsibility |
|---|---|
| `crates/cairn-infra/src/bin/tau-stub.rs` | **Create.** Cross-platform Rust test helper that mimics `tau serve` over stdio in several modes (ready/never-ready/stuck/dies-after-run). Used only by tests; not shipped (release binaries are `cairn-cli`/`cairn-daemon`). |
| `crates/cairn-infra/src/tau/process.rs` | **Modify.** `TauServe`: bounded readiness read, `Timeouts`, `is_alive`, `shutdown` → `ShutdownOutcome`, graceful `Drop`, `spawn_command`. |
| `crates/cairn-infra/src/tau/supervisor.rs` | **Create.** `Backoff` (pure), `TauChannel` seam + impl for `TauServe`, `TauSidecar` (the supervised `AgentRuntime`). |
| `crates/cairn-infra/src/tau/mod.rs` | **Modify.** `pub mod supervisor;` + re-export `TauSidecar`. |
| `crates/cairn-infra/src/lib.rs:25` | **Modify.** Re-export `TauSidecar`. |
| `crates/cairn-infra/tests/tau_lifecycle.rs` | **Create.** Integration tests for readiness timeout + graceful/kill shutdown (need the stub binary via `CARGO_BIN_EXE_*`). |
| `crates/cairn-infra/tests/tau_sidecar.rs` | **Create.** Integration tests: sidecar reuses one live process; respawns after a crash. |
| `crates/cairn-daemon/src/main.rs:139-149` | **Modify.** Build `TauSidecar` instead of `TauServeRuntime`. |

---

## Task 1: `tau-stub` cross-platform test binary

A pure-Rust stand-in for `tau serve`, selected by its first CLI argument so tests can drive each supervision path without a real tau. Modes: `ready-run` (default), `no-ready`, `no-exit`, `die-after-run`.

**Files:**
- Create: `crates/cairn-infra/src/bin/tau-stub.rs`
- Test: `crates/cairn-infra/tests/tau_lifecycle.rs`

- [ ] **Step 1: Write the stub binary**

```rust
//! Test-only stand-in for `tau serve` (selected by argv[1]). Not shipped: the
//! release binaries are `cairn-cli`/`cairn-daemon`, which never invoke this.
//!
//! Modes:
//!   ready-run     (default) emit readiness, answer handshake + one run, exit on stdin EOF
//!   no-ready      emit nothing on stderr, block forever (never signals ready)
//!   no-exit       emit readiness, answer handshake, then ignore stdin EOF (stays alive)
//!   die-after-run emit readiness, answer handshake + one run, then exit immediately

use std::io::{BufRead, Write};

fn ready() {
    eprintln!("ready");
    let _ = std::io::stderr().flush();
}

fn answer_line(out: &mut impl Write, line: &str) -> bool {
    // Returns true after handling a run (caller may choose to exit).
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
    let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("");
    match method {
        "meta.handshake" => {
            writeln!(out, "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}").unwrap();
            let _ = out.flush();
            false
        }
        "runtime.run_streaming" => {
            writeln!(out, "{{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{{\"id\":{id},\"kind\":\"TextDelta\",\"data\":{{\"text\":\"hi\"}}}}}}").unwrap();
            writeln!(out, "{{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{{\"id\":{id},\"kind\":\"RunCompleted\",\"data\":{{}}}}}}").unwrap();
            writeln!(out, "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}").unwrap();
            let _ = out.flush();
            true
        }
        _ => false,
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let forever = || loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    };

    if mode == "no-ready" {
        // Never signal ready; block so the parent's readiness wait times out.
        forever();
    }

    ready();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let handled_run = answer_line(&mut out, &line);
        if handled_run && mode == "die-after-run" {
            return; // process dies right after one run
        }
    }
    // stdin EOF.
    if mode == "no-exit" {
        forever(); // ignore EOF: parent must kill us after the grace window
    }
    // ready-run / die-after-run: fall through and exit gracefully.
}
```

- [ ] **Step 2: Write a self-contained fixture smoke test**

This test must compile and **pass** at this task (the pre-commit hook runs the
full clippy `-D warnings` + test suite on every commit, so a commit may not leave
a non-compiling or failing test in the tree). It exercises the stub directly over
`std::process`, with **no dependency on `TauServe`** (which gains
`spawn_command` only in Task 2).

Create `crates/cairn-infra/tests/tau_lifecycle.rs`:

```rust
//! Integration tests for the `tau-stub` helper and (from Task 2 on) the
//! `TauServe` process primitive. The stub binary is located via CARGO_BIN_EXE.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tau-stub"))
}

#[test]
fn stub_signals_ready_and_answers_handshake() {
    // Validates the fixture without TauServe: readiness marker on stderr, then a
    // handshake round-trip on stdout, then graceful exit on stdin close.
    let mut child = Command::new(stub())
        .arg("ready-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stub");

    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    err.read_line(&mut line).unwrap();
    assert!(line.contains("ready"), "stderr readiness line: {line:?}");

    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"meta.handshake\",\"params\":{{}}}}"
    )
    .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut reply = String::new();
    out.read_line(&mut reply).unwrap();
    assert!(reply.contains("\"id\":1"), "handshake reply: {reply:?}");

    drop(stdin); // EOF → stub exits
    let _ = child.wait();
}
```

- [ ] **Step 3: Run the smoke test to verify it passes**

Run: `cargo test -p cairn-infra --test tau_lifecycle stub_signals_ready_and_answers_handshake`
Expected: PASS. Also run `just lint` (clippy `-D warnings`) and confirm it is clean — the stub binary is linted as an `--all-targets` target.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-infra/src/bin/tau-stub.rs crates/cairn-infra/tests/tau_lifecycle.rs
git commit -m "test(tau): cross-platform tau-stub helper binary + lifecycle test scaffold"
```

---

## Task 2: Bounded readiness read + `Timeouts` + struct restructure (`TauServe`)

Replace the unbounded stderr readiness read (`process.rs:56`) with a reader-thread + `recv_timeout`. Introduce `Timeouts` and a `spawn_command` core, and make `client` an `Option` (needed by Task 3's graceful shutdown).

**Files:**
- Modify: `crates/cairn-infra/src/tau/process.rs`
- Test: `crates/cairn-infra/tests/tau_lifecycle.rs`

- [ ] **Step 1: Write the failing test (readiness timeout)**

First add these imports to the top of `crates/cairn-infra/tests/tau_lifecycle.rs`
(below the existing `use` lines):

```rust
use std::time::{Duration, Instant};

use cairn_infra::tau::process::{TauServe, Timeouts};
```

Then add the test:

```rust
#[test]
fn readiness_read_times_out_for_silent_tau() {
    // A tau that starts but never writes its readiness line must not hang spawn:
    // the bounded read fires and the child is reaped.
    let mut cmd = std::process::Command::new(stub());
    cmd.arg("no-ready");
    let timeouts = Timeouts {
        ready: Duration::from_millis(200),
        shutdown_grace: Duration::from_millis(200),
    };
    let start = std::time::Instant::now();
    let err = TauServe::spawn_command(cmd, timeouts).expect_err("must time out");
    assert!(start.elapsed() < Duration::from_secs(5), "returned promptly");
    assert!(
        err.to_string().contains("readiness"),
        "error names the readiness wait: {err}"
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-infra --test tau_lifecycle readiness_read_times_out_for_silent_tau`
Expected: FAIL to compile (`spawn_command`/`Timeouts` missing).

- [ ] **Step 3: Rewrite the top of `process.rs` (imports, constants, struct, Timeouts)**

Replace the current header + `struct TauServe` (lines 1-15) with:

```rust
//! Owns one `tau serve` subprocess and the client speaking to it.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

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
```

- [ ] **Step 4: Replace `spawn` / `connect` with the bounded, command-driven versions**

Replace the existing `impl TauServe { pub fn spawn(...) ... fn connect(...) ... }` block (lines 21-74) up to but **not** including `run_streaming` with:

```rust
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
```

- [ ] **Step 5: Update `run_streaming` for the `Option` client**

Replace the existing `run_streaming` body (lines 77-84) with:

```rust
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
```

(Leave the existing `impl Drop` for now; Task 3 rewrites it. The crate will not compile until Task 3 because `Drop` still references `self.child.kill()` only — that is fine, but `client` field type changed. Update `Drop` minimally here so the crate compiles: replace the Drop body's nothing — actually keep Drop as-is; it only touches `self.child`, which still exists. It compiles.)

- [ ] **Step 6: Run the readiness + smoke tests**

Run: `cargo test -p cairn-infra --test tau_lifecycle`
Expected: PASS — `stub_speaks_handshake_and_a_run` and `readiness_read_times_out_for_silent_tau`.

- [ ] **Step 7: Run the existing process unit tests**

Run: `cargo test -p cairn-infra tau::process`
Expected: PASS — `spawn_fails_for_missing_binary` still errors; `live_run_streams_when_tau_present` self-skips (TAU_BIN unset).

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-infra/src/tau/process.rs crates/cairn-infra/tests/tau_lifecycle.rs
git commit -m "feat(tau): bound the serve readiness read with a timeout"
```

---

## Task 3: `is_alive` + graceful shutdown (`TauServe`)

Add liveness detection and replace the SIGKILL-only `Drop` with close-stdin → grace-poll → kill, exposed as a testable `shutdown()`.

**Files:**
- Modify: `crates/cairn-infra/src/tau/process.rs`
- Test: `crates/cairn-infra/tests/tau_lifecycle.rs`

- [ ] **Step 1: Write the failing tests**

First update the process import in `crates/cairn-infra/tests/tau_lifecycle.rs` to
add `ShutdownOutcome`:

```rust
use cairn_infra::tau::process::{ShutdownOutcome, TauServe, Timeouts};
```

Then add the tests:

```rust
#[test]
fn shutdown_is_graceful_when_child_exits_on_eof() {
    let mut cmd = std::process::Command::new(stub());
    cmd.arg("ready-run"); // exits when stdin closes
    let mut serve = TauServe::spawn_command(cmd, Timeouts::default()).expect("spawn");
    assert_eq!(serve.shutdown(), ShutdownOutcome::Graceful);
}

#[test]
fn shutdown_kills_child_that_ignores_eof() {
    let mut cmd = std::process::Command::new(stub());
    cmd.arg("no-exit"); // answers handshake, then never exits
    let timeouts = Timeouts {
        ready: Duration::from_secs(5),
        shutdown_grace: Duration::from_millis(200),
    };
    let mut serve = TauServe::spawn_command(cmd, timeouts).expect("spawn");
    assert_eq!(serve.shutdown(), ShutdownOutcome::Killed);
}

#[test]
fn is_alive_tracks_the_child() {
    let mut cmd = std::process::Command::new(stub());
    cmd.arg("ready-run");
    let mut serve = TauServe::spawn_command(cmd, Timeouts::default()).expect("spawn");
    assert!(serve.is_alive());
    assert_eq!(serve.shutdown(), ShutdownOutcome::Graceful);
    assert!(!serve.is_alive(), "dead after shutdown");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p cairn-infra --test tau_lifecycle shutdown`
Expected: FAIL to compile (`shutdown`, `is_alive` missing).

- [ ] **Step 3: Add `is_alive` + `shutdown` and rewrite `Drop`**

Add `use std::time::Instant;` to the imports at the top of `process.rs` (alongside the existing `use std::time::Duration;`), then add these methods at the end of the `impl TauServe` block (right after `run_streaming`):

```rust
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
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        ShutdownOutcome::Killed
    }
```

Replace the entire existing `impl Drop for TauServe { ... }` block (lines 87-95) with:

```rust
impl Drop for TauServe {
    fn drop(&mut self) {
        // Graceful: close stdin → wait-with-grace → kill. Outcome ignored on drop.
        let _ = self.shutdown();
    }
}
```

- [ ] **Step 4: Run the shutdown tests**

Run: `cargo test -p cairn-infra --test tau_lifecycle`
Expected: PASS (all five lifecycle tests).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/tau/process.rs crates/cairn-infra/tests/tau_lifecycle.rs
git commit -m "feat(tau): graceful shutdown (close stdin, grace, kill) + is_alive"
```

---

## Task 4: `Backoff` crash-loop guard (pure)

A clock-free respawn backoff used by the supervisor.

**Files:**
- Create: `crates/cairn-infra/src/tau/supervisor.rs`
- Modify: `crates/cairn-infra/src/tau/mod.rs`

- [ ] **Step 1: Register the module**

In `crates/cairn-infra/src/tau/mod.rs`, add after `pub mod process;`:

```rust
pub mod supervisor;
```

(Re-export of `TauSidecar` is added in Task 7, once the type exists.)

- [ ] **Step 2: Write the failing tests + module skeleton**

Create `crates/cairn-infra/src/tau/supervisor.rs`:

```rust
//! `TauSidecar`: a long-lived, daemon-supervised `tau serve` behind the
//! `AgentRuntime` port. Serializes concurrent answers, restarts on death, and
//! throttles respawn crash-loops with [`Backoff`].

use std::time::Duration;

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
        let ms = base_ms.checked_shl(self.failures - 1).unwrap_or(u64::MAX).min(cap_ms);
        Duration::from_millis(ms)
    }

    fn record_success(&mut self) {
        self.failures = 0;
    }

    fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p cairn-infra tau::supervisor::tests::backoff`
Expected: PASS (both backoff tests).

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-infra/src/tau/supervisor.rs crates/cairn-infra/src/tau/mod.rs
git commit -m "feat(tau): clock-free respawn backoff for the sidecar supervisor"
```

---

## Task 5: `TauChannel` seam + `TauSidecar` supervisor

The supervised `AgentRuntime`: one connection behind a `Mutex`, lazy spawn, restart-on-use, serialized runs. The `TauChannel` seam makes the state machine testable with an in-memory fake.

**Files:**
- Modify: `crates/cairn-infra/src/tau/supervisor.rs`

- [ ] **Step 1: Write the failing tests (fake-backed state machine)**

Add to the `tests` module in `crates/cairn-infra/src/tau/supervisor.rs`:

```rust
    use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, PortError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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
        let sidecar = TauSidecar::with_spawner(cfg(), |_| {
            Err(PortError::Adapter("boom".into()))
        });
        let mut sink = VecSink::default();
        assert!(sidecar.answer("q", &mut sink).is_err());
    }

    fn cfg() -> crate::tau::config::TauConfig {
        crate::tau::config::TauConfig {
            bin: "unused".into(),
            agent: "a".into(),
            project: None,
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p cairn-infra tau::supervisor`
Expected: FAIL to compile (`TauChannel`, `TauSidecar` missing).

- [ ] **Step 3: Implement `TauChannel`, the `TauServe` impl, and `TauSidecar`**

Add to the top-level of `crates/cairn-infra/src/tau/supervisor.rs` (after the `use`/const block, before `#[cfg(test)]`):

```rust
use std::sync::Mutex;

use cairn_ports::{AgentRuntime, AgentSink, PortError};

use crate::tau::config::TauConfig;
use crate::tau::process::TauServe;

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
    fn ensure_alive<'a>(&self, state: &'a mut State) -> Result<&'a mut Box<dyn TauChannel>, PortError> {
        let need_spawn = match &mut state.conn {
            Some(conn) => !conn.is_alive(),
            None => true,
        };
        if need_spawn {
            state.conn = None; // drop the dead one (runs its graceful shutdown)
            let delay = state.backoff.delay_before_retry();
            if !delay.is_zero() {
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
        Ok(state.conn.as_mut().expect("conn present after ensure_alive"))
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
```

- [ ] **Step 4: Run the supervisor tests**

Run: `cargo test -p cairn-infra tau::supervisor`
Expected: PASS — `answers_serialize_against_one_process`, `spawn_failure_surfaces_as_err`, and both backoff tests.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/tau/supervisor.rs
git commit -m "feat(tau): TauSidecar supervisor — serialized, restart-on-use AgentRuntime"
```

---

## Task 6: Integration — reuse and respawn against a real process

Drive `TauSidecar` through real `TauServe` processes (the stub) to prove the warm process is reused and that a crash triggers exactly one respawn.

**Files:**
- Create: `crates/cairn-infra/tests/tau_sidecar.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/cairn-infra/tests/tau_sidecar.rs`:

```rust
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

    // Each run exits the stub; the next answer must detect death and respawn.
    for _ in 0..2 {
        let mut sink = Collect::default();
        sidecar.answer("q", &mut sink).unwrap();
        assert!(sink.0.iter().any(|e| matches!(e, AgentEvent::Completed)));
    }
    assert_eq!(count.load(Ordering::SeqCst), 2, "respawned after the crash");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p cairn-infra --test tau_sidecar`
Expected: FAIL to compile only if exports are missing. `TauChannel`/`TauSidecar` are `pub` (Task 5); `tau::process`/`tau::supervisor`/`tau::config` are `pub` modules. If a path is unresolved, fix the `use` to match the actual module path — do not change visibility of internals.

- [ ] **Step 3: Run to verify they pass**

Run: `cargo test -p cairn-infra --test tau_sidecar`
Expected: PASS — both tests.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-infra/tests/tau_sidecar.rs
git commit -m "test(tau): sidecar reuses one process and respawns after a crash"
```

---

## Task 7: Wire the daemon + export `TauSidecar`

Swap the daemon's runtime and export the new type. The CLI is intentionally left on `TauServeRuntime`.

**Files:**
- Modify: `crates/cairn-infra/src/tau/mod.rs`
- Modify: `crates/cairn-infra/src/lib.rs:25`
- Modify: `crates/cairn-daemon/src/main.rs:139-149`

- [ ] **Step 1: Export `TauSidecar` from the tau module**

In `crates/cairn-infra/src/tau/mod.rs`, change the re-export line:

```rust
pub use config::TauConfig;
pub use runtime::TauServeRuntime;
pub use supervisor::TauSidecar;
```

- [ ] **Step 2: Re-export from the crate root**

In `crates/cairn-infra/src/lib.rs:25`, change:

```rust
pub use tau::{TauConfig, TauServeRuntime, TauSidecar};
```

- [ ] **Step 3: Swap the daemon runtime**

In `crates/cairn-daemon/src/main.rs`, replace the `Some(cfg)` arm (lines 141-144) of the runtime match:

```rust
            Some(cfg) => {
                tracing::info!("ask: tau sidecar enabled (supervised, long-lived)");
                Arc::new(cairn_infra::TauSidecar::new(cfg))
            }
```

(Leave the `None => ... NullRuntime` arm unchanged.)

- [ ] **Step 4: Build the whole workspace**

Run: `cargo build`
Expected: PASS — daemon compiles against `TauSidecar`.

- [ ] **Step 5: Run the daemon's existing tests**

Run: `cargo test -p cairn-daemon`
Expected: PASS — no regression (the `/ask` wiring is unchanged; only the concrete runtime type differs).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-infra/src/tau/mod.rs crates/cairn-infra/src/lib.rs crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): supervise a single long-lived tau serve via TauSidecar"
```

---

## Task 8: Full verification + format/lint

**Files:** none (verification only).

- [ ] **Step 1: Format**

Run: `cargo fmt --all`
Expected: no diff (or apply it).

- [ ] **Step 2: Clippy (workspace, warnings as errors)**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS. Note: the new `src/bin/tau-stub.rs` is linted too; keep it warning-clean.

- [ ] **Step 3: Full test suite**

Run: `cargo test --workspace`
Expected: PASS. The live `live_run_streams_when_tau_present` self-skips (TAU_BIN unset), keeping CI hermetic.

- [ ] **Step 4: Confirm the branch, then commit any fmt/lint fixes**

```bash
git branch --show-current   # expect: implement-tau-daemon-sidecar
git add -A
git commit -m "chore(tau): fmt + clippy clean for the sidecar supervisor" || echo "nothing to commit"
```

---

## Self-Review notes (already applied)

- **Spec coverage:** readiness timeout (Task 2), graceful shutdown (Task 3), restart/health policy + backoff (Tasks 4–5), serialized concurrency (Task 5), daemon wiring + lazy spawn (Task 7), CLI left one-shot (untouched), hermetic tests + cross-platform stub (Tasks 1, 6, 8), live test self-skips (Task 8). The `TauChannel` seam (spec §"TauChannel seam") is Task 5.
- **Deferred per spec (no task):** multiplexed runs, proactive `meta.ping`, `[tau]` TOML, SIGTERM handler — all listed as Non-goals.
- **Type consistency:** `Timeouts{ready, shutdown_grace}`, `ShutdownOutcome{Graceful,Killed}`, `TauChannel{is_alive,run_streaming}`, `TauSidecar::{new,with_spawner}`, `Backoff::{delay_before_retry,record_success,record_failure}`, and `spawn_command(Command, Timeouts)` are used identically across all tasks.
- **`CARGO_BIN_EXE_tau-stub`:** set by Cargo for the crate's integration tests because `src/bin/tau-stub.rs` is a bin target of `cairn-infra`; available in `tests/*.rs` only (not `src/` unit tests), which is why all stub-driven tests live under `tests/`.
