//! The `cairn-daemon` binary: serve a cairn over HTTP + WebSocket on localhost.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cairn_app::{Engine, Event};
use cairn_daemon::{build_router, cors_layer, AppState, Config};
use cairn_infra::{GitVcs, LexicalSemanticIndex, LocalFsStore, NotifyWatcher, TantivyIndex};
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
    /// Expose write tools (write_note, rename_note, delete_note, commit) on the
    /// `/mcp` route. Default off: `/mcp` is read-only unless this is set.
    #[arg(long)]
    mcp_write: bool,
}

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    ensure_cairn(&cli.cairn).map_err(|e| e.to_string())?;

    // Load config before building the engine so index settings are available.
    let config = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::load_default(&cli.cairn)?,
    };

    if config.sync.quiet_period_ms.is_some() {
        tracing::warn!("sync.quiet_period_ms is deprecated; use idle_seconds");
    }

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
        eng.set_semantic_index(Box::new(LexicalSemanticIndex::new()));
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
    if cli.mcp_write {
        tracing::info!("mcp: /mcp write tools enabled (read + write)");
    } else {
        tracing::info!("mcp: /mcp read-only (pass --mcp-write to enable note mutation)");
    }

    // Back the plugin `host/agent` callback with the same runtime as `/ask`.
    engine.set_runtime(runtime.clone());

    // The seal loop's activity channel: attached to `AppState` only when
    // auto-commit is on, so `mark_activity` is a no-op otherwise and the loop
    // (never spawned) has no dangling sender.
    let (seal_tx, seal_rx) = std::sync::mpsc::channel();
    let mut state = AppState::new(engine)
        .with_allowed_origins(cors_origins.clone())
        .with_token(token)
        .with_runtime(runtime)
        .with_mcp_write(cli.mcp_write);
    if config.sync.auto_commit {
        state = state.with_sealer(seal_tx);
    }
    let state = state;
    let app = build_router(state.clone()).layer(cors_layer(&cors_origins));

    if config.sync.auto_commit {
        let idle = config.sync.idle();
        let backstop = config.sync.backstop();
        tracing::info!(
            "seal: auto-committing sessions after {:?} idle ({:?} backstop)",
            idle,
            backstop
        );
        // `without_sealer()`, not `.clone()`: the loop must not hold its own
        // live `seal_tx`, or the sender never fully drops and the shutdown-flush
        // (`RecvTimeoutError::Disconnected`) branch of `run_seal_loop` can never
        // fire. See `AppState::without_sealer` for the full rationale.
        let sealer = state.without_sealer();
        tokio::task::spawn_blocking(move || {
            cairn_service::run_seal_loop(&seal_rx, idle, backstop, || sealer.seal_blocking());
        });
    }

    if !cli.no_watch {
        match NotifyWatcher.watch(&cli.cairn) {
            Ok(handle) => {
                let grace = Duration::from_millis(config.sync.confirm_grace_ms);
                let watch_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    cairn_service::run_watch_loop(&handle, |change| {
                        watch_state.apply_change_confirmed_blocking(change, grace);
                        watch_state.mark_activity();
                    });
                });
                tracing::info!("watching {} for changes", cli.cairn.display());
            }
            Err(e) => tracing::warn!("file watcher disabled: {e}"),
        }
    }

    // Collab commit-agent: debounce-materialize + commit sessioned notes. Runs
    // independently of the file watcher — the daemon is the sole disk writer for
    // a live session (design spec §12). Ticks every 250ms; a session commits
    // after `idle()` of no ops (or immediately once abandoned).
    {
        let flush_state = state.clone();
        let quiet = config.sync.idle();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            // A slow pass (many notes / slow git) must not fire back-to-back
            // ticks with no idle; delay the schedule instead of bursting.
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let s = flush_state.clone();
                match tokio::task::spawn_blocking(move || s.run_collab_flush_pass(quiet)).await {
                    Ok(()) => {}
                    // A panic in one pass must not kill the commit-agent for the
                    // daemon's lifetime; log and keep ticking. Only a cancelled
                    // join (runtime shutting down) ends the loop.
                    Err(e) if e.is_cancelled() => break,
                    Err(e) => tracing::error!(error = %e, "collab flush pass panicked; continuing"),
                }
            }
        });
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
