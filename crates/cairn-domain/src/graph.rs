//! The link graph derived from a set of notes: forward links and backlinks.
//!
//! Link targets are matched to notes by file stem (the note path without
//! its directory or `.md` extension), case-sensitively, mirroring the
//! common wikilink resolution rule.

use std::collections::{BTreeMap, BTreeSet};

use crate::{extract_links, Note, NotePath};

/// A derived graph of notes and the links between them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    pub fn build<'a>(notes: impl IntoIterator<Item = &'a Note>) -> Self {
        let notes: Vec<&Note> = notes.into_iter().collect();
        // Last note wins when two notes share a stem; callers should keep
        // note stems unique within a cairn.
        let by_stem: BTreeMap<&str, &NotePath> = notes
            .iter()
            .copied()
            .map(|n| (n.path.stem(), &n.path))
            .collect();

        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();

        for note in notes.iter().copied() {
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

    /// The note's undirected degree: forward-link count plus backlink count.
    /// A mutual link (a↔b) contributes to both endpoints. Unknown note ⇒ 0.
    #[must_use]
    pub fn degree(&self, path: &NotePath) -> u32 {
        let f = self.forward.get(path).map_or(0, Vec::len);
        let b = self.backward.get(path).map_or(0, Vec::len);
        u32::try_from(f + b).unwrap_or(u32::MAX)
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

/// Which slice of a graph to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphScope {
    /// The whole graph.
    Full,
    /// The undirected neighborhood of `path` out to `depth` hops (path = depth 0).
    Focused { path: NotePath, depth: u32 },
}

/// A set-diff between two graphs, by node path and `(from, to)` edge. All
/// vectors are sorted ascending.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphDelta {
    /// Nodes present in `other` but not `self`.
    pub nodes_added: Vec<NotePath>,
    /// Nodes present in `self` but not `other`.
    pub nodes_removed: Vec<NotePath>,
    /// Edges present in `other` but not `self`.
    pub edges_added: Vec<(NotePath, NotePath)>,
    /// Edges present in `self` but not `other`.
    pub edges_removed: Vec<(NotePath, NotePath)>,
}

impl Graph {
    /// Restrict to the undirected neighborhood of `path` within `depth` hops.
    /// `path` itself is depth 0. An edge is kept iff both endpoints are kept.
    /// Empty if `path` is absent from the graph.
    #[must_use]
    pub fn focused(&self, path: &NotePath, depth: u32) -> Graph {
        if !self.forward.contains_key(path) {
            return Graph::default();
        }
        let mut kept: BTreeSet<NotePath> = BTreeSet::new();
        kept.insert(path.clone());
        let mut frontier = vec![path.clone()];
        for _ in 0..depth {
            let mut next = Vec::new();
            for n in &frontier {
                for nb in self.forward_links(n).iter().chain(self.backlinks(n).iter()) {
                    if kept.insert(nb.clone()) {
                        next.push(nb.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        for n in &kept {
            forward.insert(
                n.clone(),
                self.forward_links(n)
                    .iter()
                    .filter(|t| kept.contains(*t))
                    .cloned()
                    .collect(),
            );
            backward.insert(
                n.clone(),
                self.backlinks(n)
                    .iter()
                    .filter(|s| kept.contains(*s))
                    .cloned()
                    .collect(),
            );
        }
        Graph { forward, backward }
    }

    /// Apply a [`GraphScope`]: `Full` clones, `Focused` delegates to [`Self::focused`].
    #[must_use]
    pub fn scoped(&self, scope: &GraphScope) -> Graph {
        match scope {
            GraphScope::Full => self.clone(),
            GraphScope::Focused { path, depth } => self.focused(path, *depth),
        }
    }

    /// Set-diff `self` (older) against `other` (newer).
    #[must_use]
    pub fn diff(&self, other: &Graph) -> GraphDelta {
        let a_nodes: BTreeSet<NotePath> = self.nodes().into_iter().cloned().collect();
        let b_nodes: BTreeSet<NotePath> = other.nodes().into_iter().cloned().collect();
        let a_edges: BTreeSet<(NotePath, NotePath)> = self
            .edges()
            .into_iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect();
        let b_edges: BTreeSet<(NotePath, NotePath)> = other
            .edges()
            .into_iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect();
        GraphDelta {
            nodes_added: b_nodes.difference(&a_nodes).cloned().collect(),
            nodes_removed: a_nodes.difference(&b_nodes).cloned().collect(),
            edges_added: b_edges.difference(&a_edges).cloned().collect(),
            edges_removed: a_edges.difference(&b_edges).cloned().collect(),
        }
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
        let notes = [note("a.md", "links to [[b]]"), note("dir/b.md", "no links")];
        let g = Graph::build(notes.iter());
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("dir/b.md").unwrap();
        assert_eq!(g.forward_links(&a), &[b.clone()]);
        assert_eq!(g.backlinks(&b), &[a]);
    }

    #[test]
    fn drops_unresolved_targets() {
        let notes = [note("a.md", "links to [[missing]]")];
        let g = Graph::build(notes.iter());
        assert!(g.forward_links(&NotePath::new("a.md").unwrap()).is_empty());
    }

    #[test]
    fn nodes_and_edges_expose_the_graph() {
        let notes = [note("a.md", "see [[b]]"), note("b.md", "no links")];
        let g = Graph::build(notes.iter());
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        assert_eq!(g.nodes(), vec![&a, &b]);
        assert_eq!(g.edges(), vec![(&a, &b)]);
    }

    #[test]
    fn focused_returns_undirected_neighborhood_within_depth() {
        // a -> b -> c -> d ; focus b depth 1 = {a,b,c} (undirected), edges among them.
        let notes = [
            note("a.md", "[[b]]"),
            note("b.md", "[[c]]"),
            note("c.md", "[[d]]"),
            note("d.md", "x"),
        ];
        let g = Graph::build(notes.iter());
        let b = NotePath::new("b.md").unwrap();
        let f = g.focused(&b, 1);
        let mut nodes: Vec<&str> = f.nodes().iter().map(|p| p.as_str()).collect();
        nodes.sort_unstable();
        assert_eq!(nodes, vec!["a.md", "b.md", "c.md"]);
        // Edge a->b and b->c are kept; c->d is dropped (d not in set).
        let edges: Vec<(String, String)> = f
            .edges()
            .iter()
            .map(|(x, y)| (x.as_str().to_string(), y.as_str().to_string()))
            .collect();
        assert!(edges.contains(&("a.md".into(), "b.md".into())));
        assert!(edges.contains(&("b.md".into(), "c.md".into())));
        assert!(!edges.iter().any(|(_, t)| t == "d.md"));
    }

    #[test]
    fn focused_on_absent_path_is_empty() {
        let g = Graph::build([note("a.md", "x")].iter());
        let missing = NotePath::new("zzz.md").unwrap();
        assert!(g.focused(&missing, 5).nodes().is_empty());
    }

    #[test]
    fn scoped_full_is_identity_focused_delegates() {
        let notes = [note("a.md", "[[b]]"), note("b.md", "x")];
        let g = Graph::build(notes.iter());
        assert_eq!(g.scoped(&GraphScope::Full), g);
        let b = NotePath::new("b.md").unwrap();
        assert_eq!(
            g.scoped(&GraphScope::Focused {
                path: b.clone(),
                depth: 0
            }),
            g.focused(&b, 0)
        );
    }

    #[test]
    fn degree_counts_forward_and_backlinks_undirected() {
        // hub -> a,b,c ⇒ hub degree 3 (forward); each leaf degree 1 (back).
        let notes = [
            note("hub.md", "[[a]] [[b]] [[c]]"),
            note("a.md", "x"),
            note("b.md", "x"),
            note("c.md", "x"),
        ];
        let g = Graph::build(notes.iter());
        assert_eq!(g.degree(&NotePath::new("hub.md").unwrap()), 3);
        assert_eq!(g.degree(&NotePath::new("a.md").unwrap()), 1);
        // Mutual link counts on both sides.
        let m = Graph::build([note("x.md", "[[y]]"), note("y.md", "[[x]]")].iter());
        assert_eq!(m.degree(&NotePath::new("x.md").unwrap()), 2);
        // Unknown note ⇒ 0.
        assert_eq!(g.degree(&NotePath::new("missing.md").unwrap()), 0);
    }

    #[test]
    fn diff_reports_added_and_removed_nodes_and_edges() {
        // from: a->b ; to: a (no link) + c->b
        let from = Graph::build([note("a.md", "[[b]]"), note("b.md", "x")].iter());
        let to = Graph::build(
            [
                note("a.md", "no link"),
                note("b.md", "x"),
                note("c.md", "[[b]]"),
            ]
            .iter(),
        );
        let d = from.diff(&to);
        assert_eq!(
            d.nodes_added.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            vec!["c.md"]
        );
        assert!(d.nodes_removed.is_empty());
        assert_eq!(
            d.edges_added
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
            vec![("c.md", "b.md")]
        );
        assert_eq!(
            d.edges_removed
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
            vec![("a.md", "b.md")]
        );
    }
}
