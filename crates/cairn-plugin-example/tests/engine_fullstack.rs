//! Full-stack integration: a REAL subprocess plugin driven through the REAL
//! engine, over a REAL cairn on disk.
//!
//! The rest of the plugin suite covers each seam in isolation: `tests/host.rs`
//! spawns the real `cairn-plugin-example` binary but services its host-callbacks
//! with an in-memory `MapCallbacks`; the engine tests in `cairn-app` exercise the
//! real `EngineCallbacks` bridge but drive it from an in-process *stub* host. No
//! single test wires the two real halves together. These do:
//!
//!   subprocess plugin  ──stdio JSON-RPC──▶  ProcessPluginHost
//!        ▲                                        │ host-callback
//!        │ invoke result                          ▼
//!   Engine::invoke_plugin_command ──▶ EngineCallbacks ──▶ Engine::write_note
//!                                                              │
//!                                             LocalFsStore (file) + EventSink
//!
//! We assert on both observable effects of that round-trip: the emitted `Event`
//! reaches the sink, and the note is persisted to the vault on disk.

use cairn_app::{Engine, Event};
use cairn_domain::NotePath;
use cairn_infra::{ProcessPluginHost, TrustedPlugins};
use cairn_ports::{PluginEvent, Sandbox, SandboxCapabilities, SandboxError};
use cairn_startup::build_engine;
use std::path::Path;
use std::process::Command;

/// Test double: spawns the command verbatim (no OS jail). The real sandbox is an
/// orthogonal concern covered elsewhere; here we exercise the engine/plugin
/// round-trip, so we want the child to actually run on every OS.
struct PermissiveSandbox;
impl Sandbox for PermissiveSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        _dir: &Path,
        cmd: &Path,
        args: &[String],
        _caps: SandboxCapabilities,
    ) -> Result<Command, SandboxError> {
        let mut c = Command::new(cmd);
        c.args(args);
        Ok(c)
    }
}

/// Write `<vault>/.cairn/plugins/example/manifest.toml` pointing at the built
/// example binary with the given capability list (TOML array body, e.g.
/// `"\"vault:write\""`).
///
/// The command path goes in a TOML *literal* (single-quoted) string: on Windows
/// the path contains backslashes, which a basic `"..."` string would treat as
/// invalid escapes.
fn write_manifest(vault_root: &Path, caps: &str) {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let pdir = vault_root.join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[{caps}]\n"
        ),
    )
    .unwrap();
}

/// Build a real engine over `vault_root` and inject a real `ProcessPluginHost`
/// that spawns the example binary. The manifest must already exist.
fn engine_with_real_plugin(vault_root: &Path) -> Engine {
    let host = ProcessPluginHost::load(
        &vault_root.join(".cairn").join("plugins"),
        &TrustedPlugins::from_ids(["example".to_string()]),
        &PermissiveSandbox,
    )
    .unwrap();
    // `build_engine` git-inits the vault and wires the concrete store/index/vcs.
    let mut engine = build_engine(vault_root).unwrap();
    engine.set_plugin_host(Box::new(host));
    engine
}

#[test]
fn invoke_writenote_persists_to_vault_and_emits_event() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_manifest(root, "\"vault:write\"");
    let mut engine = engine_with_real_plugin(root);

    // Sanity: the real subprocess handshake declared its command.
    assert!(
        engine
            .list_plugins()
            .iter()
            .any(|p| p.id == "example" && p.commands.iter().any(|c| c.id == "writeNote")),
        "example plugin must load and declare writeNote"
    );

    // Drive an invoke whose handler makes a `write_note` host-callback. That
    // callback crosses back into the real engine via EngineCallbacks.
    let mut sink: Vec<Event> = Vec::new();
    let out = engine
        .invoke_plugin_command(
            "example",
            "writeNote",
            &serde_json::json!({ "path": "n.md", "contents": "hi from the plugin" }),
            &mut sink,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({ "written": true }));

    // (1) The event emitted by the real engine reached the sink.
    let changed = NotePath::new("n.md").unwrap();
    assert!(
        sink.contains(&Event::NoteChanged(changed.clone())),
        "expected NoteChanged in sink, got {sink:?}"
    );
    assert!(
        sink.iter().any(|e| matches!(e, Event::Reindexed(_))),
        "engine write should also reindex, got {sink:?}"
    );

    // (2) The note was persisted to the vault — assert on the raw file on disk,
    // not just the engine's view, to prove it round-tripped through LocalFsStore.
    let on_disk = std::fs::read_to_string(root.join("n.md")).unwrap();
    assert_eq!(on_disk, "hi from the plugin");
    // And the engine reads back the same content.
    assert_eq!(engine.read_note(&changed).unwrap(), "hi from the plugin");
}

#[test]
fn dispatch_event_routes_handler_write_through_real_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // The example's `on_event` handler writes a marker note via `write_note`, so
    // it needs both the `vault:events` subscription and `vault:write`.
    write_manifest(root, "\"vault:events\", \"vault:write\"");
    let mut engine = engine_with_real_plugin(root);

    // Deliver a cairn event to the subscribed subprocess plugin. Its handler
    // calls back into the engine to persist `seen.md = <changed path>`.
    let mut sink: Vec<Event> = Vec::new();
    engine.dispatch_plugin_event(
        &PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
        &mut sink,
    );

    // The handler's write landed on disk through the real engine...
    let seen = std::fs::read_to_string(root.join("seen.md")).unwrap();
    assert_eq!(seen, "x.md");
    // ...and emitted its own NoteChanged for the marker note into the sink.
    assert!(
        sink.contains(&Event::NoteChanged(NotePath::new("seen.md").unwrap())),
        "handler write should emit NoteChanged(seen.md), got {sink:?}"
    );
}
