//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_daemon::{build_router, cors_layer, AppState, CairnEngine, Config};
use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex};
use cairn_ports::Watcher;
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
    /// Disable the filesystem watcher (no live events on external edits).
    #[arg(long)]
    no_watch: bool,
    /// Path to a TOML settings file (default: `<cairn>/cairn.toml` if present).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Allow a browser origin to call the daemon (CORS). Repeatable; merged
    /// with `[cors].origins` from the settings file.
    #[arg(long = "cors-origin")]
    cors_origin: Vec<String>,
}

fn build_engine(root: &Path) -> Result<CairnEngine, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    let index = TantivyIndex::in_memory().map_err(|e| e.to_string())?;
    Ok(Engine::new(store, index, vcs))
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

    let state = AppState::new(engine);

    // CORS allowlist: settings file (or default <cairn>/cairn.toml) ∪ --cors-origin.
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::load_default(&cli.cairn)?,
    };
    let cors_origins = cairn_daemon::merge_cors_origins(config.cors.origins, &cli.cors_origin);
    if cors_origins.is_empty() {
        println!(
            "CORS: no cross-origin origins allowed (add [cors].origins to {}/cairn.toml or pass --cors-origin)",
            cli.cairn.display()
        );
    } else {
        println!("CORS: allowing {}", cors_origins.join(", "));
    }

    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));

    if !cli.no_watch {
        match NotifyWatcher.watch(&cli.cairn) {
            Ok(handle) => {
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    while let Ok(change) = handle.changes.recv() {
                        watch_state.apply_change_blocking(&change);
                    }
                });
                println!("watching {} for changes", cli.cairn.display());
            }
            Err(e) => eprintln!("warning: file watcher disabled: {e}"),
        }
    }

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
