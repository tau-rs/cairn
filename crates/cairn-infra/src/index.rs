//! A simple in-memory `SearchIndex` (substring match). Tantivy replaces
//! this later behind the same port.

use cairn_domain::Note;
use cairn_ports::{PortError, SearchHit, SearchIndex};

/// Keeps note bodies in memory and matches queries by case-insensitive
/// substring.
#[derive(Debug, Default)]
pub struct InMemoryIndex {
    docs: Vec<Note>,
}

/// Char-safe truncation of a body to at most `max` bytes, for the in-memory
/// index's snippet (a faithful-enough test double; precise highlighting is
/// Tantivy's job).
fn truncate_snippet(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_string();
    }
    let mut end = max;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body[..end].to_string()
}

impl SearchIndex for InMemoryIndex {
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.docs = notes.to_vec();
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        let q = query.to_lowercase();
        let mut hits: Vec<SearchHit> = self
            .docs
            .iter()
            .filter(|n| {
                n.body.to_lowercase().contains(&q) || n.path.as_str().to_lowercase().contains(&q)
            })
            .map(|n| SearchHit {
                path: n.path.clone(),
                score: 1.0,
                snippet: truncate_snippet(&n.body, 160),
                highlights: Vec::new(),
            })
            .collect();
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(hits)
    }

    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        if let Some(slot) = self.docs.iter_mut().find(|d| d.path == note.path) {
            *slot = note.clone();
        } else {
            self.docs.push(note.clone());
        }
        Ok(())
    }

    fn remove(&mut self, path: &cairn_domain::NotePath) -> Result<(), PortError> {
        self.docs.retain(|d| &d.path != path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::NotePath;

    fn note(path: &str, body: &str) -> Note {
        Note {
            path: NotePath::new(path).unwrap(),
            frontmatter: None,
            body: body.into(),
        }
    }

    #[test]
    fn matches_by_path_and_sorts_results() {
        let mut idx = InMemoryIndex::default();
        idx.reindex(&[note("zeta.md", "alpha note"), note("alpha.md", "zeta body")])
            .unwrap();
        // "alpha" matches zeta.md by body and alpha.md by path; sorted by path.
        let hits = idx.search("alpha").unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha.md", "zeta.md"]);
    }

    #[test]
    fn finds_by_body_substring_case_insensitive() {
        let mut idx = InMemoryIndex::default();
        idx.reindex(&[note("a.md", "Hello World"), note("b.md", "other")])
            .unwrap();
        let hits = idx.search("hello").unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md"]);
        let hit = &hits[0];
        assert_eq!(hit.score, 1.0);
        assert!(hit.snippet.contains("Hello World"));
    }

    #[test]
    fn upsert_then_remove() {
        let mut idx = InMemoryIndex::default();
        idx.upsert(&note("a.md", "hello target")).unwrap();
        assert_eq!(
            idx.search("target")
                .unwrap()
                .iter()
                .map(|h| h.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.md"]
        );
        idx.upsert(&note("a.md", "changed")).unwrap(); // replace, not duplicate
        assert!(idx.search("target").unwrap().is_empty());
        assert_eq!(idx.search("changed").unwrap().len(), 1);
        idx.remove(&NotePath::new("a.md").unwrap()).unwrap();
        assert!(idx.search("changed").unwrap().is_empty());
    }
}
