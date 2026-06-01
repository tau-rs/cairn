# Tantivy Full-Text Search — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main`.

---

## 1. Goal

Replace the in-memory substring `SearchIndex` with a **Tantivy**-backed adapter that
gives **relevance-ranked** full-text search while **preserving mid-word substring
matching** (today's behavior — `ell` matches `hello`). Results gain a **relevance
score** and a **highlighted snippet** so a UI can render search previews. The swap is
behind the existing `SearchIndex` port, so the app/contract orchestration is unchanged
except for the enriched result shape.

This is a single sub-project: one new adapter, one enriched result DTO, and the wiring
to use it.

---

## 2. Decisions (locked during brainstorming)

1. **Storage:** design for both RAM and on-disk; **implement RAM only now**. Tantivy's
   `Directory` trait is the seam (`RamDirectory` now, `MmapDirectory` later).
2. **Matching:** **n-gram tokenizer** (substring/prefix preserved) **+ BM25 ranking** —
   no regression from today's substring search.
3. **Results:** **enriched** — each result carries `score` + `snippet` (+ highlight
   ranges). New `QueryResponse` variant.
4. **MSRV:** **bump 1.85 → 1.88** and use **Tantivy 0.26.1** (latest). Verified: 0.26.1
   declares rustc 1.86; some transitive deps need 1.88, so 1.88 is the floor.

---

## 3. Hexagonal layering

| Layer | Adds / changes | Depends on |
|---|---|---|
| ports | `SearchHit` enriched with `score`, `snippet`, `highlights` | domain |
| infra | new `TantivyIndex` adapter; `InMemoryIndex` updated to new `SearchHit` | ports, tantivy |
| app | none (`Engine::search` passes `SearchHit`s through) | ports |
| contract | `SearchResult` DTO + `QueryResponse::SearchResults` | — |
| service | `Query::Search` arm builds `SearchResults` | app, contract |
| cli | `cairn search` prints path + snippet | service, contract |
| daemon | wire `TantivyIndex`; search returns `search_results` | service |

The `SearchIndex` **trait signature is unchanged** — only the `SearchHit` struct it
returns grows. Both adapters (`TantivyIndex`, `InMemoryIndex`) implement the same port.

---

## 4. Ports (`cairn-ports`) — enriched `SearchHit`

```rust
/// A single search match, ranked.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The matching note.
    pub path: NotePath,
    /// Relevance score (BM25; higher is more relevant). Not normalized — use for
    /// relative ordering only.
    pub score: f32,
    /// A short excerpt of the body around the best match (plain text, no markup).
    /// Empty if no snippet could be generated.
    pub snippet: String,
    /// Byte ranges within `snippet` that matched the query, for UI highlighting.
    /// Each is `(start, end)`, half-open, in bytes.
    pub highlights: Vec<(u32, u32)>,
}
```
`SearchHit` drops `Eq` (it now holds `f32`); it derives `PartialEq` for tests. The
`SearchIndex` trait (`reindex`/`search`/`upsert`/`remove`) is otherwise unchanged. A
`(u32, u32)` tuple (not `[u32; 2]`) is used so the mirrored contract DTO maps cleanly
through ts-rs (tuple → `[number, number]`).

---

## 5. Infra (`cairn-infra`) — `TantivyIndex`

### 5.1 Schema and tokenizer
- Fields:
  - `path` — `STRING | STORED` (exact term, used as the delete key and returned verbatim).
  - `path_text` — `TEXT` with the n-gram tokenizer (so path text is substring-searchable,
    matching today's "match by path" behavior).
  - `body` — `TEXT | STORED` with the n-gram tokenizer (stored so the `SnippetGenerator`
    can read it back).
- Tokenizer: register `NgramTokenizer::new(2, 3, false)` (2- and 3-grams, not prefix-only)
  behind a lowercasing filter, registered under a name (e.g. `"ngram"`) and set as both
  the indexing and query tokenizer for `body`/`path_text`.

### 5.2 Constructors (the storage seam)
```rust
impl TantivyIndex {
    /// In-memory index (RamDirectory), rebuilt from notes via `reindex`.
    pub fn in_memory() -> Result<Self, PortError>;
    // Future (NOT built now): pub fn open_at(dir: &Path) -> Result<Self, PortError>;
}
```
`in_memory` builds the schema, registers the tokenizer, creates `Index::create_in_ram`,
and opens an `IndexReader` with `ReloadPolicy::OnCommitWithDelay`.

### 5.3 Operations (all map Tantivy errors → `PortError::Adapter`)
- `reindex(notes)`: open an `IndexWriter` (fixed heap budget, e.g. 15 MB),
  `delete_all_documents()`, add one document per note, `commit()`.
- `upsert(note)`: `delete_term(path_term)`, add the document, `commit()`.
- `remove(path)`: `delete_term(path_term)`, `commit()`.
- `search(query)`:
  1. If `query` trimmed is shorter than the n-gram min (2 chars) → return `vec![]`.
  2. Tokenize the query with the same n-gram tokenizer; build a `BooleanQuery` requiring
     the query's n-gram terms (over `body` and `path_text`), so a contiguous substring
     scores highest. (Approximate substring: scattered n-grams can match — acceptable
     noise for a note search bar.)
  3. Collect `TopDocs::with_limit(50)`.
  4. For each hit: read stored `path`; run a `SnippetGenerator` on `body` to produce the
     excerpt and its highlighted ranges → fill `snippet` + `highlights` (map Tantivy's
     highlighted ranges to `[u32; 2]`). Snippet-gen failure → empty snippet, non-fatal.
  5. Return `SearchHit`s in Tantivy's ranked order (do **not** re-sort by path).

`#[forbid(unsafe_code)]` holds — the adapter writes no `unsafe`; Tantivy's internal
`unsafe` is a dependency concern, not ours.

### 5.4 `InMemoryIndex` (kept as a test double)
Updated to the new `SearchHit`: `score = 1.0` constant, `snippet` = a fixed-width excerpt
of the body around the first substring match (empty if matched only by path), `highlights`
= the single matched range within that excerpt (or empty). Still sorts by path. It remains
the index used by app/service unit tests (fast, deterministic); `TantivyIndex` is the
production adapter and carries the search-quality tests.

---

## 6. Contract (`cairn-contract`)

```rust
/// One ranked search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchResult {
    /// Relative note path.
    pub path: String,
    /// Relevance score (relative ordering only).
    pub score: f32,
    /// Plain-text excerpt around the match.
    pub snippet: String,
    /// `(start, end)` byte ranges within `snippet` that matched, for highlighting.
    pub highlights: Vec<(u32, u32)>,
}
```
- `QueryResponse` gains:
  ```rust
  /// Ranked search results (response to `Search`).
  SearchResults {
      /// Best match first.
      results: Vec<SearchResult>,
  },
  ```
- `Query::Search` now maps to `SearchResults` (was `Paths`). `GetBacklinks` and
  `NotesByTag` keep returning `Paths`. Regenerate the TS bindings
  (`cargo test -p cairn-contract`).

`SearchResult` holds `f32`, so it derives `PartialEq` but **not** `Eq`. Consequently
`QueryResponse` must also **drop its `Eq` derive** (keeping `Debug, Clone, PartialEq,
Serialize, Deserialize, TS`) — `assert_eq!` in tests only needs `PartialEq`, so nothing
breaks. `ts-rs` renders `(u32, u32)` as the tuple `[number, number]`.

---

## 7. Dispatcher (`cairn-service`)

`dispatch_query` `Query::Search` arm: call `engine.search(query)?`, map each `SearchHit`
to a `SearchResult` (`path.as_str().to_string()`, `score`, `snippet`, `highlights`),
return `QueryResponse::SearchResults { results }`. No change to `From<PortError>`.

---

## 8. CLI (`cairn-cli`)

`cairn search <query>` prints, per result, the path then an indented snippet line:
```
notes/rust.md
    ...the borrow checker enforces ownership...
```
(Snippet omitted when empty.) Implementation reads `QueryResponse::SearchResults`.

---

## 9. Wiring

Replace `InMemoryIndex::default()` with `TantivyIndex::in_memory()?` at the three
production construction sites:
- `crates/cairn-cli/src/main.rs` `build_engine`
- `crates/cairn-daemon/src/main.rs`
- `crates/cairn-daemon/src/lib.rs` — the `CairnEngine` type alias becomes
  `Engine<LocalFsStore, TantivyIndex, GitVcs>`, and the test `state()` helper.

The daemon HTTP search test updates from expecting `paths` to `search_results`.

---

## 10. Project / toolchain changes

- `rust-toolchain.toml`: `1.85` → `1.88`.
- `.github/workflows/ci.yml`: relabel the `locked-check (MSRV 1.85)` job to
  `MSRV 1.88` and set its toolchain to `1.88`.
- Workspace `Cargo.toml`: add `tantivy = "0.26"`; `crates/cairn-infra/Cargo.toml` depends
  on it.
- `deny.toml`: add `Unicode-3.0`, `Zlib`, `Unlicense` to the license allowlist (the
  Tantivy tree is otherwise all MIT/Apache/BSD; ~168 new transitive crates, all
  permissive — no copyleft-only deps). Remove the obsolete "pin cargo-deny 0.18.3 for
  1.85" note in project docs/memory.
- Expect a noticeably longer CI build (Tantivy is heavy).

---

## 11. Testing

- **infra `TantivyIndex`:** ngram substring match (`ell` finds `hello`); BM25 ordering
  (a note with the term in body/title ranks above an incidental match); match by path;
  `upsert` replaces (old content no longer found, new found); `remove` deletes; snippet
  contains the matched term; every `highlights` range lies within its `snippet`;
  sub-2-char query → empty.
- **infra `InMemoryIndex`:** existing tests updated to the new `SearchHit` (assert
  `score`/`snippet` fields; behavior otherwise unchanged).
- **contract:** `QueryResponse::SearchResults` serde round-trip (tag `search_results`) +
  the TS binding includes `SearchResult`.
- **service:** dispatch `Search` → `SearchResults`; result fields populated.
- **app/service:** existing `search` assertions are updated to compare result **paths**
  (exact `SearchHit`/`QueryResponse` struct-equality no longer holds now that
  `score`/`snippet` are populated); the service `Search` test asserts `SearchResults`
  instead of `Paths`.
- **cli:** integration — write a note, `cairn search <term>`, assert the path and a
  snippet line print.
- **daemon:** HTTP `POST /query {"type":"search",...}` → 200 + `{"type":"search_results",
  ...}` with a populated `results` array.

---

## 12. Out of scope

On-disk persistence (designed via the `Directory` seam, not built); field-scoped,
boolean, or phrase query syntax (the query is treated as one substring/n-gram bag);
fuzzy/typo tolerance; configurable n-gram or snippet sizes; search result paging beyond
the fixed top-50; stemming/stop-words.
