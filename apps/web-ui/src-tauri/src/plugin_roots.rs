use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The serve allow-list: plugin id → canonicalised ui/ dir. Only dirs in here
/// are ever served by the plugin-sandbox protocol. Replaced wholesale on each
/// `set_plugin_ui_roots` so unloaded plugins immediately stop being serveable.
#[derive(Default)]
pub struct PluginRoots(pub Mutex<HashMap<String, PathBuf>>);

impl PluginRoots {
    /// Replace the whole map. Each incoming root is canonicalised; entries that
    /// don't exist / fail to canonicalise / aren't dirs are dropped.
    pub fn replace(&self, incoming: HashMap<String, String>) {
        let mut map = self.0.lock().expect("plugin roots mutex poisoned");
        map.clear();
        for (id, path) in incoming {
            if let Ok(canon) = std::fs::canonicalize(&path) {
                if canon.is_dir() {
                    map.insert(id, canon);
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<PathBuf> {
        self.0
            .lock()
            .expect("plugin roots mutex poisoned")
            .get(id)
            .cloned()
    }
}

/// Replace the registered ui-root allow-list. Called by the host after
/// loadPlugins with `{ id: absoluteUiDir }` for every plugin that has a bundle.
#[tauri::command]
pub fn set_plugin_ui_roots(
    roots: HashMap<String, String>,
    state: tauri::State<'_, PluginRoots>,
) -> Result<(), String> {
    state.replace(roots);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_canonicalises_and_drops_missing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("ui")).unwrap();
        let roots = PluginRoots::default();
        let mut incoming = HashMap::new();
        incoming.insert(
            "good".into(),
            d.path().join("ui").to_string_lossy().into_owned(),
        );
        incoming.insert("missing".into(), "/no/such/dir".into());
        roots.replace(incoming);
        assert!(roots.get("good").is_some());
        assert!(roots.get("missing").is_none());
    }

    #[test]
    fn replace_drops_stale_ids() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("ui")).unwrap();
        let ui = d.path().join("ui").to_string_lossy().into_owned();
        let roots = PluginRoots::default();
        roots.replace(HashMap::from([("a".to_string(), ui.clone())]));
        assert!(roots.get("a").is_some());
        // second call without "a" must evict it
        roots.replace(HashMap::from([("b".to_string(), ui)]));
        assert!(roots.get("a").is_none());
        assert!(roots.get("b").is_some());
    }
}
