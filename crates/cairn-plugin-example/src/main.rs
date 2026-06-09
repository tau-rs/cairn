//! A minimal example cairn plugin: registers an `echo` and a `noteLen` command.
//! `echo` returns its args unchanged; `noteLen` calls back to the host via
//! `host/readNote` and returns the byte-length of the note contents.
//! Speaks JSON-RPC/NDJSON over stdio; exits on stdin EOF.

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, ReadNoteParams,
    ReadNoteResult, Request, Response, RpcError, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
    METHOD_READ_NOTE,
};
use std::io::{self, BufRead, BufReader, Write};

fn main() {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();
    let mut cb_id: u64 = 1000; // ids for host-callback requests (distinct range)

    while let Some(req) = read_message::<_, Request>(&mut reader).unwrap_or(None) {
        let resp = handle(&req, &mut reader, &mut stdout, &mut cb_id);
        if write_message(&mut stdout, &resp).is_err() {
            break;
        }
    }
}

fn handle<R: BufRead, W: Write>(
    req: &Request,
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
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
                name: "example".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                commands: vec![
                    CommandDecl {
                        id: "echo".to_string(),
                        title: "Echo".to_string(),
                    },
                    CommandDecl {
                        id: "noteLen".to_string(),
                        title: "Note length".to_string(),
                    },
                ],
            };
            resp.result = Some(serde_json::to_value(init).unwrap());
        }
        METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
            Ok(p) if p.command == "echo" => resp.result = Some(p.args),
            Ok(p) if p.command == "noteLen" => {
                let path = p
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                match read_note_via_host(reader, stdout, cb_id, &path) {
                    Ok(contents) => {
                        resp.result = Some(serde_json::json!({ "len": contents.len() }));
                    }
                    Err(err) => resp.error = Some(err),
                }
            }
            Ok(p) => {
                resp.error = Some(RpcError {
                    code: -32601,
                    message: format!("unknown command {}", p.command),
                });
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

/// Send a `host/readNote` callback to the host and block for its response.
fn read_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
) -> Result<String, RpcError> {
    *cb_id += 1;
    let req = Request {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: *cb_id,
        method: METHOD_READ_NOTE.to_string(),
        params: serde_json::to_value(ReadNoteParams {
            path: path.to_string(),
        })
        .unwrap(),
    };
    write_message(stdout, &req).map_err(|e| RpcError {
        code: -32603,
        message: format!("callback write failed: {e}"),
    })?;
    let cb_resp: Response = read_message(reader)
        .map_err(|e| RpcError {
            code: -32603,
            message: format!("callback read failed: {e}"),
        })?
        .ok_or_else(|| RpcError {
            code: -32603,
            message: "host closed before callback response".to_string(),
        })?;
    if let Some(err) = cb_resp.error {
        return Err(err); // propagate the host's denial/failure
    }
    let result = cb_resp.result.ok_or_else(|| RpcError {
        code: -32603,
        message: "empty callback response".to_string(),
    })?;
    let rn: ReadNoteResult = serde_json::from_value(result).map_err(|e| RpcError {
        code: -32603,
        message: e.to_string(),
    })?;
    Ok(rn.contents)
}
