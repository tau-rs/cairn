//! cairn plugin SDK: write a plugin as command declarations + typed handlers;
//! the SDK owns the JSON-RPC/NDJSON stdio loop and the host-callback round-trip.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)
//!
//! # Example
//!
//! ```no_run
//! use cairn_plugin_sdk::{Host, Plugin, PluginError};
//! use serde::Deserialize;
//! use serde_json::{json, Value};
//!
//! #[derive(Deserialize)]
//! struct NoteLenArgs {
//!     path: String,
//! }
//!
//! let mut plugin = Plugin::new("example", env!("CARGO_PKG_VERSION"));
//!
//! // Raw JSON when you want it:
//! plugin.command("echo", "Echo", |args: Value, _host: &mut Host| Ok(args));
//!
//! // Typed args + a host-callback when you want safety:
//! plugin.command("noteLen", "Note length", |a: NoteLenArgs, host: &mut Host| {
//!     let contents = host.read_note(&a.path)?;
//!     Ok::<Value, PluginError>(json!({ "len": contents.len() }))
//! });
//!
//! plugin.run(); // owns the stdio loop; returns only on stdin EOF
//! ```
//!
//! # Trust and privileges
//!
//! A plugin the daemon agrees to run is **fully-trusted code**. It is spawned as
//! a child of the daemon and runs with the daemon's full operating-system
//! privileges — the same user, filesystem, and process access.
//!
//! The daemon's trusted-list (`[plugins].trusted` in `cairn.toml`) gates *whether*
//! a plugin runs, not *what it can do*. The `capabilities` declared in a plugin's
//! manifest serve two distinct roles:
//!
//! - `fs:read`, `fs:write`, and `events` gate host-callback methods on [`Host`]
//!   (`read_note`/`write_note`/`search`/`list_notes`). They only narrow that
//!   host-RPC surface and are orthogonal to the OS sandbox, which separately
//!   denies the plugin process direct filesystem writes and vault reads
//!   regardless of what it declares.
//! - `net` is consumed by the OS sandbox (bubblewrap on Linux, sandbox-exec on
//!   macOS). If a plugin does **not** declare `net`, the jail denies all outbound
//!   network connections at the OS level. Declaring `net` lifts that restriction.
//!   `net` gates no host-callback method.
//!
//! Approving a plugin is therefore equivalent to trusting its author and its exact
//! on-disk contents to run as you. See the trust design doc
//! (`docs/superpowers/specs/2026-06-11-cairn-plugin-trust-design.md`) and
//! the SDK design doc
//! (`docs/superpowers/specs/2026-06-10-plugin-sdk-design.md`).

use std::io::{BufRead, Write};

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, DeleteNoteParams, InitializeResult, InvokeParams,
    ListNotesResult, ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchParams,
    SearchResultDto, WriteNoteParams, JSONRPC_VERSION, METHOD_CAIRN_EVENT, METHOD_DELETE_NOTE,
    METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES, METHOD_READ_NOTE, METHOD_SEARCH,
    METHOD_WRITE_NOTE,
};
use serde_json::Value;

pub use cairn_plugin_protocol::{CairnEvent, NoteSummaryDto, SearchHitDto};

/// An error from a command handler or a host-callback. Maps to a JSON-RPC error
/// object on the wire.
#[derive(Debug, Clone)]
pub struct PluginError {
    pub code: i64,
    pub message: String,
}

impl PluginError {
    /// A handler error with the JSON-RPC "internal error" code (-32603).
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}
impl std::error::Error for PluginError {}

impl From<&str> for PluginError {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
impl From<String> for PluginError {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}
impl From<RpcError> for PluginError {
    fn from(e: RpcError) -> Self {
        Self {
            code: e.code,
            message: e.message,
        }
    }
}
impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(e.to_string())
    }
}

/// Handle passed to each command handler for calling back to the host. Each call
/// is gated host-side by the plugin's manifest-declared capabilities.
pub struct Host<'a> {
    reader: &'a mut dyn BufRead,
    stdout: &'a mut dyn Write,
    next_cb_id: &'a mut u64,
}

impl Host<'_> {
    /// Send one host-callback request and return its `result` Value (or the
    /// host's error, preserving its code+message).
    fn call(&mut self, method: &str, params: Value) -> Result<Value, PluginError> {
        *self.next_cb_id += 1;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: *self.next_cb_id,
            method: method.to_string(),
            params,
        };
        write_message(&mut self.stdout, &req)
            .map_err(|e| PluginError::new(format!("callback write failed: {e}")))?;
        let resp: Response = read_message(&mut self.reader)
            .map_err(|e| PluginError::new(format!("callback read failed: {e}")))?
            .ok_or_else(|| PluginError::new("host closed before callback response"))?;
        if let Some(err) = resp.error {
            return Err(PluginError::from(err));
        }
        resp.result
            .ok_or_else(|| PluginError::new("empty callback response"))
    }

    /// Read a note's raw contents (`host/readNote`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn read_note(&mut self, path: &str) -> Result<String, PluginError> {
        let params = serde_json::to_value(ReadNoteParams {
            path: path.to_string(),
        })?;
        let result = self.call(METHOD_READ_NOTE, params)?;
        let rn: ReadNoteResult = serde_json::from_value(result)?;
        Ok(rn.contents)
    }

    /// Create or overwrite a note (`host/writeNote`, requires `fs:write`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PluginError> {
        let params = serde_json::to_value(WriteNoteParams {
            path: path.to_string(),
            contents: contents.to_string(),
        })?;
        // host/writeNote returns an empty `{}` body on success; nothing to extract.
        self.call(METHOD_WRITE_NOTE, params)?;
        Ok(())
    }

    /// Delete a note (`host/deleteNote`, requires `fs:write`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn delete_note(&mut self, path: &str) -> Result<(), PluginError> {
        let params = serde_json::to_value(DeleteNoteParams {
            path: path.to_string(),
        })?;
        // host/deleteNote returns an empty `{}` body on success; nothing to extract.
        self.call(METHOD_DELETE_NOTE, params)?;
        Ok(())
    }

    /// Ranked full-text search (`host/search`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchHitDto>, PluginError> {
        let params = serde_json::to_value(SearchParams {
            query: query.to_string(),
        })?;
        let result = self.call(METHOD_SEARCH, params)?;
        let sr: SearchResultDto = serde_json::from_value(result)?;
        Ok(sr.hits)
    }

    /// List all notes (`host/listNotes`, requires `fs:read`).
    ///
    /// # Errors
    /// [`PluginError`] if the host denies/fails the callback.
    pub fn list_notes(&mut self) -> Result<Vec<NoteSummaryDto>, PluginError> {
        let result = self.call(METHOD_LIST_NOTES, Value::Null)?;
        let ln: ListNotesResult = serde_json::from_value(result)?;
        Ok(ln.notes)
    }
}

/// Type alias for the erased command handler stored in [`RegisteredCommand`].
type ErasedHandler = Box<dyn FnMut(Value, &mut Host<'_>) -> Result<Value, PluginError>>;

/// The erased event handler stored on the `Plugin`. Returns `()` (events are
/// acked, not result-bearing).
type ErasedEventHandler = Box<dyn FnMut(CairnEvent, &mut Host<'_>) -> Result<(), PluginError>>;

/// A registered command: id, title, and a type-erased handler. The handler is
/// higher-ranked over the `Host` borrow so one stored closure accepts a Host of
/// any lifetime.
struct RegisteredCommand {
    id: String,
    title: String,
    handler: ErasedHandler,
}

/// A plugin: a name/version and a set of typed commands. Build it, then
/// [`Plugin::run`].
pub struct Plugin {
    name: String,
    version: String,
    commands: Vec<RegisteredCommand>,
    event_handler: Option<ErasedEventHandler>,
    contributions: Vec<cairn_plugin_protocol::PluginContribution>,
}

impl Plugin {
    /// Create an empty plugin.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            commands: Vec::new(),
            event_handler: None,
            contributions: Vec::new(),
        }
    }

    /// Register a handler for pushed cairn events (`cairn/event`). The handler
    /// gets capability-gated `Host` access to react (read/write the cairn).
    pub fn on_event<F>(&mut self, handler: F)
    where
        F: FnMut(CairnEvent, &mut Host<'_>) -> Result<(), PluginError> + 'static,
    {
        self.event_handler = Some(Box::new(handler));
    }

    /// Register a typed command. `A` is deserialized from the invoke args; `O` is
    /// serialized into the result. Malformed args fail the invoke with JSON-RPC
    /// code -32602.
    pub fn command<A, O, F>(
        &mut self,
        id: impl Into<String>,
        title: impl Into<String>,
        mut handler: F,
    ) where
        A: serde::de::DeserializeOwned,
        O: serde::Serialize,
        F: FnMut(A, &mut Host<'_>) -> Result<O, PluginError> + 'static,
    {
        let boxed: ErasedHandler = Box::new(move |raw: Value, host: &mut Host<'_>| {
            let args: A = serde_json::from_value(raw).map_err(|e| PluginError {
                code: -32602,
                message: e.to_string(),
            })?;
            let out: O = handler(args, host)?;
            Ok(serde_json::to_value(out)?)
        });
        self.commands.push(RegisteredCommand {
            id: id.into(),
            title: title.into(),
            handler: boxed,
        });
    }

    /// Declare a UI contribution surfaced to the shell at `initialize`.
    pub fn contribution(&mut self, c: cairn_plugin_protocol::PluginContribution) {
        self.contributions.push(c);
    }

    /// Run the stdio loop until stdin EOF, using real stdin/stdout.
    pub fn run(self) {
        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin.lock());
        let mut stdout = std::io::stdout();
        self.run_io(&mut reader, &mut stdout);
    }

    /// The loop, parameterized over IO for testing. Reads a `Request`, dispatches,
    /// writes a `Response`, until EOF or a read/write error.
    fn run_io(mut self, mut reader: &mut dyn BufRead, mut stdout: &mut dyn Write) {
        let mut next_cb_id: u64 = 1000;
        while let Ok(Some(req)) = read_message::<_, Request>(&mut reader) {
            let resp = self.handle(&req, &mut reader, &mut stdout, &mut next_cb_id);
            if write_message(&mut stdout, &resp).is_err() {
                break;
            }
        }
    }

    fn handle(
        &mut self,
        req: &Request,
        reader: &mut dyn BufRead,
        stdout: &mut dyn Write,
        next_cb_id: &mut u64,
    ) -> Response {
        let mut resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: req.id,
            result: None,
            error: None,
        };
        match req.method.as_str() {
            METHOD_INITIALIZE => {
                let init = InitializeResult {
                    name: self.name.clone(),
                    version: self.version.clone(),
                    commands: self
                        .commands
                        .iter()
                        .map(|c| CommandDecl {
                            id: c.id.clone(),
                            title: c.title.clone(),
                        })
                        .collect(),
                    contributions: self.contributions.clone(),
                };
                resp.result = Some(serde_json::to_value(init).unwrap_or(Value::Null));
            }
            METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
                Ok(p) => match self.commands.iter_mut().find(|c| c.id == p.command) {
                    Some(cmd) => {
                        let mut host = Host {
                            reader,
                            stdout,
                            next_cb_id,
                        };
                        match (cmd.handler)(p.args, &mut host) {
                            Ok(value) => resp.result = Some(value),
                            Err(e) => {
                                resp.error = Some(RpcError {
                                    code: e.code,
                                    message: e.message,
                                })
                            }
                        }
                    }
                    None => {
                        resp.error = Some(RpcError {
                            code: -32601,
                            message: format!("unknown command {}", p.command),
                        });
                    }
                },
                Err(e) => {
                    resp.error = Some(RpcError {
                        code: -32602,
                        message: e.to_string(),
                    });
                }
            },
            METHOD_CAIRN_EVENT => match serde_json::from_value::<CairnEvent>(req.params.clone()) {
                Ok(ev) => {
                    if let Some(handler) = self.event_handler.as_mut() {
                        let mut host = Host {
                            reader,
                            stdout,
                            next_cb_id,
                        };
                        match handler(ev, &mut host) {
                            Ok(()) => resp.result = Some(serde_json::json!({})),
                            Err(e) => {
                                resp.error = Some(RpcError {
                                    code: e.code,
                                    message: e.message,
                                })
                            }
                        }
                    } else {
                        resp.result = Some(serde_json::json!({})); // no handler: ack
                    }
                }
                Err(e) => {
                    resp.error = Some(RpcError {
                        code: -32602,
                        message: e.to_string(),
                    });
                }
            },
            other => {
                resp.error = Some(RpcError {
                    code: -32601,
                    message: format!("unknown method {other}"),
                });
            }
        }
        resp
    }
}

#[cfg(test)]
mod host_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_note_sends_request_and_parses_response() {
        // Canned host response to our host/readNote callback.
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: Some(
                serde_json::to_value(ReadNoteResult {
                    contents: "hello".to_string(),
                })
                .unwrap(),
            ),
            error: None,
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        let contents = {
            let mut host = Host {
                reader: &mut reader,
                stdout: &mut out,
                next_cb_id: &mut cb_id,
            };
            host.read_note("note.md").unwrap()
        };
        assert_eq!(contents, "hello");
        assert_eq!(cb_id, 1001);
        // The SDK wrote a host/readNote request with the right params.
        let first_line = out.split(|&b| b == b'\n').next().unwrap();
        let written: Request = serde_json::from_slice(first_line).unwrap();
        assert_eq!(written.method, METHOD_READ_NOTE);
        assert_eq!(written.params["path"], "note.md");
    }

    #[test]
    fn delete_note_sends_request() {
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: Some(serde_json::json!({})),
            error: None,
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        {
            let mut host = Host {
                reader: &mut reader,
                stdout: &mut out,
                next_cb_id: &mut cb_id,
            };
            host.delete_note("gone.md").unwrap();
        }
        assert_eq!(cb_id, 1001); // the callback-id counter was incremented
        let first_line = out.split(|&b| b == b'\n').next().unwrap();
        let written: Request = serde_json::from_slice(first_line).unwrap();
        assert_eq!(written.method, METHOD_DELETE_NOTE);
        assert_eq!(written.params["path"], "gone.md");
    }

    #[test]
    fn denied_callback_becomes_error_preserving_code() {
        let mut response_bytes = Vec::new();
        let resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: 1001,
            result: None,
            error: Some(RpcError {
                code: -32001,
                message: "capability fs:read not declared".to_string(),
            }),
        };
        write_message(&mut response_bytes, &resp).unwrap();

        let mut reader = Cursor::new(response_bytes);
        let mut out: Vec<u8> = Vec::new();
        let mut cb_id = 1000u64;
        let mut host = Host {
            reader: &mut reader,
            stdout: &mut out,
            next_cb_id: &mut cb_id,
        };
        let err = host.read_note("note.md").unwrap_err();
        assert_eq!(err.code, -32001);
        assert!(err.message.contains("fs:read"));
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use cairn_plugin_protocol::{InitializeResult, METHOD_INITIALIZE, METHOD_INVOKE};
    use std::io::Cursor;

    fn request_line(id: u64, method: &str, params: Value) -> Vec<u8> {
        let mut buf = Vec::new();
        write_message(
            &mut buf,
            &Request {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id,
                method: method.to_string(),
                params,
            },
        )
        .unwrap();
        buf
    }

    fn drive(plugin: Plugin, input: &[u8]) -> Vec<Response> {
        let mut reader = Cursor::new(input.to_vec());
        let mut out: Vec<u8> = Vec::new();
        plugin.run_io(&mut reader, &mut out);
        out.split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice::<Response>(l).unwrap())
            .collect()
    }

    #[test]
    fn initialize_lists_commands_in_order() {
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.command("a", "A", |v: Value, _h| Ok(v));
        plugin.command("b", "B", |v: Value, _h| Ok(v));
        let out = drive(plugin, &request_line(1, METHOD_INITIALIZE, Value::Null));
        let init: InitializeResult =
            serde_json::from_value(out[0].result.clone().unwrap()).unwrap();
        assert_eq!(init.name, "ex");
        assert_eq!(init.version, "0.1.0");
        assert_eq!(
            init.commands
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn initialize_includes_declared_contributions() {
        use cairn_plugin_protocol::{PluginContribution, PluginSlot, PluginWidget};
        let mut p = Plugin::new("t", "0.1.0");
        p.contribution(PluginContribution {
            id: "s".into(),
            slot: PluginSlot::SidebarSection,
            widget: PluginWidget::Text {
                text: "hi".into(),
                muted: None,
            },
            title: None,
            icon: None,
            order: None,
        });
        let out = drive(p, &request_line(1, METHOD_INITIALIZE, Value::Null));
        let init: InitializeResult =
            serde_json::from_value(out[0].result.clone().unwrap()).unwrap();
        assert_eq!(init.contributions.len(), 1);
    }

    #[test]
    fn echo_roundtrips_and_unknown_command_is_minus_32601() {
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.command("echo", "Echo", |v: Value, _h| Ok(v));
        let mut input = request_line(
            1,
            METHOD_INVOKE,
            serde_json::json!({ "command": "echo", "args": { "x": 1 } }),
        );
        input.extend(request_line(
            2,
            METHOD_INVOKE,
            serde_json::json!({ "command": "nope", "args": null }),
        ));
        let out = drive(plugin, &input);
        assert_eq!(
            out[0].result.clone().unwrap(),
            serde_json::json!({ "x": 1 })
        );
        assert_eq!(out[1].error.clone().unwrap().code, -32601);
    }

    #[test]
    fn bad_args_is_minus_32602() {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
        }
        let mut plugin = Plugin::new("ex", "0.1.0");
        // Reads `a.path` so the field isn't dead; on missing `path`, deserialize fails.
        plugin.command("needs", "Needs", |a: Args, _h| Ok(Value::String(a.path)));
        let out = drive(
            plugin,
            &request_line(
                1,
                METHOD_INVOKE,
                serde_json::json!({ "command": "needs", "args": {} }),
            ),
        );
        assert_eq!(out[0].error.clone().unwrap().code, -32602);
    }

    #[test]
    fn on_event_acks_and_handles() {
        use cairn_plugin_protocol::{CairnEvent, CairnEventKind, METHOD_CAIRN_EVENT};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();
        let mut plugin = Plugin::new("ex", "0.1.0");
        plugin.on_event(move |ev: CairnEvent, _host| {
            assert_eq!(ev.path, "x.md");
            ran2.store(true, Ordering::SeqCst);
            Ok(())
        });
        let ev = CairnEvent {
            kind: CairnEventKind::NoteChanged,
            path: "x.md".into(),
        };
        let out = drive(
            plugin,
            &request_line(1, METHOD_CAIRN_EVENT, serde_json::to_value(ev).unwrap()),
        );
        assert!(ran.load(Ordering::SeqCst), "handler should have run");
        assert_eq!(out[0].result.clone().unwrap(), serde_json::json!({}));
    }
}
