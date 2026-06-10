//! cairn plugin SDK: write a plugin as command declarations + typed handlers;
//! the SDK owns the JSON-RPC/NDJSON stdio loop and the host-callback round-trip.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)

use std::io::{BufRead, Write};

use cairn_plugin_protocol::{
    read_message, write_message, ListNotesResult, ReadNoteParams, ReadNoteResult, Request,
    Response, RpcError, SearchParams, SearchResultDto, WriteNoteParams, JSONRPC_VERSION,
    METHOD_LIST_NOTES, METHOD_READ_NOTE, METHOD_SEARCH, METHOD_WRITE_NOTE,
};
use serde_json::Value;

pub use cairn_plugin_protocol::{NoteSummaryDto, SearchHitDto};

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
