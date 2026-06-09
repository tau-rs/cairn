//! A minimal example cairn plugin: registers an `echo` command that returns its
//! args unchanged. Speaks JSON-RPC/NDJSON over stdio; exits on stdin EOF.

use std::io::{self, BufReader};

use cairn_plugin_protocol::{
    read_message, write_message, CommandDecl, InitializeResult, InvokeParams, Request, Response,
    RpcError, JSONRPC_VERSION, METHOD_INITIALIZE, METHOD_INVOKE,
};

fn main() {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();

    while let Some(req) = read_message::<_, Request>(&mut reader).unwrap_or(None) {
        let resp = handle(&req);
        if write_message(&mut stdout, &resp).is_err() {
            break;
        }
    }
}

fn handle(req: &Request) -> Response {
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
                commands: vec![CommandDecl {
                    id: "echo".to_string(),
                    title: "Echo".to_string(),
                }],
            };
            resp.result = Some(serde_json::to_value(init).unwrap());
        }
        METHOD_INVOKE => match serde_json::from_value::<InvokeParams>(req.params.clone()) {
            Ok(p) if p.command == "echo" => resp.result = Some(p.args),
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
