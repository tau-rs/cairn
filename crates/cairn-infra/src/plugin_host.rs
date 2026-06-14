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
    write_message, CairnEvent, CairnEventKind, Capability, CommandDecl, DeleteNoteParams, Incoming,
    InitializeParams, InitializeResult, InvokeParams, ListNotesResult, Manifest, NoteSummaryDto,
    ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchHitDto, SearchParams,
    SearchResultDto, WriteNoteParams, CALLBACK_DENIED, CALLBACK_FAILED, JSONRPC_VERSION,
    METHOD_CAIRN_EVENT, METHOD_DELETE_NOTE, METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
use cairn_ports::{
    AdapterError, EventDispatchError, PluginCallbacks, PluginCommand, PluginEvent, PluginHost,
    PluginInfo, PortError, Sandbox,
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

/// The capability a host-callback method requires, or `None` if the method is
/// unknown to the host.
fn required_cap(method: &str) -> Option<Capability> {
    match method {
        METHOD_READ_NOTE => Some(Capability::VaultRead),
        METHOD_WRITE_NOTE => Some(Capability::VaultWrite),
        METHOD_DELETE_NOTE => Some(Capability::VaultWrite),
        METHOD_SEARCH => Some(Capability::VaultRead),
        METHOD_LIST_NOTES => Some(Capability::VaultRead),
        _ => None,
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

/// How an on-disk plugin stands relative to the trusted set, as reported by
/// [`inspect_plugins`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    /// Not listed in `[plugins].trusted`.
    Untrusted,
    /// Trusted, but no content hash is pinned.
    TrustedUnpinned,
    /// Trusted and the pinned hash matches the directory's current contents.
    Pinned,
    /// Trusted with a pinned hash, but the contents have changed since.
    Drift,
    /// The manifest is missing/malformed or the directory cannot be hashed.
    Unreadable,
}

/// The manifest fields surfaced at approval time (read-only view; no spawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub command: String,
    pub capabilities: Vec<Capability>,
}

/// One plugin directory as seen by [`inspect_plugins`].
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInspection {
    pub dir_name: String,
    /// `None` when the manifest is missing/malformed.
    pub manifest: Option<InspectedManifest>,
    /// `None` when the directory cannot be hashed (e.g. contains a symlink).
    pub computed_hash: Option<PinnedHash>,
    pub status: TrustStatus,
}

/// Read-only inspection of every plugin directory under `dir`, for the CLI's
/// `plugin list` / `plugin trust` review flow. **No process is spawned.**
///
/// Unlike the daemon's automatic load path, this **does** parse untrusted
/// manifests — that is the point of letting a user review a plugin before
/// trusting it. It is a user-initiated, read-only parse (no code execution), so
/// it does not weaken the load path's "never parse untrusted bytes during spawn"
/// invariant.
///
/// # Errors
/// [`PortError::Adapter`] only on an unexpected IO error reading `dir`; an absent
/// `dir` yields an empty `Vec`. A per-plugin problem (bad manifest, un-hashable
/// dir) becomes [`TrustStatus::Unreadable`], never an error.
pub fn inspect_plugins(
    dir: &Path,
    trusted: &TrustedPlugins,
) -> Result<Vec<PluginInspection>, PortError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(adapt(e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let plugin_dir = match entry {
            Ok(e) if e.path().is_dir() => e.path(),
            _ => continue,
        };
        let Some(dir_name) = plugin_dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
        else {
            continue; // unnameable in a trust list
        };
        let dir_name = dir_name.to_string();
        let computed = PinnedHash::of_dir(&plugin_dir).ok();
        let manifest = read_inspected_manifest(&plugin_dir).ok();
        let status = match (trusted.get(&dir_name), &manifest, &computed) {
            // Cannot review what we cannot read/hash.
            (_, None, _) | (_, _, None) => TrustStatus::Unreadable,
            (None, _, _) => TrustStatus::Untrusted,
            (Some(None), _, _) => TrustStatus::TrustedUnpinned,
            (Some(Some(pin)), _, Some(c)) => {
                if pin == c {
                    TrustStatus::Pinned
                } else {
                    TrustStatus::Drift
                }
            }
        };
        out.push(PluginInspection {
            dir_name,
            manifest,
            computed_hash: computed,
            status,
        });
    }
    Ok(out)
}

/// Read + parse a plugin's `manifest.toml` into the surfaced subset. An unknown
/// capability fails here (fail-closed), surfacing as `Unreadable`.
fn read_inspected_manifest(plugin_dir: &Path) -> Result<InspectedManifest, PortError> {
    let raw = std::fs::read_to_string(plugin_dir.join("manifest.toml")).map_err(adapt)?;
    let m: Manifest = toml::from_str(&raw).map_err(adapt)?;
    Ok(InspectedManifest {
        id: m.id,
        name: m.name,
        version: m.version,
        command: m.engine.command,
        capabilities: m.engine.capabilities,
    })
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
    capabilities: Vec<Capability>,
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
            Some(cap) if !self.capabilities.contains(&cap) => {
                resp.error = Some(RpcError {
                    code: CALLBACK_DENIED,
                    message: format!("capability {} not declared", cap.wire()),
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
        let mut command = sandbox
            .wrap(vault_root, plugin_dir, &cmd_path, &manifest.engine.args)
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
            timeout,
            info: PluginInfo {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                commands: Vec::new(),
                contributions: Vec::new(),
            },
            next_id: 0,
            capabilities: manifest.engine.capabilities.clone(),
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
        Ok(plugin)
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
            if p.capabilities.contains(&Capability::VaultEvents) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::RefusingSandbox;
    use cairn_plugin_protocol::Capability;
    use cairn_ports::SandboxError;
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

    fn write_manifest_with(root: &Path, dir_name: &str, body: &str) {
        let pdir = root.join(dir_name);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("manifest.toml"), body).unwrap();
    }

    #[test]
    fn inspect_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let got = inspect_plugins(&tmp.path().join("missing"), &TrustedPlugins::none()).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn inspect_reports_untrusted_with_capabilities() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(
            tmp.path(),
            "p",
            "id=\"p\"\nname=\"P\"\nversion=\"1\"\n\
             [engine]\ncommand=\"./p\"\ncapabilities=[\"vault:read\",\"net\"]\n",
        );
        let got = inspect_plugins(tmp.path(), &TrustedPlugins::none()).unwrap();
        assert_eq!(got.len(), 1);
        let p = &got[0];
        assert_eq!(p.dir_name, "p");
        assert_eq!(p.status, TrustStatus::Untrusted);
        let m = p.manifest.as_ref().unwrap();
        assert_eq!(m.capabilities, vec![Capability::VaultRead, Capability::Net]);
        assert!(p.computed_hash.is_some());
    }

    #[test]
    fn inspect_distinguishes_pinned_drift_unpinned() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(
            tmp.path(),
            "p",
            "id=\"p\"\nname=\"P\"\nversion=\"1\"\n[engine]\ncommand=\"./p\"\n",
        );
        let real = PinnedHash::of_dir(&tmp.path().join("p")).unwrap();

        // Pinned + matching hash -> Pinned.
        let pinned =
            TrustedPlugins::from_entries([("p".to_string(), Some(real.to_string()))]).unwrap();
        assert_eq!(
            inspect_plugins(tmp.path(), &pinned).unwrap()[0].status,
            TrustStatus::Pinned
        );

        // Pinned + wrong hash -> Drift.
        let wrong = format!("sha256:{}", "0".repeat(64));
        let drift = TrustedPlugins::from_entries([("p".to_string(), Some(wrong))]).unwrap();
        assert_eq!(
            inspect_plugins(tmp.path(), &drift).unwrap()[0].status,
            TrustStatus::Drift
        );

        // Trusted, no pin -> TrustedUnpinned.
        let unpinned = TrustedPlugins::from_ids(["p".to_string()]);
        assert_eq!(
            inspect_plugins(tmp.path(), &unpinned).unwrap()[0].status,
            TrustStatus::TrustedUnpinned
        );
    }

    #[test]
    fn inspect_marks_malformed_manifest_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with(tmp.path(), "p", "this is not valid toml {{{");
        let got =
            inspect_plugins(tmp.path(), &TrustedPlugins::from_ids(["p".to_string()])).unwrap();
        assert_eq!(got[0].status, TrustStatus::Unreadable);
        assert!(got[0].manifest.is_none());
    }

    #[test]
    fn inspect_skips_non_dir_entries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("stray.txt"), "noise").unwrap();
        let got = inspect_plugins(tmp.path(), &TrustedPlugins::none()).unwrap();
        assert!(got.is_empty());
    }
}
