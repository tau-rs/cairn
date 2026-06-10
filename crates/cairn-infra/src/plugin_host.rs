//! `PluginHost` backed by child processes speaking JSON-RPC/NDJSON over stdio.

use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, Incoming, InitializeParams, InitializeResult,
    InvokeParams, ListNotesResult, Manifest, NoteSummaryDto, ReadNoteParams, ReadNoteResult,
    Request, Response, RpcError, SearchHitDto, SearchParams, SearchResultDto, WriteNoteParams,
    CALLBACK_DENIED, CALLBACK_FAILED, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
    METHOD_LIST_NOTES, METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
use cairn_ports::{PluginCallbacks, PluginCommand, PluginHost, PluginInfo, PortError};

fn adapt<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Adapter(e.to_string())
}

/// The capability a host-callback method requires, or `None` if the method is
/// unknown to the host.
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some("fs:read"),
        METHOD_WRITE_NOTE => Some("fs:write"),
        METHOD_SEARCH => Some("fs:read"),
        METHOD_LIST_NOTES => Some("fs:read"),
        _ => None,
    }
}

struct LoadedPlugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    info: PluginInfo,
    next_id: u64,
    /// Capabilities the manifest declared; gates host-callbacks.
    capabilities: Vec<String>,
}

impl LoadedPlugin {
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
        let resp: Response = read_message(&mut self.stdout)
            .map_err(adapt)?
            .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
        if let Some(err) = resp.error {
            return Err(PortError::Adapter(format!("plugin error: {}", err.message)));
        }
        resp.result
            .ok_or_else(|| PortError::Adapter("plugin response had no result".into()))
    }

    /// Invoke a command, servicing any host-callbacks the plugin sends until it
    /// returns the response to our invoke request.
    fn invoke_command(
        &mut self,
        params: serde_json::Value,
        callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        self.next_id += 1;
        let invoke_id = self.next_id;
        let req = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: invoke_id,
            method: METHOD_INVOKE.to_string(),
            params,
        };
        write_message(&mut self.stdin, &req).map_err(adapt)?;
        loop {
            let msg: Incoming = read_message(&mut self.stdout)
                .map_err(adapt)?
                .ok_or_else(|| PortError::Adapter("plugin closed its output".into()))?;
            match msg {
                Incoming::Response(resp) => {
                    if resp.id != invoke_id {
                        continue; // stray id; one-in-flight invariant, ignore
                    }
                    if let Some(err) = resp.error {
                        return Err(PortError::Adapter(format!("plugin error: {}", err.message)));
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawns and talks to plugins under a plugins directory.
#[derive(Default)]
pub struct ProcessPluginHost {
    loaded: Vec<LoadedPlugin>,
}

impl ProcessPluginHost {
    /// Load every `<dir>/<id>/manifest.toml`: spawn the binary, handshake, and
    /// keep the process. A missing dir loads nothing; a plugin that fails to
    /// spawn/handshake is skipped (logged), not fatal.
    ///
    /// # Errors
    /// [`PortError::Adapter`] only on an unexpected IO error reading the dir.
    pub fn load(dir: &Path) -> Result<Self, PortError> {
        let mut loaded = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(adapt(e)),
        };
        for entry in entries {
            let plugin_dir = match entry {
                Ok(e) if e.path().is_dir() => e.path(),
                _ => continue,
            };
            match Self::spawn_plugin(&plugin_dir) {
                Ok(p) => loaded.push(p),
                Err(e) => eprintln!("plugin: skipping {}: {e}", plugin_dir.display()),
            }
        }
        Ok(Self { loaded })
    }

    fn spawn_plugin(plugin_dir: &Path) -> Result<LoadedPlugin, PortError> {
        let raw = std::fs::read_to_string(plugin_dir.join("manifest.toml")).map_err(adapt)?;
        let manifest: Manifest = toml::from_str(&raw).map_err(adapt)?;

        let cmd_path = {
            let p = Path::new(&manifest.engine.command);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                plugin_dir.join(p)
            }
        };
        let mut child = Command::new(&cmd_path)
            .args(&manifest.engine.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(adapt)?;
        let stdin = child.stdin.take().ok_or_else(|| adapt("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| adapt("no stdout"))?);

        let mut plugin = LoadedPlugin {
            child,
            stdin,
            stdout,
            info: PluginInfo {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                commands: Vec::new(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_absent_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let host = ProcessPluginHost::load(&tmp.path().join("missing")).unwrap();
        assert!(host.plugins().is_empty());
    }

    #[test]
    fn unspawnable_plugin_is_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let pdir = tmp.path().join("broken");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("manifest.toml"),
            "id=\"broken\"\nname=\"B\"\nversion=\"0\"\n[engine]\ncommand=\"/nonexistent/xyz\"\n",
        )
        .unwrap();
        let host = ProcessPluginHost::load(tmp.path()).unwrap();
        assert!(host.plugins().is_empty());
    }
}
