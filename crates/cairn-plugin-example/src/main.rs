//! A minimal example cairn plugin: registers an `echo` and a `noteLen` command.
//! `echo` returns its args unchanged; `noteLen` calls back to the host via
//! `host/readNote` and returns the byte-length of the note contents.
//! Speaks JSON-RPC/NDJSON over stdio; exits on stdin EOF.

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, ListNotesResult,
    ReadNoteParams, ReadNoteResult, Request, Response, RpcError, SearchParams, SearchResultDto,
    WriteNoteParams, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE, METHOD_LIST_NOTES,
    METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
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
                    CommandDecl {
                        id: "writeNote".to_string(),
                        title: "Write note".to_string(),
                    },
                    CommandDecl {
                        id: "noteCount".to_string(),
                        title: "Note count".to_string(),
                    },
                    CommandDecl {
                        id: "find".to_string(),
                        title: "Find".to_string(),
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
            Ok(p) if p.command == "writeNote" => {
                let path = p
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let contents = p
                    .args
                    .get("contents")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                match write_note_via_host(reader, stdout, cb_id, &path, &contents) {
                    Ok(()) => resp.result = Some(serde_json::json!({ "written": true })),
                    Err(err) => resp.error = Some(err),
                }
            }
            Ok(p) if p.command == "noteCount" => match list_notes_via_host(reader, stdout, cb_id) {
                Ok(notes) => resp.result = Some(serde_json::json!({ "count": notes.notes.len() })),
                Err(err) => resp.error = Some(err),
            },
            Ok(p) if p.command == "find" => {
                let query = p
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                match search_via_host(reader, stdout, cb_id, &query) {
                    Ok(res) => resp.result = Some(serde_json::json!({ "hits": res.hits.len() })),
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

/// Send a host-callback request and return its `result` Value (or the host's error).
fn call_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, RpcError> {
    *cb_id += 1;
    let req = Request {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: *cb_id,
        method: method.to_string(),
        params,
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
        return Err(err);
    }
    cb_resp.result.ok_or_else(|| RpcError {
        code: -32603,
        message: "empty callback response".to_string(),
    })
}

/// Send a `host/readNote` callback to the host and block for its response.
fn read_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
) -> Result<String, RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_READ_NOTE,
        serde_json::to_value(ReadNoteParams {
            path: path.to_string(),
        })
        .unwrap(),
    )?;
    let rn: ReadNoteResult = serde_json::from_value(result).map_err(|e| RpcError {
        code: -32603,
        message: e.to_string(),
    })?;
    Ok(rn.contents)
}

/// Send a `host/writeNote` callback; success carries an empty `{}` body.
fn write_note_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    path: &str,
    contents: &str,
) -> Result<(), RpcError> {
    call_host(
        reader,
        stdout,
        cb_id,
        METHOD_WRITE_NOTE,
        serde_json::to_value(WriteNoteParams {
            path: path.to_string(),
            contents: contents.to_string(),
        })
        .unwrap(),
    )?;
    Ok(())
}

/// Send a `host/listNotes` callback.
fn list_notes_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
) -> Result<ListNotesResult, RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_LIST_NOTES,
        serde_json::Value::Null,
    )?;
    serde_json::from_value(result).map_err(|e| RpcError {
        code: -32603,
        message: e.to_string(),
    })
}

/// Send a `host/search` callback.
fn search_via_host<R: BufRead, W: Write>(
    reader: &mut R,
    stdout: &mut W,
    cb_id: &mut u64,
    query: &str,
) -> Result<SearchResultDto, RpcError> {
    let result = call_host(
        reader,
        stdout,
        cb_id,
        METHOD_SEARCH,
        serde_json::to_value(SearchParams {
            query: query.to_string(),
        })
        .unwrap(),
    )?;
    serde_json::from_value(result).map_err(|e| RpcError {
        code: -32603,
        message: e.to_string(),
    })
}
