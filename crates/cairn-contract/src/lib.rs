//! The transport-blind contract: serializable Command / Query / Event DTOs
//! with generated TypeScript bindings. This is the surface a UI consumes.
//!
//! These DTOs are intentionally independent of `cairn-domain` types so the
//! wire format can stay stable while the domain evolves.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A request that mutates the cairn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Create or overwrite a note.
    WriteNote {
        /// Relative note path.
        path: String,
        /// Full markdown contents.
        contents: String,
    },
    /// Delete a note.
    DeleteNote {
        /// Relative note path.
        path: String,
    },
    /// Commit all changes with a message.
    Commit {
        /// Commit message.
        message: String,
    },
}

/// A read-only request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    /// Read a note's contents.
    GetNote {
        /// Relative note path.
        path: String,
    },
    /// Search note content.
    Search {
        /// Query string.
        query: String,
    },
    /// List the notes that link to a note.
    GetBacklinks {
        /// Relative note path.
        path: String,
    },
}

/// A push event emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A note was created or updated.
    NoteChanged {
        /// Relative note path.
        path: String,
    },
    /// A note was deleted.
    NoteDeleted {
        /// Relative note path.
        path: String,
    },
    /// The cairn was committed.
    Committed {
        /// Short commit id.
        commit: String,
    },
    /// The index finished rebuilding.
    Reindexed {
        /// Number of notes indexed.
        count: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serializes_with_type_tag() {
        let cmd = Command::WriteNote {
            path: "a.md".into(),
            contents: "hi".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"write_note\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }
}
