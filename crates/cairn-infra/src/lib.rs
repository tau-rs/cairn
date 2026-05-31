//! Adapters implementing Cairn ports against real systems.

pub mod index;
pub mod localfs;

pub use index::InMemoryIndex;
pub use localfs::LocalFsStore;
