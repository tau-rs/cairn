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

impl Graph {
    /// Build a graph from all notes. Targets are resolved to a note whose
    /// stem equals the target text; unresolved targets are dropped.
    #[must_use]
    pub fn build(notes: &[Note]) -> Self {
        // Last note wins when two notes share a stem; callers should keep
        // note stems unique within a cairn.
        let by_stem: BTreeMap<&str, &NotePath> =
            notes.iter().map(|n| (n.path.stem(), &n.path)).collect();

        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();

        for note in notes {
            let mut targets: Vec<NotePath> = extract_links(&note.body)
                .into_iter()
                .filter_map(|t| by_stem.get(t.0.as_str()).copied().cloned())
                .collect();
            targets.sort();
            targets.dedup();
            for t in &targets {
                backward
                    .entry(t.clone())
                    .or_default()
                    .push(note.path.clone());
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
    #[must_use]
    pub fn forward_links(&self, path: &NotePath) -> &[NotePath] {
        self.forward.get(path).map_or(&[], Vec::as_slice)
    }

    /// Notes that link to `path`.
    #[must_use]
    pub fn backlinks(&self, path: &NotePath) -> &[NotePath] {
        self.backward.get(path).map_or(&[], Vec::as_slice)
    }

    /// All note paths in the graph, sorted.
    #[must_use]
    pub fn nodes(&self) -> Vec<&NotePath> {
        self.forward.keys().collect()
    }

    /// All directed `(from, to)` link edges.
    #[must_use]
    pub fn edges(&self) -> Vec<(&NotePath, &NotePath)> {
        self.forward
            .iter()
            .flat_map(|(from, tos)| tos.iter().map(move |to| (from, to)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, body: &str) -> Note {
        Note {
            path: NotePath::new(path).unwrap(),
            frontmatter: None,
            body: body.into(),
        }
    }

    #[test]
    fn resolves_forward_and_backlinks_by_stem() {
        let notes = vec![note("a.md", "links to [[b]]"), note("dir/b.md", "no links")];
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

    #[test]
    fn nodes_and_edges_expose_the_graph() {
        let notes = vec![note("a.md", "see [[b]]"), note("b.md", "no links")];
        let g = Graph::build(&notes);
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        assert_eq!(g.nodes(), vec![&a, &b]);
        assert_eq!(g.edges(), vec![(&a, &b)]);
    }
}
