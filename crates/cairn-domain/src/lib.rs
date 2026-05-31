//! Pure domain model for Cairn: notes, links, and the link graph.
//! No I/O lives here.

pub mod note;
pub use note::{Note, NotePath, NotePathError};

pub mod link;
pub use link::{extract_links, LinkTarget};

pub mod graph;
pub use graph::Graph;
