# Neural on-device SemanticIndex adapter — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a second `SemanticIndex` adapter backed by on-device neural embeddings (all-MiniLM-L6-v2 via candle), selectable at startup behind a `neural` cargo feature, with zero changes to the port/contract/queries.

**Architecture:** A pure-Rust generic `NeuralSemanticIndex<E: Embedder>` holds the index logic (lifecycle, cosine ranking over pooled unit vectors, C-full cross-token attribution). The `Embedder` seam supplies text→per-token embeddings; a deterministic `FakeEmbedder` drives offline tests, and a feature-gated `CandleMiniLm` supplies the real model. Startup picks neural when the feature is compiled and weights load, else falls back to lexical.

**Tech Stack:** Rust (edition 2021, MSRV 1.88), candle-core / candle-nn / candle-transformers (BERT), tokenizers, dirs. All neural deps `optional = true` behind feature `neural`.

## Global Constraints

- MSRV **1.88** — every dependency version pinned must build on 1.88.
- `unsafe_code = "forbid"` (workspace lint) — **no `unsafe` blocks** in our crates. Load safetensors via `candle_core::safetensors::load` (safe), never `VarBuilder::from_mmaped_safetensors` (unsafe).
- `set_semantic_index` requires `Box<dyn SemanticIndex + Send>` — the adapter must be `Send`.
- New dep ⇒ `git add Cargo.lock` in the **same commit**; keep `cargo deny --all-features check` green (CI runs `--all-features`).
- Merge queue enabled: branch → PR → `gh pr merge --auto --squash`; no manual rebase/local-merge. Shared working dir → prefer worktree + `git -C`.
- DoD: `cargo test --workspace` + `cargo clippy --workspace --locked` + `cargo fmt --check` all green (default features), **plus** `cargo clippy/test -p cairn-infra --features neural` green.
- Error construction at this boundary: `PortError::Adapter(AdapterError::message("…"))` for message-only, `AdapterError::new(source)` to wrap a typed error.

---

### Task 1: Pre-flight gate — pin candle at MSRV 1.88, vet licenses, wire the feature

Adds the optional deps and feature flag, and **proves** candle builds on 1.88 with a license-clean tree. This is a gate: if candle's MSRV exceeds 1.88, **STOP and surface it** — do not bump the workspace MSRV.

**Files:**
- Modify: `crates/cairn-infra/Cargo.toml`
- Modify: `Cargo.lock` (generated)

**Interfaces:**
- Produces: cargo feature `cairn-infra/neural` enabling `candle-core`, `candle-nn`, `candle-transformers`, `tokenizers`, `dirs`.

- [ ] **Step 1: Add optional deps + feature to `crates/cairn-infra/Cargo.toml`**

Under `[dependencies]` add (pin the newest patch of each that builds on 1.88 — see Step 2):
```toml
candle-core = { version = "0.9", optional = true }
candle-nn = { version = "0.9", optional = true }
candle-transformers = { version = "0.9", optional = true }
tokenizers = { version = "0.20", optional = true, default-features = false, features = ["onig"] }
dirs = { version = "5", optional = true }
```
Add a `[features]` section:
```toml
[features]
neural = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:tokenizers", "dep:dirs"]
```

- [ ] **Step 2: Verify MSRV 1.88 build (the gate)**

Run: `cargo +1.88 build -p cairn-infra --features neural`
Expected: compiles. If it fails on a candle/tokenizers MSRV requirement, **downgrade** the offending crate to the newest version whose `rust-version` ≤ 1.88 and retry. If no combination builds on 1.88, STOP: report the minimum Rust each requires and await a decision (the workspace MSRV is not ours to bump).

- [ ] **Step 3: Verify license tree is clean**

Run: `cargo deny --all-features check licenses`
Expected: `licenses ok`. If a new license appears (candle pulls `gemm`, `half`, `safetensors`, `memmap2`, `rayon`; tokenizers pulls `onig`, `esaxx-rs`, `macro_rules_attribute`), read its text, then add it to the `allow = [...]` list in `deny.toml` **with a one-line rationale comment** matching the file's existing style. Re-run until clean.

- [ ] **Step 4: Verify no duplicate-version *errors* and format**

Run: `cargo deny --all-features check bans && cargo fmt --check`
Expected: `bans` warns (not errors) on dupes — acceptable per `deny.toml` (`multiple-versions = "warn"`). fmt clean.

- [ ] **Step 5: Commit (deps + lockfile together)**

```bash
git add crates/cairn-infra/Cargo.toml Cargo.lock deny.toml
git commit -m "build(infra): add optional candle/tokenizers deps behind neural feature"
```

---

### Task 2: `Embedder` seam + `NeuralSemanticIndex` lifecycle & ranking (offline, pure Rust)

The always-compiled core: the seam, the index, cosine ranking over pooled unit vectors, and the full lifecycle — all tested with a deterministic fake. No candle. `shared` is left empty here; Task 3 fills it.

**Files:**
- Create: `crates/cairn-infra/src/semantic_neural.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (module decl + re-export)

**Interfaces:**
- Consumes: `cairn_domain::{Note, NotePath}`, `cairn_ports::{PortError, AdapterError, SemanticIndex, Similarity}`.
- Produces:
  - `trait Embedder { fn embed_tokens(&self, text: &str) -> Result<Vec<(String, Vec<f32>)>, PortError>; fn dim(&self) -> usize; }` (private)
  - `pub struct NeuralSemanticIndex<E: Embedder>` with `impl<E: Embedder + Send> SemanticIndex for NeuralSemanticIndex<E>`
  - `struct NoteEmbedding { pooled: Vec<f32>, tokens: Vec<(String, Vec<f32>)> }` (private)
  - free fns `fn dot(a: &[f32], b: &[f32]) -> f32`, `fn unit_normalize(v: &mut [f32])` (private)

- [ ] **Step 1: Write the module skeleton + the failing tests**

Create `crates/cairn-infra/src/semantic_neural.rs`:
```rust
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
/// [`Embedder`] so tests inject a deterministic fake.
pub struct NeuralSemanticIndex<E: Embedder> {
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

impl<E: Embedder> NeuralSemanticIndex<E> {
    /// Build the stored embedding for one note: dedup tokens (first wins),
    /// mean-pool the unique token vectors, unit-normalize the pooled vector.
    fn build(&self, text: &str) -> Result<NoteEmbedding, PortError> {
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
                shared: Vec::new(), // filled in Task 3
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
            note("b.md", "feline on the rug"), // paraphrase of a
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
```

- [ ] **Step 2: Wire the module in `crates/cairn-infra/src/lib.rs`**

Add after `mod semantic;` (keep alphabetical grouping):
```rust
mod semantic_neural;
```
Add after `pub use semantic::LexicalSemanticIndex;`:
```rust
pub use semantic_neural::NeuralSemanticIndex;
```

- [ ] **Step 3: Run tests to verify they fail then pass**

Run: `cargo test -p cairn-infra semantic_neural`
Expected: compiles and all `semantic_neural::tests` PASS (they are written against the implementation above, so this task lands them green in one commit).

- [ ] **Step 4: Lint + format**

Run: `cargo clippy -p cairn-infra --all-targets && cargo fmt --check`
Expected: no warnings, formatted.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/semantic_neural.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): neural semantic index core + Embedder seam (ranking, lifecycle)"
```

---

### Task 3: C-full `why` — cross-token nearest-neighbor attribution

Fill `Similarity::shared` by scoring each focus token by its max cosine to any neighbor token, keeping the top terms above a threshold. Ranking is untouched (still pooled cosine).

**Files:**
- Modify: `crates/cairn-infra/src/semantic_neural.rs`

**Interfaces:**
- Produces: `fn attribute(focus: &[(String, Vec<f32>)], neighbor: &[(String, Vec<f32>)]) -> Vec<String>` (private)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:
```rust
#[test]
fn shared_surfaces_paraphrase_bridge_tokens() {
    let mut idx = ix();
    idx.reindex(&[note("a.md", "cat on the mat"), note("b.md", "feline on the rug")])
        .unwrap();
    let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 5).unwrap();
    let shared = &n.iter().find(|s| s.path.as_str() == "b.md").unwrap().shared;
    // cat≈feline and mat≈rug drive the match, even with no literal overlap.
    assert!(shared.iter().any(|t| t == "cat"), "got {shared:?}");
    assert!(shared.iter().any(|t| t == "mat"), "got {shared:?}");
    // stopword-ish tokens embed to zero → below threshold → not named.
    assert!(!shared.iter().any(|t| t == "on" || t == "the"), "got {shared:?}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra shared_surfaces_paraphrase_bridge_tokens`
Expected: FAIL — `shared` is currently always empty, so the `any(== "cat")` assert fails.

- [ ] **Step 3: Implement `attribute` and call it**

Add the free function (near `dot`):
```rust
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
    scored.into_iter().take(MAX_SHARED_TERMS).map(|(t, _)| t).collect()
}
```
In `neighbors`, replace `shared: Vec::new(), // filled in Task 3` with:
```rust
shared: attribute(&f.tokens, &e.tokens),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra semantic_neural`
Expected: all PASS, including `shared_surfaces_paraphrase_bridge_tokens`.

- [ ] **Step 5: Lint, format, commit**

```bash
cargo clippy -p cairn-infra --all-targets && cargo fmt --check
git add crates/cairn-infra/src/semantic_neural.rs
git commit -m "feat(infra): cross-token attribution for neural why provenance"
```

---

### Task 4: `CandleMiniLm` embedder + weights path (feature-gated)

The real model, compiled only under `--features neural`. Loads all-MiniLM-L6-v2 from a directory, embeds per-token via BERT last-hidden-state. One `#[ignore]`d integration test drives it when weights are present.

**Files:**
- Modify: `crates/cairn-infra/src/semantic_neural.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (feature-gated re-exports)

**Interfaces:**
- Consumes: `Embedder`, `NeuralSemanticIndex` (Task 2), `PortError`/`AdapterError`.
- Produces (all `#[cfg(feature = "neural")]`):
  - `pub struct CandleMiniLm`
  - `impl NeuralSemanticIndex<CandleMiniLm> { pub fn open(dir: &Path) -> Result<Self, PortError> }`
  - `pub fn minilm_weights_path() -> std::path::PathBuf`

- [ ] **Step 1: Add the gated imports + `CandleMiniLm` + `open` + weights path**

At the top of `semantic_neural.rs`, add gated imports:
```rust
#[cfg(feature = "neural")]
use std::path::{Path, PathBuf};

#[cfg(feature = "neural")]
use cairn_ports::AdapterError;
#[cfg(feature = "neural")]
use candle_core::{safetensors, DType, Device, Tensor};
#[cfg(feature = "neural")]
use candle_nn::VarBuilder;
#[cfg(feature = "neural")]
use candle_transformers::models::bert::{BertModel, Config};
#[cfg(feature = "neural")]
use tokenizers::Tokenizer;
```
Add the gated block (near the end, before `#[cfg(test)] mod tests`):
```rust
/// all-MiniLM-L6-v2 run on-device via candle. Deterministic on CPU (f32,
/// inference-only). `Send`: `BertModel`/`Tokenizer`/`Device` are all `Send`.
#[cfg(feature = "neural")]
pub struct CandleMiniLm {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

#[cfg(feature = "neural")]
fn adapt(e: impl std::error::Error + Send + Sync + 'static) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

#[cfg(feature = "neural")]
impl CandleMiniLm {
    /// Load `config.json`, `tokenizer.json`, `model.safetensors` from `dir`.
    /// No network: a missing file is an error the caller handles (startup
    /// falls back to lexical).
    fn open(dir: &Path) -> Result<Self, PortError> {
        let device = Device::Cpu;
        let cfg_text = std::fs::read_to_string(dir.join("config.json")).map_err(adapt)?;
        let config: Config = serde_json::from_str(&cfg_text).map_err(adapt)?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| PortError::Adapter(AdapterError::message(e.to_string())))?;
        // SAFE loader (no mmap) — the workspace forbids `unsafe`.
        let tensors = safetensors::load(dir.join("model.safetensors"), &device).map_err(adapt)?;
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &device);
        let model = BertModel::load(vb, &config).map_err(adapt)?;
        Ok(Self { model, tokenizer, device })
    }
}

#[cfg(feature = "neural")]
impl NeuralSemanticIndex<CandleMiniLm> {
    /// Open a neural index backed by weights in `dir`.
    ///
    /// # Errors
    /// [`PortError::Adapter`] if the model/tokenizer fail to load.
    pub fn open(dir: &Path) -> Result<Self, PortError> {
        Ok(Self {
            embedder: CandleMiniLm::open(dir)?,
            notes: HashMap::new(),
        })
    }
}

#[cfg(feature = "neural")]
impl Embedder for CandleMiniLm {
    fn embed_tokens(&self, text: &str) -> Result<Vec<(String, Vec<f32>)>, PortError> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| PortError::Adapter(AdapterError::message(e.to_string())))?;
        let ids = enc.get_ids();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let toks = enc.get_tokens();
        let specials = enc.get_special_tokens_mask();
        let input_ids = Tensor::new(ids, &self.device).map_err(adapt)?.unsqueeze(0).map_err(adapt)?;
        let token_type_ids = input_ids.zeros_like().map_err(adapt)?;
        // NOTE: match the pinned candle-transformers `BertModel::forward`
        // signature. 0.9.x is `forward(input_ids, token_type_ids, attention_mask: Option<&Tensor>)`.
        // If the pinned version is 2-arg, drop the trailing `None`.
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, None)
            .map_err(adapt)?
            .squeeze(0)
            .map_err(adapt)?; // [seq, hidden]
        let mut out = Vec::new();
        for (i, tok) in toks.iter().enumerate() {
            if specials.get(i).copied() == Some(1) {
                continue; // drop [CLS]/[SEP]/[PAD]
            }
            let mut row: Vec<f32> = hidden.get(i).map_err(adapt)?.to_vec1().map_err(adapt)?;
            unit_normalize(&mut row);
            out.push((tok.clone(), row));
        }
        Ok(out)
    }

    fn dim(&self) -> usize {
        self.model.config.hidden_size
    }
}

/// Resolve the MiniLM weights directory: `$CAIRN_MINILM_WEIGHTS` if set, else
/// `<cache_dir>/cairn/models/all-MiniLM-L6-v2`. No network — a missing dir is
/// handled by the caller.
#[cfg(feature = "neural")]
#[must_use]
pub fn minilm_weights_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CAIRN_MINILM_WEIGHTS") {
        return PathBuf::from(p);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cairn/models/all-MiniLM-L6-v2")
}
```

> If `self.model.config.hidden_size` is not accessible in the pinned version, store the dim at `open` time (read `config.hidden_size` before `BertModel::load` and keep it in `CandleMiniLm`).

- [ ] **Step 2: Feature-gated re-exports in `crates/cairn-infra/src/lib.rs`**

Add:
```rust
#[cfg(feature = "neural")]
pub use semantic_neural::{minilm_weights_path, CandleMiniLm};
```

- [ ] **Step 3: Add the `#[ignore]` integration test**

Append inside `mod tests` (gated so it only compiles under the feature):
```rust
#[cfg(feature = "neural")]
#[test]
#[ignore = "requires CAIRN_MINILM_WEIGHTS to point at a MiniLM model dir"]
fn candle_minilm_ranks_semantically_related() {
    let dir = std::env::var("CAIRN_MINILM_WEIGHTS").expect("set CAIRN_MINILM_WEIGHTS");
    let mut idx = NeuralSemanticIndex::open(std::path::Path::new(&dir)).unwrap();
    idx.reindex(&[
        note("a.md", "The cat sat on the mat"),
        note("b.md", "A feline rested on the rug"), // paraphrase
        note("c.md", "Quarterly revenue projections"),
    ])
    .unwrap();
    let n = idx.neighbors(&NotePath::new("a.md").unwrap(), 1).unwrap();
    assert_eq!(n[0].path.as_str(), "b.md");
}
```

- [ ] **Step 4: Verify — default build untouched, feature build green**

Run:
```
cargo test -p cairn-infra                                   # default: candle NOT compiled, core tests pass
cargo clippy -p cairn-infra --features neural --all-targets # gated code lints clean
cargo test -p cairn-infra --features neural                 # #[ignore]d model test is skipped
```
Expected: all green; the ignored test is reported as ignored, not run.

- [ ] **Step 5: (optional) exercise the real model if weights are available**

Run: `CAIRN_MINILM_WEIGHTS=<dir> cargo test -p cairn-infra --features neural -- --ignored`
Expected: PASS if a valid MiniLM dir is provided. If no weights are on hand, note this in the task handoff — CI's neural job (Task 6) covers it.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-infra/src/semantic_neural.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): CandleMiniLm neural embedder behind neural feature"
```

---

### Task 5: Startup selection with graceful fallback

Wire adapter choice at the single injection site. Feature off → lexical (unchanged). Feature on → neural if weights load, else `warn!` + lexical.

**Files:**
- Modify: `crates/cairn-startup/Cargo.toml`
- Modify: `crates/cairn-startup/src/lib.rs` (around line 59)

**Interfaces:**
- Consumes: `cairn_infra::{LexicalSemanticIndex, NeuralSemanticIndex, minilm_weights_path}`, `cairn_ports::SemanticIndex`.

- [ ] **Step 1: Add deps + forwarding feature to `crates/cairn-startup/Cargo.toml`**

Under `[dependencies]` add:
```toml
cairn-ports = { path = "../cairn-ports" }
tracing = { workspace = true }
```
Add:
```toml
[features]
neural = ["cairn-infra/neural"]
```

- [ ] **Step 2: Replace the injection line in `crates/cairn-startup/src/lib.rs`**

Replace `engine.set_semantic_index(Box::new(LexicalSemanticIndex::new()));` with:
```rust
    engine.set_semantic_index(semantic_index());
```
Add these two cfg-selected helpers above `build_engine` (or just below it in the same module):
```rust
#[cfg(not(feature = "neural"))]
fn semantic_index() -> Box<dyn cairn_ports::SemanticIndex + Send> {
    Box::new(LexicalSemanticIndex::new())
}

/// Neural when weights load; otherwise warn and fall back to lexical so the
/// engine always builds (neural is an opt-in upgrade, never a hard dependency).
#[cfg(feature = "neural")]
fn semantic_index() -> Box<dyn cairn_ports::SemanticIndex + Send> {
    let path = cairn_infra::minilm_weights_path();
    match cairn_infra::NeuralSemanticIndex::open(&path) {
        Ok(ix) => Box::new(ix),
        Err(e) => {
            tracing::warn!(%e, path = %path.display(), "neural weights unavailable; using lexical");
            Box::new(LexicalSemanticIndex::new())
        }
    }
}
```
Ensure `LexicalSemanticIndex` is in scope (it is already imported at the existing call site; keep that import).

- [ ] **Step 3: Verify both feature modes build and test**

Run:
```
cargo test -p cairn-startup                  # default → lexical path
cargo build -p cairn-startup --features neural   # neural path compiles
cargo clippy -p cairn-startup --all-targets
cargo fmt --check
```
Expected: all green. (The existing `build_engine` test still passes — fallback guarantees a working engine even without weights.)

- [ ] **Step 4: Commit (deps + lockfile)**

```bash
git add crates/cairn-startup/Cargo.toml crates/cairn-startup/src/lib.rs Cargo.lock
git commit -m "feat(startup): select neural semantic index by feature with lexical fallback"
```

---

### Task 6: CI — feature-aware clippy/test job

Ensure the neural code stays compiled, linted, and its ignored test exercised. License coverage is already handled (CI `cargo-deny` runs `--all-features`).

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add a `neural` job**

Add a job mirroring the existing `clippy`/`test` job style (reuse the same toolchain + `Swatinem/rust-cache` setup already in the file):
```yaml
  neural:
    name: neural feature
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<pinned-sha as elsewhere in this file>
      - uses: <same rust-toolchain action/sha as the clippy job>
      - uses: Swatinem/rust-cache@<same sha as other jobs>
        with:
          shared-key: neural
      - run: cargo clippy -p cairn-infra -p cairn-startup --features neural --locked --all-targets
      - run: cargo test -p cairn-infra --features neural --locked
```
Use the **exact pinned action SHAs already used by neighboring jobs** in `ci.yml` (do not introduce new/unpinned actions). Add `neural` to the `needs: [...]` list of the final gate job (`ci.yml:129`).

- [ ] **Step 2: Validate workflow locally**

Run: `actionlint .github/workflows/ci.yml` (if available) or re-read the diff to confirm SHAs/indentation match sibling jobs.
Expected: no schema errors; job structure matches existing jobs.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build, lint, and test the neural feature"
```

---

## Final verification (before PR)

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --locked --all-targets` (default features) — green
- [ ] `cargo test --workspace` (default features) — green, neural core covered via FakeEmbedder
- [ ] `cargo clippy -p cairn-infra -p cairn-startup --features neural --locked --all-targets` — green
- [ ] `cargo test -p cairn-infra --features neural` — green (model test ignored)
- [ ] `cargo deny --all-features check` — advisories/licenses/sources ok (bans may warn)
- [ ] Open PR against `main`; `gh pr merge --auto --squash`. Do not manually update the branch (merge queue).

## Self-review notes (spec coverage)

- Spec §1 feature gating → Task 1. §2 module/seam/split-gating → Tasks 2 & 4. §3 ranking + C-full `why` → Tasks 2 & 3. §4 startup fallback → Task 5. §5 tests (offline core + ignored model) → Tasks 2/3/4. §6 pre-flight (MSRV, licenses, Cargo.lock) → Task 1; CI coverage → Task 6.
- Types are consistent across tasks: `Embedder::embed_tokens`, `NoteEmbedding{pooled,tokens}`, `NeuralSemanticIndex<E>`, `dot`, `unit_normalize`, `attribute`, `minilm_weights_path`, `CandleMiniLm` are defined once and referenced with the same signatures throughout.
- Open version-sensitivity flagged inline (candle-transformers `forward` arity; `config.hidden_size` access) — resolved against the version pinned in Task 1.
