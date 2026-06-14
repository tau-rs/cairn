//! Wire-format types and NDJSON framing for the cairn plugin protocol
//! (JSON-RPC 2.0 over stdio, MCP-style). No transport or process logic here.
//! (`unsafe_code` is forbidden workspace-wide via `[lints] workspace = true`.)

use std::io::{BufRead, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const JSONRPC_VERSION: &str = "2.0";
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_INVOKE: &str = "invokeCommand";

/// Plugin -> host: read a note's raw contents. Requires the `fs:read` capability.
pub const METHOD_READ_NOTE: &str = "host/readNote";
/// Plugin -> host: create/overwrite a note. Requires the `fs:write` capability.
pub const METHOD_WRITE_NOTE: &str = "host/writeNote";
/// Plugin -> host: ranked full-text search. Requires the `fs:read` capability.
pub const METHOD_SEARCH: &str = "host/search";
/// Plugin -> host: list all notes (path + title). Requires the `fs:read` capability.
pub const METHOD_LIST_NOTES: &str = "host/listNotes";
/// Plugin -> host: delete a note. Requires the `fs:write` capability.
pub const METHOD_DELETE_NOTE: &str = "host/deleteNote";
/// Host -> plugin: a cairn change event. Delivered to plugins declaring `events`.
pub const METHOD_CAIRN_EVENT: &str = "cairn/event";

/// A capability a plugin declares in its manifest's `[engine].capabilities`.
///
/// Two enforcement domains:
/// - `vault:*` gate the **host-callback RPC** surface (`host/readNote`, …) and
///   are enforced today by the host (`cairn-infra` `service_callback`).
/// - `net` / `exec` / `fs:read` gate the **OS sandbox** around the spawned
///   child. They are declared and surfaced to the user now, and enforced by the
///   capability-derived sandbox profile (issue #63); until then the fixed jail
///   is stricter-or-equal, so declaring them never grants more than today.
///
/// These `vault:*` names intentionally supersede the legacy `CAP_FS_READ` /
/// `CAP_FS_WRITE` / `CAP_EVENTS` string constants (notably `"events"` →
/// `"vault:events"`); the host gate migrates off those string consts in a
/// follow-up task.
///
/// The enum is **closed**: serde rejects any unknown string, so a typo or a
/// capability from a newer manifest fails the manifest parse (fail-closed) and
/// the host refuses the plugin rather than silently under-granting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    /// Read/search/list notes via the host channel.
    #[serde(rename = "vault:read")]
    VaultRead,
    /// Create/overwrite/delete notes via the host channel.
    #[serde(rename = "vault:write")]
    VaultWrite,
    /// Receive pushed cairn change events.
    #[serde(rename = "vault:events")]
    VaultEvents,
    /// Make outbound network connections (sandbox; enforced by #63).
    #[serde(rename = "net")]
    Net,
    /// Spawn subprocesses (sandbox; enforced by #63).
    #[serde(rename = "exec")]
    Exec,
    /// Read real files outside the vault, broadly (sandbox; enforced by #63).
    #[serde(rename = "fs:read")]
    FsRead,
}

impl Capability {
    /// The manifest/wire string for this capability (e.g. `"vault:read"`).
    // Each arm must match the #[serde(rename = "...")] above; the roundtrip
    // test (`capability_roundtrips_via_wire_string`) guards this.
    pub fn wire(&self) -> &'static str {
        match self {
            Capability::VaultRead => "vault:read",
            Capability::VaultWrite => "vault:write",
            Capability::VaultEvents => "vault:events",
            Capability::Net => "net",
            Capability::Exec => "exec",
            Capability::FsRead => "fs:read",
        }
    }

    /// A plain-English line for the first-run approval screen.
    pub fn summary(&self) -> &'static str {
        match self {
            Capability::VaultRead => "read and search your notes",
            Capability::VaultWrite => "create, overwrite, and delete your notes",
            Capability::VaultEvents => "be notified when your notes change",
            Capability::Net => "make outbound network connections",
            Capability::Exec => "run other programs",
            Capability::FsRead => "read files on your computer outside your notes",
        }
    }

    /// Whether this capability is actually enforced by the current build. The
    /// three `vault:*` caps gate the live host-RPC channel (`true`); the three
    /// sandbox caps are declared now and enforced by #63 (`false`). Drives the
    /// "enforced in a future release" label on the approval screen.
    pub fn enforced_today(&self) -> bool {
        matches!(
            self,
            Capability::VaultRead | Capability::VaultWrite | Capability::VaultEvents
        )
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire())
    }
}

/// JSON-RPC error code: the host refused a callback (capability not declared, or
/// unknown host method).
pub const CALLBACK_DENIED: i64 = -32001;
/// JSON-RPC error code: a callback's host operation failed (e.g. note not found,
/// or malformed params).
pub const CALLBACK_FAILED: i64 = -32002;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub name: String,
    pub version: String,
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub contributions: Vec<PluginContribution>,
}

/// A command the plugin declares it can handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginIcon {
    Tag,
    Search,
    Note,
    Folder,
    Link,
    Star,
    Info,
    Play,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginSlot {
    #[serde(rename = "sidebar.section")]
    SidebarSection,
    #[serde(rename = "topbar.action")]
    TopbarAction,
    #[serde(rename = "command")]
    Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginListItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginWidget {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        muted: Option<bool>,
    },
    Action {
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        icon: Option<PluginIcon>,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    List {
        items: Vec<PluginListItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginContribution {
    pub id: String,
    pub slot: PluginSlot,
    pub widget: PluginWidget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<PluginIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
}

/// Params of the `invokeCommand` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeParams {
    pub command: String,
    pub args: serde_json::Value,
}

/// Params of the `host/readNote` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadNoteParams {
    pub path: String,
}

/// Result of the `host/readNote` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadNoteResult {
    pub contents: String,
}

/// Params of the `host/writeNote` callback. Success result is an empty object `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteNoteParams {
    pub path: String,
    pub contents: String,
}

/// Params of the `host/deleteNote` callback. Success result is an empty object `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteNoteParams {
    pub path: String,
}

/// The kind of a cairn change pushed to plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CairnEventKind {
    NoteChanged,
    NoteDeleted,
}

/// Params of the `cairn/event` request (host -> plugin). Ack result is `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CairnEvent {
    pub kind: CairnEventKind,
    pub path: String,
}

/// Params of the `host/search` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
}

/// One ranked search hit (host -> plugin). Plugin-protocol-local; intentionally
/// omits the contract's UI-only highlight ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub path: String,
    pub score: f32,
    pub snippet: String,
}

/// Result of the `host/search` callback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub hits: Vec<SearchHitDto>,
}

/// One note summary (host -> plugin): path + display title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteSummaryDto {
    pub path: String,
    pub title: String,
}

/// Result of the `host/listNotes` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummaryDto>,
}

/// A message the host reads from a plugin *during* an invoke: either a callback
/// request from the plugin, or the response to the host's invoke. Distinguished
/// untagged by the presence of `method` (Request) vs `result`/`error` (Response).
/// The `Request` variant is listed first so serde tries it before `Response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    // Tried first: requires `method`. MUST stay before `Response` — `Response`
    // has no `deny_unknown_fields`, so a request JSON would otherwise decode as
    // a `Response` with its `method` field silently ignored.
    Request(Request),
    Response(Response),
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
    /// Declared capabilities (typed; see [`Capability`]). The host gates every
    /// plugin->host callback on this list (see `cairn-infra`
    /// `plugin_host::service_callback`): a callback whose required capability is
    /// absent here is denied. An **unknown** capability string fails this parse
    /// (fail-closed), so the plugin is refused rather than silently
    /// under-granted. Note the boundary's limits (audit `security.md` S3):
    /// capabilities are *self-declared*, and the `vault:*` gate only narrows the
    /// host-callback RPC surface; the `net`/`exec`/`fs:read` sandbox caps are
    /// enforced by the capability-derived profile (#63).
    #[serde(default)]
    pub capabilities: Vec<Capability>,
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
            contributions: vec![],
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

    #[test]
    fn incoming_decodes_request_and_response_variants() {
        // A message carrying `method` is a host-callback Request.
        let req_json =
            r#"{"jsonrpc":"2.0","id":7,"method":"host/readNote","params":{"path":"a.md"}}"#;
        match serde_json::from_str::<Incoming>(req_json).unwrap() {
            Incoming::Request(r) => {
                assert_eq!(r.method, METHOD_READ_NOTE);
                assert_eq!(r.id, 7);
            }
            Incoming::Response(_) => panic!("expected Request variant"),
        }

        // A message carrying `result` (no `method`) is a Response.
        let resp_json = r#"{"jsonrpc":"2.0","id":7,"result":{"contents":"hi"}}"#;
        match serde_json::from_str::<Incoming>(resp_json).unwrap() {
            Incoming::Response(r) => {
                assert_eq!(r.id, 7);
                assert_eq!(r.result.unwrap()["contents"], "hi");
            }
            Incoming::Request(_) => panic!("expected Response variant"),
        }
    }

    #[test]
    fn read_note_result_roundtrips() {
        let rn = ReadNoteResult {
            contents: "body".into(),
        };
        let v = serde_json::to_value(&rn).unwrap();
        assert_eq!(
            serde_json::from_value::<ReadNoteResult>(v)
                .unwrap()
                .contents,
            "body"
        );
    }

    #[test]
    fn slice3b_dtos_roundtrip() {
        let wp = WriteNoteParams {
            path: "a.md".into(),
            contents: "body".into(),
        };
        let v = serde_json::to_value(&wp).unwrap();
        assert_eq!(serde_json::from_value::<WriteNoteParams>(v).unwrap(), wp);

        let sp = SearchParams {
            query: "hello".into(),
        };
        assert_eq!(
            serde_json::from_value::<SearchParams>(serde_json::to_value(&sp).unwrap()).unwrap(),
            sp
        );

        let sr = SearchResultDto {
            hits: vec![SearchHitDto {
                path: "a.md".into(),
                score: 1.5,
                snippet: "hi".into(),
            }],
        };
        let back: SearchResultDto =
            serde_json::from_value(serde_json::to_value(&sr).unwrap()).unwrap();
        assert_eq!(back, sr); // full equality also asserts the f32 score + snippet survive

        let ln = ListNotesResult {
            notes: vec![NoteSummaryDto {
                path: "a.md".into(),
                title: "A".into(),
            }],
        };
        let back: ListNotesResult =
            serde_json::from_value(serde_json::to_value(&ln).unwrap()).unwrap();
        assert_eq!(back.notes, ln.notes);
    }

    #[test]
    fn delete_note_params_roundtrips() {
        let dp = DeleteNoteParams {
            path: "a.md".into(),
        };
        let v = serde_json::to_value(&dp).unwrap();
        assert_eq!(serde_json::from_value::<DeleteNoteParams>(v).unwrap(), dp);
    }

    #[test]
    fn cairn_event_roundtrips() {
        let ev = CairnEvent {
            kind: CairnEventKind::NoteChanged,
            path: "a.md".into(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "noteChanged"); // camelCase rename
        assert_eq!(serde_json::from_value::<CairnEvent>(v).unwrap(), ev);

        let del = CairnEvent {
            kind: CairnEventKind::NoteDeleted,
            path: "b.md".into(),
        };
        assert_eq!(serde_json::to_value(&del).unwrap()["kind"], "noteDeleted");
    }

    #[test]
    fn capability_roundtrips_via_wire_string() {
        for cap in [
            Capability::VaultRead,
            Capability::VaultWrite,
            Capability::VaultEvents,
            Capability::Net,
            Capability::Exec,
            Capability::FsRead,
        ] {
            let v = serde_json::to_value(cap).unwrap();
            assert_eq!(v, serde_json::Value::String(cap.wire().to_string()));
            let back: Capability = serde_json::from_value(v).unwrap();
            assert_eq!(back, cap);
        }
    }

    #[test]
    fn unknown_capability_is_rejected() {
        // typo, and an old name that no longer exists -> hard error (fail-closed)
        assert!(serde_json::from_value::<Capability>(serde_json::json!("net:outbund")).is_err());
        assert!(serde_json::from_value::<Capability>(serde_json::json!("fs:write")).is_err());
        assert!(serde_json::from_value::<Capability>(serde_json::json!("events")).is_err());
    }

    #[test]
    fn enforced_today_only_for_vault_caps() {
        assert!(Capability::VaultRead.enforced_today());
        assert!(Capability::VaultWrite.enforced_today());
        assert!(Capability::VaultEvents.enforced_today());
        assert!(!Capability::Net.enforced_today());
        assert!(!Capability::Exec.enforced_today());
        assert!(!Capability::FsRead.enforced_today());
    }

    #[test]
    fn capability_displays_as_wire_string() {
        assert_eq!(Capability::VaultRead.to_string(), "vault:read");
        assert_eq!(Capability::FsRead.to_string(), "fs:read");
    }

    #[test]
    fn manifest_parses_typed_capabilities() {
        let m: Manifest = toml::from_str(
            "id=\"x\"\nname=\"X\"\nversion=\"0\"\n\
             [engine]\ncommand=\"./x\"\ncapabilities=[\"vault:read\", \"net\"]\n",
        )
        .unwrap();
        assert_eq!(
            m.engine.capabilities,
            vec![Capability::VaultRead, Capability::Net]
        );
    }

    #[test]
    fn manifest_rejects_unknown_capability() {
        let r: Result<Manifest, _> = toml::from_str(
            "id=\"x\"\nname=\"X\"\nversion=\"0\"\n\
             [engine]\ncommand=\"./x\"\ncapabilities=[\"fs:write\"]\n",
        );
        assert!(
            r.is_err(),
            "unknown capability must fail the manifest parse"
        );
    }
}
