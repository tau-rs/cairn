//! Wire-format types and NDJSON framing for the cairn plugin protocol
//! (JSON-RPC 2.0 over stdio, MCP-style). No transport or process logic here.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)

use std::io::{BufRead, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const JSONRPC_VERSION: &str = "2.0";
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_INVOKE: &str = "invokeCommand";

/// A JSON-RPC request (host -> plugin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A JSON-RPC response (plugin -> host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// Params of the `initialize` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub host_version: String,
}

/// Result of `initialize`: the plugin's identity + declared commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandDecl>,
}

/// A command the plugin declares it can handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title: String,
}

/// Params of the `invokeCommand` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeParams {
    pub command: String,
    pub args: serde_json::Value,
}

/// A plugin manifest (`<cairn>/.cairn/plugins/<id>/manifest.toml`). Parsed by
/// the host; this crate only defines the shape (no `toml` dependency).
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub engine: EngineSection,
}

/// The `[engine]` section of a manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineSection {
    /// Executable to spawn (absolute, or relative to the plugin's directory).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Declared capabilities — recorded only (not enforced in slice 1).
    #[serde(default)]
    pub capabilities: Vec<String>,
}

fn invalid_data(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Write one NDJSON message (a serializable value) + `\n` to `w`, flushing.
///
/// # Errors
/// IO or serialization failure.
pub fn write_message<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    serde_json::to_writer(&mut *w, msg).map_err(invalid_data)?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Read one NDJSON message from `r` (skipping blank lines), deserializing into
/// `T`. `Ok(None)` on clean EOF.
///
/// # Errors
/// IO failure, or malformed JSON (`InvalidData`).
pub fn read_message<R: BufRead, T: DeserializeOwned>(r: &mut R) -> std::io::Result<Option<T>> {
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return serde_json::from_str(trimmed)
            .map(Some)
            .map_err(invalid_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_response_roundtrip_over_ndjson() {
        let req = Request {
            jsonrpc: JSONRPC_VERSION.into(),
            id: 1,
            method: METHOD_INVOKE.into(),
            params: serde_json::json!({"command": "echo", "args": {"x": 1}}),
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &req).unwrap();
        assert!(buf.ends_with(b"\n"));
        let mut r = std::io::Cursor::new(buf);
        let got: Request = read_message(&mut r).unwrap().unwrap();
        assert_eq!(got.id, 1);
        assert_eq!(got.method, METHOD_INVOKE);
        assert_eq!(got.params["command"], "echo");
    }

    #[test]
    fn initialize_result_roundtrips() {
        let init = InitializeResult {
            name: "example".into(),
            version: "0.1.0".into(),
            commands: vec![CommandDecl {
                id: "echo".into(),
                title: "Echo".into(),
            }],
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &init).unwrap();
        let mut r = std::io::Cursor::new(buf);
        let got: InitializeResult = read_message(&mut r).unwrap().unwrap();
        assert_eq!(got, init);
    }

    #[test]
    fn eof_is_none_blank_skipped_malformed_errors() {
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(read_message::<_, Request>(&mut empty).unwrap().is_none());
        let mut blanks = std::io::Cursor::new(b"\n  \n".to_vec());
        assert!(read_message::<_, Request>(&mut blanks).unwrap().is_none());
        let mut bad = std::io::Cursor::new(b"{not json\n".to_vec());
        assert!(read_message::<_, Request>(&mut bad).is_err());
    }

    #[test]
    fn manifest_parses_from_toml() {
        let m: Manifest = toml::from_str(
            "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\n[engine]\ncommand = \"./x\"\n",
        )
        .unwrap();
        assert_eq!(m.id, "x");
        assert_eq!(m.engine.command, "./x");
        assert!(m.engine.args.is_empty());
        assert!(m.engine.capabilities.is_empty());
    }
}
