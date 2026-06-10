//! Adapters implementing Cairn ports against real systems.

pub mod git;
pub mod index;
pub mod localfs;
pub mod notify_watcher;
mod plugin_host;
pub mod seams;
mod tantivy_index;

pub use git::GitVcs;
pub use index::InMemoryIndex;
pub use localfs::{ensure_cairn_dir, LocalFsStore};
pub use notify_watcher::NotifyWatcher;
pub use plugin_host::{ProcessPluginHost, DEFAULT_PLUGIN_TIMEOUT};
pub use seams::{BlockingExecutor, NoCollab, NoopWatcher, NullRuntime};
pub use tantivy_index::TantivyIndex;
