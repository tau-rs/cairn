//! Pure domain model for Cairn: notes, links, and the link graph.
//! No I/O lives here.

pub mod note;
pub use note::{Note, NotePath, NotePathError};
