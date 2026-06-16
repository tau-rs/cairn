//! Integration tests for the `tau-stub` helper and (from a later task) the
//! `TauServe` process primitive. The stub binary is located via CARGO_BIN_EXE.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cairn_infra::tau::process::{ShutdownOutcome, TauServe, Timeouts};

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

#[test]
fn readiness_read_times_out_for_silent_tau() {
    // A tau that starts but never writes its readiness line must not hang spawn:
    // the bounded read fires and the child is reaped.
    let mut cmd = Command::new(stub());
    cmd.arg("no-ready");
    let timeouts = Timeouts {
        ready: Duration::from_millis(200),
        shutdown_grace: Duration::from_millis(200),
    };
    let start = Instant::now();
    let err = TauServe::spawn_command(cmd, timeouts).expect_err("must time out");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "returned promptly"
    );
    assert!(
        err.to_string().contains("readiness"),
        "error names the readiness wait: {err}"
    );
}

#[test]
fn shutdown_is_graceful_when_child_exits_on_eof() {
    let mut cmd = Command::new(stub());
    cmd.arg("ready-run"); // exits when stdin closes
    let mut serve = TauServe::spawn_command(cmd, Timeouts::default()).expect("spawn");
    assert_eq!(serve.shutdown(), ShutdownOutcome::Graceful);
}

#[test]
fn shutdown_kills_child_that_ignores_eof() {
    let mut cmd = Command::new(stub());
    cmd.arg("no-exit"); // answers handshake, then never exits
    let timeouts = Timeouts {
        ready: Duration::from_secs(5),
        // The stub never exits, so `Killed` is guaranteed regardless; keep the
        // grace short so the test does not idle the full window on slow CI.
        shutdown_grace: Duration::from_millis(50),
    };
    let mut serve = TauServe::spawn_command(cmd, timeouts).expect("spawn");
    assert_eq!(serve.shutdown(), ShutdownOutcome::Killed);
}

#[test]
fn is_alive_tracks_the_child() {
    let mut cmd = Command::new(stub());
    cmd.arg("ready-run");
    let mut serve = TauServe::spawn_command(cmd, Timeouts::default()).expect("spawn");
    assert!(serve.is_alive());
    assert_eq!(serve.shutdown(), ShutdownOutcome::Graceful);
    assert!(!serve.is_alive(), "dead after shutdown");
}
