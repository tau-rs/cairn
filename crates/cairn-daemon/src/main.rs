//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_daemon::{build_router, AppState, CairnEngine};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "cairn-daemon",
    about = "Serve a cairn over HTTP + WebSocket on localhost"
)]
struct Cli {
    /// Path to an existing, initialized cairn.
    #[arg(long, default_value = ".")]
    cairn: PathBuf,
    /// Port to bind on 127.0.0.1.
    #[arg(long, default_value_t = 7777)]
    port: u16,
}

fn build_engine(root: &Path) -> Result<CairnEngine, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    Ok(Engine::new(store, InMemoryIndex::default(), vcs))
}

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    // `.git` is a dir in a normal repo but a file in worktrees/submodules.
    // (Duplicated in cairn-cli; de-dup if a shared startup crate appears.)
    if !cli.cairn.join(".git").exists() {
        return Err(format!(
            "not a cairn at {0} (run `cairn --cairn {0} init` first)",
            cli.cairn.display()
        ));
    }
    let mut engine = build_engine(&cli.cairn)?;
    let mut startup: Vec<Event> = Vec::new();
    engine.reindex(&mut startup).map_err(|e| e.to_string())?;

    let app = build_router(AppState::new(engine));
    let addr = format!("127.0.0.1:{}", cli.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| e.to_string())?;
    println!("cairn-daemon listening on http://{addr}");
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
