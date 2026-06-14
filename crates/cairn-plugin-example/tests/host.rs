use cairn_domain::{Note, NotePath};
use cairn_infra::{PinnedHash, ProcessPluginHost, TrustedPlugins};
use cairn_ports::{PluginCallbacks, PluginHost, PortError, Sandbox, SandboxError, SearchHit};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Test double: spawns the command verbatim (no OS jail).
struct PermissiveSandbox;
impl Sandbox for PermissiveSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        _dir: &Path,
        cmd: &Path,
        args: &[String],
    ) -> Result<Command, SandboxError> {
        let mut c = Command::new(cmd);
        c.args(args);
        Ok(c)
    }
}

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

/// Populate `<tmp>/.cairn/plugins/example` with a valid manifest (absolute
/// command) and return that plugin dir. Mirrors the existing test setup.
fn setup_example_dir(tmp: &std::path::Path) -> std::path::PathBuf {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let pdir = tmp.join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "");
    pdir
}

/// Load a host from `<tmp>/.cairn/plugins`, trusting the `example` plugin.
fn load_example(tmp: &std::path::Path) -> ProcessPluginHost {
    let dir = tmp.join(".cairn").join("plugins");
    ProcessPluginHost::load(
        &dir,
        &TrustedPlugins::from_ids(["example".to_string()]),
        &PermissiveSandbox,
    )
    .unwrap()
}

/// Like `load_example` but with an explicit per-message timeout.
fn load_example_with_timeout(
    tmp: &std::path::Path,
    timeout: std::time::Duration,
) -> ProcessPluginHost {
    let dir = tmp.join(".cairn").join("plugins");
    ProcessPluginHost::load_with_timeout(
        &dir,
        timeout,
        &TrustedPlugins::from_ids(["example".to_string()]),
        &PermissiveSandbox,
    )
    .unwrap()
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
                    path: NotePath::new(path)
                        .map_err(|e| PortError::Adapter(e.to_string().into()))?,
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
                    .map_err(|e| PortError::Adapter(e.to_string().into()))
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

    let mut host = load_example(tmp.path());
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
    // Literal (single-quote) TOML string for the path; declare vault:read.
    std::fs::write(
        pdir.join("manifest.toml"),
        format!(
            "id=\"example\"\nname=\"Example\"\nversion=\"0.1.0\"\n\
             [engine]\ncommand='{bin}'\ncapabilities=[\"vault:read\"]\n"
        ),
    )
    .unwrap();

    let mut host = load_example(tmp.path());
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

    let mut host = load_example(tmp.path());
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
    write_manifest(&pdir, bin, "\"vault:write\"");
    let mut host = load_example(tmp.path());
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
    write_manifest(&pdir, bin, "\"vault:read\""); // read but NOT write
    let mut host = load_example(tmp.path());
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
    write_manifest(&pdir, bin, "\"vault:read\"");
    let mut host = load_example(tmp.path());
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
    write_manifest(&pdir, bin, "\"vault:read\"");
    let mut host = load_example(tmp.path());
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
fn delete_note_via_callback() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"vault:write\"");
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::from([("n.md".to_string(), "body".to_string())]));
    let out = host
        .invoke(
            "example",
            "deleteNote",
            &serde_json::json!({"path": "n.md"}),
            &mut cb,
        )
        .unwrap();
    assert_eq!(out, serde_json::json!({"deleted": true}));
    assert!(!cb.0.contains_key("n.md"), "the note should be removed");
}

#[test]
fn delete_denied_without_fs_write() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"vault:read\""); // read but NOT write
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::from([("n.md".to_string(), "body".to_string())]));
    let err = host
        .invoke(
            "example",
            "deleteNote",
            &serde_json::json!({"path": "n.md"}),
            &mut cb,
        )
        .unwrap_err();
    assert!(
        matches!(err, PortError::Adapter(_)),
        "expected Adapter, got {err:?}"
    );
    assert!(cb.0.contains_key("n.md"), "denied delete must not mutate");
}

#[test]
fn search_denied_without_fs_read() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no capabilities
    let mut host = load_example(tmp.path());
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

#[test]
fn event_delivered_and_handler_writes() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"vault:events\", \"vault:write\"");
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new());
    host.dispatch_event(
        &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
        &mut cb,
    );
    // The example's on_event handler wrote seen.md = the changed path, via host.write_note.
    assert_eq!(cb.0.get("seen.md").map(String::as_str), Some("x.md"));
}

#[test]
fn event_skipped_without_events_cap() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, "\"vault:write\""); // vault:write but NOT vault:events
    let mut host = load_example(tmp.path());
    let mut cb = MapCallbacks(HashMap::new());
    host.dispatch_event(
        &cairn_ports::PluginEvent::NoteChanged(NotePath::new("x.md").unwrap()),
        &mut cb,
    );
    assert!(cb.0.is_empty(), "no events cap -> no delivery");
}

#[test]
fn invoke_times_out_and_kills_plugin() {
    use std::time::{Duration, Instant};
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, bin, ""); // no caps needed; `hang` makes no callbacks
    let mut host = load_example_with_timeout(tmp.path(), Duration::from_millis(2_000));
    let mut cb = MapCallbacks(HashMap::new());

    let start = Instant::now();
    let err = host
        .invoke("example", "hang", &serde_json::Value::Null, &mut cb)
        .unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "hang should time out quickly"
    );
    assert!(
        matches!(&err, PortError::Adapter(m) if m.to_string().contains("timed out")),
        "expected a timeout Adapter, got {err:?}"
    );

    // The plugin was killed, so a follow-up invoke fails fast (no re-hang).
    let err2 = host
        .invoke("example", "echo", &serde_json::json!({"x": 1}), &mut cb)
        .unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "follow-up should not hang"
    );
    assert!(
        matches!(err2, PortError::Adapter(_)),
        "expected Adapter, got {err2:?}"
    );
}

#[test]
fn approved_plugin_is_spawned_unapproved_is_not() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    // Two valid plugins on disk, each with a working binary.
    write_manifest(&plugins.join("example"), bin, "");
    write_manifest(&plugins.join("rogue"), bin, "");
    // Only `example` is trusted; `rogue` must be skipped by the trust gate.
    let host = ProcessPluginHost::load(
        &plugins,
        &TrustedPlugins::from_ids(["example".to_string()]),
        &PermissiveSandbox,
    )
    .unwrap();
    let ids: Vec<String> = host.plugins().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, vec!["example".to_string()]);
}

#[test]
fn default_deny_spawns_nothing() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    write_manifest(&plugins.join("example"), bin, "");
    let host =
        ProcessPluginHost::load(&plugins, &TrustedPlugins::none(), &PermissiveSandbox).unwrap();
    assert!(host.plugins().is_empty());
}

#[test]
fn pinned_matching_hash_spawns() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    let pin = PinnedHash::of_dir(&pdir).unwrap().to_string();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_entries([("example".to_string(), Some(pin))]).unwrap();
    let host = ProcessPluginHost::load(&dir, &trusted, &PermissiveSandbox).unwrap();
    assert_eq!(host.plugins().len(), 1);
}

#[test]
fn drifted_hash_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    let pin = PinnedHash::of_dir(&pdir).unwrap().to_string();
    // Tamper: add a file so the tree no longer matches the pin.
    std::fs::write(pdir.join("evil.txt"), b"tampered").unwrap();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_entries([("example".to_string(), Some(pin))]).unwrap();
    let host = ProcessPluginHost::load(&dir, &trusted, &PermissiveSandbox).unwrap();
    assert!(host.plugins().is_empty());
}

#[test]
fn unpinned_trusted_spawns() {
    let tmp = tempfile::tempdir().unwrap();
    setup_example_dir(tmp.path());
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_ids(["example".to_string()]);
    let host = ProcessPluginHost::load(&dir, &trusted, &PermissiveSandbox).unwrap();
    assert_eq!(host.plugins().len(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_in_trusted_dir_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    std::os::unix::fs::symlink(pdir.join("manifest.toml"), pdir.join("link.toml")).unwrap();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_ids(["example".to_string()]);
    let host = ProcessPluginHost::load(&dir, &trusted, &PermissiveSandbox).unwrap();
    assert!(host.plugins().is_empty());
}

#[test]
fn example_declares_contributions_at_initialize() {
    let tmp = tempfile::tempdir().unwrap();
    setup_example_dir(tmp.path());
    let host = load_example(tmp.path());
    let plugins = host.plugins();
    let contribs = &plugins[0].contributions;
    assert_eq!(contribs.len(), 2);
    assert!(contribs
        .iter()
        .any(|c| matches!(c.slot, cairn_plugin_protocol::PluginSlot::SidebarSection)));
    assert!(contribs
        .iter()
        .any(|c| matches!(c.slot, cairn_plugin_protocol::PluginSlot::TopbarAction)));
}

#[test]
fn trusted_dir_with_mismatched_manifest_id_is_rejected() {
    let bin = env!("CARGO_BIN_EXE_cairn-plugin-example");
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join(".cairn").join("plugins");
    // Directory `example` is trusted, but its manifest claims id `evil`.
    let pdir = plugins.join("example");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        format!("id=\"evil\"\nname=\"E\"\nversion=\"0\"\n[engine]\ncommand='{bin}'\n"),
    )
    .unwrap();
    let host = ProcessPluginHost::load(
        &plugins,
        &TrustedPlugins::from_ids(["example".to_string()]),
        &PermissiveSandbox,
    )
    .unwrap();
    assert!(
        host.plugins().is_empty(),
        "id-mismatched plugin must not load"
    );
}
