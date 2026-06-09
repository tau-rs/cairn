use cairn_infra::ProcessPluginHost;
use cairn_ports::{PluginCallbacks, PluginHost, PortError};
use std::collections::HashMap;

/// A test double for host-callbacks: serves notes from an in-memory map.
struct MapCallbacks(HashMap<String, String>);
impl PluginCallbacks for MapCallbacks {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("note {path}")))
    }
}

#[test]
fn host_loads_invokes_and_rejects_unknown() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    // The command path goes in a TOML *literal* string (single quotes): on
    // Windows the path has backslashes, which a basic ("...") string would treat
    // as invalid escapes.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!("id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n[engine]\ncommand='{bin}'\n"),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());

    let plugins = host.plugins();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "example");
    assert!(plugins[0].commands.iter().any(|c| c.id == "echo"));

    let out = host
        .invoke(
            "example",
            "echo",
            &serde_json::json!({"x": 1, "y": "z"}),
            &mut cb,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({"x": 1, "y": "z"}));

    assert!(matches!(
        host.invoke("missing", "echo", &serde_json::Value::Null, &mut cb),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        host.invoke("example", "nope", &serde_json::Value::Null, &mut cb),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn note_len_reads_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    // Literal (single-quote) TOML string for the path; declare fs:read.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[\"fs:read\"]\n"
        ),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([(
        "note.md".to_string(),
        "hello body".to_string(),
    )]));

    let out = host
        .invoke(
            "example",
            "noteLen",
            &serde_json::json!({"path": "note.md"}),
            &mut cb,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({"len": 10}));
}

#[test]
fn note_len_denied_without_capability() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    // No capabilities declared -> the host must deny host/readNote.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\n"
        ),
    )
    .unwrap();

    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([(
        "note.md".to_string(),
        "hello body".to_string(),
    )]));

    let err = host
        .invoke(
            "example",
            "noteLen",
            &serde_json::json!({"path": "note.md"}),
            &mut cb,
        )
        .unwrap_err();
    assert!(
        matches!(err, PortError::Adapter(_)),
        "expected Adapter, got {err:?}"
    );
}
