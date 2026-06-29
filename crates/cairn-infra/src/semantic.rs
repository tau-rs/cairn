//! Dependency-free lexical semantic index: IDF-weighted term vectors + cosine.
//! The first adapter behind the `SemanticIndex` port; a neural on-device adapter
//! can replace it later behind the same trait.

use std::collections::HashMap;

use cairn_domain::{Note, NotePath};
use cairn_ports::{PortError, SemanticIndex, Similarity};

/// Tokens shorter than this are dropped (noise / single letters).
const MIN_TOKEN_LEN: usize = 3;
/// How many overlapping terms to name in `Similarity::shared`.
const MAX_SHARED_TERMS: usize = 6;

/// A tiny English stopword set — common words carry no topical signal even
/// after IDF weighting on a small vault.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "her", "was", "one",
    "our", "out", "has", "had", "his", "she", "they", "this", "that", "with", "from", "have",
    "your", "what", "when", "were",
];

/// Lowercase, split on non-alphanumeric, drop stopwords and very short tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= MIN_TOKEN_LEN)
        .map(str::to_lowercase)
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// In-memory lexical index: per-note term frequencies + corpus document
/// frequencies. IDF stays exact under incremental upsert/remove.
#[derive(Debug, Default)]
pub struct LexicalSemanticIndex {
    tf: HashMap<NotePath, HashMap<String, u32>>,
    df: HashMap<String, u32>,
}

impl LexicalSemanticIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn doc_count(&self) -> usize {
        self.tf.len()
    }

    /// Inverse document frequency, smoothed so a term in every doc is ~0.
    fn idf(&self, term: &str) -> f32 {
        let n = self.doc_count() as f32;
        let df = *self.df.get(term).unwrap_or(&0) as f32;
        // ln((N + 1) / (df + 1)) + 1 — standard smoothed IDF, always > 0.
        ((n + 1.0) / (df + 1.0)).ln() + 1.0
    }

    /// IDF-weighted term vector for one note's term counts.
    fn vector(&self, tf: &HashMap<String, u32>) -> HashMap<String, f32> {
        tf.iter()
            .map(|(term, &count)| (term.clone(), count as f32 * self.idf(term)))
            .collect()
    }

    /// Remove a path's contribution to `df` (used by both remove and upsert-replace).
    fn retract_df(&mut self, terms: impl Iterator<Item = String>) {
        for term in terms {
            if let Some(c) = self.df.get_mut(&term) {
                *c -= 1;
                if *c == 0 {
                    self.df.remove(&term);
                }
            }
        }
    }
}

impl SemanticIndex for LexicalSemanticIndex {
    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        // Replace: retract the old term set from df first.
        if let Some(old) = self.tf.remove(&note.path) {
            self.retract_df(old.into_keys());
        }
        let mut counts: HashMap<String, u32> = HashMap::new();
        for tok in tokenize(&note.body) {
            *counts.entry(tok).or_insert(0) += 1;
        }
        for term in counts.keys() {
            *self.df.entry(term.clone()).or_insert(0) += 1;
        }
        self.tf.insert(note.path.clone(), counts);
        Ok(())
    }

    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        if let Some(old) = self.tf.remove(path) {
            self.retract_df(old.into_keys());
        }
        Ok(())
    }

    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.tf.clear();
        self.df.clear();
        for note in notes {
            self.upsert(note)?;
        }
        Ok(())
    }

    fn neighbors(&self, focus: &NotePath, top_k: usize) -> Result<Vec<Similarity>, PortError> {
        let Some(focus_tf) = self.tf.get(focus) else {
            return Ok(Vec::new());
        };
        let fv = self.vector(focus_tf);
        let fnorm = fv.values().map(|w| w * w).sum::<f32>().sqrt();
        if fnorm == 0.0 {
            return Ok(Vec::new());
        }

        let mut scored: Vec<Similarity> = Vec::new();
        for (path, tf) in &self.tf {
            if path == focus {
                continue;
            }
            let ov = self.vector(tf);
            let onorm = ov.values().map(|w| w * w).sum::<f32>().sqrt();
            if onorm == 0.0 {
                continue;
            }
            // Dot product over the smaller map.
            let (small, large) = if fv.len() <= ov.len() {
                (&fv, &ov)
            } else {
                (&ov, &fv)
            };
            let mut dot = 0.0_f32;
            let mut overlap: Vec<(String, f32)> = Vec::new();
            for (term, w) in small {
                if let Some(w2) = large.get(term) {
                    dot += w * w2;
                    overlap.push((term.clone(), w * w2));
                }
            }
            let score = dot / (fnorm * onorm);
            if score <= 0.0 {
                continue;
            }
            overlap.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let shared = overlap
                .into_iter()
                .take(MAX_SHARED_TERMS)
                .map(|(t, _)| t)
                .collect();
            scored.push(Similarity {
                path: path.clone(),
                score,
                shared,
            });
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path)) // stable tie-break
        });
        scored.truncate(top_k);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, body: &str) -> Note {
        Note::parse(NotePath::new(path).unwrap(), body)
    }

    #[test]
    fn ranks_same_topic_above_unrelated() {
        let notes = [
            note("rust.md", "rust ownership borrow lifetime move semantics"),
            note(
                "borrow.md",
                "borrow checker ownership lifetime rust references",
            ),
            note("cooking.md", "tomato basil pasta garlic olive oil"),
        ];
        let mut idx = LexicalSemanticIndex::default();
        idx.reindex(&notes).unwrap();

        let n = idx
            .neighbors(&NotePath::new("rust.md").unwrap(), 5)
            .unwrap();
        assert_eq!(
            n.first().unwrap().path.as_str(),
            "borrow.md",
            "topical match ranks first"
        );
        // cooking is unrelated → either absent or strictly lower.
        let borrow_score = n
            .iter()
            .find(|s| s.path.as_str() == "borrow.md")
            .unwrap()
            .score;
        let cooking_score = n
            .iter()
            .find(|s| s.path.as_str() == "cooking.md")
            .map(|s| s.score)
            .unwrap_or(0.0);
        assert!(borrow_score > cooking_score);
    }

    #[test]
    fn shared_names_real_overlapping_terms() {
        let notes = [
            note("a.md", "ownership ownership borrow lifetime"),
            note("b.md", "ownership borrow lifetime references"),
        ];
        let mut idx = LexicalSemanticIndex::default();
        idx.reindex(&notes).unwrap();
        let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        let shared = &n.iter().find(|s| s.path.as_str() == "b.md").unwrap().shared;
        assert!(shared.iter().any(|t| t == "ownership"));
        assert!(shared.iter().any(|t| t == "borrow"));
    }

    #[test]
    fn never_suggests_self_and_empty_corpus_is_empty() {
        let mut idx = LexicalSemanticIndex::default();
        assert!(idx
            .neighbors(&NotePath::new("a.md").unwrap(), 5)
            .unwrap()
            .is_empty());
        idx.reindex(&[note("a.md", "solo note")]).unwrap();
        let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        assert!(n.iter().all(|s| s.path.as_str() != "a.md"));
    }

    #[test]
    fn remove_stops_affecting_scores() {
        let notes = [
            note("a.md", "rust ownership borrow"),
            note("b.md", "rust ownership borrow"),
        ];
        let mut idx = LexicalSemanticIndex::default();
        idx.reindex(&notes).unwrap();
        assert!(!idx
            .neighbors(&NotePath::new("a.md").unwrap(), 5)
            .unwrap()
            .is_empty());
        idx.remove(&NotePath::new("b.md").unwrap()).unwrap();
        assert!(
            idx.neighbors(&NotePath::new("a.md").unwrap(), 5)
                .unwrap()
                .is_empty(),
            "after removing the only neighbor, none remain"
        );
    }

    #[test]
    fn upsert_matches_reindex() {
        let notes = [
            note("a.md", "rust ownership borrow"),
            note("b.md", "rust ownership borrow"),
        ];
        let mut a = LexicalSemanticIndex::default();
        a.reindex(&notes).unwrap();
        let mut b = LexicalSemanticIndex::default();
        b.upsert(&notes[0]).unwrap();
        b.upsert(&notes[1]).unwrap();
        let na = a.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        let nb = b.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        assert_eq!(na.len(), nb.len());
        assert_eq!(na[0].path, nb[0].path);
    }
}
