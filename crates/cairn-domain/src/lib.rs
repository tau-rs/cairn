//! Pure domain model for Cairn: notes, links, and the link graph.
//! No I/O lives here.

pub mod note;
pub use note::{Note, NotePath, NotePathError};

pub mod link;
pub use link::{extract_links, rewrite_link_target, LinkTarget};

pub mod graph;
pub use graph::Graph;

pub mod block;

pub mod crdt;
