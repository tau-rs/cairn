# AI-suggested semantic edges — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface semantically related but not-explicitly-linked notes behind a `GetSuggestions { scope }` query, returning explainable `SuggestedEdge { from, to, weight, why }`.

**Architecture:** Hexagonal. A new `SemanticIndex` **port** (`cairn-ports`) with a dependency-free lexical TF-IDF+cosine adapter (`cairn-infra`) and a neutral `NullSemanticIndex` seam. The `Engine` (`cairn-app`) gains a lazily-built, in-memory semantic index kept live through the existing `apply_write`/`apply_change`/`apply_removal` hooks, and a `suggestions(scope)` use-case that excludes self + already-linked pairs + sub-floor noise. The wire contract (`cairn-contract`) gains the query/response/scope/edge DTOs; `dispatch_query` (`cairn-service`) maps between them. Real adapter injected at the composition root (`cairn-startup`, daemon).

**Tech Stack:** Rust (edition 2021, MSRV 1.88), `serde`, `ts-rs` (TypeScript bindings), `thiserror`, `cargo nextest`. No new dependencies.

## Global Constraints

- MSRV Rust 1.88; edition 2021. Every crate: `#![forbid(unsafe_code)]` (workspace lint `unsafe_code = "forbid"` already enforces this — do not add `unsafe`).
- **No new dependencies.** The lexical adapter is pure `std`. (Neural/candle is an explicit follow-up, not this plan.)
- `thiserror` at boundaries; `anyhow` internally (no `anyhow` needed here — port returns `PortError`).
- Contract DTOs are independent of `cairn-domain`; `cairn-app` never imports `cairn-contract` (mapping lives in `cairn-service`).
- Test runner: `cargo nextest run -p <crate>` (mirrors CI). Doc/format/clippy gates run via lefthook on commit — keep `cargo fmt` clean and zero clippy warnings.
- Constants (not wire-exposed, not config): `SUGGEST_TOP_K = 5`, `SUGGEST_FLOOR = 0.1_f32`, `VAULT_EDGE_CAP = 100`. Tests assert **invariants, not these magic numbers**.
- `weight` is cosine similarity in `0..1`, **ranking only** — never metric distance.

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-contract/src/lib.rs` | `Query::GetSuggestions`, `QueryResponse::Suggestions`, `GraphScope`, `SuggestedEdge` + serde tests | 1 |
| `crates/cairn-contract/bindings/*.ts` | Generated TS bindings (committed) | 1 |
| `crates/cairn-service/src/lib.rs` | `dispatch_query` arm (stub → real), `map_scope`, `map_suggested_edge` | 1, 6 |
| `crates/cairn-daemon/src/lib.rs` | `query_kind` telemetry arm | 1 |
| `crates/cairn-ports/src/lib.rs` | `SemanticIndex` trait + `Similarity` struct + `InertSemanticIndex` default seam | 2 |
| `crates/cairn-infra/src/semantic.rs` (new) | `LexicalSemanticIndex` (TF-IDF + cosine) | 3 |
| `crates/cairn-infra/src/lib.rs` | re-export `LexicalSemanticIndex` | 3 |
| `crates/cairn-app/src/lib.rs` | `semantic` field, `set_semantic_index`, incremental hooks, `Scope`, `SuggestedEdgeData`, `suggestions()` | 4, 5 |
| `crates/cairn-startup/src/lib.rs` | inject `LexicalSemanticIndex` in `build_engine` | 7 |
| `crates/cairn-daemon/src/main.rs` | inject `LexicalSemanticIndex` in persistent engine path | 7 |

---

### Task 1: Contract types + seam stub (the frozen wire surface)

Adds the wire DTOs and keeps the whole workspace compiling by stubbing the two exhaustive `Query` matches. After this task `GetSuggestions` is reachable end-to-end and returns an empty list.

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- Modify: `crates/cairn-service/src/lib.rs` (add `dispatch_query` arm)
- Modify: `crates/cairn-daemon/src/lib.rs:323` (`query_kind`)
- Generated: `crates/cairn-contract/bindings/GraphScope.ts`, `SuggestedEdge.ts`, updated `Query.ts`, `QueryResponse.ts`

**Interfaces:**
- Produces: `Query::GetSuggestions { scope: GraphScope }`; `GraphScope` enum (`Note { path: String }` | `Vault`); `SuggestedEdge { from: String, to: String, weight: f32, why: Option<String> }`; `QueryResponse::Suggestions { suggestions: Vec<SuggestedEdge> }`.

- [ ] **Step 1: Add the DTOs to `cairn-contract`**

In `crates/cairn-contract/src/lib.rs`, add the `GetSuggestions` variant to `Query` (after `NotesByTag`):

```rust
    /// Suggested (non-explicit) semantic edges within a scope.
    GetSuggestions {
        /// What to compute suggestions over.
        scope: GraphScope,
    },
```

Add a new scope enum near `GraphEdge`:

```rust
/// What a `GetSuggestions` query computes suggestions over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphScope {
    /// Suggestions for one focus note (the related-notes panel).
    Note {
        /// Relative note path.
        path: String,
    },
    /// Top suggested edges across the whole vault (the graph overlay).
    Vault,
}

/// A suggested, non-explicit edge between two notes by path. `weight` is a
/// similarity *ranking* in `0..1`, not a plottable distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestedEdge {
    /// Source note path.
    pub from: String,
    /// Target note path.
    pub to: String,
    /// Cosine similarity, `0..1`. Relative ordering only.
    pub weight: f32,
    /// Human-readable provenance, e.g. `"shared: ownership, borrow"`. `None` if unknown.
    pub why: Option<String>,
}
```

Add the response variant to `QueryResponse` (after `History`):

```rust
    /// Suggested semantic edges (response to `GetSuggestions`).
    Suggestions {
        /// Best match first.
        suggestions: Vec<SuggestedEdge>,
    },
```

- [ ] **Step 2: Write failing serde round-trip tests**

Add to the `tests` module in `crates/cairn-contract/src/lib.rs`:

```rust
    #[test]
    fn get_suggestions_query_roundtrips() {
        let q = Query::GetSuggestions {
            scope: GraphScope::Note { path: "a.md".into() },
        };
        let j = serde_json::to_string(&q).unwrap();
        assert!(j.contains("\"type\":\"get_suggestions\""));
        assert!(j.contains("\"type\":\"note\""));
        assert_eq!(serde_json::from_str::<Query>(&j).unwrap(), q);

        let vault = Query::GetSuggestions { scope: GraphScope::Vault };
        let jv = serde_json::to_string(&vault).unwrap();
        assert!(jv.contains("\"scope\":{\"type\":\"vault\"}"));
        assert_eq!(serde_json::from_str::<Query>(&jv).unwrap(), vault);
    }

    #[test]
    fn suggestions_response_roundtrips() {
        let r = QueryResponse::Suggestions {
            suggestions: vec![SuggestedEdge {
                from: "a.md".into(),
                to: "b.md".into(),
                weight: 0.42,
                why: Some("shared: rust, ownership".into()),
            }],
        };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"suggestions\""));
        assert!(j.contains("\"weight\":0.42"));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), r);
    }
```

- [ ] **Step 3: Run tests — expect compile failure across the workspace**

Run: `cargo nextest run -p cairn-contract -p cairn-service -p cairn-daemon`
Expected: FAIL — `cairn-service` and `cairn-daemon` fail to compile with `non-exhaustive patterns: ... GetSuggestions not covered` (two exhaustive matches on `Query`).

- [ ] **Step 4: Add the stub `dispatch_query` arm in `cairn-service`**

In `crates/cairn-service/src/lib.rs`, add to the `match query` in `dispatch_query` (after the `ListPlugins` arm). The import line at the top must also gain `SuggestedEdge` — leave it unused for now is not allowed, so reference it via the full path here:

```rust
        Query::GetSuggestions { scope: _ } => {
            // SEAM: real adapter wired in Task 6.
            Ok(QueryResponse::Suggestions { suggestions: Vec::new() })
        }
```

- [ ] **Step 5: Add the `query_kind` telemetry arm in `cairn-daemon`**

In `crates/cairn-daemon/src/lib.rs`, find `fn query_kind` (~line 323) and add an arm:

```rust
        Query::GetSuggestions { .. } => "get_suggestions",
```

- [ ] **Step 6: Run tests — expect pass + regenerate bindings**

Run: `cargo nextest run -p cairn-contract -p cairn-service -p cairn-daemon`
Expected: PASS.

ts-rs writes bindings during the test run. Confirm new/updated files:

Run: `git status --short crates/cairn-contract/bindings/`
Expected: new `GraphScope.ts`, `SuggestedEdge.ts`; modified `Query.ts`, `QueryResponse.ts`.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/bindings/ \
        crates/cairn-service/src/lib.rs crates/cairn-daemon/src/lib.rs
git commit -m "feat(contract): GetSuggestions seam — GraphScope + SuggestedEdge DTOs"
```

---

### Task 2: `SemanticIndex` port + `InertSemanticIndex` default

The port, its `Similarity` value type, and the engine's inert default all live in `cairn-ports`, beside `NoopPluginHost` — so the inner hexagon never imports an adapter for its default. The real adapter (`LexicalSemanticIndex`, Task 3) is the only `cairn-infra` type.

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Test: `crates/cairn-ports/src/lib.rs` `tests` module

**Interfaces:**
- Produces: `trait SemanticIndex { upsert(&mut self, &Note) -> Result<(), PortError>; remove(&mut self, &NotePath) -> Result<(), PortError>; reindex(&mut self, &[Note]) -> Result<(), PortError>; neighbors(&self, &NotePath, usize) -> Result<Vec<Similarity>, PortError>; }`; `struct Similarity { path: NotePath, score: f32, shared: Vec<String> }`; `struct InertSemanticIndex` (the no-suggestions default).
- Consumes: `cairn_domain::{Note, NotePath}`, `cairn_ports::PortError` (Task 2 defines the trait alongside them).

- [ ] **Step 1: Define the port in `cairn-ports`**

In `crates/cairn-ports/src/lib.rs`, after the `SearchIndex` trait + `SearchHit` block, add:

```rust
/// One semantically-similar note, with the terms behind the similarity.
#[derive(Debug, Clone, PartialEq)]
pub struct Similarity {
    /// The similar note.
    pub path: NotePath,
    /// Cosine similarity in `0..1`. Relative ordering only.
    pub score: f32,
    /// Top overlapping high-weight terms — the provenance for `why`.
    pub shared: Vec<String>,
}

/// Embedding/vector index over note content. Seam: [`NullSemanticIndex`].
/// Mirrors [`SearchIndex`]'s lifecycle so the engine can drive it from the
/// same write/change/remove call sites.
pub trait SemanticIndex {
    /// Insert or replace a single note's vector.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
    /// Remove a single note's vector.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
    /// Rebuild the whole index from the given notes.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError>;
    /// The `top_k` notes most similar to `focus`, nearest first. Never includes
    /// `focus` itself. An unknown `focus` yields `Ok(vec![])`.
    ///
    /// # Errors
    /// Returns [`PortError`] if the adapter fails.
    fn neighbors(&self, focus: &NotePath, top_k: usize) -> Result<Vec<Similarity>, PortError>;
}
```

`Note` is already imported at the top of the file (`use cairn_domain::{... Note ...}`). If `Note` is not in that `use`, add it.

- [ ] **Step 2: Add the inert default test**

In `crates/cairn-ports/src/lib.rs`, add a test module (or extend the existing top-level `tests` module):

```rust
    #[test]
    fn inert_semantic_index_is_inert() {
        use cairn_domain::{Note, NotePath};
        let mut idx = InertSemanticIndex;
        let note = Note::parse(NotePath::new("a.md").unwrap(), "hello");
        assert!(idx.upsert(&note).is_ok());
        assert!(idx.reindex(&[note]).is_ok());
        assert!(idx.remove(&NotePath::new("a.md").unwrap()).is_ok());
        assert!(idx
            .neighbors(&NotePath::new("a.md").unwrap(), 5)
            .unwrap()
            .is_empty());
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo nextest run -p cairn-ports inert_semantic_index_is_inert`
Expected: FAIL — `cannot find type InertSemanticIndex`.

- [ ] **Step 4: Implement `InertSemanticIndex`**

In `crates/cairn-ports/src/lib.rs`, after the `NoopPluginHost` impl, add:

```rust
/// Inert semantic-index seam — the engine's no-suggestions default.
#[derive(Debug, Default)]
pub struct InertSemanticIndex;
impl SemanticIndex for InertSemanticIndex {
    fn upsert(&mut self, _note: &Note) -> Result<(), PortError> { Ok(()) }
    fn remove(&mut self, _path: &NotePath) -> Result<(), PortError> { Ok(()) }
    fn reindex(&mut self, _notes: &[Note]) -> Result<(), PortError> { Ok(()) }
    fn neighbors(&self, _focus: &NotePath, _top_k: usize) -> Result<Vec<Similarity>, PortError> {
        Ok(Vec::new())
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p cairn-ports inert_semantic_index_is_inert`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-ports/src/lib.rs
git commit -m "feat(ports): SemanticIndex port + InertSemanticIndex default"
```

---

### Task 3: `LexicalSemanticIndex` adapter (TF-IDF + cosine)

**Files:**
- Create: `crates/cairn-infra/src/semantic.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (`mod semantic;` + re-export)
- Test: `crates/cairn-infra/src/semantic.rs` `tests` module

**Interfaces:**
- Consumes: `SemanticIndex`, `Similarity`, `PortError` (Task 2); `cairn_domain::{Note, NotePath}`.
- Produces: `struct LexicalSemanticIndex` implementing `SemanticIndex`; `LexicalSemanticIndex::new() -> Self` (or `Default`).

- [ ] **Step 1: Create the module skeleton + tokenizer**

Create `crates/cairn-infra/src/semantic.rs`:

```rust
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
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can",
    "her", "was", "one", "our", "out", "has", "had", "his", "she", "they",
    "this", "that", "with", "from", "have", "your", "what", "when", "were",
];

/// Lowercase, split on non-alphanumeric, drop stopwords and very short tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= MIN_TOKEN_LEN)
        .map(str::to_lowercase)
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}
```

- [ ] **Step 2: Write failing adapter tests**

Append to `crates/cairn-infra/src/semantic.rs`:

```rust
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
            note("borrow.md", "borrow checker ownership lifetime rust references"),
            note("cooking.md", "tomato basil pasta garlic olive oil"),
        ];
        let mut idx = LexicalSemanticIndex::default();
        idx.reindex(&notes).unwrap();

        let n = idx.neighbors(&NotePath::new("rust.md").unwrap(), 5).unwrap();
        assert_eq!(n.first().unwrap().path.as_str(), "borrow.md", "topical match ranks first");
        // cooking is unrelated → either absent or strictly lower.
        let borrow_score = n.iter().find(|s| s.path.as_str() == "borrow.md").unwrap().score;
        let cooking_score = n.iter().find(|s| s.path.as_str() == "cooking.md").map(|s| s.score).unwrap_or(0.0);
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
        assert!(idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap().is_empty());
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
        assert!(!idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap().is_empty());
        idx.remove(&NotePath::new("b.md").unwrap()).unwrap();
        assert!(
            idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap().is_empty(),
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p cairn-infra semantic::`
Expected: FAIL — `cannot find ... LexicalSemanticIndex`.

- [ ] **Step 4: Implement `LexicalSemanticIndex`**

Insert after the `tokenize` function in `crates/cairn-infra/src/semantic.rs` (before the `tests` module):

```rust
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
            let (small, large) = if fv.len() <= ov.len() { (&fv, &ov) } else { (&ov, &fv) };
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
            let shared = overlap.into_iter().take(MAX_SHARED_TERMS).map(|(t, _)| t).collect();
            scored.push(Similarity { path: path.clone(), score, shared });
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
```

- [ ] **Step 5: Wire the module + re-export**

In `crates/cairn-infra/src/lib.rs`: add `mod semantic;` with the other `mod` lines, and add `LexicalSemanticIndex` to the public re-exports (mirror how `TantivyIndex` / `InMemoryIndex` are re-exported, e.g. `pub use semantic::LexicalSemanticIndex;`).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-infra semantic::`
Expected: PASS (all five).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-infra/src/semantic.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): LexicalSemanticIndex — TF-IDF + cosine adapter"
```

---

### Task 4: Engine semantic field + incremental hooks

Adds the boxed port to `Engine` (default `NullSemanticIndex`), a setter, and drives `upsert`/`remove` from the three existing change sites — gated on a "built" flag so pre-suggestion writes don't do redundant work.

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`
- Test: `crates/cairn-app/src/lib.rs` `tests` module

**Interfaces:**
- Consumes: `cairn_ports::{SemanticIndex, Similarity, InertSemanticIndex}` (the inert default, defined in `cairn-ports` per Task 2 — `cairn-app` must NOT depend on `cairn-infra`).
- Produces: `Engine::set_semantic_index(&mut self, Box<dyn SemanticIndex + Send>)`; private fields `semantic: RefCell<Box<dyn SemanticIndex + Send>>`, `semantic_built: Cell<bool>`.

> **No `cairn-infra` dependency in `cairn-app`.** The inner hexagon cannot import adapters, so the default comes from `cairn_ports::InertSemanticIndex` (defined in Task 2, beside `NoopPluginHost`). The real `LexicalSemanticIndex` is injected only at the composition root (Task 7).

- [ ] **Step 1: Add the field + default + setter**

In `crates/cairn-app/src/lib.rs`:

Add imports: extend the `use cairn_ports::{...}` to include `SemanticIndex, Similarity, InertSemanticIndex` (the inert default added to `cairn-ports` per the decision above). Add `use std::cell::Cell;` next to the existing `use std::cell::RefCell;`.

Add fields to `struct Engine`:

```rust
    semantic: RefCell<Box<dyn SemanticIndex + Send>>,
    semantic_built: Cell<bool>,
```

In `Engine::new`, initialize them:

```rust
            semantic: RefCell::new(Box::new(InertSemanticIndex)),
            semantic_built: Cell::new(false),
```

Add the setter near `set_plugin_host`:

```rust
    /// Replace the semantic index (the composition root injects the real one).
    /// Resets the lazy-build flag so the next `suggestions` call rebuilds it.
    pub fn set_semantic_index(&mut self, index: Box<dyn SemanticIndex + Send>) {
        self.semantic = RefCell::new(index);
        self.semantic_built.set(false);
    }
```

- [ ] **Step 2: Write the failing hook test**

Add to the `tests` module in `crates/cairn-app/src/lib.rs`:

```rust
    /// A SemanticIndex that records the calls made to it.
    #[derive(Default)]
    struct RecordingSemantic {
        upserts: std::cell::RefCell<Vec<String>>,
        removes: std::cell::RefCell<Vec<String>>,
        built: std::cell::Cell<bool>,
    }
    impl cairn_ports::SemanticIndex for RecordingSemantic {
        fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
            self.upserts.borrow_mut().push(note.path.as_str().to_string());
            Ok(())
        }
        fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
            self.removes.borrow_mut().push(path.as_str().to_string());
            Ok(())
        }
        fn reindex(&mut self, _notes: &[Note]) -> Result<(), PortError> {
            self.built.set(true);
            Ok(())
        }
        fn neighbors(&self, _f: &NotePath, _k: usize) -> Result<Vec<cairn_ports::Similarity>, PortError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn writes_and_deletes_drive_semantic_index_after_build() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        eng.set_semantic_index(Box::new(RecordingSemantic::default()));
        let mut ev = Vec::new();
        let a = NotePath::new("a.md").unwrap();

        // Before any suggestions() build, writes do NOT upsert (lazy build will capture).
        eng.write_note(&a, "rust ownership", &mut ev).unwrap();
        // Force the built flag via a suggestions() call (Task 5 adds it); until then,
        // assert the gate by calling the internal builder. See Task 5 for the live test.
    }
```

> Note: the *observable* live behavior (writes upsert only after build) is fully tested in Task 5 once `suggestions()` exists. This task's test just confirms the field + setter compile and a write through a recording index does not panic. Keep this minimal test; Task 5 adds the behavioral assertions.

- [ ] **Step 3: Run test to verify it compiles/fails appropriately**

Run: `cargo nextest run -p cairn-app writes_and_deletes_drive_semantic_index_after_build`
Expected: FAIL to compile until Step 1 is done; PASS once Step 1 compiles (the test body asserts nothing yet).

- [ ] **Step 4: Add the gated hooks in the three change sites**

In `apply_write` (after `self.index.upsert(&note)?;`):

```rust
                if self.semantic_built.get() {
                    self.semantic.get_mut().upsert(&note)?;
                }
```

In `apply_change`'s `Changed` arm (after its `self.index.upsert(&note)?;`):

```rust
                if self.semantic_built.get() {
                    self.semantic.get_mut().upsert(&note)?;
                }
```

In `apply_removal` (after `self.index.remove(path)?;`):

```rust
            if self.semantic_built.get() {
                self.semantic.get_mut().remove(path)?;
            }
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo nextest run -p cairn-app`
Expected: PASS (existing tests unaffected; new test passes).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-app/src/lib.rs
git commit -m "feat(app): Engine semantic-index field, setter, incremental hooks"
```

---

### Task 5: `Engine::suggestions` use-case

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`
- Test: `crates/cairn-app/src/lib.rs` `tests` module

**Interfaces:**
- Consumes: the `semantic` field + `semantic_built` (Task 4); `Graph::build`, `with_notes` (existing).
- Produces: `pub enum Scope { Note(NotePath), Vault }`; `pub struct SuggestedEdgeData { from: NotePath, to: NotePath, weight: f32, why: Option<String> }`; `Engine::suggestions(&self, scope: &Scope) -> Result<Vec<SuggestedEdgeData>, PortError>`; consts `SUGGEST_TOP_K`, `SUGGEST_FLOOR`, `VAULT_EDGE_CAP`.

- [ ] **Step 1: Write failing use-case tests**

Add to the `tests` module in `crates/cairn-app/src/lib.rs`:

```rust
    use cairn_infra::LexicalSemanticIndex;

    fn lexical_engine(dir: &std::path::Path) -> Engine {
        let mut e = engine(dir);
        e.set_semantic_index(Box::new(LexicalSemanticIndex::new()));
        e
    }

    #[test]
    fn suggestions_exclude_self_and_already_linked() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        // a links to b explicitly; c is unlinked but topically identical to a.
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership borrow [[b]]", &mut ev).unwrap();
        eng.write_note(&NotePath::new("b.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();
        eng.write_note(&NotePath::new("c.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();

        let s = eng.suggestions(&Scope::Note(NotePath::new("a.md").unwrap())).unwrap();
        // self never appears; already-linked b never appears; c (unlinked, related) does.
        assert!(s.iter().all(|e| e.to.as_str() != "a.md"));
        assert!(s.iter().all(|e| e.to.as_str() != "b.md"), "already-linked excluded");
        assert!(s.iter().any(|e| e.to.as_str() == "c.md"), "unlinked related surfaced");
        assert!(s.iter().all(|e| e.from.as_str() == "a.md"));
    }

    #[test]
    fn suggestions_below_floor_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership borrow", &mut ev).unwrap();
        eng.write_note(&NotePath::new("z.md").unwrap(), "tomato basil pasta garlic", &mut ev).unwrap();
        let s = eng.suggestions(&Scope::Note(NotePath::new("a.md").unwrap())).unwrap();
        assert!(s.iter().all(|e| e.to.as_str() != "z.md"), "unrelated note below floor excluded");
    }

    #[test]
    fn vault_scope_dedups_pairs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();
        eng.write_note(&NotePath::new("b.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();
        let s = eng.suggestions(&Scope::Vault).unwrap();
        // exactly one undirected pair (a,b), canonical from < to.
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].from.as_str(), "a.md");
        assert_eq!(s[0].to.as_str(), "b.md");
    }

    #[test]
    fn suggestions_lazy_build_then_stays_live() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = lexical_engine(tmp.path());
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership borrow", &mut ev).unwrap();
        // First call lazily builds from existing notes.
        let _ = eng.suggestions(&Scope::Note(NotePath::new("a.md").unwrap())).unwrap();
        // A later write must be reflected (index is now live).
        eng.write_note(&NotePath::new("d.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();
        let s = eng.suggestions(&Scope::Note(NotePath::new("a.md").unwrap())).unwrap();
        assert!(s.iter().any(|e| e.to.as_str() == "d.md"), "post-build write surfaced");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p cairn-app suggestions`
Expected: FAIL — `cannot find type Scope` / `no method suggestions`.

- [ ] **Step 3: Implement `Scope`, `SuggestedEdgeData`, consts, and `suggestions`**

Add near the top of `crates/cairn-app/src/lib.rs` (after the `Event` enum):

```rust
/// What to compute suggestions over (the app-layer mirror of the wire `GraphScope`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One focus note.
    Note(NotePath),
    /// The whole vault.
    Vault,
}

/// A suggested non-explicit edge (the app-layer mirror of the wire `SuggestedEdge`).
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestedEdgeData {
    /// Source note.
    pub from: NotePath,
    /// Target note.
    pub to: NotePath,
    /// Cosine similarity, `0..1` — ranking only.
    pub weight: f32,
    /// Provenance (shared terms), or `None`.
    pub why: Option<String>,
}

/// Suggestions returned per focus note.
const SUGGEST_TOP_K: usize = 5;
/// Similarity below this is dropped as noise.
const SUGGEST_FLOOR: f32 = 0.1;
/// Max edges returned for a `Vault` scope.
const VAULT_EDGE_CAP: usize = 100;
```

Add the methods in the `impl Engine` block (after `notes_by_tag` is fine):

```rust
    /// Ensure the semantic index is built from the current notes (lazy, once).
    fn ensure_semantic_built(&self) -> Result<(), PortError> {
        if self.semantic_built.get() {
            return Ok(());
        }
        let notes: Vec<Note> = self.with_notes(|m| m.values().cloned().collect())?;
        self.semantic.borrow_mut().reindex(&notes)?;
        self.semantic_built.set(true);
        Ok(())
    }

    /// Format `why` provenance from shared terms.
    fn why_from(shared: &[String]) -> Option<String> {
        if shared.is_empty() {
            None
        } else {
            Some(format!("shared: {}", shared.join(", ")))
        }
    }

    /// Suggested non-explicit edges within `scope`. Excludes self, already-linked
    /// pairs, and sub-floor similarities.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if a `Note` scope's path is unknown; [`PortError`]
    /// on a port failure.
    pub fn suggestions(&self, scope: &Scope) -> Result<Vec<SuggestedEdgeData>, PortError> {
        self.ensure_semantic_built()?;
        let graph = self.graph()?;
        match scope {
            Scope::Note(path) => {
                // Unknown focus → NotFound (mirrors read_note semantics).
                if !self.with_notes(|m| m.contains_key(path))? {
                    return Err(PortError::NotFound(path.as_str().to_string()));
                }
                let mut linked: HashSet<NotePath> = HashSet::new();
                linked.extend(graph.forward_links(path).iter().cloned());
                linked.extend(graph.backlinks(path).iter().cloned());
                let mut out = Vec::new();
                for s in self.semantic.borrow().neighbors(path, SUGGEST_TOP_K)? {
                    if s.score < SUGGEST_FLOOR || &s.path == path || linked.contains(&s.path) {
                        continue;
                    }
                    out.push(SuggestedEdgeData {
                        from: path.clone(),
                        to: s.path,
                        weight: s.score,
                        why: Self::why_from(&s.shared),
                    });
                }
                Ok(out)
            }
            Scope::Vault => {
                let paths: Vec<NotePath> = self.with_notes(|m| m.keys().cloned().collect())?;
                let mut seen: HashSet<(NotePath, NotePath)> = HashSet::new();
                let mut out: Vec<SuggestedEdgeData> = Vec::new();
                for focus in &paths {
                    let mut linked: HashSet<NotePath> = HashSet::new();
                    linked.extend(graph.forward_links(focus).iter().cloned());
                    linked.extend(graph.backlinks(focus).iter().cloned());
                    for s in self.semantic.borrow().neighbors(focus, SUGGEST_TOP_K)? {
                        if s.score < SUGGEST_FLOOR || &s.path == focus || linked.contains(&s.path) {
                            continue;
                        }
                        // Canonical undirected pair (from < to) for dedup.
                        let (from, to) = if focus < &s.path {
                            (focus.clone(), s.path.clone())
                        } else {
                            (s.path.clone(), focus.clone())
                        };
                        if !seen.insert((from.clone(), to.clone())) {
                            continue;
                        }
                        out.push(SuggestedEdgeData {
                            from,
                            to,
                            weight: s.score,
                            why: Self::why_from(&s.shared),
                        });
                    }
                }
                out.sort_by(|a, b| {
                    b.weight
                        .partial_cmp(&a.weight)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| (&a.from, &a.to).cmp(&(&b.from, &b.to)))
                });
                out.truncate(VAULT_EDGE_CAP);
                Ok(out)
            }
        }
    }
```

`HashSet` is already imported (`use std::collections::{HashMap, HashSet};`). Confirm `NotePath: Ord` (it is — used as `BTreeMap` key in `graph.rs`), so `<` comparison and `.cmp` work.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-app suggestions vault_scope`
Expected: PASS (all four).

- [ ] **Step 5: Run the full app crate to check nothing regressed**

Run: `cargo nextest run -p cairn-app`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): Engine::suggestions use-case with exclusion + dedup"
```

---

### Task 6: Replace the dispatch stub with the real call

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`
- Test: `crates/cairn-service/src/lib.rs` `tests` module

**Interfaces:**
- Consumes: `Engine::suggestions`, `Scope`, `SuggestedEdgeData` (Task 5); wire `GraphScope`, `SuggestedEdge` (Task 1).
- Produces: `map_scope`, `map_suggested_edge` (private helpers).

- [ ] **Step 1: Write failing dispatch tests**

Add to the `tests` module in `crates/cairn-service/src/lib.rs`:

```rust
    #[test]
    fn get_suggestions_dispatch_returns_unlinked_related() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = cairn_startup::build_engine(tmp.path()).unwrap();
        let mut ev: Vec<AppEvent> = Vec::new();
        dispatch_command(&mut eng, &Command::WriteNote {
            path: "a.md".into(), contents: "rust ownership borrow".into(),
        }, &mut ev).unwrap();
        dispatch_command(&mut eng, &Command::WriteNote {
            path: "c.md".into(), contents: "rust ownership borrow lifetime".into(),
        }, &mut ev).unwrap();

        let resp = dispatch_query(&eng, &Query::GetSuggestions {
            scope: cairn_contract::GraphScope::Note { path: "a.md".into() },
        }).unwrap();
        match resp {
            QueryResponse::Suggestions { suggestions } => {
                assert!(suggestions.iter().any(|e| e.to == "c.md"));
                assert!(suggestions.iter().all(|e| e.from == "a.md"));
            }
            other => panic!("expected Suggestions, got {other:?}"),
        }
    }

    #[test]
    fn get_suggestions_unknown_focus_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let eng = cairn_startup::build_engine(tmp.path()).unwrap();
        let err = dispatch_query(&eng, &Query::GetSuggestions {
            scope: cairn_contract::GraphScope::Note { path: "missing.md".into() },
        }).unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }
```

> Note: `build_engine` injects `LexicalSemanticIndex` only after Task 7. Until then these tests use the inert default and the first test would fail (`c.md` not surfaced). **Order Task 7 before re-running this test**, OR inject in-test. To keep this task self-contained, inject in-test: replace `build_engine(...)` with the helper below.

Add this helper to the test module:

```rust
    fn lexical_engine(dir: &std::path::Path) -> cairn_app::Engine {
        let mut e = cairn_startup::build_engine(dir).unwrap();
        e.set_semantic_index(Box::new(cairn_infra::LexicalSemanticIndex::new()));
        e
    }
```

and use `lexical_engine(tmp.path())` in the first test. The `cairn-service` dev-dependencies already include `cairn-infra` (used by existing tests).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p cairn-service get_suggestions`
Expected: FAIL — the stub returns `[]`, so `c.md` is not found and `NotFound` is not returned.

- [ ] **Step 3: Implement the real arm + mappers**

In `crates/cairn-service/src/lib.rs`, replace the stub `Query::GetSuggestions` arm with:

```rust
        Query::GetSuggestions { scope } => {
            let scope = map_scope(scope)?;
            let suggestions = engine
                .suggestions(&scope)?
                .into_iter()
                .map(map_suggested_edge)
                .collect();
            Ok(QueryResponse::Suggestions { suggestions })
        }
```

Add the private helpers near `parse_path`:

```rust
fn map_scope(scope: &cairn_contract::GraphScope) -> Result<cairn_app::Scope, ServiceError> {
    use cairn_contract::GraphScope;
    Ok(match scope {
        GraphScope::Note { path } => cairn_app::Scope::Note(parse_path(path)?),
        GraphScope::Vault => cairn_app::Scope::Vault,
    })
}

fn map_suggested_edge(e: cairn_app::SuggestedEdgeData) -> cairn_contract::SuggestedEdge {
    cairn_contract::SuggestedEdge {
        from: e.from.as_str().to_string(),
        to: e.to.as_str().to_string(),
        weight: e.weight,
        why: e.why,
    }
}
```

Add `SuggestedEdge` to the `cairn_contract::{...}` import (or reference fully-qualified as above — the arm uses `QueryResponse::Suggestions`, already imported).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cairn-service get_suggestions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-service/src/lib.rs
git commit -m "feat(service): dispatch GetSuggestions to Engine::suggestions"
```

---

### Task 7: Composition-root wiring

Injects `LexicalSemanticIndex` everywhere the engine is built for real use, so `GetSuggestions` returns live results over the CLI `/query` and daemon.

**Files:**
- Modify: `crates/cairn-startup/src/lib.rs` (`build_engine`)
- Modify: `crates/cairn-daemon/src/main.rs:69` (persistent engine)
- Test: `crates/cairn-startup/src/lib.rs` `tests` module

**Interfaces:**
- Consumes: `Engine::set_semantic_index` (Task 4), `cairn_infra::LexicalSemanticIndex` (Task 3).

- [ ] **Step 1: Write a failing end-to-end test in `cairn-startup`**

Add to the `tests` module in `crates/cairn-startup/src/lib.rs`:

```rust
    #[test]
    fn build_engine_wires_semantic_suggestions() {
        use cairn_app::Scope;
        use cairn_domain::NotePath;
        let tmp = tempfile::tempdir().unwrap();
        GitVcs::open_or_init(tmp.path()).unwrap();
        let mut eng = build_engine(tmp.path()).unwrap();
        let mut ev = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "rust ownership borrow", &mut ev).unwrap();
        eng.write_note(&NotePath::new("c.md").unwrap(), "rust ownership borrow lifetime", &mut ev).unwrap();
        let s = eng.suggestions(&Scope::Note(NotePath::new("a.md").unwrap())).unwrap();
        assert!(s.iter().any(|e| e.to.as_str() == "c.md"), "real adapter wired by build_engine");
    }
```

`cairn-startup` dev-dependencies need `cairn-domain` and `tempfile` (tempfile already used in its tests; add `cairn-domain` to `[dev-dependencies]` in `crates/cairn-startup/Cargo.toml` if absent). `cairn-app` is a normal dependency already.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p cairn-startup build_engine_wires_semantic_suggestions`
Expected: FAIL — inert default returns no neighbors, so `c.md` not surfaced.

- [ ] **Step 3: Wire `build_engine`**

In `crates/cairn-startup/src/lib.rs`, add `LexicalSemanticIndex` to the `use cairn_infra::{...}` import and inject it before returning:

```rust
    let index = TantivyIndex::in_memory().map_err(|e| StartupError::Build(e.to_string()))?;
    let mut engine = Engine::new(store, index, vcs);
    engine.set_semantic_index(Box::new(LexicalSemanticIndex::new()));
    Ok(engine)
```

- [ ] **Step 4: Wire the daemon persistent engine**

In `crates/cairn-daemon/src/main.rs`, add `LexicalSemanticIndex` to the `use cairn_infra::{...}` line, and after line 69 (`let mut eng = Engine::new(store, index, vcs);`) insert:

```rust
        eng.set_semantic_index(Box::new(LexicalSemanticIndex::new()));
```

(The other daemon branch at line 74 uses `build_engine`, which Step 3 already covers.)

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo nextest run -p cairn-startup -p cairn-daemon`
Expected: PASS.

- [ ] **Step 6: Full workspace gate**

Run: `cargo nextest run --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --check`
Expected: PASS, zero warnings, formatting clean.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-startup/ crates/cairn-daemon/src/main.rs
git commit -m "feat(startup): inject LexicalSemanticIndex at the composition root"
```

---

## Self-Review

**1. Spec coverage:**
- Contract types (`GetSuggestions`/`Suggestions`/`GraphScope`/`SuggestedEdge`) → Task 1. ✔
- `SemanticIndex` port + `Similarity` → Task 2. ✔
- `LexicalSemanticIndex` (TF-IDF + cosine, `why` from shared terms, exact IDF under upsert/remove) → Task 3. ✔
- Inert default seam (`InertSemanticIndex`, in `cairn-ports` beside `NoopPluginHost`) → Task 2. ✔
- Engine field + setter + incremental hooks → Task 4. ✔
- Lazy build (interior mutability via `RefCell`/`Cell`) → Task 4 (fields) + Task 5 (`ensure_semantic_built`). ✔
- `suggestions(scope)`: exclude self + already-linked + floor; `Vault` dedup/cap → Task 5. ✔
- Dispatch arm + wire mapping + error mapping (`NotFound` for unknown focus) → Task 1 (stub) + Task 6 (real). ✔
- Composition-root injection (CLI/ephemeral + daemon persistent) → Task 7. ✔
- Constants not wire-exposed; tests assert invariants not numbers → Tasks 4/5 (consts private; tests check ordering/exclusion). ✔
- `query_kind` telemetry arm (build-breaking match) → Task 1 Step 5. ✔

**2. Placeholder scan:** No `TBD`/`TODO`/"add error handling"/"similar to Task N". Each code step shows full code. The Task 4 minimal test is intentionally light with an inline note that Task 5 carries the behavioral assertions — not a placeholder, a deliberate split. ✔

**3. Type consistency:**
- `SemanticIndex::neighbors(&self, &NotePath, usize) -> Result<Vec<Similarity>, PortError>` — identical in Tasks 2, 3, 4 (fake), 5 (caller). ✔
- `Similarity { path, score, shared }` — consistent across Tasks 2/3/5. ✔
- `Scope::{Note(NotePath), Vault}` and `SuggestedEdgeData { from, to, weight, why }` — defined Task 5, consumed Task 6. ✔
- Wire `GraphScope::{Note{path}, Vault}` and `SuggestedEdge { from, to, weight, why }` — defined Task 1, mapped Task 6. ✔
- One inert default only: `cairn_ports::InertSemanticIndex` (Task 2), used as `Engine::new`'s default (Task 4). The real `LexicalSemanticIndex` (Task 3) is injected at the composition root (Task 7). No redundant second null type. ✔
