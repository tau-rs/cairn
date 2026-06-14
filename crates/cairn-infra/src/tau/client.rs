//! The serve-mode JSON-RPC client, generic over a line reader and writer.

use std::io::{BufRead, Write};

use cairn_ports::{AdapterError, AgentEvent, AgentSink, PortError};
use serde_json::json;

use crate::tau::wire::{map_event, Incoming, Request};

/// Speaks tau serve-mode over any line-oriented transport.
pub struct ServeClient<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    next_id: u64,
}

fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

impl<R: BufRead, W: Write> ServeClient<R, W> {
    /// Wrap an already-connected transport (the subprocess's stdout/stdin, or
    /// an in-memory pipe in tests).
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 0,
        }
    }

    fn send(&mut self, method: &str, params: serde_json::Value) -> Result<u64, PortError> {
        self.next_id += 1;
        let id = self.next_id;
        let req = Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let line = serde_json::to_string(&req).map_err(adapt)?;
        self.writer.write_all(line.as_bytes()).map_err(adapt)?;
        self.writer.write_all(b"\n").map_err(adapt)?;
        self.writer.flush().map_err(adapt)?;
        Ok(id)
    }

    /// Read the next non-blank line as an [`Incoming`]; `Ok(None)` on EOF.
    fn read_msg(&mut self) -> Result<Option<Incoming>, PortError> {
        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf).map_err(adapt)?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(Some(serde_json::from_str(trimmed).map_err(adapt)?));
        }
    }

    /// Perform the protocol handshake. Errors on version mismatch or EOF.
    pub fn handshake(&mut self) -> Result<(), PortError> {
        let id = self.send(
            "meta.handshake",
            json!({"client_name": "cairn", "client_version": "0.0.0", "protocol_version": 1}),
        )?;
        loop {
            match self.read_msg()? {
                None => {
                    return Err(PortError::Adapter(
                        "tau serve closed during handshake".into(),
                    ))
                }
                Some(msg) if msg.id == Some(id) => {
                    if let Some(err) = msg.error {
                        return Err(PortError::Adapter(
                            format!("tau handshake rejected: {err}").into(),
                        ));
                    }
                    return Ok(());
                }
                Some(_) => continue, // ignore stray notifications before the reply
            }
        }
    }

    /// Run `agent` over `prompt`, emitting each increment to `sink`. Terminates
    /// on `RunCompleted`/`FatalError` or the matching JSON-RPC response,
    /// whichever arrives first.
    pub fn run_streaming(
        &mut self,
        agent: &str,
        prompt: &str,
        sink: &mut dyn AgentSink,
    ) -> Result<(), PortError> {
        let id = self.send(
            "runtime.run_streaming",
            json!({"agent": agent, "prompt": prompt}),
        )?;
        loop {
            match self.read_msg()? {
                None => {
                    sink.emit(AgentEvent::Failed {
                        message: "tau serve closed mid-run".into(),
                    });
                    return Ok(());
                }
                Some(msg) if msg.method.as_deref() == Some("runtime.event") => {
                    if msg.params.get("id").and_then(|v| v.as_u64()) != Some(id) {
                        continue;
                    }
                    let kind = msg
                        .params
                        .get("kind")
                        .and_then(|k| k.as_str())
                        .unwrap_or("");
                    // Borrow the data subtree (no clone — it can be a large
                    // tool-output blob); fall back to a 'static Null sentinel.
                    const NULL: serde_json::Value = serde_json::Value::Null;
                    let data = msg.params.get("data").unwrap_or(&NULL);
                    if let Some(ev) = map_event(kind, data) {
                        let done = matches!(ev, AgentEvent::Completed | AgentEvent::Failed { .. });
                        sink.emit(ev);
                        if done {
                            return Ok(());
                        }
                    }
                }
                Some(msg) if msg.id == Some(id) => {
                    if let Some(err) = msg.error {
                        sink.emit(AgentEvent::Failed {
                            message: format!("{err}"),
                        });
                    }
                    return Ok(());
                }
                Some(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[derive(Default)]
    struct VecSink(Vec<AgentEvent>);
    impl AgentSink for VecSink {
        fn emit(&mut self, e: AgentEvent) {
            self.0.push(e);
        }
    }

    fn client(script: &str) -> ServeClient<BufReader<Cursor<Vec<u8>>>, Vec<u8>> {
        ServeClient::new(
            BufReader::new(Cursor::new(script.as_bytes().to_vec())),
            Vec::new(),
        )
    }

    #[test]
    fn handshake_accepts_matching_reply() {
        let mut c = client("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        c.handshake().unwrap();
        let sent = String::from_utf8(c.writer.clone()).unwrap();
        assert!(sent.contains("\"method\":\"meta.handshake\""));
        assert!(sent.contains("\"protocol_version\":1"));
    }

    #[test]
    fn handshake_errors_on_rejection() {
        let mut c = client("{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000}}\n");
        assert!(c.handshake().is_err());
    }

    #[test]
    fn run_streaming_emits_deltas_then_completed() {
        let script = concat!(
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"TextDelta\",\"data\":{\"text\":\"He\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"TextDelta\",\"data\":{\"text\":\"llo\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"RunCompleted\",\"data\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
        );
        let mut c = client(script);
        let mut sink = VecSink::default();
        c.run_streaming("greeter", "hi", &mut sink).unwrap();
        assert_eq!(
            sink.0,
            vec![
                AgentEvent::TextDelta("He".into()),
                AgentEvent::TextDelta("llo".into()),
                AgentEvent::Completed,
            ]
        );
        let sent = String::from_utf8(c.writer.clone()).unwrap();
        assert!(sent.contains("\"method\":\"runtime.run_streaming\""));
        assert!(sent.contains("\"agent\":\"greeter\""));
    }

    #[test]
    fn run_streaming_ignores_events_for_other_ids() {
        let script = concat!(
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":99,\"kind\":\"TextDelta\",\"data\":{\"text\":\"nope\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"runtime.event\",\"params\":{\"id\":1,\"kind\":\"RunCompleted\",\"data\":{}}}\n",
        );
        let mut c = client(script);
        let mut sink = VecSink::default();
        c.run_streaming("a", "b", &mut sink).unwrap();
        assert_eq!(sink.0, vec![AgentEvent::Completed]);
    }

    #[test]
    fn eof_mid_run_yields_failed() {
        let mut c = client("");
        let mut sink = VecSink::default();
        c.run_streaming("a", "b", &mut sink).unwrap();
        assert!(matches!(sink.0.as_slice(), [AgentEvent::Failed { .. }]));
    }
}
