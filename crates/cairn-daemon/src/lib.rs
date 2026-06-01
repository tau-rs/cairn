//! HTTP + WebSocket transport over the cairn dispatcher. Binds localhost
//! only; no authentication (LoopbackTrust). The engine runs synchronously
//! under a mutex via `spawn_blocking`.

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
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, ServiceError};
use tokio::sync::broadcast;

/// The concrete engine the daemon serves.
pub type CairnEngine = Engine<LocalFsStore, InMemoryIndex, GitVcs>;

/// Shared daemon state: the engine behind a mutex + an event broadcast.
#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<CairnEngine>>,
    events: broadcast::Sender<WireEvent>,
}

/// An `EventSink` that republishes engine events as wire events.
struct BroadcastSink(broadcast::Sender<WireEvent>);
impl EventSink for BroadcastSink {
    fn emit(&mut self, event: AppEvent) {
        // No subscribers is not an error.
        let _ = self.0.send(app_event_to_wire(event));
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
        }
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
        let mut sink = BroadcastSink(self.events.clone());
        dispatch_command(&mut guard, command, &mut sink)
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
                        if socket.send(Message::Text(text)).await.is_err() {
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

/// Build the axum router for the given state.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/command", post(command_handler))
        .route("/query", post(query_handler))
        .route("/events", get(events_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}
