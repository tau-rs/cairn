//! HTTP + WebSocket transport over the cairn dispatcher. Binds localhost
//! only; no authentication (LoopbackTrust). The engine runs synchronously
//! under a mutex via `spawn_blocking`.

pub mod config;
pub use config::Config;

use std::sync::{Arc, Mutex};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{
    Command, CommandResponse, ContractError, Event as WireEvent, Query, QueryResponse,
};
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, ServiceError};
use tokio::sync::broadcast;

/// The concrete engine the daemon serves.
pub type CairnEngine = Engine<LocalFsStore, TantivyIndex, GitVcs>;

/// Shared daemon state: the engine behind a mutex + an event broadcast.
#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<CairnEngine>>,
    events: broadcast::Sender<WireEvent>,
    /// Origins permitted to open the `/events` WebSocket. Same allowlist the
    /// CORS layer enforces; empty denies all (deny-by-default).
    allowed_origins: Arc<[String]>,
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
    pub fn new(engine: CairnEngine) -> Self {
        // 256 comfortably exceeds any realistic per-command event burst;
        // slow subscribers lag-drop rather than back-pressure the engine.
        let (events, _rx) = broadcast::channel(256);
        Self {
            engine: Arc::new(Mutex::new(engine)),
            events,
            allowed_origins: Arc::from([]),
        }
    }

    /// Set the origins permitted to open the `/events` WebSocket. Reuse the
    /// daemon's CORS allowlist so HTTP and WS share one origin policy.
    #[must_use]
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = Arc::from(origins.into_boxed_slice());
        self
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
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
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
        let guard = self.engine.lock().expect("engine mutex poisoned");
        dispatch_query(&guard, query)
    }

    /// Apply a watcher-reported filesystem change, publishing any resulting
    /// events to subscribers. Best-effort: a transient failure is logged, not
    /// propagated, so the watch loop keeps running.
    pub fn apply_change_blocking(&self, change: &cairn_ports::FsChange) {
        let mut guard = self.engine.lock().expect("engine mutex poisoned");
        let mut tap = EventTap {
            tx: self.events.clone(),
            collected: Vec::new(),
        };
        if let Err(e) = guard.apply_change(change, &mut tap) {
            eprintln!("watch: apply_change failed: {e}");
            return;
        }
        let collected = tap.collected;
        for pe in collected.iter().filter_map(to_plugin_event) {
            let mut fwd = BroadcastSink(self.events.clone());
            guard.dispatch_plugin_event(&pe, &mut fwd);
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
        Err(_join) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ContractError::Internal {
                message: "internal error".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn command_handler(State(state): State<AppState>, Json(command): Json<Command>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_command_blocking(&command)).await;
    service_response(result)
}

async fn query_handler(State(state): State<AppState>, Json(query): Json<Query>) -> Response {
    let result = tokio::task::spawn_blocking(move || state.run_query_blocking(&query)).await;
    service_response(result)
}

async fn events_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
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
                match event {
                    Ok(ev) => {
                        let Ok(text) = serde_json::to_string(&ev) else { continue };
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
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
        .allow_headers([header::CONTENT_TYPE])
}

/// Build the axum router for the given state.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route("/events", get(events_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}
