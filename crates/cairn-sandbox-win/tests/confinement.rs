//! Windows-only behavioral tests for the AppContainer launcher. Each spawns the
//! built launcher binary (`CARGO_BIN_EXE_cairn-sandbox-win`) and asserts the
//! confinement guarantee. Skipped unless AppContainer is available on the host.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn-sandbox-win"))
}

/// True if `--probe` succeeds (AppContainer usable on this host/runner).
fn appcontainer_usable() -> bool {
    Command::new(launcher())
        .arg("--probe")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn probe_reports_appcontainer_available() {
    // GitHub windows-latest supports AppContainer; assert the probe succeeds.
    assert!(
        appcontainer_usable(),
        "--probe must succeed on a runner with AppContainer"
    );
}
