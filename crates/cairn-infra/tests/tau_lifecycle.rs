//! Integration tests for the `tau-stub` helper and (from a later task) the
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
