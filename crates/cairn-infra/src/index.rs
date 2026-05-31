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
            })
            .collect();
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(hits)
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
        assert_eq!(
            hits,
            vec![
                SearchHit {
                    path: NotePath::new("alpha.md").unwrap()
                },
                SearchHit {
                    path: NotePath::new("zeta.md").unwrap()
                },
            ]
        );
    }

    #[test]
    fn finds_by_body_substring_case_insensitive() {
        let mut idx = InMemoryIndex::default();
        idx.reindex(&[note("a.md", "Hello World"), note("b.md", "other")])
            .unwrap();
        let hits = idx.search("hello").unwrap();
        assert_eq!(
            hits,
            vec![SearchHit {
                path: NotePath::new("a.md").unwrap()
            }]
        );
    }
}
