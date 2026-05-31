//! Adapters implementing Cairn ports against real systems.

pub mod git;
pub mod index;
pub mod localfs;
pub mod seams;

pub use git::GitVcs;
pub use index::InMemoryIndex;
pub use localfs::LocalFsStore;
pub use seams::{BlockingExecutor, NoCollab, NoopWatcher, NullRuntime};
