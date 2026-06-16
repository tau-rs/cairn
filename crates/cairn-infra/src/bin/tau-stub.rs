//! Test-only stand-in for `tau serve` (selected by argv[1]). Not shipped: the
//! release job builds only `--bin cairn --bin cairn-daemon` and packages just
//! those (`.github/workflows/heavy.yml`); this bin exists solely so integration
//! tests can locate it via `CARGO_BIN_EXE_tau-stub`.
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
    // ready-run: fall through and exit gracefully.
}
