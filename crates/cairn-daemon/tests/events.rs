use cairn_app::Engine;
use cairn_contract::Command;
use cairn_daemon::AppState;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
use cairn_ports::{
    EventDispatchError, PluginCallbacks, PluginEvent, PluginHost, PluginInfo, PortError,
};
use std::sync::{Arc, Mutex};

/// A stub host that records every dispatched cairn event.
struct RecordingHost(Arc<Mutex<Vec<PluginEvent>>>);
impl PluginHost for RecordingHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        Vec::new()
    }
    fn invoke(
        &mut self,
        plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        Err(PortError::NotFound(format!("plugin {plugin}")))
    }
    fn dispatch_event(
        &mut self,
        event: &PluginEvent,
        _callbacks: &mut dyn PluginCallbacks,
    ) -> Vec<EventDispatchError> {
        self.0.lock().unwrap().push(event.clone());
        Vec::new()
    }
}

#[test]
fn write_command_forwards_note_event_to_plugins() {
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(
        LocalFsStore::open(tmp.path()).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(tmp.path()).unwrap(),
    );
    let recorded = Arc::new(Mutex::new(Vec::new()));
    engine.set_plugin_host(Box::new(RecordingHost(recorded.clone())));
    let state = AppState::new(engine);

    state
        .run_command_blocking(&Command::WriteNote {
            path: "a.md".into(),
            contents: "hi".into(),
        })
        .unwrap();

    let events = recorded.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PluginEvent::NoteChanged(p) if p.as_str() == "a.md")),
        "expected NoteChanged(a.md) forwarded, got {events:?}"
    );
}
