//! A note: a relative path inside a cairn plus its markdown content,
//! split into an optional raw frontmatter block and a body.

/// A note's location, always a forward-slash relative path inside a cairn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotePath(String);

impl NotePath {
    /// Build a `NotePath`, normalizing backslashes and rejecting absolute
    /// or parent-escaping paths.
    pub fn new(raw: &str) -> Result<Self, NotePathError> {
        let norm = raw.replace('\\', "/");
        if norm.starts_with('/') {
            return Err(NotePathError::Absolute);
        }
        if norm.split('/').any(|seg| seg == "..") {
            return Err(NotePathError::Escapes);
        }
        if norm.is_empty() {
            return Err(NotePathError::Empty);
        }
        Ok(Self(norm))
    }

    /// The path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Errors building a [`NotePath`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotePathError {
    /// Path was absolute.
    #[error("note path must be relative")]
    Absolute,
    /// Path tried to escape the cairn with `..`.
    #[error("note path must not contain ..")]
    Escapes,
    /// Path was empty.
    #[error("note path must not be empty")]
    Empty,
}

/// A parsed note: its path, an optional raw YAML frontmatter block
/// (without the `---` fences), and the markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Location inside the cairn.
    pub path: NotePath,
    /// Raw frontmatter text (YAML), if a fenced block was present.
    pub frontmatter: Option<String>,
    /// Markdown body (everything after the frontmatter block).
    pub body: String,
}

impl Note {
    /// Parse raw file contents into a [`Note`]. A leading `---\n ... \n---\n`
    /// block is captured as `frontmatter`; everything else is `body`.
    pub fn parse(path: NotePath, raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix("---\n") {
            if let Some(end) = rest.find("\n---\n") {
                let fm = rest[..end].to_string();
                let body = rest[end + "\n---\n".len()..].to_string();
                return Self {
                    path,
                    frontmatter: Some(fm),
                    body,
                };
            }
        }
        Self {
            path,
            frontmatter: None,
            body: raw.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_absolute_and_escaping_paths() {
        assert_eq!(NotePath::new("/etc/passwd"), Err(NotePathError::Absolute));
        assert_eq!(NotePath::new("../secret"), Err(NotePathError::Escapes));
        assert_eq!(NotePath::new(""), Err(NotePathError::Empty));
    }

    #[test]
    fn normalizes_backslashes() {
        assert_eq!(
            NotePath::new(r"sub\note.md").unwrap().as_str(),
            "sub/note.md"
        );
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let p = NotePath::new("a.md").unwrap();
        let n = Note::parse(p, "---\ntitle: Hi\n---\nHello world");
        assert_eq!(n.frontmatter.as_deref(), Some("title: Hi"));
        assert_eq!(n.body, "Hello world");
    }

    #[test]
    fn note_without_frontmatter_is_all_body() {
        let p = NotePath::new("a.md").unwrap();
        let n = Note::parse(p, "Just text");
        assert_eq!(n.frontmatter, None);
        assert_eq!(n.body, "Just text");
    }
}
