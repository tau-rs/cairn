# Design: Neural on-device SemanticIndex adapter (Stream 3)

**Date:** 2026-07-19
**Branch:** `neural-semantic-index-adapter`
**Status:** Approved — ready for implementation plan

## Goal

Add a second `SemanticIndex` adapter backed by on-device neural embeddings
(MiniLM-class), selectable at startup, with **zero** changes to the port,
contract, queries, CLI, or daemon. The port was built for exactly this swap;
this stream exercises it.

Out of scope: the `SemanticIndex` trait, `GetSuggestions`, CLI, daemon — all
unchanged.

## Locked design decisions

| # | Decision | Choice |
|---|----------|--------|
| Testability | how the adapter is tested given weights can't be fetched offline | **A** — internal `Embedder` seam; index logic tested offline via a deterministic fake, real model behind the same trait |
| `why` provenance | what fills `Similarity.shared` | **C-full** — store per-token embeddings per note; cross-token nearest-neighbor attribution |
| Startup fallback | weights missing/corrupt when feature is on | **(i)** — `tracing::warn!` and fall back to `LexicalSemanticIndex`; engine always builds |
| Crate | candle vs ort | **candle** (`candle-core` + `candle-nn` + `candle-transformers` BERT) + `tokenizers` |
| Model | MiniLM-class | **all-MiniLM-L6-v2** (384-dim) |

## Constraints (from repo)

- MSRV **1.88**; `unsafe_code = "forbid"` at workspace level (applies to our
  crates, not deps).
- `deny.toml`: strict license allow-list; `[graph] all-features = false` — the
  neural dep tree is **not** license-scanned by default runs.
- Merge queue enabled: branch → PR → `gh pr merge --auto --squash`; no manual
  rebase / local merge. Shared working dir → operate via worktree + `git -C`.
- New dep ⇒ `git add Cargo.lock` in the same commit; keep cargo-deny green.
- DoD: `cargo test --workspace` + `cargo clippy --workspace --locked` +
  `cargo fmt --check` all green.

## 1. Feature gating

Keeps the default build dep-light and CI fast — candle only compiles when
explicitly enabled.

`crates/cairn-infra/Cargo.toml`:
```toml
[features]
neural = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:tokenizers", "dep:dirs"]
```
All candle/tokenizers/dirs deps are `optional = true`. `cairn-startup` gains a
`neural` feature that forwards to `cairn-infra/neural`. Default
`cargo build` / `cargo test --workspace` never compiles candle.

## 2. Module `crates/cairn-infra/src/semantic_neural.rs`

Gating is **split** so the index/attribution logic is exercised by the default
offline test run, while candle is compiled only under the feature.

```rust
// ── ALWAYS compiled (pure Rust, no deps) ─────────────────────────
/// text → per-UNIQUE-token unit embeddings; pooling is their mean.
trait Embedder {
    fn embed_tokens(&self, text: &str) -> Result<Vec<(String, Vec<f32>)>, PortError>;
    fn dim(&self) -> usize;
}

struct NoteEmbedding {
    pooled: Vec<f32>,                // unit-normalized mean → ranking
    tokens: Vec<(String, Vec<f32>)>, // unique tokens → C-full attribution
}

pub struct NeuralSemanticIndex<E: Embedder> {
    embedder: E,
    notes: HashMap<NotePath, NoteEmbedding>,
}
// impl SemanticIndex for NeuralSemanticIndex<E>: upsert/remove/reindex/neighbors.

// ── ONLY under `feature = "neural"` ──────────────────────────────
#[cfg(feature = "neural")]
pub struct CandleMiniLm { model: BertModel, tokenizer: Tokenizer, device: Device }

#[cfg(feature = "neural")]
impl NeuralSemanticIndex<CandleMiniLm> {
    /// Load config.json + tokenizer.json + model.safetensors from `dir`.
    pub fn open(dir: &Path) -> Result<Self, PortError> { /* ... */ }
}
```

- `NeuralSemanticIndex<E>` and `Embedder` are pure Rust and always compiled →
  no candle needed for the logic tests.
- `CandleMiniLm` and `open()` are gated; the startup neural path is therefore
  gated too, keeping default builds candle-free.

### Embedding lifecycle

- `upsert(note)`: `embed_tokens(body)` → dedupe repeated tokens (memory scales
  with **unique** tokens, per the C-full tradeoff), unit-normalize each token
  vector, `pooled = normalize(mean(token_vecs))`. Store `NoteEmbedding`.
- `remove(path)`: drop the entry.
- `reindex(notes)`: clear + upsert all.

### CandleMiniLm::embed_tokens

Tokenize → input_ids → BERT forward (CPU, f32, inference = deterministic) →
last hidden state `[1, seq, dim]` → per-token vectors, drop special tokens
(`[CLS]`/`[SEP]`/`[PAD]`), unit-normalize, dedupe by token string.

## 3. Ranking + C-full `why`

- **Rank:** `pooled` is unit-normalized ⇒ cosine = dot product. Sort desc,
  stable tie-break on `path`, skip `focus`, truncate `top_k`. Unknown `focus`
  → `Ok(vec![])`. Mirrors the lexical adapter's contract exactly.
- **`shared` (C-full attribution):** for each neighbor, score every focus token
  by its **max cosine to any neighbor token**; take the top `MAX_SHARED_TERMS`
  focus tokens above a small threshold.

```
focus "cat on the mat" ~ "feline on the rug"
  cat→0.71  mat→0.55  on→0.12  the→0.04   ⇒ shared = ["cat", "mat"]
  (surfaces the paraphrase bridge even with zero literal term overlap)
```

**Accepted cost of C-full:** per-token embeddings are stored per note
(~unique-tokens × dim × 4 B). Order-of-magnitude larger resident memory than a
pooled-only index; signed off as the price of token-level provenance.

## 4. Startup selection — `crates/cairn-startup/src/lib.rs:59`

```rust
#[cfg(not(feature = "neural"))]
fn semantic_index() -> Box<dyn SemanticIndex> {
    Box::new(LexicalSemanticIndex::new())
}

#[cfg(feature = "neural")]
fn semantic_index() -> Box<dyn SemanticIndex> {
    let path = weights_path(); // $CAIRN_MINILM_WEIGHTS else <cache>/cairn/models/all-MiniLM-L6-v2
    match NeuralSemanticIndex::open(&path) {
        Ok(ix) => Box::new(ix),
        Err(e) => {
            tracing::warn!(%e, path = %path.display(),
                "neural weights unavailable; using lexical");
            Box::new(LexicalSemanticIndex::new())
        }
    }
}
```

Weights path source: env `CAIRN_MINILM_WEIGHTS` if set, else a documented
default `<cache_dir>/cairn/models/all-MiniLM-L6-v2/` (via `dirs`). **No network
anywhere** — a missing file is simply the fallback branch.

## 5. Tests

**Offline, always run (part of DoD `cargo test --workspace`):**
`FakeEmbedder` produces deterministic token vectors over a tiny fixed vocab
constructed so that e.g. `cat` ≈ `feline`. Covers:
- determinism on a fixed note set,
- related notes rank first,
- unknown focus → `Ok(vec![])`,
- upsert-then-remove updates neighbors,
- reindex rebuilds,
- **attribution surfaces the bridge token** (C-full path).

**Gated + `#[ignore]` (opt-in):** real `CandleMiniLm` loaded from
`$CAIRN_MINILM_WEIGHTS`, asserts semantically-related paraphrases rank first.
CI opt-in job: `cargo test -p cairn-infra --features neural -- --ignored`.

## 6. Pre-flight gates (done FIRST, before writing adapter code)

1. **License vetting (primary risk).** Vet the candle/tokenizers dependency
   tree against `deny.toml`. Because `[graph] all-features = false`, run
   `cargo deny --all-features check licenses`; add allow-list entries with
   written rationale for each new license; add a feature-aware
   deny/clippy/test CI job so the neural tree stays covered.
2. **MSRV.** Confirm a candle version builds on **1.88** and pin it. If
   candle's MSRV exceeds 1.88, that is a blocker to surface before any code.
3. **Cargo.lock.** `git add Cargo.lock` in the dep-adding commit.

## Files touched

- `crates/cairn-infra/Cargo.toml` — `[features] neural`, optional deps
- `crates/cairn-infra/src/lib.rs` — module decl + conditional re-exports
- `crates/cairn-infra/src/semantic_neural.rs` — **new**
- `crates/cairn-startup/Cargo.toml` — `neural` feature forwarding
- `crates/cairn-startup/src/lib.rs` — selection logic at `build_engine`
- `deny.toml` + CI — license allow-list + feature-aware job
- `Cargo.lock`

**Untouched:** `cairn-ports` (the `SemanticIndex` trait), contract,
`GetSuggestions`, CLI, daemon.
