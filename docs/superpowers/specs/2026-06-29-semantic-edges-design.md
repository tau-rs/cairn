# AI-suggested semantic edges — design

**Date:** 2026-06-29
**Status:** approved (brainstorm)
**Workspace/branch:** `albany` / `semantic-edges-suggestions` (an isolated worktree off `main`)

## Summary

Surface notes that are **semantically related but not explicitly linked**, behind a
`GetSuggestions { scope }` query that returns `SuggestedEdge { from, to, weight, why }`.
This is Cairn's take on Obsidian "Smart Connections" (local, on-device related-notes),
with two deliberate differentiators:

1. **First-class** — suggested edges are part of the same graph the UI renders, not a
   bolted-on side panel.
2. **Explainable** — every edge populates `why` with the concrete signal behind it
   (the overlapping salient terms), not an opaque score.

The longer-term loop this design keeps open (but does **not** build here): AI suggests →
human accepts → acceptance writes a real `[[wikilink]]` → a git commit → permanently
queryable.

### Note on the "frozen seam"

The brief referenced an already-merged contract seam (`GetSuggestions` stub,
`2026-06-22-graph-contract-seam-design.md`, `.context/research-graph-viz.md`). None of
these exist in the repo. Per the decision recorded with the user, this design **authors
the contract types and the adapter together** rather than filling a pre-existing stub.
The contract shapes below are therefore defined here, not inherited.

## Decisions

| # | Decision | Choice |
|---|----------|--------|
| 1 | Embedding source | **Lexical TF-IDF + cosine**, pure Rust, no ML/network deps. Neural on-device (candle/ort + MiniLM) is a follow-up behind the *same* port. |
| 2 | `GraphScope` shape | Two-variant enum `Note { path }` \| `Vault`, both served by one `neighbors` primitive. |
| 3 | Storage | **In-memory, lazily built** from the engine note cache on first use; kept live via incremental upsert/remove. **No `.cairn/` persistence** in v1. |
| 4 | Tuning | Internal `const`s: `top_k = 5` per focus, similarity floor `0.1`, `Vault` = top-5/node deduped, global cap `100`. **Not** wire-exposed. |

## Architecture (hexagonal — dependencies point inward)

```
cairn-contract   + Query::GetSuggestions{scope}
  (wire DTOs)     + QueryResponse::Suggestions{suggestions}
                  + GraphScope { Note{path} | Vault }
                  + SuggestedEdge { from, to, weight, why }   → ts-rs bindings
       ▲
cairn-service    dispatch_query arm: GetSuggestions
                   → engine.suggestions(scope) → map Similarity → SuggestedEdge
       ▲
cairn-app        Engine gains `semantic: Box<dyn SemanticIndex + Send>`
  (Engine)         (default NullSemanticIndex), `set_semantic_index` setter,
                   `suggestions(scope)` use-case.
                   apply_write / apply_change / apply_removal also drive
                   semantic.upsert / semantic.remove.
       ▲
cairn-ports      trait SemanticIndex { upsert, remove, reindex, neighbors }
                 struct Similarity { path, score, shared }
       ▲
cairn-infra      LexicalSemanticIndex (TF-IDF + cosine) ; NullSemanticIndex (seam)
cairn-startup    composition root injects LexicalSemanticIndex via set_semantic_index
```

Engineering defaults honored: `thiserror` at boundaries / `anyhow` internally;
`forbid(unsafe_code)`; MSRV 1.88; the embedding/vector store is a new **port** with an
adapter in `cairn-infra`; the seam ships a neutral default (the
`NullRuntime` / `NoopPluginHost` pattern).

## Contract types (`cairn-contract`)

```rust
// New Query variant
Query::GetSuggestions { scope: GraphScope }

// New scope enum
#[derive(…, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphScope {
    /// Suggestions for one focus note (the Smart-Connections-style panel).
    Note { path: String },
    /// Top suggested edges across the whole vault (the graph-overlay case).
    Vault,
}

// New edge DTO
#[derive(…, Serialize, Deserialize, TS)]
pub struct SuggestedEdge {
    pub from: String,          // source note path
    pub to: String,            // target note path
    pub weight: f32,           // cosine similarity 0..1 — RANKING ONLY, never plotted as distance
    pub why: Option<String>,   // provenance, e.g. "shared: ownership, borrow, lifetime"
}

// New response variant
QueryResponse::Suggestions { suggestions: Vec<SuggestedEdge> }
```

Wire examples:

```json
{"type":"get_suggestions","scope":{"type":"note","path":"rust/ownership.md"}}
{"type":"get_suggestions","scope":{"type":"vault"}}
```

`weight` is a similarity **ranking**, not metric geometry — consistent with the brief's
UMAP/t-SNE caveat (a UI must never treat it as a plottable distance).

## Port (`cairn-ports`)

```rust
pub trait SemanticIndex {
    fn upsert(&mut self, note: &Note) -> Result<(), PortError>;
    fn remove(&mut self, path: &NotePath) -> Result<(), PortError>;
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError>;
    /// Notes most similar to `focus`, nearest first, with the terms behind each.
    fn neighbors(&self, focus: &NotePath, top_k: usize) -> Result<Vec<Similarity>, PortError>;
}

pub struct Similarity {
    pub path: NotePath,
    pub score: f32,           // cosine, 0..1
    pub shared: Vec<String>,  // top overlapping high-weight terms → feeds `why`
}
```

## Adapter (`cairn-infra`)

### `LexicalSemanticIndex`

```rust
struct LexicalSemanticIndex {
    tf: HashMap<NotePath, HashMap<String, u32>>, // per-note term counts
    df: HashMap<String, u32>,                    // corpus document frequency
    n: usize,                                    // doc count; IDF(term) = ln(n / df[term])
}
```

- Tokenization: lowercase, split on non-alphanumeric, drop a small stopword set and very
  short tokens. (Exact tokenizer is an implementation detail; tests pin behavior.)
- `reindex` rebuilds `tf`/`df`/`n` from all notes.
- `upsert`/`remove` adjust `tf` and `df` deltas so **IDF stays exact** between rebuilds —
  no full recompute per write.
- `neighbors(focus, k)`: build the IDF-weighted vector for `focus` and every other note,
  cosine-rank, take top-`k`. `shared` = the overlapping terms with the highest combined
  weight (capped to a few).
- Errors surface as `PortError::Adapter(AdapterError)`.

### `NullSemanticIndex` (seam)

`neighbors` → `Ok(vec![])`; `upsert`/`remove`/`reindex` → `Ok(())`. The engine's default so
it composes and runs before the real adapter is injected.

## Use-case (`cairn-app`)

`Engine` gains a `semantic` field holding `Box<dyn SemanticIndex + Send>` (default
`NullSemanticIndex`), a `set_semantic_index` setter (composition root injects the real one,
mirroring `set_plugin_host`), and:

```rust
pub fn suggestions(&self, scope: &Scope) -> Result<Vec<SuggestedEdgeData>, PortError>;
```

(`Scope` / `SuggestedEdgeData` are the app-layer equivalents; the service maps to/from the
wire DTOs, never importing `cairn-contract` into the app — the existing convention.)

Constants:

```rust
const SUGGEST_TOP_K: usize = 5;
const SUGGEST_FLOOR: f32 = 0.1;
const VAULT_EDGE_CAP: usize = 100;
```

Behavior:

- **Lazy build (interior mutability):** `dispatch_query` borrows `&Engine`, so
  `suggestions(&self)` must build under a shared borrow. The `semantic` field is therefore
  wrapped for interior mutability following the existing `notes_cache: RefCell<…>` precedent
  (a `RefCell` around the boxed index plus a "built" flag); the `&mut self` write paths use
  `get_mut`. First `suggestions` call uses `with_notes(...)` (loads + caches all notes once)
  to `reindex` the semantic index. This defers cost off the startup/warm-reconcile path so
  the daemon's read-skipping optimization is not defeated.
- **`Note { path }`:** `neighbors(path, TOP_K)`, then drop (a) self, (b) any note already
  joined to `path` by a forward link or backlink (computed via `Graph::build` over the note
  cache), (c) anything below `FLOOR`. Emit `SuggestedEdge { from: path, to: n.path, … }`.
- **`Vault`:** union of per-note `neighbors(_, 5)`, drop already-linked pairs + sub-floor,
  dedup to unique unordered pairs (canonical `from < to`), rank globally, cap `VAULT_EDGE_CAP`.
- `why = Some("shared: " + shared.join(", "))` when `shared` is non-empty, else `None`.

Incremental upkeep: `apply_write`, `apply_change`, `apply_removal` call
`semantic.upsert/remove` beside the existing `index.upsert/remove`, so the watcher,
self-writes, renames, and deletes keep it live automatically.

## Dispatch (`cairn-service`)

New `dispatch_query` arm:

```rust
Query::GetSuggestions { scope } => {
    let scope = map_scope(scope)?;                  // wire → app; NotFound/InvalidRequest mapping
    let suggestions = engine.suggestions(&scope)?   // PortError → ServiceError (existing chain)
        .into_iter().map(map_suggested_edge).collect();
    Ok(QueryResponse::Suggestions { suggestions })
}
```

## Error handling

- Port failures → `PortError` → `ServiceError` → `ContractError`, preserving the typed
  `#[source]` (the existing `AdapterError` chain; flattened to a message only at the wire).
- Invalid focus path → `InvalidRequest`.
- `Note` scope whose path does not exist → `NotFound`.
- `Vault` never errors on emptiness — returns `[]`.
- `NullSemanticIndex` default → every scope returns `[]`.

## Testing (part of done)

- **Contract** (`cairn-contract`): serde round-trip + wire-tag assertions for
  `Query::GetSuggestions`, `QueryResponse::Suggestions`, `GraphScope`, `SuggestedEdge`
  (the existing pattern in that crate); ts-rs bindings emitted.
- **Adapter** (`LexicalSemanticIndex`): same-topic notes rank above unrelated ones;
  `shared` names real overlapping terms; `upsert`/`remove` keep IDF exact (a removed note
  stops affecting scores); empty corpus → `[]`.
- **Use-case** (`Engine::suggestions`): an already-linked pair never appears; the focus
  note never suggests itself; a sub-floor note is excluded; `Vault` dedups pairs and
  respects the cap; lazy build happens once and stays live across a subsequent write.
  **Assert invariants, not the magic constants**, so tuning doesn't break the suite.
- **Dispatch** (`cairn-service`): `GetSuggestions` round-trips through `dispatch_query`;
  the `NullSemanticIndex` default yields `Suggestions { suggestions: [] }`.

## Out of scope (v1)

- Neural / candle-ort adapter (follow-up behind the same `SemanticIndex` port).
- Persistence of vectors to `.cairn/` (the neural follow-up's concern).
- The accept → `[[wikilink]]` → commit write-path. The design **does not preclude** it:
  `SuggestedEdge.from`/`.to` are note paths a future `AcceptSuggestion` command can consume.
- A `cairn.toml [suggestions]` config block (the `[plugins] timeout_secs` precedent makes
  it an easy follow-up).
```
