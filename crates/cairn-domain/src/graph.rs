//! The link graph derived from a set of notes: forward links and backlinks.
//!
//! Link targets are matched to notes by file stem (the note path without
//! its directory or `.md` extension), case-sensitively, mirroring the
//! common wikilink resolution rule.

use std::collections::{BTreeMap, BTreeSet};

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

    /// The note's undirected degree: forward-link count plus backlink count.
    /// A mutual link (a↔b) contributes to both endpoints. Unknown note ⇒ 0.
    #[must_use]
    pub fn degree(&self, path: &NotePath) -> u32 {
        let f = self.forward.get(path).map_or(0, Vec::len);
        let b = self.backward.get(path).map_or(0, Vec::len);
        u32::try_from(f + b).unwrap_or(u32::MAX)
    }

    /// Undirected BFS from `focus` out to `depth` hops. Returns the reached
    /// nodes (including `focus`, sorted) and the directed `(from, to)` link
    /// edges whose *both* endpoints are reached. `focus` absent from the graph
    /// ⇒ `(vec![], vec![])`; `depth == 0` ⇒ just `focus`.
    #[must_use]
    pub fn neighborhood(
        &self,
        focus: &NotePath,
        depth: u32,
    ) -> (Vec<NotePath>, Vec<(NotePath, NotePath)>) {
        // `build` inserts a `forward` entry for every note, so this is the
        // presence check for "is this a known note".
        if !self.forward.contains_key(focus) {
            return (Vec::new(), Vec::new());
        }
        let mut reached: BTreeSet<NotePath> = BTreeSet::new();
        reached.insert(focus.clone());
        let mut frontier = vec![focus.clone()];
        for _ in 0..depth {
            let mut next = Vec::new();
            for n in &frontier {
                let neighbors = self
                    .forward
                    .get(n)
                    .into_iter()
                    .flatten()
                    .chain(self.backward.get(n).into_iter().flatten());
                for m in neighbors {
                    if reached.insert(m.clone()) {
                        next.push(m.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let nodes: Vec<NotePath> = reached.iter().cloned().collect();
        let edges: Vec<(NotePath, NotePath)> = self
            .forward
            .iter()
            .flat_map(|(from, tos)| tos.iter().map(move |to| (from, to)))
            .filter(|(from, to)| reached.contains(*from) && reached.contains(*to))
            .map(|(from, to)| (from.clone(), to.clone()))
            .collect();
        (nodes, edges)
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
    fn degree_counts_forward_and_backlinks_undirected() {
        // star: hub links to a, b, c ⇒ hub degree 3 (forward), each leaf degree 1 (back).
        let notes = [
            note("hub.md", "[[a]] [[b]] [[c]]"),
            note("a.md", "no links"),
            note("b.md", "no links"),
            note("c.md", "no links"),
        ];
        let g = Graph::build(notes.iter());
        assert_eq!(g.degree(&NotePath::new("hub.md").unwrap()), 3);
        assert_eq!(g.degree(&NotePath::new("a.md").unwrap()), 1);
        // A mutual link counts on both sides.
        let mutual = [note("x.md", "[[y]]"), note("y.md", "[[x]]")];
        let gm = Graph::build(mutual.iter());
        assert_eq!(gm.degree(&NotePath::new("x.md").unwrap()), 2); // 1 forward + 1 back
                                                                   // Unknown note ⇒ 0.
        assert_eq!(g.degree(&NotePath::new("missing.md").unwrap()), 0);
    }

    #[test]
    fn neighborhood_bounds_star_and_chain_by_depth() {
        // Star centered on hub.
        let star = [
            note("hub.md", "[[a]] [[b]]"),
            note("a.md", "x"),
            note("b.md", "x"),
            note("far.md", "unrelated"),
        ];
        let g = Graph::build(star.iter());
        let hub = NotePath::new("hub.md").unwrap();

        // depth 0 ⇒ just the focus, no edges.
        let (n0, e0) = g.neighborhood(&hub, 0);
        assert_eq!(n0, vec![hub.clone()]);
        assert!(e0.is_empty());

        // depth 1 ⇒ hub + a + b (sorted), with both hub→a, hub→b edges.
        let (n1, e1) = g.neighborhood(&hub, 1);
        assert_eq!(
            n1,
            vec![
                NotePath::new("a.md").unwrap(),
                NotePath::new("b.md").unwrap(),
                hub.clone(),
            ]
        );
        assert_eq!(
            e1,
            vec![
                (hub.clone(), NotePath::new("a.md").unwrap()),
                (hub.clone(), NotePath::new("b.md").unwrap()),
            ]
        );
        assert!(!n1.contains(&NotePath::new("far.md").unwrap()));
    }

    #[test]
    fn neighborhood_walks_chain_undirected_across_link_direction() {
        // chain a -> b -> c: from b, depth 1 reaches a (via backlink) and c (forward).
        let chain = [
            note("a.md", "[[b]]"),
            note("b.md", "[[c]]"),
            note("c.md", "end"),
        ];
        let g = Graph::build(chain.iter());
        let b = NotePath::new("b.md").unwrap();

        let (n1, _e1) = g.neighborhood(&b, 1);
        assert_eq!(
            n1,
            vec![
                NotePath::new("a.md").unwrap(),
                b.clone(),
                NotePath::new("c.md").unwrap(),
            ]
        );

        // depth 2 from a reaches the whole chain; edges a->b and b->c both included.
        let a = NotePath::new("a.md").unwrap();
        let (n2, e2) = g.neighborhood(&a, 2);
        assert_eq!(n2.len(), 3);
        assert_eq!(
            e2,
            vec![
                (a.clone(), b.clone()),
                (b.clone(), NotePath::new("c.md").unwrap()),
            ]
        );
    }

    #[test]
    fn neighborhood_missing_focus_is_empty() {
        let g = Graph::build([note("a.md", "x")].iter());
        let (nodes, edges) = g.neighborhood(&NotePath::new("ghost.md").unwrap(), 2);
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }
}
