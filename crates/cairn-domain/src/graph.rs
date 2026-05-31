//! The link graph derived from a set of notes: forward links and backlinks.
//!
//! Link targets are matched to notes by file stem (the note path without
//! its directory or `.md` extension), case-sensitively, mirroring the
//! common wikilink resolution rule.

use std::collections::BTreeMap;

use crate::{extract_links, Note, NotePath};

/// A derived graph of notes and the links between them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// note -> the notes it links to (resolved, deduplicated, sorted)
    forward: BTreeMap<NotePath, Vec<NotePath>>,
    /// note -> the notes that link to it (resolved, deduplicated, sorted)
    backward: BTreeMap<NotePath, Vec<NotePath>>,
}

fn stem(path: &NotePath) -> &str {
    let s = path.as_str();
    let after_slash = s.rsplit('/').next().unwrap_or(s);
    after_slash.strip_suffix(".md").unwrap_or(after_slash)
}

impl Graph {
    /// Build a graph from all notes. Targets are resolved to a note whose
    /// stem equals the target text; unresolved targets are dropped.
    pub fn build(notes: &[Note]) -> Self {
        let by_stem: BTreeMap<&str, &NotePath> =
            notes.iter().map(|n| (stem(&n.path), &n.path)).collect();

        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();

        for note in notes {
            let mut targets: Vec<NotePath> = extract_links(&note.body)
                .into_iter()
                .filter_map(|t| by_stem.get(t.0.as_str()).map(|p| (*p).clone()))
                .collect();
            targets.sort();
            targets.dedup();
            for t in &targets {
                backward.entry(t.clone()).or_default().push(note.path.clone());
            }
            forward.insert(note.path.clone(), targets);
        }
        for v in backward.values_mut() {
            v.sort();
            v.dedup();
        }
        Self { forward, backward }
    }

    /// Notes that `path` links to.
    pub fn forward_links(&self, path: &NotePath) -> &[NotePath] {
        self.forward.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Notes that link to `path`.
    pub fn backlinks(&self, path: &NotePath) -> &[NotePath] {
        self.backward.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, body: &str) -> Note {
        Note { path: NotePath::new(path).unwrap(), frontmatter: None, body: body.into() }
    }

    #[test]
    fn resolves_forward_and_backlinks_by_stem() {
        let notes = vec![
            note("a.md", "links to [[b]]"),
            note("dir/b.md", "no links"),
        ];
        let g = Graph::build(&notes);
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("dir/b.md").unwrap();
        assert_eq!(g.forward_links(&a), &[b.clone()]);
        assert_eq!(g.backlinks(&b), &[a]);
    }

    #[test]
    fn drops_unresolved_targets() {
        let notes = vec![note("a.md", "links to [[missing]]")];
        let g = Graph::build(&notes);
        assert!(g.forward_links(&NotePath::new("a.md").unwrap()).is_empty());
    }
}
