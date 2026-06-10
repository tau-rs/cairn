use cairn_domain::{Note, NotePath};
use cairn_infra::ProcessPluginHost;
use cairn_ports::{PluginCallbacks, PluginHost, PortError, SearchHit};
use std::collections::HashMap;

fn write_manifest(pdir: &std::path::Path, bin: &str, caps: &str) {
    std::fs::create_dir_all(pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[{caps}]\n"
        ),
    )
    .unwrap();
}

/// A test double for host-callbacks: serves notes from an in-memory map.
struct MapCallbacks(HashMap<String, String>);
impl PluginCallbacks for MapCallbacks {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("note {path}")))
    }

    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError> {
        self.0.insert(path.to_string(), contents.to_string());
        Ok(())
    }

    fn delete_note(&mut self, path: &str) -> Result<(), PortError> {
        // NB: Ok even if absent, unlike Engine::delete_note (which returns NotFound).
        self.0.remove(path);
        Ok(())
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        // Substring match over values. Hit order is unspecified (HashMap order);
        // tests must assert only on counts, not ordering.
        let mut hits = Vec::new();
        for (path, contents) in &self.0 {
            if contents.contains(query) {
                hits.push(SearchHit {
                    path: NotePath::new(path).map_err(|e| PortError::Adapter(e.to_string()))?,
                    score: 1.0,
                    snippet: contents.clone(),
                    highlights: Vec::new(),
                });
            }
        }
        Ok(hits)
    }

    fn list_notes(&mut self) -> Result<Vec<Note>, PortError> {
        let mut notes: Vec<Note> = self
            .0
            .iter()
            .map(|(path, contents)| {
                NotePath::new(path)
                    .map(|np| Note::parse(np, contents))
                    .map_err(|e| PortError::Adapter(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        notes.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(notes)
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

#[test]
fn write_note_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:write\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    let out = host
        .invoke(
            "example",
            "writeNote",
            &serde_json::json!({"path": "n.md", "contents": "hi there"}),
            &mut cb,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({"written": true}));
    assert_eq!(cb.0.get("n.md").map(String::as_str), Some("hi there"));
}

#[test]
fn write_denied_without_fs_write() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\""); // read but NOT write
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::new());
    let err = host
        .invoke(
            "example",
            "writeNote",
            &serde_json::json!({"path": "n.md", "contents": "x"}),
            &mut cb,
        )
        .unwrap_err();
    assert!(
        matches!(err, PortError::Adapter(_)),
        "expected Adapter, got {err:?}"
    );
    assert!(cb.0.is_empty(), "denied write must not mutate");
}

#[test]
fn note_count_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([
        ("a.md".to_string(), "alpha".to_string()),
        ("b.md".to_string(), "beta".to_string()),
    ]));
    let out = host
        .invoke("example", "noteCount", &serde_json::Value::Null, &mut cb)
        .unwrap();
    assert_eq!(out, serde_json::json!({"count": 2}));
}

#[test]
fn find_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"fs:read\"");
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([
        ("a.md".to_string(), "the quick fox".to_string()),
        ("b.md".to_string(), "lazy dog".to_string()),
    ]));
    let out = host
        .invoke(
            "example",
            "find",
            &serde_json::json!({"query": "quick"}),
            &mut cb,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({"hits": 1}));
}

#[test]
fn search_denied_without_fs_read() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no capabilities
    let mut host = ProcessPluginHost::load(&tmp.path().join(".cairn").join("plugins")).unwrap();
    let mut cb = MapCallbacks(HashMap::from([("a.md".to_string(), "x".to_string())]));
    let err = host
        .invoke(
            "example",
            "find",
            &serde_json::json!({"query": "x"}),
            &mut cb,
        )
        .unwrap_err();
    assert!(
        matches!(err, PortError::Adapter(_)),
        "expected Adapter, got {err:?}"
    );
}
