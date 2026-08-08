//! HTTP + WebSocket transport over the cairn dispatcher. Binds localhost only.
//! `/command`, `/query`, `/ask`, and `/mcp` require a local bearer token (audit
//! S5; see the `auth` module and [`AppState::with_token`]). `/mcp` additionally
//! accepts the token as a `?token=` query param for headerless MCP clients. The
//! engine runs synchronously under a mutex via `spawn_blocking`.

pub mod config;
pub use config::Config;

mod auth;
pub use auth::generate_token_file;

mod mcp;

pub mod collab;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    AnswerEvent, AskRequest, Command, CommandResponse, ContractError, Event as WireEvent, Query,
    QueryResponse,
};
use cairn_ports::{AgentEvent, AgentRuntime, AgentSink};
use cairn_service::{
    agent_event_to_wire, app_event_to_wire, dispatch_command, dispatch_query_mut,
    gather_answer_context, ServiceError,
};
use tokio::sync::broadcast;
use tracing::Instrument;

/// Shared daemon state: the engine behind a mutex + an event broadcast.
#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<Engine>>,
    events: broadcast::Sender<WireEvent>,
    /// Origins permitted to open the `/events` WebSocket. Same allowlist the
    /// CORS layer enforces; empty denies all (deny-by-default).
    allowed_origins: Arc<[String]>,
    /// Bearer token required on `/command` and `/query`. `None` disables auth
    /// (the in-process/library/test default); the `cairn-daemon` binary always
    /// sets a token via [`AppState::with_token`].
    token: Option<Arc<str>>,
    /// Agent runtime backing `POST /ask`. Defaults to `NullRuntime` (which errors
    /// until `TAU_BIN` is set); the binary injects `TauServeRuntime` via
    /// [`AppState::with_runtime`].
    runtime: Arc<dyn AgentRuntime + Send + Sync>,
    /// Whether the `/mcp` route advertises and accepts write tools. Default
    /// `false` (read-only), mirroring the plugin trust default-deny posture; the
    /// binary opts in via `--mcp-write` ([`AppState::with_mcp_write`]).
    mcp_write: bool,
    /// Live CRDT collaboration sessions, one per open note. Independent of the
    /// engine mutex so a relay fault cannot stall `/command`.
    collab: collab::Collab,
}

/// An `EventSink` that republishes engine events as wire events.
struct BroadcastSink(broadcast::Sender<WireEvent>);
impl EventSink for BroadcastSink {
    fn emit(&mut self, event: AppEvent) {
        // No subscribers is not an error.
        let _ = self.0.send(app_event_to_wire(event));
    }
}

/// An `EventSink` that broadcasts engine events to the WS channel AND collects
/// them, so the daemon can forward note events to plugins after the operation.
struct EventTap {
    tx: broadcast::Sender<WireEvent>,
    collected: Vec<AppEvent>,
}
impl EventSink for EventTap {
    fn emit(&mut self, event: AppEvent) {
        self.collected.push(event.clone());
        let _ = self.tx.send(app_event_to_wire(event));
    }
}

/// An `AgentSink` that forwards each agent increment as a wire `AnswerEvent`
/// over an mpsc channel. `blocking_send` gives natural backpressure and is
/// lossless (unlike the broadcast `/events` channel); a closed receiver (client
/// gone) turns sends into no-ops — the run still finishes (no v1 cancellation).
struct AnswerStreamSink {
    tx: tokio::sync::mpsc::Sender<AnswerEvent>,
}
impl AgentSink for AnswerStreamSink {
    fn emit(&mut self, event: AgentEvent) {
        if let Some(wire) = agent_event_to_wire(event) {
            let _ = self.tx.blocking_send(wire);
        }
    }
}

/// Map an engine event to the plugin-facing event, or `None` if plugins don't
/// receive it (only note mutations are forwarded).
fn to_plugin_event(event: &AppEvent) -> Option<cairn_ports::PluginEvent> {
    match event {
        AppEvent::NoteChanged(p) => Some(cairn_ports::PluginEvent::NoteChanged(p.clone())),
        AppEvent::NoteDeleted(p) => Some(cairn_ports::PluginEvent::NoteDeleted(p.clone())),
        AppEvent::Committed(_) | AppEvent::Reindexed(_) => None,
    }
}

impl AppState {
    /// Build state from an engine.
    #[must_use]
    pub fn new(engine: Engine) -> Self {
        // 256 comfortably exceeds any realistic per-command event burst;
        // slow subscribers lag-drop rather than back-pressure the engine.
        let (events, _rx) = broadcast::channel(256);
        Self {
            engine: Arc::new(Mutex::new(engine)),
            events,
            allowed_origins: Arc::from([]),
            token: None,
            runtime: Arc::new(cairn_infra::NullRuntime),
            mcp_write: false,
            collab: collab::registry(),
        }
    }

    /// Set the origins permitted to open the `/events` WebSocket. Reuse the
    /// daemon's CORS allowlist so HTTP and WS share one origin policy.
    #[must_use]
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = Arc::from(origins.into_boxed_slice());
        self
    }

    /// Require this bearer token on `/command` and `/query`. Reuse the same
    /// optional-builder shape as [`AppState::with_allowed_origins`].
    #[must_use]
    pub fn with_token(mut self, token: impl Into<Arc<str>>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Inject the agent runtime backing `POST /ask`.
    #[must_use]
    pub fn with_runtime(mut self, runtime: Arc<dyn AgentRuntime + Send + Sync>) -> Self {
        self.runtime = runtime;
        self
    }

    /// Enable write tools on the `/mcp` route. Default off (read-only).
    #[must_use]
    pub fn with_mcp_write(mut self, enabled: bool) -> Self {
        self.mcp_write = enabled;
        self
    }

    /// Lock the engine, recovering from poisoning instead of propagating it.
    ///
    /// A panic in any engine operation poisons the `Mutex`; with `.expect(...)`
    /// every subsequent request would panic and 500 forever (audit: mutex-
    /// poisoning DoS). The data behind the lock is a single engine whose
    /// invariants are re-established on the next operation, so recovering the
    /// guard and continuing is correct.
    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run a command synchronously, publishing produced events.
    ///
    /// Blocks the current thread while it holds the engine lock and runs
    /// (blocking) engine work. Call it from a blocking context such as
    /// [`tokio::task::spawn_blocking`], never directly on an async executor
    /// thread.
    ///
    /// # Errors
    /// Returns [`ServiceError`] on invalid input or engine failure.
    pub fn run_command_blocking(&self, command: &Command) -> Result<CommandResponse, ServiceError> {
        let mut guard = self.engine();
        let mut tap = EventTap {
            tx: self.events.clone(),
            collected: Vec::new(),
        };
        let result = dispatch_command(&mut guard, command, &mut tap);
        let collected = tap.collected;
        if result.is_ok() {
            // Forward note events to plugins after the command completes (no plugin
            // is mid-invoke). Handler-generated events use a broadcast-only sink, so
            // they reach WS but are not re-forwarded to plugins (non-recursive).
            for pe in collected.iter().filter_map(to_plugin_event) {
                let mut fwd = BroadcastSink(self.events.clone());
                guard.dispatch_plugin_event(&pe, &mut fwd);
            }
        }
        result
    }

    /// Run a query synchronously.
    ///
    /// Blocks the current thread while it holds the engine lock. Call it from a
    /// blocking context such as [`tokio::task::spawn_blocking`], never directly
    /// on an async executor thread.
    ///
    /// # Errors
    /// Returns [`ServiceError`] on invalid input or engine failure.
    pub fn run_query_blocking(&self, query: &Query) -> Result<QueryResponse, ServiceError> {
        let mut guard = self.engine();
        dispatch_query_mut(&mut guard, query)
    }

    /// Apply a watcher-reported filesystem change, publishing any resulting
    /// events to subscribers. Best-effort: a transient failure is logged, not
    /// propagated, so the watch loop keeps running.
    pub fn apply_change_blocking(&self, change: &cairn_ports::FsChange) {
        // A sessioned note is owned by the daemon: defer a foreign `Changed` to
        // the collab flush (which folds it) instead of the generic re-index /
        // auto-commit. Whole-file `Removed` is not intercepted — content-edit
        // fold-back only (spec §13.5); a spurious delete self-heals on re-flush.
        if let cairn_ports::FsChange::Changed(path) = change {
            if collab::is_sessioned(&self.collab, path) {
                // Read disk OUTSIDE the collab lock (engine lock), then mark.
                // The engine guard is a temporary here — it drops at the end of
                // this statement, so the collab lock in `note_foreign_edit`
                // below is never taken while the engine lock is held (lock
                // order: collab → engine → collab, sequential, never nested).
                let disk = self.engine().read_note(path).unwrap_or_default();
                collab::note_foreign_edit(&self.collab, path, &disk);
                return;
            }
        }
        let mut guard = self.engine();
        let mut tap = EventTap {
            tx: self.events.clone(),
            collected: Vec::new(),
        };
        if let Err(e) = guard.apply_change(change, &mut tap) {
            tracing::warn!("watch: apply_change failed: {e}");
            return;
        }
        let collected = tap.collected;
        for pe in collected.iter().filter_map(to_plugin_event) {
            let mut fwd = BroadcastSink(self.events.clone());
            guard.dispatch_plugin_event(&pe, &mut fwd);
        }
    }

    /// Apply a watcher change, but for a `Removed` first wait `grace` and re-stat:
    /// a non-atomic or tmp-rename write can make the watcher see the file
    /// momentarily gone, and applying that `Removed` would spuriously — possibly
    /// terminally — delete the note. If the file is back after the grace, the
    /// change is re-routed as `Changed` (re-index) instead of a delete. A
    /// `Changed` is applied directly (the engine re-stats and a partial read
    /// self-heals via the memo on the next event).
    pub fn apply_change_confirmed_blocking(
        &self,
        change: &cairn_ports::FsChange,
        grace: std::time::Duration,
    ) {
        if let cairn_ports::FsChange::Removed(path) = change {
            std::thread::sleep(grace);
            if self.engine().exists_on_disk(path) {
                self.apply_change_blocking(&cairn_ports::FsChange::Changed(path.clone()));
                return;
            }
        }
        self.apply_change_blocking(change);
    }

    /// Commit externally-detected edits, coalesced by the watch loop's quiet
    /// period. No-ops when the working tree is clean (avoids an empty commit when
    /// the changes were already committed, or the watcher event was spurious).
    /// Best-effort: a failure is logged, not propagated.
    pub fn commit_external_blocking(&self, message: &str) {
        let mut guard = self.engine();
        match guard.has_uncommitted_changes() {
            Ok(false) => return,
            Ok(true) => {}
            Err(e) => {
                tracing::warn!("watch: auto-commit dirty-check failed: {e}");
                return;
            }
        }
        let mut tap = EventTap {
            tx: self.events.clone(),
            collected: Vec::new(),
        };
        if let Err(e) = guard.commit(message, &mut tap) {
            tracing::warn!("watch: auto-commit failed: {e}");
        }
    }

    /// One collab commit-agent pass: materialize + git-commit every session that
    /// is dirty and quiescent (or abandoned). Phase 1 (`drain_due`) works under
    /// the collab lock only; phase 2 takes the engine lock per note. The write
    /// goes through `write_note` so the file-watcher self-suppresses the echo,
    /// and a disk-vs-baseline check skips (never clobbers) a foreign edit — A2
    /// folds those back instead. Best-effort: every failure logs and continues.
    /// Run from a blocking context (`spawn_blocking`), like the watch loop.
    /// See the design spec §12.
    pub fn run_collab_flush_pass(&self, debounce: Duration) {
        let items = collab::drain_due(&self.collab, debounce);
        for item in items {
            let mut guard = self.engine();
            let disk = guard.read_note(&item.path).unwrap_or_default();
            if disk != item.baseline {
                // Release the engine lock before taking the collab lock, then fold
                // the foreign on-disk edit into the live replica and fan it out
                // (collab lock only). No write this pass — the fold leaves the
                // session dirty, so the NEXT pass re-materializes the merged result
                // and writes it via the normal disk==baseline path (spec §13.1).
                drop(guard);
                tracing::info!(
                    note = %item.path.as_str(),
                    "collab flush: foreign on-disk edit; folding back"
                );
                collab::fold_foreign(&self.collab, &item.path, &disk);
                continue;
            }
            let mut tap = EventTap {
                tx: self.events.clone(),
                collected: Vec::new(),
            };
            if let Err(e) = guard.write_note(&item.path, &item.markdown, &mut tap) {
                drop(guard);
                tracing::warn!(note = %item.path.as_str(), error = %e, "collab flush: write failed");
                collab::settle_flush(&self.collab, &item.path, collab::FlushOutcome::WriteError);
                continue;
            }
            // The bytes are on disk now; commit is best-effort. On commit failure
            // the note stays dirty-on-disk and re-commits on the next op (the
            // baseline still advances, so the retry is not a false conflict).
            match guard.has_uncommitted_changes() {
                Ok(true) => {
                    let msg = format!("cairn: collab sync {}", item.path.as_str());
                    if let Err(e) = guard.commit(&msg, &mut tap) {
                        tracing::warn!(error = %e, "collab flush: commit failed");
                    }
                }
                Ok(false) => {} // materialize matched disk; nothing to commit
                Err(e) => tracing::warn!(error = %e, "collab flush: dirty-check failed"),
            }
            drop(guard);
            // Write landed ⇒ baseline advances (even if the commit failed) and an
            // abandoned, settled session is reaped — only now, post-write.
            collab::settle_flush(
                &self.collab,
                &item.path,
                collab::FlushOutcome::Committed(item.markdown),
            );
        }
    }
}

fn status_for(err: &ServiceError) -> StatusCode {
    match err {
        ServiceError::NotFound(_) => StatusCode::NOT_FOUND,
        ServiceError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Convert a dispatch result (possibly a `spawn_blocking` join error) into an
/// HTTP response. Join failures return a generic message — internal panic text
/// is never leaked to clients.
fn service_response<T: serde::Serialize>(
    result: Result<Result<T, ServiceError>, tokio::task::JoinError>,
) -> Response {
    match result {
        Ok(Ok(resp)) => (StatusCode::OK, Json(resp)).into_response(),
        Ok(Err(svc)) => (status_for(&svc), Json(ContractError::from(svc))).into_response(),
        Err(join) => {
            // A worker thread panicked. Log the JoinError with the request span's
            // context (audit G5) so the panic is correlated to the request, but
            // return only a generic message — never leak panic text to clients.
            tracing::error!(error = %join, "request worker panicked");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ContractError::Internal {
                    message: "internal error".to_string(),
                }),
            )
                .into_response()
        }
    }
}

/// The wire `type` tag for a command (matches the serde `rename_all = "snake_case"`),
/// used as the `command` span field so a logged request matches what a client sent.
fn command_kind(command: &Command) -> &'static str {
    match command {
        Command::WriteNote { .. } => "write_note",
        Command::DeleteNote { .. } => "delete_note",
        Command::RenameNote { .. } => "rename_note",
        Command::Commit { .. } => "commit",
        Command::RestoreNote { .. } => "restore_note",
        Command::InvokePluginCommand { .. } => "invoke_plugin_command",
    }
}

/// The wire `type` tag for a query (matches the serde `rename_all = "snake_case"`).
fn query_kind(query: &Query) -> &'static str {
    match query {
        Query::GetNote { .. } => "get_note",
        Query::RenderNote { .. } => "render_note",
        Query::Search { .. } => "search",
        Query::GetBacklinks { .. } => "get_backlinks",
        Query::ListNotes => "list_notes",
        Query::GetGraph { .. } => "get_graph",
        Query::GraphAt { .. } => "graph_at",
        Query::GraphDiff { .. } => "graph_diff",
        Query::ListTags => "list_tags",
        Query::NotesByTag { .. } => "notes_by_tag",
        Query::ListPlugins => "list_plugins",
        Query::NoteHistory { .. } => "note_history",
        Query::VaultHistory { .. } => "vault_history",
        Query::NoteAt { .. } => "note_at",
        Query::GetSuggestions { .. } => "get_suggestions",
    }
}

async fn command_handler(State(state): State<AppState>, Json(command): Json<Command>) -> Response {
    let span = tracing::info_span!(
        "request",
        method = "POST",
        path = "/command",
        command = command_kind(&command),
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    async move {
        let start = std::time::Instant::now();
        let result =
            tokio::task::spawn_blocking(move || state.run_command_blocking(&command)).await;
        let response = service_response(result);
        let span = tracing::Span::current();
        span.record("status", response.status().as_u16());
        span.record("duration_ms", start.elapsed().as_millis() as u64);
        span.record(
            "outcome",
            if response.status().is_success() {
                "ok"
            } else {
                "error"
            },
        );
        tracing::info!("request completed");
        response
    }
    .instrument(span)
    .await
}

async fn query_handler(State(state): State<AppState>, Json(query): Json<Query>) -> Response {
    let span = tracing::info_span!(
        "request",
        method = "POST",
        path = "/query",
        command = query_kind(&query),
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    async move {
        let start = std::time::Instant::now();
        let result = tokio::task::spawn_blocking(move || state.run_query_blocking(&query)).await;
        let response = service_response(result);
        let span = tracing::Span::current();
        span.record("status", response.status().as_u16());
        span.record("duration_ms", start.elapsed().as_millis() as u64);
        span.record(
            "outcome",
            if response.status().is_success() {
                "ok"
            } else {
                "error"
            },
        );
        tracing::info!("request completed");
        response
    }
    .instrument(span)
    .await
}

/// `POST /ask`: gather note context under the engine lock, release it, then
/// stream the agent answer as Server-Sent `AnswerEvent` frames. The agent run
/// (seconds; spawns a subprocess) is kept off the async reactor (`spawn_blocking`)
/// and off the engine mutex (released after the gather).
async fn ask_handler(State(state): State<AppState>, Json(req): Json<AskRequest>) -> Response {
    let span = tracing::info_span!(
        "request",
        method = "POST",
        path = "/ask",
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    async move {
        let start = std::time::Instant::now();
        let top_k = req.top_k.unwrap_or(5);

        // Record the pre-stream phase outcome on the span and return `response`.
        // For a streaming response the body is produced AFTER we return, so
        // `duration_ms` measures time-to-stream-start, not the full answer; a
        // producer-task panic is logged separately on the stream's close path.
        let finish = |response: Response, outcome: &'static str| -> Response {
            let span = tracing::Span::current();
            span.record("status", response.status().as_u16());
            span.record("duration_ms", start.elapsed().as_millis() as u64);
            span.record("outcome", outcome);
            tracing::info!("request completed");
            response
        };

        // 1. Gather context under the lock, in a blocking task (engine work blocks).
        let gather_state = state.clone();
        let query = req.query;
        let gathered = tokio::task::spawn_blocking(move || {
            let guard = gather_state.engine();
            gather_answer_context(&guard, &query, top_k)
        })
        .await;
        let (prompt, cited) = match gathered {
            Ok(Ok(v)) => v,
            Ok(Err(svc)) => return finish(service_response::<()>(Ok(Err(svc))), "error"),
            Err(join) => return finish(service_response::<()>(Err(join)), "error"),
        };

        // 2. Stream the agent run, lock-free.
        let (tx, rx) = tokio::sync::mpsc::channel::<AnswerEvent>(64);
        let runtime = state.runtime.clone();
        let producer = tokio::task::spawn_blocking(move || {
            let _ = tx.blocking_send(AnswerEvent::Sources { paths: cited });
            let mut sink = AnswerStreamSink { tx };
            // A run that starts then fails reports via AgentEvent::Failed on the sink;
            // an Err means it failed before any event (e.g. NullRuntime) — surface it
            // through the same mapping path rather than touching the channel directly.
            if let Err(e) = runtime.answer(&prompt, &mut sink) {
                sink.emit(AgentEvent::Failed {
                    message: e.to_string(),
                });
            }
        });

        // Carry the producer handle in the stream state so its completion is observed:
        // a panic in the detached blocking task would otherwise be silently swallowed
        // (unlike command/query handlers, which await their worker via service_response).
        let stream = futures_util::stream::unfold(
            (rx, Some(producer)),
            |(mut rx, mut producer)| async move {
                match rx.recv().await {
                    Some(ev) => {
                        let frame = SseEvent::default()
                            .json_data(&ev)
                            .unwrap_or_else(|_| SseEvent::default().comment("serialize error"));
                        Some((Ok::<_, std::convert::Infallible>(frame), (rx, producer)))
                    }
                    None => {
                        // Channel closed: the producer finished. Drive its handle once
                        // to surface a panic, then end the stream.
                        if let Some(handle) = producer.take() {
                            if let Err(e) = handle.await {
                                tracing::error!(error = %e, "ask: answer producer task failed");
                            }
                        }
                        None
                    }
                }
            },
        );
        finish(
            Sse::new(stream)
                .keep_alive(KeepAlive::new())
                .into_response(),
            "streaming",
        )
    }
    .instrument(span)
    .await
}

/// True if `origin` (the request's `Origin` header value) is present and in the
/// allowlist. Browsers always send `Origin` on a WS handshake; a missing or
/// non-UTF-8 header is treated as disallowed (deny-by-default, mirroring CORS).
///
/// Matches by exact string equality against the same allowlist the CORS layer
/// uses. Browsers serialize `Origin` canonically (lowercase scheme/host, explicit
/// port, no path), so this agrees with tower-http's parsed comparison for every
/// well-formed origin; it can only ever be equal-or-stricter, never looser.
fn ws_origin_allowed(allowed: &[String], origin: Option<&axum::http::HeaderValue>) -> bool {
    match origin.and_then(|o| o.to_str().ok()) {
        Some(value) => allowed.iter().any(|a| a == value),
        None => false,
    }
}

async fn events_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Browsers do not apply CORS to WebSocket upgrades, so validate Origin here
    // against the same allowlist (audit S2). Reject before upgrading.
    if !ws_origin_allowed(&state.allowed_origins, headers.get(header::ORIGIN)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
}

async fn collab_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Same Origin gate as `/events` (browsers skip CORS on WS upgrades). The
    // token is enforced by the `mcp_require_token` route layer.
    if !ws_origin_allowed(&state.allowed_origins, headers.get(header::ORIGIN)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let collab = state.collab.clone();
    let seed_state = state.clone();
    ws.on_upgrade(move |socket| {
        collab::run_collab(socket, collab, move |path| {
            // Seed from the note's current content; empty if it does not exist.
            seed_state.engine().read_note(path).unwrap_or_default()
        })
    })
}

/// What the WS forward loop should do with a single broadcast `recv` outcome.
enum WsForward {
    /// Send this serialized event to the client.
    Send(String),
    /// Skip this event (it was dropped); keep the connection open.
    Skip,
    /// The broadcast channel closed; end the loop.
    Stop,
}

/// Decide the WS forward action for one broadcast outcome, logging the
/// intentionally best-effort drops (audit G4): a subscriber that lagged past the
/// channel capacity, or an event that failed to serialize. Both keep the socket
/// open, but neither should be silent.
fn ws_event_action(event: Result<WireEvent, broadcast::error::RecvError>) -> WsForward {
    match event {
        Ok(ev) => match serde_json::to_string(&ev) {
            Ok(text) => WsForward::Send(text),
            Err(e) => {
                tracing::warn!(error = %e, "ws: dropping event, serialize failed");
                WsForward::Skip
            }
        },
        Err(broadcast::error::RecvError::Lagged(skipped)) => {
            tracing::warn!(skipped, "ws: subscriber lagged, dropped events");
            WsForward::Skip
        }
        Err(broadcast::error::RecvError::Closed) => WsForward::Stop,
    }
}

async fn forward_events(mut socket: WebSocket, mut rx: broadcast::Receiver<WireEvent>) {
    loop {
        tokio::select! {
            // Drain inbound frames so tungstenite can auto-reply to pings;
            // a close frame, end-of-stream, or error ends the loop.
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => {} // ignore data/ping/pong (pings auto-replied)
                    Some(Err(_)) | None => break,
                }
            }
            // Forward broadcast events as JSON text frames.
            event = rx.recv() => {
                match ws_event_action(event) {
                    WsForward::Send(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    WsForward::Skip => continue,
                    WsForward::Stop => break,
                }
            }
        }
    }
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

/// Merge config-file origins with CLI `--cors-origin` values into the effective
/// CORS allowlist: appends the CLI origins, drops any `*` (not part of the
/// deny-by-default model — it would also panic the layer), then sorts and
/// deduplicates. This is what should be both displayed at startup and passed to
/// [`cors_layer`], so the printed allowlist reflects what is actually allowed.
#[must_use]
pub fn merge_cors_origins(file: Vec<String>, cli: &[String]) -> Vec<String> {
    let mut origins = file;
    origins.extend(cli.iter().cloned());
    origins.retain(|o| o != "*");
    origins.sort();
    origins.dedup();
    origins
}

/// Build a CORS layer allowing exactly `origins`. Deny-by-default: an empty
/// list allows no cross-origin request. Methods GET/POST/OPTIONS, header
/// `content-type`, no credentials.
pub fn cors_layer(origins: &[String]) -> tower_http::cors::CorsLayer {
    use axum::http::{header, HeaderValue, Method};
    // Skip `*`: a wildcard in `AllowOrigin::list` panics in tower-http, and our
    // deny-by-default allowlist has no wildcard. Malformed entries are ignored.
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter(|o| o.as_str() != "*")
        .filter_map(|o| o.parse().ok())
        .collect();
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::list(allowed))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

/// Build the axum router for the given state.
///
/// `/command` and `/query` require the bearer token (audit S5). `/health` is an
/// open liveness probe; `/events` keeps its own Origin gate (audit S2) and is
/// not token-gated in this increment.
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route("/ask", post(ask_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));
    // `/mcp` is token-gated too, but via its own middleware that also accepts the
    // token as a `?token=` query param (tau β.3's MCP config carries a bare URL
    // with no header).
    let mcp = Router::new()
        .route("/mcp", post(mcp::mcp_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::mcp_require_token,
        ));
    // `/collab` is note-multiplexed and Origin-gated like `/events`, but also
    // needs the `?token=` handshake (browser WS clients cannot set headers), so
    // it reuses `mcp_require_token` rather than `/events`'s open group.
    let collab = Router::new()
        .route("/collab", get(collab_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::mcp_require_token,
        ));
    let open = Router::new()
        .route("/events", get(events_handler))
        .route("/health", get(health_handler));
    protected
        .merge(mcp)
        .merge(collab)
        .merge(open)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};

    fn state(dir: &std::path::Path) -> AppState {
        AppState::new(Engine::new(
            LocalFsStore::open(dir).unwrap(),
            TantivyIndex::in_memory().unwrap(),
            GitVcs::open_or_init(dir).unwrap(),
        ))
    }

    #[test]
    fn poisoned_engine_mutex_still_serves_requests() {
        let tmp = tempfile::tempdir().unwrap();
        let state = state(tmp.path());

        // Poison the mutex: panic while holding the engine lock.
        let st = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = st.engine.lock().unwrap();
            panic!("simulated engine panic under lock");
        })
        .join();
        assert!(
            state.engine.is_poisoned(),
            "precondition: mutex is poisoned"
        );

        // Despite poisoning, a request must still succeed rather than 500 forever.
        let resp = state.run_query_blocking(&Query::ListNotes);
        assert!(
            resp.is_ok(),
            "poisoned mutex must be recovered, not propagated"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn lagged_subscriber_is_logged_and_skipped() {
        // A subscriber that fell behind the broadcast capacity drops events;
        // the loop continues but the drop is observable (audit G4).
        let action = ws_event_action(Err(broadcast::error::RecvError::Lagged(7)));
        assert!(matches!(action, WsForward::Skip));
        assert!(logs_contain("ws: subscriber lagged, dropped events"));
        assert!(logs_contain("skipped=7"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn worker_panic_logs_join_error_and_returns_500() {
        // A panicking spawn_blocking worker yields a JoinError; it must be logged
        // with request context and return a generic 500 (audit G5), never leaking
        // the panic text to the client.
        let join_err = tokio::task::spawn_blocking(|| panic!("simulated worker panic"))
            .await
            .expect_err("the worker panicked, so awaiting it yields a JoinError");
        let resp = service_response::<()>(Err(join_err));

        // The client gets a generic 500; panic text never reaches the response body.
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(logs_contain("request worker panicked"));
    }
}

#[cfg(test)]
mod collab_flush_tests {
    use super::*;
    use cairn_app::Engine;
    use cairn_domain::{block::BlockKind, BlockId, BlockOp, NotePath};
    use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
    use cairn_ports::FsChange;

    fn ins(text: &str) -> BlockOp {
        ins_at(0, text)
    }

    fn ins_at(counter: u64, text: &str) -> BlockOp {
        BlockOp::Insert {
            id: BlockId {
                replica: 1,
                counter,
            },
            after: None,
            lamport: counter + 1,
            kind: BlockKind::Paragraph,
            text: text.into(),
        }
    }

    fn engine_over(tmp: &std::path::Path) -> Engine {
        Engine::new(
            LocalFsStore::open(tmp).unwrap(),
            TantivyIndex::in_memory().unwrap(),
            GitVcs::open_or_init(tmp).unwrap(),
        )
    }

    #[test]
    fn flush_materializes_commits_and_watcher_ignores_self_write() {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new(engine_over(tmp.path()));
        let path = NotePath::new("n.md").unwrap();

        // A live edit sitting in the daemon's replica, as the WS `Op` arm leaves it.
        collab::insert_dirty_session(&state.collab, &path, "", vec![ins("live edit")]);

        // One flush pass; debounce 0 ⇒ flush immediately (deterministic, no sleeps).
        state.run_collab_flush_pass(std::time::Duration::ZERO);

        // (1) Materialized to disk.
        let on_disk = std::fs::read_to_string(tmp.path().join("n.md")).unwrap();
        assert!(
            on_disk.contains("live edit"),
            "materialized markdown on disk"
        );

        // (2) Committed: the note is readable at git HEAD.
        {
            let guard = state.engine();
            let at_head = guard
                .note_at(&path, "HEAD")
                .expect("note committed at HEAD");
            assert!(at_head.contains("live edit"), "flush created a commit");
        }

        // (3) The watcher does NOT re-ingest the self-write: apply_change on the
        // just-written file emits nothing (engine stat-guard), exactly as the
        // real watcher path would see it.
        let mut guard = state.engine();
        let mut tap = EventTap {
            tx: state.events.clone(),
            collected: Vec::new(),
        };
        guard
            .apply_change(&FsChange::Changed(path.clone()), &mut tap)
            .unwrap();
        assert!(tap.collected.is_empty(), "self-write must not re-ingest");
    }

    #[test]
    fn active_session_reflushes_second_edit_through_the_real_write_path() {
        // Two sequential flushes on a session that stays open (has a
        // participant): the second edit must commit too, proving the baseline
        // handoff (`last_written`) advances through `write_note` and doesn't
        // false-trigger the non-clobber guard on the second pass.
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new(engine_over(tmp.path()));
        let path = NotePath::new("n.md").unwrap();

        collab::insert_dirty_session(&state.collab, &path, "", vec![ins_at(0, "first")]);
        collab::add_participant(&state.collab, &path, 1); // keep it open across flushes

        state.run_collab_flush_pass(std::time::Duration::ZERO);
        // Session survived the first flush (active, not abandoned).
        {
            let guard = state.engine();
            assert!(guard.note_at(&path, "HEAD").unwrap().contains("first"));
        }

        // A second live edit into the same open session, then flush again.
        collab::merge_op(&state.collab, &path, ins_at(1, "second"));
        state.run_collab_flush_pass(std::time::Duration::ZERO);

        let at_head = {
            let guard = state.engine();
            guard.note_at(&path, "HEAD").unwrap()
        };
        assert!(at_head.contains("first"), "first edit still present");
        assert!(
            at_head.contains("second"),
            "second edit committed via re-flush (baseline advanced, no false conflict)"
        );
    }

    #[test]
    fn flush_folds_foreign_disk_edit_into_replica_and_fans_out() {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new(engine_over(tmp.path()));
        let path = NotePath::new("n.md").unwrap();

        // Open session (insert_dirty_session already sets dirty=true); baseline == "seed\n".
        collab::insert_dirty_session(&state.collab, &path, "seed\n", vec![]);
        collab::add_participant(&state.collab, &path, 1);
        // Subscribe a peer BEFORE the fold to observe fan-out.
        let mut rx = collab::test_subscribe(&state.collab, &path);

        // A foreign editor rewrites the file, adding a line.
        std::fs::write(tmp.path().join("n.md"), "seed\n\nforeign line\n").unwrap();

        // Flush pass 1: disk != baseline ⇒ fold (no write this pass).
        state.run_collab_flush_pass(std::time::Duration::ZERO);

        // Folded into the daemon replica; baseline advanced to the consumed bytes.
        let (markdown, baseline) =
            collab::test_session_markdown_and_baseline(&state.collab, &path).unwrap();
        assert!(markdown.contains("foreign line"));
        assert_eq!(baseline, "seed\n\nforeign line\n");
        // Fanned out to the peer.
        let mut saw_insert = false;
        while let Ok(f) = rx.try_recv() {
            if matches!(
                collab::fanout_op(&f),
                Some(cairn_domain::BlockOp::Insert { .. })
            ) {
                saw_insert = true;
            }
        }
        assert!(saw_insert, "foreign block fanned out as an Insert");

        // Flush pass 2: now disk == baseline ⇒ the merged result is written+committed.
        state.run_collab_flush_pass(std::time::Duration::ZERO);
        let guard = state.engine();
        assert!(guard
            .note_at(&path, "HEAD")
            .unwrap()
            .contains("foreign line"));
    }

    #[test]
    fn watcher_defers_sessioned_note_to_the_flush_not_generic_ingest() {
        // A Changed(N) for a note with an open session must NOT auto-commit via
        // the generic path; it marks the session dirty so the collab flush folds
        // it. This is the same in-crate mod as the other collab-flush tests
        // (kept out of tests/watch.rs so it can reach `collab::insert_dirty_session`
        // / `collab::add_participant` without a new public test-only API surface).
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new(engine_over(tmp.path()));
        let path = NotePath::new("n.md").unwrap();

        // Open a session; baseline == on-disk == "base\n".
        std::fs::write(tmp.path().join("n.md"), "base\n").unwrap();
        collab::insert_dirty_session(&state.collab, &path, "base\n", vec![]);
        collab::add_participant(&state.collab, &path, 1); // keep it open, not reaped
                                                          // Settle it (writes/commits "base\n", matching disk) so it sits
                                                          // dirty=false, exactly like an already-flushed open session. Debounce
                                                          // ZERO so it's due regardless of quiescence (it has a participant, so
                                                          // "empty" alone would not make it due).
        state.run_collab_flush_pass(std::time::Duration::ZERO);

        // Foreign edit on disk, then the watcher fires.
        std::fs::write(tmp.path().join("n.md"), "base\n\nforeign\n").unwrap();
        state.apply_change_blocking(&FsChange::Changed(path.clone()));

        // The flush now folds it (disk != baseline), then writes+commits on pass 2.
        state.run_collab_flush_pass(std::time::Duration::ZERO); // fold
        state.run_collab_flush_pass(std::time::Duration::ZERO); // write merged
        let guard = state.engine();
        assert!(guard.note_at(&path, "HEAD").unwrap().contains("foreign"));
    }
}
