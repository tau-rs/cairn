//! AppContainer launcher for trusted cairn plugins on Windows.
//!
//! Invoked by `cairn-infra`'s `WindowsAppContainerSandbox` exactly like
//! `bwrap`/`sandbox-exec`:
//!   cairn-sandbox-win --probe
//!   cairn-sandbox-win --plugin-dir <dir> -- <cmd> [<args>...]
//!
//! On non-Windows targets this is a stub that always fails, so the workspace
//! still builds everywhere while the real jail lives behind `cfg(windows)`.

#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("cairn-sandbox-win: AppContainer launcher is Windows-only");
    std::process::ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    win::run()
}
