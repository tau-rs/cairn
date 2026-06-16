//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cairn_app::{Engine, Event};
use cairn_daemon::{build_router, cors_layer, AppState, Config};
use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex};
use cairn_ports::Watcher;
use cairn_startup::{build_engine, ensure_cairn};
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

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    ensure_cairn(&cli.cairn).map_err(|e| e.to_string())?;

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
        tracing::info!("persisting index at {}", index_dir.display());
        eng
    } else {
        let mut eng = build_engine(&cli.cairn).map_err(|e| e.to_string())?;
        eng.reindex(&mut startup).map_err(|e| e.to_string())?;
        tracing::info!("index: in-memory (not persisted)");
        eng
    };

    // Plugin read timeout: cairn.toml `[plugins] timeout_secs`, else the host default.
    let plugin_timeout = match config.plugins.timeout_secs {
        Some(0) => {
            tracing::warn!(
                "[plugins] timeout_secs = 0 is invalid; using default {:?}",
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
    let trusted = cairn_infra::TrustedPlugins::from_entries(
        config.plugins.trusted.iter().map(|e| e.normalize()),
    )
    .map_err(|e| format!("invalid [plugins].trusted entry in cairn.toml: {e}"))?;
    if config.plugins.trusted.is_empty() {
        tracing::info!(
            "plugins: none trusted (add [plugins].trusted = [\"<dir>\"] to {}/cairn.toml to enable)",
            cli.cairn.display()
        );
    }
    let sandbox = cairn_infra::sandbox::platform_sandbox();
    match cairn_infra::ProcessPluginHost::load_with_timeout(
        &plugins_dir,
        plugin_timeout,
        &trusted,
        sandbox.as_ref(),
    ) {
        Ok(host) => {
            engine.set_plugin_host(Box::new(host));
            tracing::info!("plugins: read timeout {plugin_timeout:?}");
        }
        Err(e) => tracing::warn!("plugin host disabled: {e}"),
    }

    // CORS allowlist: settings file (or default <cairn>/cairn.toml) ∪ --cors-origin.
    let cors_origins = cairn_daemon::merge_cors_origins(config.cors.origins, &cli.cors_origin);
    if cors_origins.is_empty() {
        tracing::info!(
            "CORS: no cross-origin origins allowed (add [cors].origins to {}/cairn.toml or pass --cors-origin)",
            cli.cairn.display()
        );
    } else {
        tracing::info!("CORS: allowing {}", cors_origins.join(", "));
    }

    // Local bearer token: written to <cairn>/.cairn/token (mode 0600) and
    // regenerated each startup. Any client with filesystem access to the cairn
    // reads it and sends `Authorization: Bearer <token>` (audit S5). A write
    // failure is fatal — the daemon never serves unauthenticated.
    let token = cairn_daemon::generate_token_file(&cli.cairn)
        .map_err(|e| format!("write daemon token: {e}"))?;
    tracing::info!(
        "auth: bearer token at {}/.cairn/token (clients read this file)",
        cli.cairn.display()
    );

    // Agent runtime for `POST /ask`: tau when configured, else NullRuntime (which
    // errors until TAU_BIN is set). Mirrors the CLI's `cairn ask` wiring.
    let runtime: Arc<dyn cairn_ports::AgentRuntime + Send + Sync> =
        match cairn_infra::TauConfig::from_env() {
            Some(cfg) => {
                tracing::info!("ask: tau sidecar enabled (supervised, long-lived)");
                Arc::new(cairn_infra::TauSidecar::new(cfg))
            }
            None => {
                tracing::info!("ask: no TAU_BIN; /ask returns a configuration error");
                Arc::new(cairn_infra::NullRuntime)
            }
        };

    // The same allowlist gates the /events WS upgrade (browsers bypass CORS on
    // WebSocket handshakes; see events_handler).
    let state = AppState::new(engine)
        .with_allowed_origins(cors_origins.clone())
        .with_token(token)
        .with_runtime(runtime);
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
                tracing::info!("watching {} for changes", cli.cairn.display());
            }
            Err(e) => tracing::warn!("file watcher disabled: {e}"),
        }
    }

    let addr = format!("127.0.0.1:{}", cli.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("cairn-daemon listening on http://{addr}");
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    // Default to `info`, but quiet tantivy's per-commit index chatter (it logs
    // each segment commit/GC at info) so cairn's own logs aren't buried. Any
    // `RUST_LOG` value fully overrides this default.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tantivy=warn")),
        )
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
