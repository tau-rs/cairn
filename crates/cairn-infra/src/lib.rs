//! Adapters implementing Cairn ports against real systems.

pub mod git;
pub mod index;
pub mod localfs;
pub mod notify_watcher;
pub mod seams;
mod tantivy_index;

pub use git::GitVcs;
pub use index::InMemoryIndex;
pub use localfs::LocalFsStore;
pub use notify_watcher::NotifyWatcher;
pub use seams::{BlockingExecutor, NoCollab, NoopWatcher, NullRuntime};
pub use tantivy_index::TantivyIndex;
