//! A `Watcher` backed by `notify` + `notify-debouncer-full`. Watches a cairn
//! root recursively, reports debounced changes to `.md` files (ignoring
//! `.git/`), classifying each by current on-disk existence.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use cairn_domain::NotePath;
use cairn_ports::{FsChange, PortError, WatchHandle, Watcher};
use notify::{RecursiveMode, Watcher as NotifyWatcherTrait};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

/// Filesystem watcher over a cairn directory.
#[derive(Debug, Default)]
pub struct NotifyWatcher;

/// Map an absolute changed path to an `FsChange`, or `None` to ignore it.
/// Keeps only `.md` files under `root`, skipping any `.git/` segment.
/// Classifies by existence: present -> Changed, absent -> Removed.
fn classify(root: &Path, path: &Path) -> Option<FsChange> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.to_str()?.replace('\\', "/");
    if rel.split('/').any(|seg| seg == ".git") {
        return None;
    }
    if !rel.ends_with(".md") {
        return None;
    }
    let note_path = NotePath::new(&rel).ok()?;
    if path.exists() {
        Some(FsChange::Changed(note_path))
    } else {
        Some(FsChange::Removed(note_path))
    }
}

impl Watcher for NotifyWatcher {
    fn watch(&self, root: &Path) -> Result<WatchHandle, PortError> {
        let (tx, rx) = mpsc::channel::<FsChange>();
        // Canonicalize to resolve symlinks (e.g. /tmp -> /private/tmp on macOS)
        // so that strip_prefix works correctly against OS-reported event paths.
        let root = root
            .canonicalize()
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        let cb_root = root.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            None,
            move |result: DebounceEventResult| {
                let Ok(events) = result else { return };
                for event in events {
                    // DebouncedEvent derefs to notify::Event, so .paths is accessible directly.
                    for path in &event.paths {
                        if let Some(change) = classify(&cb_root, path) {
                            let _ = tx.send(change);
                        }
                    }
                }
            },
        )
        .map_err(|e| PortError::Adapter(e.to_string()))?;
        debouncer
            .watcher()
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(WatchHandle::new(rx, Box::new(debouncer)))
    }
}
