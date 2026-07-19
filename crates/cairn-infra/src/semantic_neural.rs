//! Neural on-device semantic index. The generic core (`NeuralSemanticIndex`,
//! `Embedder` seam) is pure Rust and always compiled; the real `CandleMiniLm`
//! embedder is gated behind the `neural` feature. Ranking is cosine over
//! mean-pooled unit token vectors; `why` provenance uses cross-token
//! nearest-neighbor attribution (see `attribute`).

use std::cmp::Ordering;
use std::collections::HashMap;

use cairn_domain::{Note, NotePath};
use cairn_ports::{PortError, SemanticIndex, Similarity};

/// How many attribution terms to name in `Similarity::shared`.
const MAX_SHARED_TERMS: usize = 6;
/// A focus token must reach this cosine to *some* neighbor token to be named.
const ATTRIBUTION_THRESHOLD: f32 = 0.3;

/// Text → per-token unit embeddings. Pooling and dedup happen in the index,
/// so an embedder only maps text to `(token, unit_vector)` pairs in order.
/// The seam that lets the index logic be tested without model weights.
trait Embedder {
    /// Per-token unit-normalized embeddings, in token order (duplicates ok).
    fn embed_tokens(&self, text: &str) -> Result<Vec<(String, Vec<f32>)>, PortError>;
    /// Embedding dimensionality.
    fn dim(&self) -> usize;
}

/// One note's stored embedding: the pooled unit vector drives ranking; the
/// per-unique-token vectors drive `why` attribution (the C-full tradeoff —
/// memory scales with a note's unique-token count).
struct NoteEmbedding {
    pooled: Vec<f32>,
    tokens: Vec<(String, Vec<f32>)>,
}

/// Embedding index behind the `SemanticIndex` port. Generic over its
/// [`Embedder`] so tests inject a deterministic fake. The `E: Embedder` bound
/// lives on the methods that need it (`build`, the `SemanticIndex` impl), not
/// the struct — keeping the private seam out of this `pub` type's signature.
pub struct NeuralSemanticIndex<E> {
    embedder: E,
    notes: HashMap<NotePath, NoteEmbedding>,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn unit_normalize(v: &mut [f32]) {
    let norm = dot(v, v).sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// The focus tokens whose meaning is most echoed by *some* neighbor token.
/// For each focus token, take its max cosine to any neighbor token; keep those
/// above [`ATTRIBUTION_THRESHOLD`], top [`MAX_SHARED_TERMS`], ties broken by term.
/// This is opaque neural provenance made legible (the C-full `why`).
fn attribute(focus: &[(String, Vec<f32>)], neighbor: &[(String, Vec<f32>)]) -> Vec<String> {
    let mut scored: Vec<(String, f32)> = focus
        .iter()
        .filter_map(|(tok, fv)| {
            let best = neighbor
                .iter()
                .map(|(_, nv)| dot(fv, nv))
                .fold(f32::MIN, f32::max);
            (best >= ATTRIBUTION_THRESHOLD).then(|| (tok.clone(), best))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored
        .into_iter()
        .take(MAX_SHARED_TERMS)
        .map(|(t, _)| t)
        .collect()
}

impl<E> NeuralSemanticIndex<E> {
    /// Build the stored embedding for one note: dedup tokens (first wins),
    /// mean-pool the unique token vectors, unit-normalize the pooled vector.
    fn build(&self, text: &str) -> Result<NoteEmbedding, PortError>
    where
        E: Embedder,
    {
        let raw = self.embedder.embed_tokens(text)?;
        let mut seen = std::collections::HashSet::new();
        let mut tokens: Vec<(String, Vec<f32>)> = Vec::new();
        for (tok, vec) in raw {
            if seen.insert(tok.clone()) {
                tokens.push((tok, vec));
            }
        }
        let dim = self.embedder.dim();
        let mut pooled = vec![0.0f32; dim];
        for (_, vec) in &tokens {
            for (p, x) in pooled.iter_mut().zip(vec) {
                *p += x;
            }
        }
        if !tokens.is_empty() {
            let n = tokens.len() as f32;
            for p in pooled.iter_mut() {
                *p /= n;
            }
        }
        unit_normalize(&mut pooled);
        Ok(NoteEmbedding { pooled, tokens })
    }
}

impl<E: Embedder + Send> SemanticIndex for NeuralSemanticIndex<E> {
    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        let emb = self.build(&note.body)?;
        self.notes.insert(note.path.clone(), emb);
        Ok(())
    }

    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        self.notes.remove(path);
        Ok(())
    }

    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.notes.clear();
        for note in notes {
            self.upsert(note)?;
        }
        Ok(())
    }

    fn neighbors(&self, focus: &NotePath, top_k: usize) -> Result<Vec<Similarity>, PortError> {
        let Some(f) = self.notes.get(focus) else {
            return Ok(Vec::new());
        };
        if f.pooled.iter().all(|x| *x == 0.0) {
            return Ok(Vec::new());
        }
        let mut scored: Vec<Similarity> = Vec::new();
        for (path, e) in &self.notes {
            if path == focus {
                continue;
            }
            let score = dot(&f.pooled, &e.pooled).clamp(0.0, 1.0);
            if score <= 0.0 {
                continue;
            }
            scored.push(Similarity {
                path: path.clone(),
                score,
                shared: attribute(&f.tokens, &e.tokens),
            });
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        scored.truncate(top_k);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic embedder over a tiny concept space. `cat`≈`feline` and
    /// `mat`≈`rug` (paraphrase bridges with no literal overlap); the
    /// revenue/projection axis is near-orthogonal to the cat/mat axis.
    /// Unknown tokens embed to the zero vector (contribute nothing).
    struct FakeEmbedder;

    impl FakeEmbedder {
        fn vec_for(tok: &str) -> Vec<f32> {
            let mut v = match tok {
                "cat" => vec![1.0, 0.0, 0.0, 0.0],
                "feline" => vec![0.98, 0.20, 0.0, 0.0],
                "mat" => vec![0.0, 1.0, 0.0, 0.0],
                "rug" => vec![0.0, 0.98, 0.20, 0.0],
                "revenue" => vec![0.0, 0.0, 1.0, 0.0],
                "projections" => vec![0.0, 0.0, 0.98, 0.20],
                _ => vec![0.0, 0.0, 0.0, 0.0],
            };
            unit_normalize(&mut v);
            v
        }
    }

    impl Embedder for FakeEmbedder {
        fn embed_tokens(&self, text: &str) -> Result<Vec<(String, Vec<f32>)>, PortError> {
            Ok(text
                .split_whitespace()
                .map(|t| (t.to_string(), Self::vec_for(t)))
                .collect())
        }
        fn dim(&self) -> usize {
            4
        }
    }

    fn ix() -> NeuralSemanticIndex<FakeEmbedder> {
        NeuralSemanticIndex {
            embedder: FakeEmbedder,
            notes: HashMap::new(),
        }
    }

    fn note(path: &str, body: &str) -> Note {
        Note::parse(NotePath::new(path).unwrap(), body)
    }

    #[test]
    fn related_notes_rank_first() {
        let mut idx = ix();
        idx.reindex(&[
            note("a.md", "cat on the mat"),
            note("b.md", "feline on the rug"),   // paraphrase of a
            note("c.md", "revenue projections"), // unrelated
        ])
        .unwrap();
        let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        assert_eq!(n.first().unwrap().path.as_str(), "b.md");
        let b = n.iter().find(|s| s.path.as_str() == "b.md").unwrap().score;
        let c = n
            .iter()
            .find(|s| s.path.as_str() == "c.md")
            .map(|s| s.score)
            .unwrap_or(0.0);
        assert!(b > c, "paraphrase {b} must outrank unrelated {c}");
    }

    #[test]
    fn unknown_focus_is_empty() {
        let idx = ix();
        assert!(idx
            .neighbors(&NotePath::new("nope.md").unwrap(), 5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn never_suggests_self() {
        let mut idx = ix();
        idx.upsert(&note("a.md", "cat mat")).unwrap();
        let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        assert!(n.iter().all(|s| s.path.as_str() != "a.md"));
    }

    #[test]
    fn remove_stops_affecting_scores() {
        let mut idx = ix();
        idx.reindex(&[note("a.md", "cat mat"), note("b.md", "cat mat")])
            .unwrap();
        assert!(!idx
            .neighbors(&NotePath::new("a.md").unwrap(), 5)
            .unwrap()
            .is_empty());
        idx.remove(&NotePath::new("b.md").unwrap()).unwrap();
        assert!(idx
            .neighbors(&NotePath::new("a.md").unwrap(), 5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn shared_surfaces_paraphrase_bridge_tokens() {
        let mut idx = ix();
        idx.reindex(&[
            note("a.md", "cat on the mat"),
            note("b.md", "feline on the rug"),
        ])
        .unwrap();
        let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        let shared = &n.iter().find(|s| s.path.as_str() == "b.md").unwrap().shared;
        // cat≈feline and mat≈rug drive the match, even with no literal overlap.
        assert!(shared.iter().any(|t| t == "cat"), "got {shared:?}");
        assert!(shared.iter().any(|t| t == "mat"), "got {shared:?}");
        // stopword-ish tokens embed to zero → below threshold → not named.
        assert!(
            !shared.iter().any(|t| t == "on" || t == "the"),
            "got {shared:?}"
        );
    }

    #[test]
    fn upsert_matches_reindex_and_is_deterministic() {
        let notes = [note("a.md", "cat on the mat"), note("b.md", "feline rug")];
        let mut a = ix();
        a.reindex(&notes).unwrap();
        let mut b = ix();
        b.upsert(&notes[0]).unwrap();
        b.upsert(&notes[1]).unwrap();
        let na = a.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        let nb = b.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
        assert_eq!(na.len(), nb.len());
        assert_eq!(na[0].path, nb[0].path);
        assert_eq!(na[0].score, nb[0].score); // deterministic
    }
}
