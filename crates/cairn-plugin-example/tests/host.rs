use cairn_infra::ProcessPluginHost;
use cairn_ports::{PluginHost, PortError};

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

    let plugins = host.plugins();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "example");
    assert!(plugins[0].commands.iter().any(|c| c.id == "echo"));

    let out = host
        .invoke("example", "echo", &serde_json::json!({"x": 1, "y": "z"}))
        .unwrap();
    assert_eq!(out, serde_json::json!({"x": 1, "y": "z"}));

    assert!(matches!(
        host.invoke("missing", "echo", &serde_json::Value::Null),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        host.invoke("example", "nope", &serde_json::Value::Null),
        Err(PortError::NotFound(_))
    ));
}
