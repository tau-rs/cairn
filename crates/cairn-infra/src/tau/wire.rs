//! Serde types for tau serve-mode JSON-RPC, and the mapping from tau's
//! `runtime.event` kinds to cairn's `AgentEvent`.

use cairn_ports::AgentEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An outbound JSON-RPC request.
#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub jsonrpc: &'a str,
    pub id: u64,
    pub method: &'a str,
    pub params: Value,
}

/// An inbound line: a response (`id` + `result`/`error`) or a notification
/// (`method` + `params`). Untagged — fields are optional and decoded leniently.
#[derive(Debug, Default, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
}

/// Map a `runtime.event` `(kind, data)` to a cairn [`AgentEvent`].
/// Returns `None` for unknown kinds (tolerated, never panics — tau's event enum
/// is `#[non_exhaustive]` upstream).
pub fn map_event(kind: &str, data: &Value) -> Option<AgentEvent> {
    let str_field = |k: &str| {
        data.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    match kind {
        "TextDelta" => Some(AgentEvent::TextDelta(str_field("text"))),
        "ToolCallStarted" => Some(AgentEvent::ToolStarted {
            tool: str_field("tool"),
        }),
        "ToolCallCompleted" => {
            let is_error = data
                .get("result")
                .and_then(|r| r.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(AgentEvent::ToolCompleted {
                tool: str_field("tool"),
                ok: !is_error,
            })
        }
        "TurnCompleted" => Some(AgentEvent::TurnCompleted),
        "RunCompleted" => Some(AgentEvent::Completed),
        "FatalError" => Some(AgentEvent::Failed {
            message: data
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("agent run failed")
                .to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_known_kinds() {
        assert_eq!(
            map_event("TextDelta", &json!({"text": "hi"})),
            Some(AgentEvent::TextDelta("hi".into()))
        );
        assert_eq!(
            map_event("RunCompleted", &json!({})),
            Some(AgentEvent::Completed)
        );
        assert_eq!(
            map_event("ToolCallStarted", &json!({"tool": "fs-read"})),
            Some(AgentEvent::ToolStarted {
                tool: "fs-read".into()
            })
        );
        assert_eq!(
            map_event(
                "ToolCallCompleted",
                &json!({"tool": "fs-read", "result": {"is_error": true}})
            ),
            Some(AgentEvent::ToolCompleted {
                tool: "fs-read".into(),
                ok: false
            })
        );
        assert_eq!(
            map_event("FatalError", &json!({"message": "boom"})),
            Some(AgentEvent::Failed {
                message: "boom".into()
            })
        );
    }

    #[test]
    fn unknown_kind_is_tolerated() {
        assert_eq!(map_event("SomeFutureKind", &json!({})), None);
    }

    #[test]
    fn incoming_decodes_notification_and_response() {
        let note: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"runtime.event","params":{"id":4,"kind":"TextDelta","data":{"text":"x"}}}"#).unwrap();
        assert_eq!(note.method.as_deref(), Some("runtime.event"));
        assert_eq!(note.params.get("id").and_then(|v| v.as_u64()), Some(4));

        let resp: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":4,"result":{}}"#).unwrap();
        assert_eq!(resp.id, Some(4));
        assert!(resp.result.is_some());
    }
}
