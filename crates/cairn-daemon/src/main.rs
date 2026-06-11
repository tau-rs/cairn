//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

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
    /// Disable the on-disk index (use an ephemeral in-memory index).
    #[arg(long)]
    no_persist: bool,
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

    // Load config before building the engine so index settings are available.
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::load_default(&cli.cairn)?,
    };

    let mut startup: Vec<Event> = Vec::new();
    let persist = config.index.persist && !cli.no_persist;
    let mut engine = if persist {
        let index_dir = config
            .index
            .path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| cli.cairn.join(".cairn").join("index"));
        cairn_infra::ensure_cairn_dir(&cli.cairn).map_err(|e| e.to_string())?;
        let store = LocalFsStore::open(&cli.cairn).map_err(|e| e.to_string())?;
        let vcs = GitVcs::open_or_init(&cli.cairn).map_err(|e| e.to_string())?;
        let index = TantivyIndex::open_at(&index_dir).map_err(|e| e.to_string())?;
        let mut eng = Engine::new(store, index, vcs);
        eng.reconcile(&mut startup).map_err(|e| e.to_string())?;
        println!("persisting index at {}", index_dir.display());
        eng
    } else {
        let mut eng = build_engine(&cli.cairn)?;
        eng.reindex(&mut startup).map_err(|e| e.to_string())?;
        println!("index: in-memory (not persisted)");
        eng
    };

    // Plugin read timeout: cairn.toml `[plugins] timeout_secs`, else the host default.
    let plugin_timeout = match config.plugins.timeout_secs {
        Some(0) => {
            eprintln!(
                "warning: [plugins] timeout_secs = 0 is invalid; using default {:?}",
                cairn_infra::DEFAULT_PLUGIN_TIMEOUT
            );
            cairn_infra::DEFAULT_PLUGIN_TIMEOUT
        }
        Some(s) => Duration::from_secs(s),
        None => cairn_infra::DEFAULT_PLUGIN_TIMEOUT,
    };
    // Load engine plugins from <cairn>/.cairn/plugins (absent dir => none).
    // Default-deny: only directories listed in [plugins].trusted are spawned.
    let plugins_dir = cli.cairn.join(".cairn").join("plugins");
    let trusted = cairn_infra::TrustedPlugins::from_ids(config.plugins.trusted.clone());
    if config.plugins.trusted.is_empty() {
        println!(
            "plugins: none trusted (add [plugins].trusted = [\"<dir>\"] to {}/cairn.toml to enable)",
            cli.cairn.display()
        );
    }
    match cairn_infra::ProcessPluginHost::load_with_timeout(&plugins_dir, plugin_timeout, &trusted)
    {
        Ok(host) => {
            engine.set_plugin_host(Box::new(host));
            println!("plugins: read timeout {plugin_timeout:?}");
        }
        Err(e) => eprintln!("warning: plugin host disabled: {e}"),
    }

    // CORS allowlist: settings file (or default <cairn>/cairn.toml) ∪ --cors-origin.
    let cors_origins = cairn_daemon::merge_cors_origins(config.cors.origins, &cli.cors_origin);
    if cors_origins.is_empty() {
        println!(
            "CORS: no cross-origin origins allowed (add [cors].origins to {}/cairn.toml or pass --cors-origin)",
            cli.cairn.display()
        );
    } else {
        println!("CORS: allowing {}", cors_origins.join(", "));
    }

    // Local bearer token: written to <cairn>/.cairn/token (mode 0600) and
    // regenerated each startup. Any client with filesystem access to the cairn
    // reads it and sends `Authorization: Bearer <token>` (audit S5). A write
    // failure is fatal — the daemon never serves unauthenticated.
    let token = cairn_daemon::generate_token_file(&cli.cairn)
        .map_err(|e| format!("write daemon token: {e}"))?;
    println!(
        "auth: bearer token at {}/.cairn/token (clients read this file)",
        cli.cairn.display()
    );

    // The same allowlist gates the /events WS upgrade (browsers bypass CORS on
    // WebSocket handshakes; see events_handler).
    let state = AppState::new(engine)
        .with_allowed_origins(cors_origins.clone())
        .with_token(token);
    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));

    if !cli.no_watch {
        match NotifyWatcher.watch(&cli.cairn) {
            Ok(handle) => {
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    cairn_service::run_watch_loop(&handle, |change| {
                        watch_state.apply_change_blocking(change)
                    });
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
