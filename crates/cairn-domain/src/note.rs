//! A note: a relative path inside a cairn plus its markdown content,
//! split into an optional raw frontmatter block and a body.

/// A note's location, always a forward-slash relative path inside a cairn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotePath(String);

impl NotePath {
    /// Build a `NotePath`, normalizing backslashes and rejecting absolute
    /// or parent-escaping paths.
    ///
    /// # Errors
    /// Returns [`NotePathError`] if the path is empty, absolute, contains a
    /// `..` segment, or contains a dot-leading segment (e.g. `.git`,
    /// `.cairn`, any dotfile) — the last guards against writes escaping into
    /// control directories.
    pub fn new(raw: &str) -> Result<Self, NotePathError> {
        let norm = raw.replace('\\', "/");
        if norm.is_empty() {
            return Err(NotePathError::Empty);
        }
        if norm.starts_with('/') {
            return Err(NotePathError::Absolute);
        }
        for seg in norm.split('/') {
            if seg == ".." {
                return Err(NotePathError::Escapes);
            }
            if seg.starts_with('.') {
                return Err(NotePathError::Hidden);
            }
        }
        Ok(Self(norm))
    }

    /// The path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The note's stem: the filename without its directory or `.md`
    /// extension (e.g. `dir/a.md` -> `a`).
    #[must_use]
    pub fn stem(&self) -> &str {
        let after_slash = self.0.rsplit('/').next().unwrap_or(&self.0);
        after_slash.strip_suffix(".md").unwrap_or(after_slash)
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
    /// A path segment began with `.` (e.g. `.git`, `.cairn`, a dotfile).
    /// These are never notes and could escape into control directories.
    #[error("note path must not contain dot-leading segments")]
    Hidden,
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
    #[must_use]
    pub fn parse(path: NotePath, raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix("---\n") {
            // The closing fence is a line containing only `---`. It can
            // appear immediately (empty frontmatter) or after YAML lines.
            if let Some(body) = rest.strip_prefix("---\n") {
                return Self {
                    path,
                    frontmatter: Some(String::new()),
                    body: body.to_string(),
                };
            }
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

    /// A human display title: the frontmatter `title:` value if present,
    /// else the first Markdown `# ` heading in the body, else the path stem.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let Some(fm) = &self.frontmatter {
            for line in fm.lines() {
                if let Some(rest) = line.trim_start().strip_prefix("title:") {
                    let t = rest.trim().trim_matches('"').trim_matches('\'').trim();
                    if !t.is_empty() {
                        return t.to_string();
                    }
                }
            }
        }
        for line in self.body.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("# ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
        self.path.stem().to_string()
    }

    /// A non-cryptographic hash of the note's content (frontmatter + body),
    /// for in-memory change detection / memoization. Not for security, and
    /// not stable across Rust versions or process restarts — do not persist
    /// it or compare hashes across processes.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.frontmatter.hash(&mut h);
        self.body.hash(&mut h);
        h.finish()
    }

    /// Tags declared in the note's frontmatter `tags:` key. Frontmatter-only
    /// (inline `#tags` are not parsed). Supports inline arrays
    /// (`tags: [a, b]`), block lists (`- a` lines), and comma/space-separated
    /// scalars (`tags: a, b` / `tags: a b`). Values are trimmed, unquoted, and
    /// de-duplicated in first-seen order. Only the literal key `tags:` matches.
    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        let Some(fm) = &self.frontmatter else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        let mut lines = fm.lines();
        while let Some(line) = lines.next() {
            let Some(rest) = line.trim_start().strip_prefix("tags:") else {
                continue;
            };
            let rest = rest.trim();
            if let Some(inner) = rest.strip_prefix('[') {
                let inner = inner.strip_suffix(']').unwrap_or(inner);
                for tok in inner.split(',') {
                    push_tag(&mut out, tok);
                }
            } else if rest.is_empty() {
                // Block list: consume following "- item" lines.
                for item_line in lines.by_ref() {
                    let t = item_line.trim_start();
                    if let Some(item) = t.strip_prefix("- ") {
                        push_tag(&mut out, item);
                    } else if t.is_empty() {
                        continue;
                    } else {
                        break;
                    }
                }
            } else {
                for tok in rest.split([',', ' ', '\t']) {
                    push_tag(&mut out, tok);
                }
            }
            break; // only the first `tags:` key
        }
        out
    }
}

/// Trim, strip one layer of surrounding quotes, and push a tag if non-empty
/// and not already present (dedup, first-seen order).
fn push_tag(out: &mut Vec<String>, raw: &str) {
    let t = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if !t.is_empty() && !out.iter().any(|x| x == t) {
        out.push(t.to_string());
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
    fn rejects_dot_leading_segments() {
        // The RCE vectors from the security audit (S1): a note write must not
        // be able to plant a plugin manifest or a `.git/config`.
        assert_eq!(
            NotePath::new(".cairn/plugins/evil/manifest.toml"),
            Err(NotePathError::Hidden)
        );
        assert_eq!(NotePath::new(".git/config"), Err(NotePathError::Hidden));
        // A hidden segment anywhere in the path, not just the first.
        assert_eq!(
            NotePath::new("notes/.git/config"),
            Err(NotePathError::Hidden)
        );
        assert_eq!(NotePath::new("a/.hidden.md"), Err(NotePathError::Hidden));
        // A lone "." (current-dir) segment.
        assert_eq!(NotePath::new("./a.md"), Err(NotePathError::Hidden));
        // Backslash-normalized variant still caught.
        assert_eq!(NotePath::new(r".cairn\x"), Err(NotePathError::Hidden));
        // ".." stays mapped to the pre-existing Escapes error.
        assert_eq!(NotePath::new("a/../b"), Err(NotePathError::Escapes));
        // Ordinary notes still accepted.
        assert_eq!(NotePath::new("dir/a.md").unwrap().as_str(), "dir/a.md");
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

    #[test]
    fn parses_empty_frontmatter_block() {
        let p = NotePath::new("a.md").unwrap();
        let n = Note::parse(p, "---\n---\nbody");
        assert_eq!(n.frontmatter.as_deref(), Some(""));
        assert_eq!(n.body, "body");
    }

    #[test]
    fn stem_strips_dir_and_extension() {
        assert_eq!(NotePath::new("dir/sub/a.md").unwrap().stem(), "a");
        assert_eq!(NotePath::new("b").unwrap().stem(), "b");
    }

    #[test]
    fn display_title_prefers_frontmatter_then_heading_then_stem() {
        let p = NotePath::new("a.md").unwrap();
        let fm = Note::parse(p.clone(), "---\ntitle: \"My Title\"\n---\n# Heading\nbody");
        assert_eq!(fm.display_title(), "My Title");

        let heading = Note::parse(p.clone(), "# The Heading\nbody");
        assert_eq!(heading.display_title(), "The Heading");

        let plain = Note::parse(p, "just text");
        assert_eq!(plain.display_title(), "a");
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let p = NotePath::new("a.md").unwrap();
        let a1 = Note::parse(p.clone(), "---\ntitle: X\n---\nbody");
        let a2 = Note::parse(p.clone(), "---\ntitle: X\n---\nbody");
        let b = Note::parse(p, "---\ntitle: X\n---\nDIFFERENT");
        assert_eq!(a1.content_hash(), a2.content_hash());
        assert_ne!(a1.content_hash(), b.content_hash());
    }

    fn tags_of(raw: &str) -> Vec<String> {
        Note::parse(NotePath::new("a.md").unwrap(), raw).tags()
    }

    #[test]
    fn tags_parses_all_frontmatter_forms() {
        assert_eq!(
            tags_of("---\ntags: [rust, \"ideas\"]\n---\nbody"),
            vec!["rust", "ideas"]
        );
        assert_eq!(
            tags_of("---\ntags:\n  - rust\n  - ideas\n---\nbody"),
            vec!["rust", "ideas"]
        );
        assert_eq!(
            tags_of("---\ntags: rust, ideas\n---\nbody"),
            vec!["rust", "ideas"]
        );
        assert_eq!(
            tags_of("---\ntags: rust ideas\n---\nbody"),
            vec!["rust", "ideas"]
        );
        assert_eq!(
            tags_of("---\ntags: notes/ideas\n---\nbody"),
            vec!["notes/ideas"]
        );
    }

    #[test]
    fn tags_dedup_empty_and_non_matches() {
        assert_eq!(tags_of("---\ntags: rust, rust, rust\n---\nb"), vec!["rust"]);
        assert!(tags_of("no frontmatter here").is_empty());
        assert!(tags_of("---\ntitle: X\n---\nb").is_empty());
        assert!(tags_of("---\ntagsfoo: rust\n---\nb").is_empty());
    }
}
