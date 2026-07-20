//! `PluginHost` backed by child processes speaking JSON-RPC/NDJSON over stdio.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::PinnedHash;
use cairn_plugin_protocol::{
    write_message, CairnEvent, CairnEventKind, CommandDecl, DeleteNoteParams, Incoming,
    InitializeParams, InitializeResult, InvokeParams, ListNotesResult, Manifest, NoteSummaryDto,
    ProcessContentParams, ProcessContentResult, ProcessorDecl, ReadNoteParams, ReadNoteResult,
    Request, Response, RpcError, SearchHitDto, SearchParams, SearchResultDto, WriteNoteParams,
    CALLBACK_DENIED, CALLBACK_FAILED, CAP_CONTENT_PROCESS, CAP_EVENTS, CAP_FS_READ, CAP_FS_WRITE,
    CAP_NET, JSONRPC_VERSION, METHOD_CAIRN_EVENT, METHOD_DELETE_NOTE, METHOD_INITIALIZE,
    METHOD_INVOKE, METHOD_LIST_NOTES, METHOD_PROCESS_CONTENT, METHOD_READ_NOTE, METHOD_SEARCH,
    METHOD_WRITE_NOTE,
};
use cairn_ports::{
    AdapterError, EventDispatchError, PluginCallbacks, PluginCommand, PluginEvent, PluginHost,
    PluginInfo, PortError, Sandbox, SandboxCapabilities,
};

/// Map a ports event to its wire form for delivery to plugins.
fn to_cairn_event(event: &PluginEvent) -> CairnEvent {
    match event {
        PluginEvent::NoteChanged(p) => CairnEvent {
            kind: CairnEventKind::NoteChanged,
            path: p.as_str().to_string(),
        },
        PluginEvent::NoteDeleted(p) => CairnEvent {
            kind: CairnEventKind::NoteDeleted,
            path: p.as_str().to_string(),
        },
    }
}

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

/// Default per-message timeout for plugin reads: a plugin silent longer than this
/// is treated as hung and killed.
pub const DEFAULT_PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Floor for the one-time `initialize` handshake during load. The per-message
/// `timeout` governs steady-state invokes — a hung invoke must fail fast, so
/// callers may set it small (tests use 2s). But a cold plugin's first handshake
/// (process start + sandbox setup + interpreter warm-up) can be far slower under
/// machine load; binding it to that small per-message timeout spuriously drops
/// trusted plugins at load. The handshake therefore reads its reply under
/// `max(timeout, STARTUP_HANDSHAKE_FLOOR)`; once it completes, the per-message
/// timeout is restored for invokes and events.
pub const STARTUP_HANDSHAKE_FLOOR: Duration = Duration::from_secs(10);

/// The capability a host-callback method requires, or `None` if the method is
/// unknown to the host.
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some(CAP_FS_READ),
        METHOD_WRITE_NOTE => Some(CAP_FS_WRITE),
        METHOD_DELETE_NOTE => Some(CAP_FS_WRITE),
        METHOD_SEARCH => Some(CAP_FS_READ),
        METHOD_LIST_NOTES => Some(CAP_FS_READ),
        _ => None,
    }
}

/// Whether any of a plugin's processor declarations matches `path` by extension.
/// An empty `extensions` list matches every note; a plugin with no declarations
/// matches nothing.
fn processor_matches(decls: &[ProcessorDecl], path: &str) -> bool {
    decls.iter().any(|d| {
        d.extensions.is_empty()
            || d.extensions
                .iter()
                .any(|e| path.ends_with(&format!(".{e}")))
    })
}

/// Fold `content` through processors invoked in `order` (a list of plugin ids,
/// pre-sorted). Fail-soft: a processor that errors is logged and skipped, and the
/// chain continues from the last good content.
fn fold_content(
    mut content: String,
    order: &[String],
    mut invoke: impl FnMut(&str, &str) -> Result<String, PortError>,
) -> String {
    for id in order {
        match invoke(id, &content) {
            Ok(next) => content = next,
            Err(e) => {
                tracing::warn!(plugin = %id, error = %e, "content processor failed; keeping prior content");
            }
        }
    }
    content
}

/// Wraps a callbacks handler for the duration of `content/process`: reads pass
/// through; writes/deletes are refused so a render stays side-effect-free (and
/// cannot loop back through the engine's event sink). Not a security boundary —
/// a plugin can still write to disk directly; this only closes the host-callback
/// path (see the design doc, D4).
struct ReadOnlyCallbacks<'a>(&'a mut dyn PluginCallbacks);

impl PluginCallbacks for ReadOnlyCallbacks<'_> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0.read_note(path)
    }
    fn search(&mut self, query: &str) -> Result<Vec<cairn_ports::SearchHit>, PortError> {
        self.0.search(query)
    }
    fn list_notes(&mut self) -> Result<Vec<cairn_domain::Note>, PortError> {
        self.0.list_notes()
    }
    fn write_note(&mut self, _path: &str, _contents: &str) -> Result<(), PortError> {
        Err(PortError::Adapter(
            "write not permitted during content processing".into(),
        ))
    }
    fn delete_note(&mut self, _path: &str) -> Result<(), PortError> {
        Err(PortError::Adapter(
            "delete not permitted during content processing".into(),
        ))
    }
}

/// The set of plugin **directory names** the user has explicitly trusted, each
/// with an optional pinned content hash. A plugin under
/// `<cairn>/.cairn/plugins/<dir>` is spawned only if `<dir>` is a key here; if
/// its value is `Some(pin)`, the directory's contents must hash to `pin` or it
/// is refused (drift). `None` = trusted but unpinned (spawns with a warning).
/// An empty map trusts nothing.
#[derive(Debug, Default, Clone)]
pub struct TrustedPlugins(HashMap<String, Option<PinnedHash>>);

impl TrustedPlugins {
    /// A map that trusts no plugin (default-deny).
    pub fn none() -> Self {
        Self::default()
    }

    /// Build from directory names, all unpinned. Retained for callers (and
    /// tests) that only express name trust.
    pub fn from_ids<I: IntoIterator<Item = String>>(ids: I) -> Self {
        Self(ids.into_iter().map(|id| (id, None)).collect())
    }

    /// Build from `(dir_name, optional_pin_string)` pairs, parsing each pin.
    ///
    /// # Errors
    /// [`PortError::Adapter`] if any pin string is malformed (fail-fast: a
    /// typo'd pin must not silently degrade to "unpinned").
    pub fn from_entries<I: IntoIterator<Item = (String, Option<String>)>>(
        entries: I,
    ) -> Result<Self, PortError> {
        let mut map = HashMap::new();
        for (dir, pin) in entries {
            let parsed = pin.map(|p| PinnedHash::parse(&p)).transpose()?;
            map.insert(dir, parsed);
        }
        Ok(Self(map))
    }

    /// Trust + pin for a directory name. Outer `None` ⇒ not trusted; inner
    /// `None` ⇒ trusted but unpinned; `Some(&pin)` ⇒ pinned.
    pub fn get(&self, dir_name: &str) -> Option<&Option<PinnedHash>> {
        self.0.get(dir_name)
    }
}

struct LoadedPlugin {
    child: Child,
    stdin: ChildStdin,
    /// Lines from the plugin's stdout, fed by a background reader thread so reads
    /// can be bounded by `timeout` (std pipe reads can't be interrupted directly).
    rx: Receiver<std::io::Result<String>>,
    reader: Option<JoinHandle<()>>,
    timeout: Duration,
    info: PluginInfo,
    next_id: u64,
    /// Capabilities the manifest declared; gates host-callbacks.
    capabilities: Vec<String>,
    /// Content-processor declarations the plugin returned at `initialize`.
    processors: Vec<ProcessorDecl>,
}

impl LoadedPlugin {
    /// Receive + parse the next message, killing the plugin if it stalls past the
    /// timeout. `Ok(None)` on a clean EOF (the reader thread ended).
    fn recv_message<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, PortError> {
        match self.rx.recv_timeout(self.timeout) {
            Ok(Ok(line)) => serde_json::from_str(&line).map(Some).map_err(adapt),
            Ok(Err(e)) => Err(adapt(e)),
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                let _ = self.child.wait(); // reap now so a long-lived host doesn't accrue zombies
                Err(PortError::Adapter(
                    format!("plugin {} timed out after {:?}", self.info.id, self.timeout).into(),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    /// Send one request and read its response.
    fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: self.next_id,
            method: method.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        let resp: Response = self
            .recv_message()?
            .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
        if let Some(err) = resp.error {
            return Err(PortError::Adapter(
                format!("plugin error: {}", err.message).into(),
            ));
        }
        resp.result
            .ok_or_else(|| PortError::Adapter("plugin response had no result".into()))
    }

    /// Invoke a command, servicing any host-callbacks the plugin sends until it
    /// returns the response to our invoke request.
    /// Send one request and run the dispatch loop, servicing host-callbacks until
    /// the matching-id response arrives. Shared by invoke and event delivery.
    fn call_with_callbacks(
        &mut self,
        method: &str,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let req_id = self.next_id;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: req_id,
            method: method.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        loop {
            let msg: Incoming = self
                .recv_message()?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
            match msg {
                Incoming::Response(resp) => {
                    if resp.id != req_id {
                        continue; // stray id; one-in-flight invariant, ignore
                    }
                    if let Some(err) = resp.error {
                        return Err(PortError::Adapter(
                            format!("plugin error: {}", err.message).into(),
                        ));
                    }
                    return resp
                        .result
                        .ok_or_else(|| PortError::Adapter("plugin response had no result".into()));
                }
                Incoming::Request(cb) => {
                    let response = self.service_callback(&cb, callbacks);
                    write_message(&mut self.stdin, &response).map_err(adapt)?;
                }
            }
        }
    }

    /// Invoke a command, servicing any host-callbacks until the plugin responds.
    fn invoke_command(
        &mut self,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.call_with_callbacks(METHOD_INVOKE, params, callbacks)
    }

    /// Deliver one cairn event, servicing any host-callbacks the handler makes.
    fn deliver_event(
        &mut self,
        event: &CairnEvent,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<(), PortError> {
        let params = serde_json::to_value(event).map_err(adapt)?;
        self.call_with_callbacks(METHOD_CAIRN_EVENT, params, callbacks)?;
        Ok(())
    }

    /// Send one `content/process` request and return the transformed content,
    /// servicing the plugin's (read-only) callbacks meanwhile.
    fn process_one(
        &mut self,
        path: &str,
        content: &str,
        cb: &mut dyn PluginCallbacks,
    ) -> Result<String, PortError> {
        let params = serde_json::to_value(ProcessContentParams {
            path: path.to_string(),
            content: content.to_string(),
        })
        .map_err(adapt)?;
        let result = self.call_with_callbacks(METHOD_PROCESS_CONTENT, params, cb)?;
        let out: ProcessContentResult = serde_json::from_value(result).map_err(adapt)?;
        Ok(out.content)
    }

    /// Build the response to one host-callback request, gating on capability.
    fn service_callback(&self, cb: &Request, callbacks: &mut dyn PluginCallbacks) -> Response {
        let mut resp = Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: cb.id,
            result: None,
            error: None,
        };
        let required = required_cap(&cb.method);
        match required {
            None => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("unknown host method {}", cb.method),
                });
            }
            Some(cap) if !self.capabilities.iter().any(|c| c == cap) => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("capability {cap} not declared"),
                });
            }
            // The cap is declared; dispatch the method. This match must stay in
            // sync with `required_cap` — the `_` arm is only reachable if a method
            // gains a capability there without a dispatch arm here.
            Some(_) => match cb.method.as_str() {
                METHOD_READ_NOTE => {
                    match serde_json::from_value::<ReadNoteParams>(cb.params.clone()) {
                        Ok(p) => match callbacks.read_note(&p.path) {
                            Ok(contents) => {
                                resp.result =
                                    serde_json::to_value(ReadNoteResult { contents }).ok();
                            }
                            Err(e) => {
                                resp.error = Some(RpcError {
                                    code: CALLBACK_FAILED,
                                    message: e.to_string(),
                                });
                            }
                        },
                        Err(e) => {
                            resp.error = Some(RpcError {
                                code: CALLBACK_FAILED,
                                message: e.to_string(),
                            });
                        }
                    }
                }
                METHOD_WRITE_NOTE => {
                    match serde_json::from_value::<WriteNoteParams>(cb.params.clone()) {
                        Ok(p) => match callbacks.write_note(&p.path, &p.contents) {
                            Ok(()) => resp.result = Some(serde_json::json!({})),
                            Err(e) => {
                                resp.error = Some(RpcError {
                                    code: CALLBACK_FAILED,
                                    message: e.to_string(),
                                });
                            }
                        },
                        Err(e) => {
                            resp.error = Some(RpcError {
                                code: CALLBACK_FAILED,
                                message: e.to_string(),
                            });
                        }
                    }
                }
                METHOD_DELETE_NOTE => {
                    match serde_json::from_value::<DeleteNoteParams>(cb.params.clone()) {
                        Ok(p) => match callbacks.delete_note(&p.path) {
                            Ok(()) => resp.result = Some(serde_json::json!({})),
                            Err(e) => {
                                resp.error = Some(RpcError {
                                    code: CALLBACK_FAILED,
                                    message: e.to_string(),
                                });
                            }
                        },
                        Err(e) => {
                            resp.error = Some(RpcError {
                                code: CALLBACK_FAILED,
                                message: e.to_string(),
                            });
                        }
                    }
                }
                METHOD_SEARCH => match serde_json::from_value::<SearchParams>(cb.params.clone()) {
                    Ok(p) => match callbacks.search(&p.query) {
                        Ok(hits) => {
                            let dto = SearchResultDto {
                                hits: hits
                                    .into_iter()
                                    .map(|h| SearchHitDto {
                                        path: h.path.as_str().to_string(),
                                        score: h.score,
                                        snippet: h.snippet,
                                    })
                                    .collect(),
                            };
                            resp.result = serde_json::to_value(dto).ok();
                        }
                        Err(e) => {
                            resp.error = Some(RpcError {
                                code: CALLBACK_FAILED,
                                message: e.to_string(),
                            });
                        }
                    },
                    Err(e) => {
                        resp.error = Some(RpcError {
                            code: CALLBACK_FAILED,
                            message: e.to_string(),
                        });
                    }
                },
                METHOD_LIST_NOTES => match callbacks.list_notes() {
                    Ok(notes) => {
                        let dto = ListNotesResult {
                            notes: notes
                                .into_iter()
                                .map(|n| NoteSummaryDto {
                                    path: n.path.as_str().to_string(),
                                    title: n.display_title(),
                                })
                                .collect(),
                        };
                        resp.result = serde_json::to_value(dto).ok();
                    }
                    Err(e) => {
                        resp.error = Some(RpcError {
                            code: CALLBACK_FAILED,
                            message: e.to_string(),
                        });
                    }
                },
                _ => {
                    resp.error = Some(RpcError {
                        code: CALLBACK_DENIED,
                        message: format!("unknown host method {}", cb.method),
                    });
                }
            },
        }
        resp
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        // Kill first so the reader thread's read_line hits EOF and exits, then join.
        // (A plugin that leaks the stdout pipe to a surviving grandchild would delay
        // EOF — and thus this join — until that grandchild also closes it.)
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}

/// Spawns and talks to plugins under a plugins directory.
#[derive(Default)]
pub struct ProcessPluginHost {
    loaded: Vec<LoadedPlugin>,
}

impl ProcessPluginHost {
    /// Load every `<dir>/<id>/manifest.toml` with the default read timeout.
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load(
        dir: &Path,
        trusted: &TrustedPlugins,
        sandbox: &dyn Sandbox,
    ) -> Result<Self, PortError> {
        Self::load_with_timeout(dir, DEFAULT_PLUGIN_TIMEOUT, trusted, sandbox)
    }

    /// Like [`Self::load`] but with an explicit per-message read `timeout` (used by
    /// tests, and the seam for future config).
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load_with_timeout(
        dir: &Path,
        timeout: Duration,
        trusted: &TrustedPlugins,
        sandbox: &dyn Sandbox,
    ) -> Result<Self, PortError> {
        let mut loaded = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(adapt(e)),
        };
        // The vault root is the user's cairn directory. The plugins dir is always
        // `<vault>/.cairn/plugins`, so the vault root is its grandparent. The
        // sandbox denies direct reads of it (plugins reach notes only via the
        // gated host channel). The daemon always passes `<vault>/.cairn/plugins`,
        // so the grandparent is derivable in practice.
        let vault_root = match dir.parent().and_then(|p| p.parent()) {
            Some(root) => root.to_path_buf(),
            None => {
                // Degenerate layout (e.g. a top-level plugins dir): we cannot
                // locate the vault root, so we cannot guarantee that vault notes
                // above `dir` are denied. Fall back to denying `dir` itself and
                // warn loudly rather than silently running with a weaker boundary.
                tracing::warn!(
                    "plugin: cannot derive vault root from {} (unexpected layout); \
                     vault-read protection may be incomplete",
                    dir.display()
                );
                dir.to_path_buf()
            }
        };
        for entry in entries {
            let plugin_dir = match entry {
                Ok(e) if e.path().is_dir() => e.path(),
                _ => continue,
            };
            // Trust gate: the directory name (not the manifest's self-declared
            // id) is the trust anchor. Untrusted dirs are skipped before their
            // manifest is even read, so attacker-controlled TOML is never parsed.
            // A non-UTF-8 directory name yields `None` here; treat it as the
            // empty string, which no sane trusted set contains, so it is skipped.
            let dir_name = plugin_dir.file_name().and_then(|n| n.to_str());
            let Some(dir_name) = dir_name.filter(|n| !n.is_empty()) else {
                continue; // unnameable in a trust list; never spawn it
            };
            let pin = match trusted.get(dir_name) {
                None => {
                    tracing::warn!(
                        "plugin: skipping {dir_name} (not in [plugins] trusted; \
                         add \"{dir_name}\" to cairn.toml to enable)"
                    );
                    continue;
                }
                Some(pin) => pin,
            };
            // Trusted: hash the directory tree before spawning. A symlink /
            // non-regular file / IO error here is a refusal, not a panic.
            let computed = match PinnedHash::of_dir(&plugin_dir) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("plugin: refusing {dir_name}: {e}");
                    continue;
                }
            };
            match pin {
                Some(expected) if &computed != expected => {
                    tracing::warn!(
                        "plugin: refusing {dir_name}: contents changed (pinned {expected}, \
                         found {computed}); re-approve by updating hash in cairn.toml"
                    );
                    continue;
                }
                // The guard arm above consumed the mismatch, so reaching here
                // means computed == expected — pinned and verified: spawn below.
                Some(_) => {}
                None => {
                    tracing::warn!(
                        "plugin: {dir_name} is trusted but unpinned; pin it by setting \
                         hash = \"{computed}\" in cairn.toml"
                    );
                }
            }
            match Self::spawn_plugin(&plugin_dir, timeout, sandbox, &vault_root) {
                Ok(p) => loaded.push(p),
                Err(e) => tracing::warn!("plugin: refusing {}: {e}", plugin_dir.display()),
            }
        }
        Ok(Self { loaded })
    }

    fn spawn_plugin(
        plugin_dir: &Path,
        timeout: Duration,
        sandbox: &dyn Sandbox,
        vault_root: &Path,
    ) -> Result<LoadedPlugin, PortError> {
        let raw = std::fs::read_to_string(plugin_dir.join("manifest.toml")).map_err(adapt)?;
        let manifest: Manifest = toml::from_str(&raw).map_err(adapt)?;

        // The directory name is the trust anchor (see `TrustedPlugins`); a
        // manifest that self-declares a different id is rejected so "directory
        // name" and "plugin id" stay the same value end to end.
        let dir_name = plugin_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if manifest.id != dir_name {
            return Err(PortError::Adapter(
                format!(
                    "manifest id \"{}\" does not match directory name \"{dir_name}\"",
                    manifest.id
                )
                .into(),
            ));
        }

        let cmd_path = {
            let p = Path::new(&manifest.engine.command);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                plugin_dir.join(p)
            }
        };
        let caps = sandbox_caps(&manifest.engine.capabilities);
        let mut command = sandbox
            .wrap(
                vault_root,
                plugin_dir,
                &cmd_path,
                &manifest.engine.args,
                caps,
            )
            .map_err(adapt)?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(adapt)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PortError::Adapter("no stdin".into()))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Adapter("no stdout".into()))?;
        let (tx, rx) = mpsc::channel::<std::io::Result<String>>();
        let reader = std::thread::spawn(move || {
            let mut stdout = BufReader::new(child_stdout);
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) => break, // EOF: drop tx -> channel disconnects
                    Ok(_) => {
                        if line.trim().is_empty() {
                            continue; // skip blank lines (matches old read_message)
                        }
                        if tx.send(Ok(line)).is_err() {
                            break; // consumer (LoadedPlugin) was dropped
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let mut plugin = LoadedPlugin {
            child,
            stdin,
            rx,
            reader: Some(reader),
            // The one-time `initialize` handshake below reads under a generous
            // floor (see `STARTUP_HANDSHAKE_FLOOR`); the caller's per-message
            // `timeout` is restored once it completes.
            timeout: timeout.max(STARTUP_HANDSHAKE_FLOOR),
            info: PluginInfo {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                commands: Vec::new(),
                contributions: Vec::new(),
            },
            next_id: 0,
            capabilities: manifest.engine.capabilities.clone(),
            processors: Vec::new(),
        };
        let init_params = serde_json::to_value(InitializeParams {
            host_version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .map_err(adapt)?;
        let result = plugin.call(METHOD_INITIALIZE, init_params)?;
        let init: InitializeResult = serde_json::from_value(result).map_err(adapt)?;
        plugin.info.commands = init
            .commands
            .into_iter()
            .map(|CommandDecl { id, title }| PluginCommand { id, title })
            .collect();
        plugin.info.name = init.name;
        plugin.info.version = init.version;
        plugin.info.contributions = init.contributions;
        plugin.processors = init.processors;
        // Handshake done: revert to the caller's per-message timeout so steady-state
        // invokes/events fail fast on a hung plugin (e.g. the `hang` fixture).
        plugin.timeout = timeout;
        Ok(plugin)
    }
}

/// Translate a manifest's self-declared capability strings into the typed
/// OS-sandbox capability set. Only sandbox-driving capabilities are mapped;
/// host-RPC capabilities (`fs:read`/`fs:write`/`events`) are irrelevant here.
fn sandbox_caps(caps: &[String]) -> SandboxCapabilities {
    SandboxCapabilities {
        net: caps.iter().any(|c| c == CAP_NET),
    }
}

impl PluginHost for ProcessPluginHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        self.loaded.iter().map(|p| p.info.clone()).collect()
    }

    fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        args: &serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        let p = self
            .loaded
            .iter_mut()
            .find(|p| p.info.id == plugin)
            .ok_or_else(|| PortError::NotFound(format!("plugin {plugin}")))?;
        if !p.info.commands.iter().any(|c| c.id == command) {
            return Err(PortError::NotFound(format!("command {command}")));
        }
        let params = serde_json::to_value(InvokeParams {
            command: command.to_string(),
            args: args.clone(),
        })
        .map_err(adapt)?;
        p.invoke_command(params, callbacks)
    }

    fn dispatch_event(
        &mut self,
        event: &PluginEvent,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Vec<EventDispatchError> {
        let cairn_event = to_cairn_event(event);
        let mut errors = Vec::new();
        for p in self.loaded.iter_mut() {
            if p.capabilities.iter().any(|c| c == CAP_EVENTS) {
                if let Err(e) = p.deliver_event(&cairn_event, callbacks) {
                    // Return the failure for the engine to log uniformly (audit
                    // G4), rather than writing to stderr from the adapter.
                    errors.push(EventDispatchError {
                        plugin: p.info.id.clone(),
                        error: e,
                    });
                }
            }
        }
        errors
    }

    fn process_content(
        &mut self,
        path: &str,
        content: &str,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<String, PortError> {
        // Candidate = declared the cap AND has a matcher hitting this path.
        let mut order: Vec<String> = self
            .loaded
            .iter()
            .filter(|p| {
                p.capabilities.iter().any(|c| c == CAP_CONTENT_PROCESS)
                    && processor_matches(&p.processors, path)
            })
            .map(|p| p.info.id.clone())
            .collect();
        order.sort(); // deterministic chain order by plugin id

        let loaded = &mut self.loaded;
        let out = fold_content(content.to_string(), &order, |id, c| {
            let p = loaded
                .iter_mut()
                .find(|p| p.info.id == id)
                .expect("id came from `order`, built from `loaded`");
            let mut ro = ReadOnlyCallbacks(&mut *callbacks);
            p.process_one(path, c, &mut ro)
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::RefusingSandbox;
    use cairn_ports::{PluginCallbacks, SandboxError};
    use std::process::Command;

    /// Test double: spawns the command verbatim (no OS jail) so the spawn path
    /// is exercised on every platform without Seatbelt.
    struct PermissiveSandbox;
    impl Sandbox for PermissiveSandbox {
        fn wrap(
            &self,
            _vault_root: &Path,
            _dir: &Path,
            cmd: &Path,
            args: &[String],
            _caps: cairn_ports::SandboxCapabilities,
        ) -> Result<Command, SandboxError> {
            let mut c = Command::new(cmd);
            c.args(args);
            Ok(c)
        }
    }

    /// Write `<root>/<dir_name>/manifest.toml` declaring `id` and a non-spawnable
    /// command.
    fn write_plugin(root: &Path, dir_name: &str, manifest_id: &str) {
        let pdir = root.join(dir_name);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("manifest.toml"),
            format!(
                "id=\"{manifest_id}\"\nname=\"N\"\nversion=\"0\"\n\
                 [engine]\ncommand=\"/nonexistent/xyz\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn load_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let trusted = TrustedPlugins::from_ids(["anything".to_string()]);
        let host =
            ProcessPluginHost::load(&tmp.path().join("missing"), &trusted, &PermissiveSandbox)
                .unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn unspawnable_plugin_is_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "broken", "broken");
        let trusted = TrustedPlugins::from_ids(["broken".to_string()]);
        // Trusted but the command can't spawn: load succeeds, plugin absent.
        let host = ProcessPluginHost::load(tmp.path(), &trusted, &PermissiveSandbox).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn untrusted_plugin_is_not_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "rogue", "rogue");
        // Empty trust set => default-deny.
        let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none(), &PermissiveSandbox)
            .unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn untrusted_manifest_is_not_parsed() {
        // A directory not in the trust set must be skipped *before* its manifest
        // is read. A malformed manifest there must therefore NOT cause an error.
        let tmp = tempfile::tempdir().unwrap();
        let pdir = tmp.path().join("rogue");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("manifest.toml"), "this is not valid toml {{{").unwrap();
        let host = ProcessPluginHost::load(tmp.path(), &TrustedPlugins::none(), &PermissiveSandbox)
            .unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn unavailable_sandbox_refuses_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "p", "p");
        let trusted = TrustedPlugins::from_ids(["p".to_string()]);
        // RefusingSandbox => the plugin is refused, never spawned.
        let host = ProcessPluginHost::load(tmp.path(), &trusted, &RefusingSandbox).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn from_ids_yields_unpinned_entries() {
        let trusted = TrustedPlugins::from_ids(["a".to_string(), "b".to_string()]);
        assert!(matches!(trusted.get("a"), Some(None)));
        assert!(matches!(trusted.get("b"), Some(None)));
        assert!(trusted.get("c").is_none());
        assert!(TrustedPlugins::none().get("a").is_none());
    }

    #[test]
    fn from_entries_parses_pins_and_rejects_bad() {
        let good = TrustedPlugins::from_entries([(
            "a".to_string(),
            Some(format!("sha256:{}", "a".repeat(64))),
        )])
        .unwrap();
        assert!(matches!(good.get("a"), Some(Some(_))));

        assert!(
            TrustedPlugins::from_entries([("a".to_string(), Some("bogus".to_string()))]).is_err()
        );
    }

    #[test]
    fn sandbox_caps_sets_net_only_when_declared() {
        use cairn_ports::SandboxCapabilities;
        assert_eq!(
            super::sandbox_caps(&["net".to_string()]),
            SandboxCapabilities { net: true }
        );
        assert_eq!(
            super::sandbox_caps(&["fs:read".to_string(), "events".to_string()]),
            SandboxCapabilities { net: false }
        );
        assert_eq!(super::sandbox_caps(&[]), SandboxCapabilities::default());
    }

    #[test]
    fn processor_matches_by_extension() {
        use cairn_plugin_protocol::ProcessorDecl;
        let md = [ProcessorDecl {
            extensions: vec!["md".into()],
        }];
        assert!(super::processor_matches(&md, "notes/a.md"));
        assert!(!super::processor_matches(&md, "notes/a.txt"));

        let all = [ProcessorDecl { extensions: vec![] }];
        assert!(super::processor_matches(&all, "anything.canvas"));

        let none: [ProcessorDecl; 0] = [];
        assert!(!super::processor_matches(&none, "a.md")); // no decls => not a candidate
    }

    #[test]
    fn fold_content_chains_in_order_and_is_fail_soft() {
        // Two processors that append their id: proves order + "each sees prior output".
        let order = vec!["a".to_string(), "b".to_string()];
        let out = super::fold_content("raw".into(), &order, |id, c| Ok(format!("{c}{id}")));
        assert_eq!(out, "rawab");

        // A failing processor is skipped; the chain keeps the last-good content.
        let out = super::fold_content("raw".into(), &order, |id, c| {
            if id == "a" {
                Err(PortError::Adapter("boom".into()))
            } else {
                Ok(format!("{c}{id}"))
            }
        });
        assert_eq!(out, "rawb"); // "a" failed -> content stayed "raw" -> "b" appended
    }

    #[test]
    fn read_only_callbacks_forward_reads_deny_writes() {
        use super::ReadOnlyCallbacks;
        // Minimal in-memory callbacks double.
        struct Cb {
            read_calls: u32,
        }
        impl cairn_ports::PluginCallbacks for Cb {
            fn read_note(&mut self, _: &str) -> Result<String, PortError> {
                self.read_calls += 1;
                Ok("body".into())
            }
            fn write_note(&mut self, _: &str, _: &str) -> Result<(), PortError> {
                Ok(())
            }
            fn search(&mut self, _: &str) -> Result<Vec<cairn_ports::SearchHit>, PortError> {
                Ok(vec![])
            }
            fn list_notes(&mut self) -> Result<Vec<cairn_domain::Note>, PortError> {
                Ok(vec![])
            }
            fn delete_note(&mut self, _: &str) -> Result<(), PortError> {
                Ok(())
            }
        }
        let mut inner = Cb { read_calls: 0 };
        let mut ro = ReadOnlyCallbacks(&mut inner);
        assert_eq!(ro.read_note("a.md").unwrap(), "body");
        assert!(ro.write_note("a.md", "x").is_err());
        assert!(ro.delete_note("a.md").is_err());
        assert_eq!(inner.read_calls, 1);
    }
}
