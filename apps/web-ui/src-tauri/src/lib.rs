mod plugin_protocol;
mod plugin_roots;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    AnswerEvent, AskRequest, Command, CommandResponse, ContractError, Query, QueryResponse,
};
use cairn_infra::{
    ensure_cairn_dir, GitVcs, LocalFsStore, NotifyWatcher, NullRuntime, SyncConfig, TantivyIndex,
    TauConfig, TauServeRuntime,
};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink, FsChange, Watcher};
use cairn_service::{
    agent_event_to_wire, app_event_to_wire, dispatch_command, dispatch_query,
    gather_answer_context, run_seal_loop, SealSignal, ServiceError,
};
use tauri::http::Response;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use plugin_roots::{set_plugin_ui_roots, PluginRoots};

/// The concrete engine the desktop app runs. `Engine` boxes its adapters, so the
/// type alias is just a readable name for the one engine this shell builds.
type CairnEngine = Engine;

/// Shared app state: the engine + its path behind a single mutex (None until a
/// cairn is opened), plus the agent runtime backing `ask`. `Clone` lets us move
/// the `Arc`s into `spawn_blocking`.
#[derive(Clone)]
struct CairnState {
    inner: Arc<Mutex<Option<(CairnEngine, PathBuf)>>>,
    /// Agent runtime for `ask`. Built from the environment at startup:
    /// `TauServeRuntime` when `TAU_BIN` is set, else `NullRuntime` (which errors
    /// at answer time, surfaced as a single `Failed` frame).
    runtime: Arc<dyn AgentRuntime + Send + Sync>,
    /// The live file-watcher for the open cairn, if any. Replaced on every
    /// open/switch (the prior one is stopped + joined first) so exactly one
    /// watcher runs at a time.
    watcher: Arc<Mutex<Option<WatcherHandle>>>,
    /// The live seal loop for the open cairn, if any. `None` when no cairn is
    /// open or the cairn set `[sync] auto_commit = false`, in which case
    /// [`CairnState::mark_activity`] is a no-op. Same lifecycle as `watcher`.
    sealer: Arc<Mutex<Option<SealerHandle>>>,
}

/// A running file-watcher thread: the stop flag its loop polls each ~250ms, plus
/// the join handle so a cairn switch/close can wait for the OS watcher to be
/// fully released before the next cairn opens.
struct WatcherHandle {
    stop: Arc<AtomicBool>,
    join: std::thread::JoinHandle<()>,
}

/// A running seal-loop thread: the activity channel commands mark on every edit,
/// plus the join handle. There is no stop flag — dropping `tx` is the stop
/// signal, and it is also the *flush* signal: `run_seal_loop` treats a
/// disconnected channel as shutdown and seals any session still open before it
/// returns. `tx` therefore must not be cloned into the loop's own thread, or the
/// sender would never fully drop and that flush could never fire.
struct SealerHandle {
    tx: std::sync::mpsc::Sender<SealSignal>,
    join: std::thread::JoinHandle<()>,
}

/// Build the agent runtime from the process environment. `TAU_BIN` (+ optional
/// `TAU_AGENT`/`TAU_PROJECT`) selects the real `tau serve` runtime; absent, the
/// `NullRuntime` placeholder errors at answer time.
fn runtime_from_env() -> Arc<dyn AgentRuntime + Send + Sync> {
    match TauConfig::from_env() {
        Some(cfg) => Arc::new(TauServeRuntime::new(cfg)),
        None => Arc::new(NullRuntime),
    }
}

impl Default for CairnState {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            runtime: runtime_from_env(),
            watcher: Arc::default(),
            sealer: Arc::default(),
        }
    }
}

/// Whether a *successful* command counts as editing-session activity.
///
/// `Commit` and `NameVersion` are excluded because they *are* the seal: counting
/// them would re-open the session the seal just closed, and the loop would then
/// commit on every idle window forever. Mirrors `cairn-daemon`'s
/// `run_command_blocking`, so both transports open and close a session on
/// exactly the same set of commands.
fn is_session_activity(command: &Command) -> bool {
    !matches!(
        command,
        Command::Commit { .. } | Command::NameVersion { .. }
    )
}

impl CairnState {
    /// Signal the seal loop that an editing session saw activity. A no-op when
    /// no sealer is attached (no cairn open, or auto-commit off). Safe to
    /// over-send: extra signals only push the idle deadline out, and a seal that
    /// finds a clean tree is `NothingToCommit`, not an error.
    fn mark_activity(&self) {
        let guard = self.sealer.lock().expect("sealer mutex poisoned");
        if let Some(sealer) = guard.as_ref() {
            if sealer.tx.send(SealSignal::Activity).is_err() {
                eprintln!("cairn: seal loop gone; auto-commit inactive");
            }
        }
    }
}

/// An `EventSink` that forwards engine events to the webview as wire events.
struct TauriSink<R: Runtime>(AppHandle<R>);
impl<R: Runtime> EventSink for TauriSink<R> {
    fn emit(&mut self, event: AppEvent) {
        let _ = self.0.emit("cairn://event", app_event_to_wire(event));
    }
}

/// Build (or open) an engine rooted at `dir`, creating the git repo if needed.
///
/// The Tantivy index is persisted on disk under `<dir>/.cairn/index` (§3), so a
/// reopen restores the prior index instead of rebuilding it from scratch. That
/// index has a single exclusive writer: this process must be the sole writer for
/// the open cairn (don't run `cairn-daemon` against the same cairn), and the
/// engine must be dropped on cairn-close/switch to release the lock.
fn open_engine(dir: &Path) -> Result<CairnEngine, ServiceError> {
    let store =
        LocalFsStore::open(dir).map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    let vcs =
        GitVcs::open_or_init(dir).map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    // Create <dir>/.cairn/ (auto-gitignored) and open the persistent index there.
    let cairn_dir =
        ensure_cairn_dir(dir).map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    let index = TantivyIndex::open_at(&cairn_dir.join("index"))
        .map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    Ok(Engine::new(store, index, vcs))
}

fn run_command_blocking<R: Runtime>(
    state: &CairnState,
    app: &AppHandle<R>,
    command: &Command,
) -> Result<CommandResponse, ServiceError> {
    let result = {
        let mut guard = state.inner.lock().expect("engine mutex poisoned");
        let Some((engine, _path)) = guard.as_mut() else {
            return Err(ServiceError::InvalidRequest("no cairn open".into()));
        };
        let mut sink = TauriSink(app.clone());
        dispatch_command(engine, command, &mut sink)
        // The engine lock is released here, before `mark_activity` — the seal
        // thread takes that same lock in `seal_open_cairn`.
    };
    if result.is_ok() && is_session_activity(command) {
        state.mark_activity();
    }
    result
}

fn run_query_blocking(state: &CairnState, query: &Query) -> Result<QueryResponse, ServiceError> {
    let guard = state.inner.lock().expect("engine mutex poisoned");
    let (engine, _path) = guard
        .as_ref()
        .ok_or_else(|| ServiceError::InvalidRequest("no cairn open".into()))?;
    dispatch_query(engine, query)
}

#[tauri::command]
async fn send_command<R: Runtime>(
    state: State<'_, CairnState>,
    app: AppHandle<R>,
    command: Command,
) -> Result<CommandResponse, ContractError> {
    let state = (*state).clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_command_blocking(&state, &app, &command).map_err(ContractError::from)
    })
    .await
    .map_err(|e| ContractError::Internal {
        message: e.to_string(),
    })?
}

/// Stream a note-grounded answer into `emit`: gather retrieval context under the
/// engine lock, emit the leading `Sources` frame, then run the agent lock-free
/// and forward each increment as a wire `AnswerEvent`. Mirrors the daemon's
/// `ask_handler`. A gather failure (no cairn open, search error) returns `Err`
/// before any frame; an agent failure is delivered as a terminal `Failed` frame.
fn stream_answer(
    state: &CairnState,
    req: &AskRequest,
    emit: &mut dyn FnMut(AnswerEvent),
) -> Result<(), ServiceError> {
    let top_k = req.top_k.unwrap_or(5);
    // Gather retrieval context under the engine lock, then drop the guard before
    // the (seconds-long) agent run so it never blocks commands/queries.
    let (prompt, cited) = {
        let guard = state.inner.lock().expect("engine mutex poisoned");
        let (engine, _path) = guard
            .as_ref()
            .ok_or_else(|| ServiceError::InvalidRequest("no cairn open".into()))?;
        gather_answer_context(engine, &req.query, top_k)?
    };
    emit(AnswerEvent::Sources { paths: cited });

    // Adapt the agent's `AgentEvent` stream into wire `AnswerEvent` frames.
    struct WireSink<'a> {
        emit: &'a mut dyn FnMut(AnswerEvent),
    }
    impl AgentSink for WireSink<'_> {
        fn emit(&mut self, event: AgentEvent) {
            if let Some(wire) = agent_event_to_wire(event) {
                (self.emit)(wire);
            }
        }
    }
    let mut sink = WireSink { emit };
    // A run that starts then fails reports via `AgentEvent::Failed` on the sink;
    // an `Err` means it failed before any event (e.g. tau not configured) —
    // route it through the same mapping so the UI sees one terminal `Failed`.
    if let Err(e) = state.runtime.answer(&prompt, &mut sink) {
        sink.emit(AgentEvent::Failed {
            message: e.to_string(),
        });
    }
    Ok(())
}

#[tauri::command]
async fn ask(
    state: State<'_, CairnState>,
    request: AskRequest,
    channel: tauri::ipc::Channel<AnswerEvent>,
) -> Result<(), ContractError> {
    let state = (*state).clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = |e: AnswerEvent| {
            // A closed channel (webview dropped the stream) turns sends into
            // no-ops; the run still finishes (no v1 cancellation), matching the
            // daemon's fire-and-forget producer.
            let _ = channel.send(e);
        };
        stream_answer(&state, &request, &mut emit).map_err(ContractError::from)
    })
    .await
    .map_err(|e| ContractError::Internal {
        message: e.to_string(),
    })?
}

// No R: Runtime generic — queries are read-only and never emit events.
#[tauri::command]
async fn run_query(
    state: State<'_, CairnState>,
    query: Query,
) -> Result<QueryResponse, ContractError> {
    let state = (*state).clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_query_blocking(&state, &query).map_err(ContractError::from)
    })
    .await
    .map_err(|e| ContractError::Internal {
        message: e.to_string(),
    })?
}

/// Open a cairn at `dir`: build the engine, reindex (emitting events), record
/// engine+path atomically in state, and persist the path for next launch.
fn open_at<R: Runtime>(
    state: &CairnState,
    app: &AppHandle<R>,
    dir: &Path,
) -> Result<(), ServiceError> {
    // Stop any running watcher and drop the current engine BEFORE building the
    // new one: the on-disk index has a single exclusive writer, so the prior
    // engine must release it first (this also covers same-dir reopen). A failed
    // open below therefore leaves an empty state — acceptable, and matches the
    // best-effort startup path.
    //
    // Order is load-bearing: watcher first (nothing can mark activity after
    // it stops), then the sealer, which FLUSHES the outgoing cairn's open
    // session — and must do so while that cairn's engine is still the one in
    // `inner`, or the seal would land in the incoming cairn instead.
    stop_watcher(state);
    stop_sealer(state);
    *state.inner.lock().expect("engine mutex poisoned") = None;

    let mut engine = open_engine(dir)?;
    let mut sink = TauriSink(app.clone());
    // reconcile (§3): load <dir>/.cairn/state.json, stat-diff against disk, and
    // re-index only what changed since last run (full build the first time).
    engine
        .reconcile(&mut sink)
        .map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    *state.inner.lock().expect("engine mutex poisoned") = Some((engine, dir.to_path_buf()));

    // Start the seal loop (#175) before the watcher, so a change detected the
    // instant watching begins already has somewhere to signal.
    *state.sealer.lock().expect("sealer mutex poisoned") = spawn_sealer(state, app, dir);

    // Start watching for external edits (§4). A failure here only means no live
    // updates — never that opening fails.
    match spawn_watcher(state, app, dir) {
        Ok(w) => *state.watcher.lock().expect("watcher mutex poisoned") = Some(w),
        Err(e) => eprintln!("cairn: failed to start watcher for {dir:?}: {e}"),
    }
    if let Err(e) = persist_path(app, dir) {
        eprintln!("cairn: failed to persist cairn path: {e}"); // non-fatal
    }
    // Scope the asset protocol to exactly this cairn dir so local note images
    // (asset://) can load (S4). The static config scope is empty; this is the
    // only widening, and only ever to a user-chosen cairn root — never $APP or
    // home. Grants are additive across opens (Tauri's scope API has no per-open
    // reset), so a prior cairn's dir stays readable for the process lifetime;
    // acceptable since every grant is a directory the user explicitly opened.
    // Best-effort: a failure only means images won't render, never that opening
    // fails.
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("cairn: failed to scope asset protocol to {dir:?}: {e}");
    }
    Ok(())
}

/// Drive a watch loop until `stop` is set (or the watcher disconnects), forwarding
/// each detected change to `on_change`. Polls with a short timeout so the stop
/// flag is honoured within ~250ms. The daemon's `run_watch_loop` blocks on
/// `recv()` forever (fine — one cairn, never stops); this app opens *different*
/// cairns over its life and must stop the watcher on switch/close, hence the
/// stoppable variant.
fn run_stoppable_watch(
    changes: &std::sync::mpsc::Receiver<FsChange>,
    stop: &AtomicBool,
    mut on_change: impl FnMut(&FsChange),
) {
    use std::sync::mpsc::RecvTimeoutError;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match changes.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(change) => on_change(&change),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Apply one detected filesystem change to the currently-open engine (if any),
/// emitting `note_changed`/`note_deleted` through `sink`. Holds the engine lock
/// only for this one change — never across the blocking receive — so it can't
/// stall `send_command`/`run_query`. A no-op when no cairn is open (a change can
/// race an in-flight cairn switch). `apply_change` is idempotent and stat-guarded,
/// so echoes of our own writes and spurious events are cheap no-ops.
fn apply_fs_change(
    inner: &Arc<Mutex<Option<(CairnEngine, PathBuf)>>>,
    change: &FsChange,
    sink: &mut dyn EventSink,
) {
    if let Ok(mut guard) = inner.lock() {
        if let Some((engine, _)) = guard.as_mut() {
            let _ = engine.apply_change(change, sink);
        }
    }
}

/// Stop and join the running watcher (if any). Setting the flag ends the loop
/// within ~250ms; joining guarantees the OS watcher is fully released before the
/// next cairn's engine (and its exclusive on-disk index writer) is built.
fn stop_watcher(state: &CairnState) {
    let handle = state.watcher.lock().expect("watcher mutex poisoned").take();
    if let Some(w) = handle {
        w.stop.store(true, Ordering::Relaxed);
        let _ = w.join.join();
    }
}

/// Spawn the stoppable watch thread for `dir`, routing each detected change into
/// the open engine via `apply_fs_change` (which emits `note_changed`/`deleted`
/// through the webview sink). Returns the handle to record in state.
fn spawn_watcher<R: Runtime>(
    state: &CairnState,
    app: &AppHandle<R>,
    dir: &Path,
) -> Result<WatcherHandle, ServiceError> {
    let handle = NotifyWatcher
        .watch(dir)
        .map_err(|e| ServiceError::Internal(e.to_string().into()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let watch_state = state.clone();
    let app = app.clone();
    let stop_thread = stop.clone();
    let join = std::thread::spawn(move || {
        run_stoppable_watch(&handle.changes, &stop_thread, |change| {
            let mut sink = TauriSink(app.clone());
            apply_fs_change(&watch_state.inner, change, &mut sink);
            // An edit made outside the app is still an editing session: sealing
            // is source-agnostic (ADR-0012), so the watcher marks activity for
            // the same reason the daemon's does. Marked unconditionally — an
            // echo of our own write only extends the idle window.
            watch_state.mark_activity();
        });
        drop(handle); // release the OS watcher
    });
    Ok(WatcherHandle { stop, join })
}

/// Commit whatever is uncommitted in the open cairn with an engine-generated
/// message — the seal loop's `on_seal`. Best-effort by design: a clean tree is
/// `NothingToCommit` (success), and a real failure is logged rather than
/// propagated, because there is no user action to fail. A no-op when no cairn is
/// open, which a seal racing a cairn switch can see.
fn seal_open_cairn<R: Runtime>(
    inner: &Arc<Mutex<Option<(CairnEngine, PathBuf)>>>,
    app: &AppHandle<R>,
) {
    let mut guard = inner.lock().expect("engine mutex poisoned");
    let Some((engine, _path)) = guard.as_mut() else {
        return;
    };
    let mut sink = TauriSink(app.clone());
    match dispatch_command(engine, &Command::Commit { message: None }, &mut sink) {
        Ok(_) => {}
        Err(e) => eprintln!("cairn: auto-commit failed: {e}"),
    }
}

/// Stop the running seal loop (if any), flushing any session still open.
///
/// Taking the handle drops its `tx`, which disconnects the channel;
/// `run_seal_loop` reads that as shutdown and seals once more before returning,
/// so the join below is what guarantees the flush actually landed. Call it
/// while the cairn being flushed is still the one in `inner`.
fn stop_sealer(state: &CairnState) {
    let handle = state.sealer.lock().expect("sealer mutex poisoned").take();
    if let Some(SealerHandle { tx, join }) = handle {
        drop(tx);
        let _ = join.join();
    }
}

/// Spawn the seal loop for `dir`, or `None` if this cairn has auto-commit off.
///
/// The policy numbers come from `<dir>/cairn.toml` `[sync]` — the same schema
/// and the same defaults (2s idle, 20min backstop) the daemon reads, so a cairn
/// behaves identically on both transports.
///
/// A malformed `[sync]` disables auto-commit rather than falling back to the
/// defaults: the file may well have been trying to say `auto_commit = false`,
/// and writing to someone's git history because we could not parse their
/// opt-out is the worse of the two failures.
fn spawn_sealer<R: Runtime>(
    state: &CairnState,
    app: &AppHandle<R>,
    dir: &Path,
) -> Option<SealerHandle> {
    let config = match SyncConfig::load(dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cairn: {e}; auto-commit disabled for this cairn");
            return None;
        }
    };
    if !config.auto_commit {
        return None;
    }
    let (idle, backstop) = (config.idle(), config.backstop());
    // Unbounded channel: `mark_activity` is called under no lock we care about
    // and must never block a command or the watcher thread.
    let (tx, rx) = std::sync::mpsc::channel();
    let inner = state.inner.clone();
    let app = app.clone();
    // The loop's thread deliberately captures no `Sender` — see `SealerHandle`.
    let join = std::thread::spawn(move || {
        run_seal_loop(&rx, idle, backstop, || seal_open_cairn(&inner, &app));
    });
    Some(SealerHandle { tx, join })
}

fn config_file<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("last-cairn.txt"))
}

fn persist_path<R: Runtime>(app: &AppHandle<R>, dir: &Path) -> std::io::Result<()> {
    let Some(f) = config_file(app) else {
        return Ok(());
    };
    if let Some(parent) = f.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = dir.to_str().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path is not valid UTF-8")
    })?;
    std::fs::write(f, s.as_bytes())
}

fn last_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let f = config_file(app)?;
    let s = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(s.trim());
    p.is_dir().then_some(p)
}

#[tauri::command]
async fn pick_and_open_cairn<R: Runtime>(
    state: State<'_, CairnState>,
    app: AppHandle<R>,
) -> Result<Option<String>, ContractError> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let Some(folder) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let dir = folder.into_path().map_err(|e| ContractError::Internal {
            message: e.to_string(),
        })?;
        let state = (*state).clone();
        let app2 = app.clone();
        let dir2 = dir.clone();
        tauri::async_runtime::spawn_blocking(move || {
            open_at(&state, &app2, &dir2).map_err(ContractError::from)
        })
        .await
        .map_err(|e| ContractError::Internal {
            message: e.to_string(),
        })??;
        Ok(Some(dir.to_string_lossy().into_owned()))
    }
    #[cfg(mobile)]
    {
        // Mobile open-a-cairn (document picker / SAF + git-on-device) is deferred.
        let _ = (&state, &app);
        Ok(None)
    }
}

#[tauri::command]
fn current_cairn(state: State<'_, CairnState>) -> Option<String> {
    state
        .inner
        .lock()
        .expect("engine mutex poisoned")
        .as_ref()
        .map(|(_, p)| p.to_string_lossy().into_owned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(CairnState::default())
        .manage(PluginRoots::default())
        // Tier-3 6b: serve each plugin's on-disk ui/ bundle at a distinct
        // opaque origin (plugin-sandbox://<id>/...). The handler is a thin
        // adapter over the unit-tested `plugin_protocol` core: id→root lookup
        // against the registered allow-list, strict path-canonicalisation, MIME,
        // and a locked-down per-frame CSP on every response.
        .register_uri_scheme_protocol("plugin-sandbox", |ctx, request| {
            let id = request.uri().host().unwrap_or("").to_string();
            let rel = request.uri().path().trim_start_matches('/').to_string();
            let roots = ctx.app_handle().state::<PluginRoots>();
            let (status, mime, body) = plugin_protocol::build_response(&roots, &id, &rel);
            Response::builder()
                .status(status)
                .header("Content-Type", mime)
                .header("Content-Security-Policy", plugin_protocol::plugin_csp())
                .header("X-Content-Type-Options", "nosniff")
                .body(body)
                .unwrap()
        })
        .setup(|app| {
            let handle = app.handle().clone();
            if let Some(dir) = last_path(&handle) {
                let state = app.state::<CairnState>();
                let _ = open_at(&state, &handle, &dir); // best-effort; empty state on failure
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_command,
            run_query,
            ask,
            pick_and_open_cairn,
            current_cairn,
            set_plugin_ui_roots
        ])
        .build(tauri::generate_context!())
        .expect("error while running cairn")
        // `.build(..).run(cb)` rather than `.run(..)`: quitting must flush the
        // open editing session. A quit fires neither of the UI's seal hints
        // (note switch, window blur), so without this the words are on disk —
        // autosave saw to that — but the session that produced them never
        // becomes a version, and the last work of every session is missing from
        // the history the Versions panel shows.
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let state = app.state::<CairnState>();
                stop_watcher(&state);
                stop_sealer(&state);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_contract::{Command, Query, QueryResponse};
    use tauri::Manager;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(CairnState::default())
            .invoke_handler(tauri::generate_handler![send_command, run_query])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app")
    }

    #[test]
    fn open_engine_then_write_and_query() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = open_engine(tmp.path()).expect("open");
        let mut sink: Vec<cairn_app::Event> = Vec::new();
        cairn_service::dispatch_command(
            &mut engine,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: "hello [[b]]".into(),
            },
            &mut sink,
        )
        .unwrap();
        let got = cairn_service::dispatch_query(
            &engine,
            &Query::GetNote {
                path: "a.md".into(),
            },
        )
        .unwrap();
        assert_eq!(
            got,
            QueryResponse::Note {
                contents: "hello [[b]]".into()
            }
        );
    }

    #[test]
    fn on_disk_index_persists_across_reopen_without_reindex() {
        // §3 persistence: the Tantivy index lives under <dir>/.cairn/index, so a
        // note written in one engine instance is still searchable after the
        // engine is dropped and re-opened — with NO reindex/reconcile call. On
        // the old in-memory index a reopen starts empty and this search misses.
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut engine = open_engine(tmp.path()).expect("open");
            let mut sink: Vec<cairn_app::Event> = Vec::new();
            cairn_service::dispatch_command(
                &mut engine,
                &Command::WriteNote {
                    path: "a.md".into(),
                    contents: "persisted body".into(),
                },
                &mut sink,
            )
            .unwrap();
        } // drop the engine → releases the on-disk index write lock
        let engine = open_engine(tmp.path()).expect("reopen");
        let got = cairn_service::dispatch_query(
            &engine,
            &Query::Search {
                query: "persisted".into(),
            },
        )
        .unwrap();
        match got {
            QueryResponse::SearchResults { results } => assert!(
                results.iter().any(|r| r.path == "a.md"),
                "reopened on-disk index should still find the persisted note"
            ),
            other => panic!("expected SearchResults, got {other:?}"),
        }
    }

    #[test]
    fn command_without_open_cairn_errors() {
        let app = test_app();
        let state = (*app.state::<CairnState>()).clone();
        let res = run_command_blocking(
            &state,
            &app.handle().clone(),
            &Command::Commit {
                message: Some("x".into()),
            },
        );
        assert!(matches!(res, Err(ServiceError::InvalidRequest(_))));
    }

    #[test]
    fn query_after_open_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state = (*app.state::<CairnState>()).clone();
        *state.inner.lock().unwrap() =
            Some((open_engine(tmp.path()).unwrap(), tmp.path().to_path_buf()));
        run_command_blocking(
            &state,
            &app.handle().clone(),
            &Command::WriteNote {
                path: "n.md".into(),
                contents: "body".into(),
            },
        )
        .unwrap();
        let r = run_query_blocking(
            &state,
            &Query::Search {
                query: "body".into(),
            },
        )
        .unwrap();
        // The Tantivy-backed engine answers Search with rich SearchResults
        // (path + score + snippet), not bare Paths.
        match r {
            QueryResponse::SearchResults { results } => assert_eq!(
                results.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(),
                vec!["n.md"]
            ),
            other => panic!("expected SearchResults, got {other:?}"),
        }
    }

    /// An `AgentRuntime` that emits a scripted sequence of `AgentEvent`s then
    /// succeeds — stands in for a real `tau serve` run.
    struct ScriptedRuntime(Vec<AgentEvent>);
    impl AgentRuntime for ScriptedRuntime {
        fn answer(
            &self,
            _prompt: &str,
            sink: &mut dyn AgentSink,
        ) -> Result<(), cairn_ports::PortError> {
            for e in &self.0 {
                sink.emit(e.clone());
            }
            Ok(())
        }
    }

    fn open_state(dir: &Path, runtime: Arc<dyn AgentRuntime + Send + Sync>) -> CairnState {
        let engine = open_engine(dir).unwrap();
        CairnState {
            inner: Arc::new(Mutex::new(Some((engine, dir.to_path_buf())))),
            runtime,
            watcher: Arc::default(),
            sealer: Arc::default(),
        }
    }

    #[test]
    fn ask_streams_sources_then_mapped_answer_events() {
        let tmp = tempfile::tempdir().unwrap();
        let state = open_state(
            tmp.path(),
            Arc::new(ScriptedRuntime(vec![
                AgentEvent::TextDelta("Hel".into()),
                AgentEvent::TextDelta("lo".into()),
                AgentEvent::Completed,
            ])),
        );
        let mut got: Vec<AnswerEvent> = Vec::new();
        stream_answer(
            &state,
            &AskRequest {
                query: "anything".into(),
                top_k: None,
            },
            &mut |e| got.push(e),
        )
        .unwrap();
        // First frame is always Sources (possibly empty), then the agent run.
        assert!(matches!(got.first(), Some(AnswerEvent::Sources { .. })));
        assert_eq!(
            &got[1..],
            &[
                AnswerEvent::TextDelta { text: "Hel".into() },
                AnswerEvent::TextDelta { text: "lo".into() },
                AnswerEvent::Completed,
            ]
        );
    }

    #[test]
    fn ask_without_open_cairn_errs_before_any_frame() {
        let state = CairnState {
            inner: Arc::new(Mutex::new(None)),
            runtime: Arc::new(ScriptedRuntime(vec![])),
            watcher: Arc::default(),
            sealer: Arc::default(),
        };
        let mut got: Vec<AnswerEvent> = Vec::new();
        let r = stream_answer(
            &state,
            &AskRequest {
                query: "x".into(),
                top_k: None,
            },
            &mut |e| got.push(e),
        );
        assert!(matches!(r, Err(ServiceError::InvalidRequest(_))));
        assert!(got.is_empty());
    }

    #[test]
    fn ask_emits_terminal_failed_when_runtime_errors() {
        let tmp = tempfile::tempdir().unwrap();
        // NullRuntime errors before any event (tau not configured).
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        let mut got: Vec<AnswerEvent> = Vec::new();
        stream_answer(
            &state,
            &AskRequest {
                query: "x".into(),
                top_k: None,
            },
            &mut |e| got.push(e),
        )
        .unwrap();
        assert!(matches!(got.first(), Some(AnswerEvent::Sources { .. })));
        assert!(matches!(got.last(), Some(AnswerEvent::Failed { .. })));
    }

    // ---- #175: desktop idle/backstop auto-seal --------------------------
    //
    // The engine owns the *policy* (`SealTimer`/`run_seal_loop` in
    // `cairn-service`, `[sync]` in `cairn-infra`); what these pin is the
    // desktop shell's half — which commands open a session, and that every way
    // a session can end still commits it.

    /// Count the commits in the vault at `dir`.
    fn commit_count(state: &CairnState) -> usize {
        match run_query_blocking(state, &Query::VaultHistory { limit: None }) {
            Ok(QueryResponse::History { revisions }) => revisions.len(),
            other => panic!("expected History, got {other:?}"),
        }
    }

    fn write_note(state: &CairnState, app: &AppHandle<tauri::test::MockRuntime>, body: &str) {
        run_command_blocking(
            state,
            app,
            &Command::WriteNote {
                path: "a.md".into(),
                contents: body.into(),
            },
        )
        .expect("write");
    }

    #[test]
    fn a_seal_is_not_itself_session_activity() {
        // Counting Commit/NameVersion would re-open the session the seal just
        // closed, and the loop would then commit on every idle window forever.
        assert!(is_session_activity(&Command::WriteNote {
            path: "a.md".into(),
            contents: "x".into()
        }));
        assert!(is_session_activity(&Command::DeleteNote {
            path: "a.md".into()
        }));
        assert!(!is_session_activity(&Command::Commit { message: None }));
        assert!(!is_session_activity(&Command::Commit {
            message: Some("explicit".into())
        }));
        assert!(!is_session_activity(&Command::NameVersion {
            commit: "abc".into(),
            name: "v1".into()
        }));
    }

    /// Attach a channel in place of a real seal loop, so a test can observe the
    /// activity signals the shell sends without waiting on any timer.
    fn attach_probe(state: &CairnState) -> std::sync::mpsc::Receiver<SealSignal> {
        let (tx, rx) = std::sync::mpsc::channel();
        *state.sealer.lock().unwrap() = Some(SealerHandle {
            tx,
            join: std::thread::spawn(|| {}),
        });
        rx
    }

    #[test]
    fn a_successful_edit_marks_activity_and_a_seal_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        let rx = attach_probe(&state);

        write_note(&state, &handle, "hello");
        assert_eq!(rx.try_iter().count(), 1, "an edit opens/extends a session");

        run_command_blocking(&state, &handle, &Command::Commit { message: None }).expect("seal");
        assert_eq!(rx.try_iter().count(), 0, "the seal must not re-open one");
    }

    #[test]
    fn a_failed_command_does_not_mark_activity() {
        // Otherwise a rejected write would hold a session open — and the seal it
        // eventually triggers would commit whatever else happened to be dirty.
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        let rx = attach_probe(&state);

        run_command_blocking(
            &state,
            &handle,
            &Command::NameVersion {
                commit: "not-a-commit".into(),
                name: "v1".into(),
            },
        )
        .expect_err("naming a nonexistent commit fails");
        assert_eq!(rx.try_iter().count(), 0);
    }

    #[test]
    fn mark_activity_without_a_sealer_is_a_no_op() {
        // Auto-commit off, or no cairn open: every command still has to work.
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        assert!(state.sealer.lock().unwrap().is_none());
        write_note(&state, &app.handle().clone(), "hello");
    }

    #[test]
    fn seal_open_cairn_commits_then_no_ops_on_a_clean_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        let before = commit_count(&state);

        write_note(&state, &handle, "hello");
        seal_open_cairn(&state.inner, &handle);
        assert_eq!(commit_count(&state), before + 1, "the session sealed");

        // A backstop firing on an idle vault must not pile up empty commits.
        seal_open_cairn(&state.inner, &handle);
        assert_eq!(commit_count(&state), before + 1, "clean tree ⇒ no-op");
    }

    #[test]
    fn seal_open_cairn_without_a_cairn_open_is_a_no_op() {
        // A seal can land while a cairn switch is in flight.
        let app = test_app();
        let state = CairnState {
            inner: Arc::new(Mutex::new(None)),
            runtime: Arc::new(NullRuntime),
            watcher: Arc::default(),
            sealer: Arc::default(),
        };
        seal_open_cairn(&state.inner, &app.handle().clone());
    }

    /// A seal policy whose timers will never fire during a test, so the only
    /// thing that can produce a commit is the shutdown flush.
    fn never_idle(dir: &Path) {
        std::fs::write(
            dir.join("cairn.toml"),
            "[sync]\nidle_seconds = 3600\nbackstop_minutes = 600\n",
        )
        .unwrap();
    }

    #[test]
    fn stopping_the_sealer_flushes_the_open_session() {
        // This is the app-quit and cairn-switch guarantee: `run_seal_loop` reads
        // a disconnected channel as shutdown and seals once more. With a 1-hour
        // idle window, a commit here can only have come from that flush.
        let tmp = tempfile::tempdir().unwrap();
        never_idle(tmp.path());
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        *state.sealer.lock().unwrap() = spawn_sealer(&state, &handle, tmp.path());
        assert!(state.sealer.lock().unwrap().is_some());

        let before = commit_count(&state);
        write_note(&state, &handle, "written but never idle");
        stop_sealer(&state);

        assert_eq!(commit_count(&state), before + 1, "quit flushed the session");
        assert!(state.sealer.lock().unwrap().is_none());
    }

    #[test]
    fn an_idle_gap_seals_the_session_with_no_hint_from_the_ui() {
        // The defect #175 reports: before this, the ONLY thing that committed on
        // desktop was `store.sealNow()` fired from a note switch or window blur.
        // Here nothing hints — a write goes quiet and the loop seals it.
        // `quiet_period_ms` (the deprecated alias) is the only sub-second idle
        // window the schema can express, which is why the test uses it.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[sync]\nquiet_period_ms = 150\nbackstop_minutes = 600\n",
        )
        .unwrap();
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        *state.sealer.lock().unwrap() = spawn_sealer(&state, &handle, tmp.path());

        let before = commit_count(&state);
        write_note(&state, &handle, "typed, then left alone");

        // Poll rather than sleep-once: generous upper bound, exits as soon as
        // the seal lands, so a slow runner cannot flake it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while commit_count(&state) == before && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(commit_count(&state), before + 1, "the idle window sealed");

        // And it seals ONCE — the session closed, so a further idle window with
        // no new edits must not produce an empty commit.
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(commit_count(&state), before + 1);
        stop_sealer(&state);
    }

    #[test]
    fn stopping_a_sealer_that_saw_no_edits_commits_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        never_idle(tmp.path());
        let app = test_app();
        let handle = app.handle().clone();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        let before = commit_count(&state);
        *state.sealer.lock().unwrap() = spawn_sealer(&state, &handle, tmp.path());
        stop_sealer(&state);
        assert_eq!(commit_count(&state), before);
    }

    #[test]
    fn auto_commit_false_means_no_seal_loop_at_all() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[sync]\nauto_commit = false\n",
        )
        .unwrap();
        let app = test_app();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        assert!(spawn_sealer(&state, &app.handle().clone(), tmp.path()).is_none());
    }

    #[test]
    fn a_malformed_sync_table_disables_auto_commit_rather_than_defaulting() {
        // `auto_comit = false` is most likely someone trying to turn sealing
        // OFF. Falling back to the shipped defaults would commit to their git
        // history precisely because we could not read their opt-out.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[sync]\nauto_comit = false\n",
        )
        .unwrap();
        let app = test_app();
        let state = open_state(tmp.path(), Arc::new(NullRuntime));
        assert!(spawn_sealer(&state, &app.handle().clone(), tmp.path()).is_none());
    }

    #[test]
    fn a_cairn_reads_its_own_sync_settings() {
        // The whole point of moving `SyncConfig` into `cairn-infra` (engine
        // #191): the desktop shell reads the same `[sync]` the daemon does,
        // including sections it does not own.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("cairn.toml"),
            "[cors]\norigins = []\n[sync]\nidle_seconds = 7\nbackstop_minutes = 45\n",
        )
        .unwrap();
        let c = SyncConfig::load(tmp.path()).expect("load");
        assert_eq!(c.idle(), std::time::Duration::from_secs(7));
        assert_eq!(c.backstop(), std::time::Duration::from_secs(45 * 60));
    }

    #[test]
    fn switching_cairns_seals_into_the_outgoing_one() {
        // `open_at` stops the sealer before it clears `inner`, so the flush
        // commits against the cairn that produced the edits. Get the order
        // wrong and one vault's words land in another vault's history.
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        never_idle(a.path());
        never_idle(b.path());
        let app = test_app();
        let handle = app.handle().clone();
        let state = (*app.state::<CairnState>()).clone();

        open_at(&state, &handle, a.path()).expect("open a");
        let a_before = commit_count(&state);
        write_note(&state, &handle, "belongs to A");

        open_at(&state, &handle, b.path()).expect("open b");
        let b_commits = commit_count(&state);

        open_at(&state, &handle, a.path()).expect("reopen a");
        assert_eq!(
            commit_count(&state),
            a_before + 1,
            "A's session sealed into A"
        );
        assert_eq!(b_commits, 0, "A's words did not follow the switch into B");
        stop_sealer(&state);
    }

    #[test]
    fn open_at_sets_state_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state = (*app.state::<CairnState>()).clone();
        open_at(&state, &app.handle().clone(), tmp.path()).expect("open_at");
        assert!(state.inner.lock().unwrap().is_some());
        assert_eq!(
            state.inner.lock().unwrap().as_ref().map(|(_, p)| p.clone()),
            Some(tmp.path().to_path_buf())
        );
    }

    #[test]
    fn open_at_same_dir_twice_releases_index_lock() {
        // The on-disk Tantivy index has a single exclusive writer. Re-opening the
        // same cairn must succeed, which means the prior engine has to be dropped
        // (releasing the writer lock) BEFORE the new one is built — otherwise
        // open_engine can't acquire the writer and the reopen fails.
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state = (*app.state::<CairnState>()).clone();
        open_at(&state, &app.handle().clone(), tmp.path()).expect("first open");
        open_at(&state, &app.handle().clone(), tmp.path()).expect("reopen same dir");
        assert_eq!(
            state.inner.lock().unwrap().as_ref().map(|(_, p)| p.clone()),
            Some(tmp.path().to_path_buf())
        );
    }

    // ---- watcher (§4: live external-edit updates) --------------------------

    use cairn_domain::NotePath;

    #[test]
    fn stoppable_watch_drains_buffered_changes_then_stops_on_disconnect() {
        let (tx, rx) = std::sync::mpsc::channel::<FsChange>();
        tx.send(FsChange::Changed(NotePath::new("a.md").unwrap()))
            .unwrap();
        tx.send(FsChange::Removed(NotePath::new("b.md").unwrap()))
            .unwrap();
        drop(tx); // disconnect: the loop drains what's buffered, then exits
        let stop = AtomicBool::new(false);
        let mut seen = Vec::new();
        run_stoppable_watch(&rx, &stop, |c| seen.push(c.clone()));
        assert_eq!(
            seen,
            vec![
                FsChange::Changed(NotePath::new("a.md").unwrap()),
                FsChange::Removed(NotePath::new("b.md").unwrap()),
            ]
        );
    }

    #[test]
    fn stoppable_watch_exits_when_stop_flag_set_even_if_connected() {
        // The channel is never disconnected (tx held past the join); only the
        // stop flag can end the loop. If the flag were ignored, join would hang.
        let (tx, rx) = std::sync::mpsc::channel::<FsChange>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let jh = std::thread::spawn(move || {
            let mut n = 0usize;
            run_stoppable_watch(&rx, &stop_thread, |_| n += 1);
            n
        });
        stop.store(true, Ordering::Relaxed);
        let n = jh.join().expect("watch thread joins after stop flag");
        assert_eq!(n, 0, "no change was sent, so on_change never fires");
        drop(tx);
    }

    #[test]
    fn apply_fs_change_emits_note_changed_for_external_write() {
        let tmp = tempfile::tempdir().unwrap();
        let inner: Arc<Mutex<Option<(CairnEngine, PathBuf)>>> = Arc::new(Mutex::new(Some((
            open_engine(tmp.path()).unwrap(),
            tmp.path().to_path_buf(),
        ))));
        // Simulate an external editor writing a note straight to disk.
        std::fs::write(tmp.path().join("ext.md"), "external content").unwrap();
        let mut sink: Vec<cairn_app::Event> = Vec::new();
        apply_fs_change(
            &inner,
            &FsChange::Changed(NotePath::new("ext.md").unwrap()),
            &mut sink,
        );
        assert!(
            sink.iter().any(|e| matches!(
                e,
                cairn_app::Event::NoteChanged(p) if *p == NotePath::new("ext.md").unwrap()
            )),
            "external write should emit NoteChanged, got {sink:?}"
        );
    }

    #[test]
    fn apply_fs_change_is_noop_without_open_cairn() {
        let inner: Arc<Mutex<Option<(CairnEngine, PathBuf)>>> = Arc::new(Mutex::new(None));
        let mut sink: Vec<cairn_app::Event> = Vec::new();
        apply_fs_change(
            &inner,
            &FsChange::Changed(NotePath::new("x.md").unwrap()),
            &mut sink,
        );
        assert!(sink.is_empty(), "no cairn open → no events");
    }

    #[test]
    fn watcher_reflects_external_edit_end_to_end() {
        // The DoD's core claim: an edit made *outside* the app (another editor,
        // `git pull`) surfaces as a NoteChanged event. Exercises the real OS
        // watcher + the stoppable loop + apply_fs_change together — the same
        // composition spawn_watcher wires, minus only the Tauri emit (a trivial
        // forward the frontend already listens to).
        use std::time::{Duration, Instant};
        let tmp = tempfile::tempdir().unwrap();
        let inner: Arc<Mutex<Option<(CairnEngine, PathBuf)>>> = Arc::new(Mutex::new(Some((
            open_engine(tmp.path()).unwrap(),
            tmp.path().to_path_buf(),
        ))));
        let handle = NotifyWatcher.watch(tmp.path()).expect("watch");
        let stop = Arc::new(AtomicBool::new(false));
        let seen: Arc<Mutex<Vec<cairn_app::Event>>> = Arc::new(Mutex::new(Vec::new()));
        let inner_t = inner.clone();
        let stop_t = stop.clone();
        let seen_t = seen.clone();
        let jh = std::thread::spawn(move || {
            run_stoppable_watch(&handle.changes, &stop_t, |change| {
                let mut sink: Vec<cairn_app::Event> = Vec::new();
                apply_fs_change(&inner_t, change, &mut sink);
                seen_t.lock().unwrap().extend(sink);
            });
            drop(handle);
        });

        // An external editor writes a note straight to disk.
        std::fs::write(tmp.path().join("live.md"), "hello from another editor").unwrap();

        let want = NotePath::new("live.md").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let reflected = loop {
            let hit = seen
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, cairn_app::Event::NoteChanged(p) if *p == want));
            if hit {
                break true;
            }
            if Instant::now() > deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        stop.store(true, Ordering::Relaxed);
        let _ = jh.join();
        assert!(
            reflected,
            "external edit was not reflected as NoteChanged within 5s: {:?}",
            seen.lock().unwrap()
        );
    }

    // --- C0 (engine auto-commit + named versions) on the desktop seam --------
    //
    // The UI landed the C0 TS shapes in #160 but left `src-tauri/Cargo.toml`
    // pinned to a pre-C0 engine, so the desktop app rendered shapes its own
    // in-process engine could not produce. Engine #184's DoD only ever ran
    // against a *daemon*. These tests pin the desktop half: they drive
    // `run_command_blocking`/`run_query_blocking` — the exact functions the
    // `send_command`/`run_query` IPC handlers call — so a future engine re-pin
    // that drops a C0 variant fails here instead of at a user's keystroke.

    /// Open a real on-disk engine into `state` and hand back both.
    fn app_with_cairn(
        tmp: &tempfile::TempDir,
    ) -> (tauri::App<tauri::test::MockRuntime>, CairnState) {
        let app = test_app();
        let state = (*app.state::<CairnState>()).clone();
        *state.inner.lock().unwrap() =
            Some((open_engine(tmp.path()).unwrap(), tmp.path().to_path_buf()));
        (app, state)
    }

    /// Newest-first revisions for `path`, via the same query the UI issues.
    fn history(state: &CairnState, path: &str) -> Vec<cairn_contract::Revision> {
        match run_query_blocking(state, &Query::NoteHistory { path: path.into() })
            .expect("note_history")
        {
            QueryResponse::History { revisions } => revisions,
            other => panic!("expected History, got {other:?}"),
        }
    }

    #[test]
    fn seal_now_without_a_message_generates_one_and_a_change_summary() {
        // `Command::Commit { message: None }` is "seal now". Pre-C0 the field was
        // a bare `String`, so this call did not typecheck against the old pin.
        let tmp = tempfile::tempdir().unwrap();
        let (app, state) = app_with_cairn(&tmp);
        let handle = app.handle().clone();
        run_command_blocking(
            &state,
            &handle,
            &Command::WriteNote {
                path: "note.md".into(),
                contents: "# Title\n\none two three\n".into(),
            },
        )
        .unwrap();

        let res = run_command_blocking(&state, &handle, &Command::Commit { message: None })
            .expect("seal now");
        let commit = match res {
            CommandResponse::Committed { commit } => commit,
            other => panic!("expected Committed, got {other:?}"),
        };
        assert!(!commit.is_empty(), "committed response carries a commit id");

        let revs = history(&state, "note.md");
        let head = revs.first().expect("one revision after sealing");
        // Engine-generated, not caller-supplied: it names the note rather than
        // echoing a message we never sent (the engine template is
        // `Add "<title>" (+N words)`).
        assert!(
            head.message.contains("Title"),
            "expected an engine-generated message naming the note, got {:?}",
            head.message
        );
        // C0's `Revision.summary` — the field the Versions panel renders as
        // "N files, +N/-N words". `None` here would render a blank row.
        let summary = head
            .summary
            .as_ref()
            .expect("engine computes a ChangeSummary for a sealed commit");
        assert_eq!(summary.files_changed, 1);
        assert!(
            summary.words_added > 0,
            "adding a note adds words: {summary:?}"
        );
        assert_eq!(summary.words_removed, 0);
        assert_eq!(head.name, None, "an unnamed commit has no label");
    }

    #[test]
    fn sealing_a_clean_tree_is_nothing_to_commit_not_an_error() {
        // The no-op the UI must render as "nothing to seal", not as a red error.
        // It is an `Ok(CommandResponse::NothingToCommit)`, a variant the pre-C0
        // engine did not have at all.
        let tmp = tempfile::tempdir().unwrap();
        let (app, state) = app_with_cairn(&tmp);
        let handle = app.handle().clone();
        run_command_blocking(
            &state,
            &handle,
            &Command::WriteNote {
                path: "note.md".into(),
                contents: "body\n".into(),
            },
        )
        .unwrap();
        run_command_blocking(&state, &handle, &Command::Commit { message: None }).unwrap();

        // Nothing dirty now — with and without an explicit message.
        for message in [None, Some("please commit".to_string())] {
            let res = run_command_blocking(&state, &handle, &Command::Commit { message })
                .expect("a clean tree is a success, not a ServiceError");
            assert_eq!(res, CommandResponse::NothingToCommit);
        }
    }

    #[test]
    fn name_version_creates_replaces_and_rejects_reuse_on_another_commit() {
        // E4. `NameVersion` did not exist on the pre-C0 pin, so the UI's
        // "name this version" action deserialized into nothing on desktop.
        let tmp = tempfile::tempdir().unwrap();
        let (app, state) = app_with_cairn(&tmp);
        let handle = app.handle().clone();

        let seal = |path: &str, contents: &str| {
            run_command_blocking(
                &state,
                &handle,
                &Command::WriteNote {
                    path: path.into(),
                    contents: contents.into(),
                },
            )
            .unwrap();
            match run_command_blocking(&state, &handle, &Command::Commit { message: None }).unwrap()
            {
                CommandResponse::Committed { commit } => commit,
                other => panic!("expected Committed, got {other:?}"),
            }
        };
        let first = seal("note.md", "one\n");
        let second = seal("note.md", "one two\n");

        let named = |commit: &str, name: &str| {
            run_command_blocking(
                &state,
                &handle,
                &Command::NameVersion {
                    commit: commit.into(),
                    name: name.into(),
                },
            )
        };
        let label_of = |commit: &str| {
            history(&state, "note.md")
                .into_iter()
                .find(|r| commit.starts_with(&r.id) || r.id.starts_with(commit))
                .unwrap_or_else(|| panic!("revision {commit} in history"))
                .name
        };

        // Create.
        assert_eq!(
            named(&first, "before the rewrite").unwrap(),
            CommandResponse::Done
        );
        assert_eq!(label_of(&first).as_deref(), Some("before the rewrite"));

        // Replace on the same commit: one name per commit.
        assert_eq!(named(&first, "draft one").unwrap(), CommandResponse::Done);
        assert_eq!(
            label_of(&first).as_deref(),
            Some("draft one"),
            "re-naming the same commit replaces the label"
        );

        // Reuse on a *different* commit: one commit per name. Must surface as a
        // usable error, not a panic or a silent success.
        let err = named(&second, "draft one")
            .expect_err("reusing a name on another commit must be rejected");
        assert!(
            matches!(err, ServiceError::InvalidRequest(_)),
            "expected InvalidRequest, got {err:?}"
        );
        let contract: ContractError = err.into();
        let rendered = format!("{contract:?}");
        assert!(
            rendered.to_lowercase().contains("draft one"),
            "the error the UI shows should name the conflicting label: {rendered}"
        );
        assert_eq!(
            label_of(&second),
            None,
            "the rejected name was not applied to the second commit"
        );
    }
}
