# Tantivy Full-Text Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the in-memory substring `SearchIndex` with a Tantivy-backed adapter giving relevance-ranked, n-gram (substring-preserving) full-text search, with each result carrying a score and a highlighted snippet.

**Architecture:** A new `TantivyIndex` adapter implements the existing `SearchIndex` port (RAM directory now, on-disk later via Tantivy's `Directory` seam). `SearchHit` gains `score`/`snippet`/`highlights`; the contract gains a `SearchResult` DTO and a `QueryResponse::SearchResults` variant. The `Engine` and dispatcher are otherwise unchanged. `InMemoryIndex` is kept as a fast test double.

**Tech Stack:** Rust (MSRV bumped 1.85 → 1.88), Tantivy 0.26.1, ts-rs (TS bindings), nextest.

**Reference (validated):** The exact Tantivy 0.26 API used here was compile-and-test-verified in a probe. The adapter code in Task 3 is that verified code adapted to the port types. Key 0.26 gotcha: `TopDocs::with_limit(n)` is NOT a `Collector` by itself — you must call `.order_by_score()`.

**Branch:** `feat/tantivy-search` (already created; the design spec is committed there).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `rust-toolchain.toml` | MSRV pin 1.85 → 1.88 | 1 |
| `Cargo.toml` (workspace) | add `tantivy = "0.26"` workspace dep | 1 |
| `crates/cairn-infra/Cargo.toml` | depend on tantivy | 1 |
| `.github/workflows/ci.yml` | relabel MSRV job 1.85 → 1.88 | 1 |
| `crates/cairn-ports/src/lib.rs` | enrich `SearchHit` (score/snippet/highlights) | 2 |
| `crates/cairn-infra/src/index.rs` | update `InMemoryIndex` to new `SearchHit` | 2 |
| `crates/cairn-app/src/lib.rs` | fix search-test assertions to compare paths | 2 |
| `crates/cairn-infra/src/tantivy_index.rs` | NEW `TantivyIndex` adapter | 3 |
| `crates/cairn-infra/src/lib.rs` | export `TantivyIndex` | 3 |
| `crates/cairn-cli/src/main.rs` | wire `TantivyIndex`; snippet output | 4, 6 |
| `crates/cairn-daemon/src/main.rs` + `src/lib.rs` | wire `TantivyIndex` | 4 |
| `crates/cairn-contract/src/lib.rs` | `SearchResult` + `QueryResponse::SearchResults` | 5 |
| `crates/cairn-service/src/lib.rs` | dispatch `Search` → `SearchResults` | 6 |
| `crates/cairn-daemon/tests/http.rs` | search HTTP test → `search_results` | 6 |
| `crates/cairn-cli/tests/cli.rs` | search CLI test → snippet output | 6 |
| `docs/handoffs/2026-06-01-ui-session-handoff.md` | document search result shape | 7 |

Each task ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass before committing.

**Note on the MSRV bump:** after editing `rust-toolchain.toml` to 1.88, the first `cargo` invocation triggers `rustup` to download the 1.88 toolchain. This is expected; let it install.

**Commit convention:** end commit messages with the project trailer if your tooling adds it; use `git -c commit.gpgsign=false commit` if signing fails.

---

### Task 1: Toolchain, dependency, and CI prep

**Files:**
- Modify: `rust-toolchain.toml`
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/cairn-infra/Cargo.toml`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Bump the MSRV pin**

In `rust-toolchain.toml`, change the channel:
```toml
[toolchain]
channel = "1.88"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2: Add the workspace tantivy dependency**

In the root `Cargo.toml`, under `[workspace.dependencies]`, add (keep the list tidy — place near the other infra deps like `git2`):
```toml
tantivy = "0.26"
```

- [ ] **Step 3: Depend on tantivy in cairn-infra**

In `crates/cairn-infra/Cargo.toml`, under `[dependencies]`, add:
```toml
tantivy = { workspace = true }
```

- [ ] **Step 4: Relabel the MSRV references in CI (cosmetic, for honesty)**

In `.github/workflows/ci.yml`, update the `locked-check` job's comment and name from `1.85` to `1.88`:
```yaml
  locked-check:
    # Guards the Cargo.lock MSRV pin: a `cargo update` / Dependabot bump
    # that pulls a crate raising MSRV above the pinned Rust 1.88 fails
    # here loudly instead of silently. rust-toolchain.toml pins 1.88, so
    # this is also the MSRV build check.
    name: locked-check (MSRV 1.88)
```
(The required branch-protection check is `ci-summary`, which is unchanged, so this rename is safe.)

- [ ] **Step 5: Build the workspace to fetch 1.88 + tantivy and refresh the lockfile**

Run: `cargo build --workspace`
Expected: rustup installs 1.88 if missing; tantivy 0.26.1 and its tree compile; `Cargo.lock` updates. No code changes yet, so it builds clean.

- [ ] **Step 6: Verify cargo-deny passes with the existing allowlist**

Run: `cargo deny check licenses 2>&1 | tail -20` (install with `cargo install cargo-deny` if absent — under 1.88 the latest works).
Expected: PASS. `deny.toml` already allows `Unicode-3.0`, `Zlib`, `MPL-2.0`, `ISC`, `Apache-2.0 WITH LLVM-exception`; the rest of the Tantivy tree resolves to MIT/Apache via OR expressions. If — and only if — it flags a specific standalone license, add that exact SPDX id to the `allow` list in `deny.toml` with a comment naming the crate, and re-run.

- [ ] **Step 7: Confirm fmt + clippy clean, then commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add rust-toolchain.toml Cargo.toml Cargo.lock crates/cairn-infra/Cargo.toml .github/workflows/ci.yml
git commit -m "build: bump MSRV to 1.88 and add tantivy 0.26 dependency"
```

---

### Task 2: Enrich `SearchHit` and update `InMemoryIndex`

This changes the port's result struct. To keep the workspace compiling, the same task updates the only `SearchHit` producer (`InMemoryIndex`) and the only test that constructs a `SearchHit` literal (`cairn-app`).

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (the `SearchHit` struct)
- Modify: `crates/cairn-infra/src/index.rs` (`InMemoryIndex::search` + its tests)
- Modify: `crates/cairn-app/src/lib.rs` (the `write_then_search_and_backlinks` assertion)

- [ ] **Step 1: Enrich `SearchHit` in the port**

In `crates/cairn-ports/src/lib.rs`, replace the `SearchHit` struct (currently `pub struct SearchHit { pub path: NotePath }` with its derives) with:
```rust
/// A single ranked search match.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The matching note.
    pub path: NotePath,
    /// Relevance score (BM25; higher is more relevant). Not normalized — use
    /// for relative ordering only.
    pub score: f32,
    /// A plain-text excerpt of the body around the best match. Empty if none.
    pub snippet: String,
    /// `(start, end)` byte ranges within `snippet` that matched, for UI
    /// highlighting. Half-open.
    pub highlights: Vec<(u32, u32)>,
}
```
Note: `SearchHit` no longer derives `Eq` or `Hash` (it now holds `f32`). If the previous derive line included `Eq`/`Hash`/`PartialOrd`/`Ord`, they are removed above intentionally.

- [ ] **Step 2: Run the ports build to confirm the struct compiles**

Run: `cargo build -p cairn-ports`
Expected: PASS.

- [ ] **Step 3: Update `InMemoryIndex::search` to populate the new fields**

In `crates/cairn-infra/src/index.rs`, add a snippet helper above the `impl SearchIndex for InMemoryIndex` block:
```rust
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
```
Then replace the `.map(|n| SearchHit { path: n.path.clone() })` closure in `search` with:
```rust
            .map(|n| SearchHit {
                path: n.path.clone(),
                score: 1.0,
                snippet: truncate_snippet(&n.body, 160),
                highlights: Vec::new(),
            })
```
Leave the filtering and `sort_by` (sort by path) unchanged.

- [ ] **Step 4: Update `InMemoryIndex` tests to the new `SearchHit` shape**

In the same file's `#[cfg(test)] mod tests`, the three tests construct `SearchHit { path: ... }` literals and compare with `assert_eq!`. Since exact equality now requires every field, change those assertions to compare **paths** instead. For example, in `matches_by_path_and_sorts_results`:
```rust
        let hits = idx.search("alpha").unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha.md", "zeta.md"]);
```
In `finds_by_body_substring_case_insensitive`:
```rust
        let hits = idx.search("hello").unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md"]);
```
In `upsert_then_remove`, replace the `assert_eq!(idx.search("target").unwrap(), vec![SearchHit{..}])` with a path check and keep the `is_empty()`/`len()` assertions:
```rust
        assert_eq!(
            idx.search("target").unwrap().iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md"]
        );
```
Add one new assertion at the end of `finds_by_body_substring_case_insensitive` to cover the enriched fields:
```rust
        let hit = &idx.search("hello").unwrap()[0];
        assert_eq!(hit.score, 1.0);
        assert!(hit.snippet.contains("Hello World"));
```

- [ ] **Step 5: Fix the `cairn-app` search assertion**

In `crates/cairn-app/src/lib.rs`, in `write_then_search_and_backlinks`, replace:
```rust
        assert_eq!(
            eng.search("target").unwrap(),
            vec![SearchHit { path: b.clone() }]
        );
```
with a path comparison:
```rust
        assert_eq!(
            eng.search("target").unwrap().iter().map(|h| &h.path).collect::<Vec<_>>(),
            vec![&b]
        );
```
If `SearchHit` is no longer otherwise referenced in that test module, remove it from the `use` line to avoid an unused-import warning (check the imports at the top of the `tests` module; `SearchHit` comes from `cairn_ports`).

- [ ] **Step 6: Run the affected crates' tests**

Run: `cargo test -p cairn-ports -p cairn-infra -p cairn-app`
Expected: all pass (InMemoryIndex tests updated; app search test compares paths).

- [ ] **Step 7: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: green (service still maps `Search → Paths` by reading `hit.path`, which is unaffected).
```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/index.rs crates/cairn-app/src/lib.rs
git commit -m "feat(ports): enrich SearchHit with score, snippet, highlights"
```

---

### Task 3: The `TantivyIndex` adapter

**Files:**
- Create: `crates/cairn-infra/src/tantivy_index.rs`
- Modify: `crates/cairn-infra/src/lib.rs` (add `mod` + re-export)

- [ ] **Step 1: Write the adapter (verified Tantivy 0.26 code)**

Create `crates/cairn-infra/src/tantivy_index.rs` with exactly:
```rust
//! Tantivy-backed `SearchIndex`: n-gram tokenized full-text search with BM25
//! ranking and snippet generation. Replaces `InMemoryIndex` in production.
//! In-memory (RamDirectory) today; an on-disk constructor can be added later
//! behind the same port via Tantivy's `Directory` seam.

use cairn_domain::{Note, NotePath};
use cairn_ports::{PortError, SearchHit, SearchIndex};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

const WRITER_HEAP: usize = 15_000_000;
const SEARCH_LIMIT: usize = 50;
const SNIPPET_MAX_CHARS: usize = 160;
/// Smallest n-gram (and minimum useful query length).
const NGRAM_MIN: usize = 2;

/// A Tantivy full-text index over note bodies and paths.
pub struct TantivyIndex {
    index: Index,
    reader: IndexReader,
    path: Field,
    path_text: Field,
    body: Field,
}

fn adapt<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Adapter(e.to_string())
}

impl TantivyIndex {
    /// Build an in-memory index (RamDirectory). Load notes via [`SearchIndex::reindex`].
    ///
    /// # Errors
    /// Returns [`PortError`] if the schema, tokenizer, or reader cannot be built.
    pub fn in_memory() -> Result<Self, PortError> {
        let mut sb = Schema::builder();
        let path = sb.add_text_field("path", STRING | STORED);
        let text_opts = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ngram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let path_text = sb.add_text_field("path_text", text_opts.clone());
        let body = sb.add_text_field("body", text_opts | STORED);
        let schema = sb.build();

        let index = Index::create_in_ram(schema);
        let ngram = NgramTokenizer::new(NGRAM_MIN, 3, false).map_err(adapt)?;
        let analyzer = TextAnalyzer::builder(ngram).filter(LowerCaser).build();
        index.tokenizers().register("ngram", analyzer);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(adapt)?;
        Ok(Self {
            index,
            reader,
            path,
            path_text,
            body,
        })
    }

    fn writer(&self) -> Result<IndexWriter, PortError> {
        self.index.writer(WRITER_HEAP).map_err(adapt)
    }

    fn term(&self, path: &NotePath) -> Term {
        Term::from_field_text(self.path, path.as_str())
    }
}

impl SearchIndex for TantivyIndex {
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_all_documents().map_err(adapt)?;
        for n in notes {
            let p = n.path.as_str().to_string();
            w.add_document(doc!(
                self.path => p.clone(),
                self.path_text => p,
                self.body => n.body.clone(),
            ))
            .map_err(adapt)?;
        }
        w.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        let q = query.trim();
        if q.chars().count() < NGRAM_MIN {
            return Ok(Vec::new());
        }
        let searcher = self.reader.searcher();
        let mut parser = QueryParser::for_index(&self.index, vec![self.body, self.path_text]);
        parser.set_conjunction_by_default();
        // Phrase-quote the query so its n-grams must be adjacent (substring
        // match); strip embedded quotes so the parser can't break out.
        let parsed = parser
            .parse_query(&format!("\"{}\"", q.replace('"', " ")))
            .map_err(adapt)?;
        let collector = TopDocs::with_limit(SEARCH_LIMIT).order_by_score();
        let top = searcher.search(&*parsed, &collector).map_err(adapt)?;

        let mut sg = SnippetGenerator::create(&searcher, &*parsed, self.body).map_err(adapt)?;
        sg.set_max_num_chars(SNIPPET_MAX_CHARS);

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let d: TantivyDocument = searcher.doc(addr).map_err(adapt)?;
            let path_str = d
                .get_first(self.path)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let path = NotePath::new(path_str).map_err(adapt)?;
            let snip = sg.snippet_from_doc(&d);
            let highlights = snip
                .highlighted()
                .iter()
                .map(|r| (r.start as u32, r.end as u32))
                .collect();
            hits.push(SearchHit {
                path,
                score,
                snippet: snip.fragment().to_string(),
                highlights,
            });
        }
        Ok(hits)
    }

    fn upsert(&mut self, note: &Note) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_term(self.term(&note.path));
        let p = note.path.as_str().to_string();
        w.add_document(doc!(
            self.path => p.clone(),
            self.path_text => p,
            self.body => note.body.clone(),
        ))
        .map_err(adapt)?;
        w.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
    }

    fn remove(&mut self, path: &NotePath) -> Result<(), PortError> {
        let mut w = self.writer()?;
        w.delete_term(self.term(path));
        w.commit().map_err(adapt)?;
        self.reader.reload().map_err(adapt)
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
    fn ranks_and_matches_substring() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.reindex(&[
            note("a.md", "the borrow checker enforces ownership"),
            note("b.md", "ownership ownership ownership rules"),
        ])
        .unwrap();

        let hits = idx.search("ownership").unwrap();
        assert!(!hits.is_empty());
        // b.md mentions the term more → should rank at/near the top.
        assert_eq!(hits[0].path.as_str(), "b.md");
        // snippet for the top hit contains the matched term.
        assert!(hits[0].snippet.to_lowercase().contains("ownership"));
        // every highlight range lies within its snippet.
        for h in &hits {
            for (s, e) in &h.highlights {
                assert!(*s <= *e && (*e as usize) <= h.snippet.len());
            }
        }

        // mid-word substring still matches via n-grams.
        let sub = idx.search("nersh").unwrap();
        assert!(sub.iter().any(|h| h.path.as_str() == "a.md"));

        // sub-2-char query returns nothing.
        assert!(idx.search("o").unwrap().is_empty());
        assert!(idx.search("  ").unwrap().is_empty());
    }

    #[test]
    fn matches_by_path() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.reindex(&[note("rust-notes.md", "unrelated body text")])
            .unwrap();
        assert!(idx
            .search("rust")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "rust-notes.md"));
    }

    #[test]
    fn upsert_replaces_and_remove_deletes() {
        let mut idx = TantivyIndex::in_memory().unwrap();
        idx.upsert(&note("a.md", "hello target")).unwrap();
        assert!(idx
            .search("target")
            .unwrap()
            .iter()
            .any(|h| h.path.as_str() == "a.md"));

        idx.upsert(&note("a.md", "changed now")).unwrap();
        assert!(idx.search("target").unwrap().is_empty());
        assert!(!idx.search("changed").unwrap().is_empty());

        idx.remove(&NotePath::new("a.md").unwrap()).unwrap();
        assert!(idx.search("changed").unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Export the adapter**

In `crates/cairn-infra/src/lib.rs`, add the module and re-export alongside the existing ones (it currently has `mod index;` and `pub use index::InMemoryIndex;`):
```rust
mod tantivy_index;
pub use tantivy_index::TantivyIndex;
```

- [ ] **Step 3: Run the adapter tests**

Run: `cargo test -p cairn-infra tantivy_index`
Expected: 3 tests pass.

- [ ] **Step 4: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: green (adapter exists but is not yet wired into production).
```bash
git add crates/cairn-infra/src/tantivy_index.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): TantivyIndex full-text SearchIndex adapter"
```

---

### Task 4: Wire production (CLI + daemon) to `TantivyIndex`

No wire-shape change here — search still returns `Paths` (the dispatcher reads `hit.path`). This swaps the concrete index so CLI/daemon now use Tantivy; their existing search tests still pass.

**Files:**
- Modify: `crates/cairn-cli/src/main.rs`
- Modify: `crates/cairn-daemon/src/main.rs`
- Modify: `crates/cairn-daemon/src/lib.rs`

- [ ] **Step 1: CLI**

In `crates/cairn-cli/src/main.rs`:
- Change the import `use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};` to `use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};`.
- Change `build_engine`'s return type `Engine<LocalFsStore, InMemoryIndex, GitVcs>` to `Engine<LocalFsStore, TantivyIndex, GitVcs>`.
- Change the body line `Ok(Engine::new(store, InMemoryIndex::default(), vcs))` to:
```rust
    let index = TantivyIndex::in_memory().map_err(|e| e.to_string())?;
    Ok(Engine::new(store, index, vcs))
```

- [ ] **Step 2: Daemon `main.rs`**

In `crates/cairn-daemon/src/main.rs`:
- Change `use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore, NotifyWatcher};` to `use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex};`.
- Change `Ok(Engine::new(store, InMemoryIndex::default(), vcs))` to:
```rust
    let index = TantivyIndex::in_memory().map_err(|e| e.to_string())?;
    Ok(Engine::new(store, index, vcs))
```
`build_engine` returns `Result<CairnEngine, String>`, so `.map_err(|e| e.to_string())?` is correct, and its return type updates automatically when the `CairnEngine` alias changes in Step 3.

- [ ] **Step 3: Daemon `lib.rs` type alias + test helper**

In `crates/cairn-daemon/src/lib.rs`:
- Change `use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};` to `use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};`.
- Change `pub type CairnEngine = Engine<LocalFsStore, InMemoryIndex, GitVcs>;` to:
```rust
pub type CairnEngine = Engine<LocalFsStore, TantivyIndex, GitVcs>;
```

- [ ] **Step 4: Update the daemon test `state()` helper**

In `crates/cairn-daemon/tests/http.rs`, the `state()` helper builds `InMemoryIndex::default()`. Change its import and body to use `TantivyIndex`:
```rust
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};
// ...
fn state(dir: &std::path::Path) -> AppState {
    let engine = Engine::new(
        LocalFsStore::open(dir).unwrap(),
        TantivyIndex::in_memory().unwrap(),
        GitVcs::open_or_init(dir).unwrap(),
    );
    AppState::new(engine)
}
```

- [ ] **Step 5: Run CLI + daemon tests**

Run: `cargo test -p cairn-cli -p cairn-daemon`
Expected: pass. The existing `write_then_search_over_http` (writes `"hello target"`, searches `"target"`) and the CLI `write_search_backlinks_commit_flow` (`search target` → contains `b.md`) still match under Tantivy, and still return the `paths` shape (unchanged dispatcher).

- [ ] **Step 6: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: green.
```bash
git add crates/cairn-cli/src/main.rs crates/cairn-daemon/src/main.rs crates/cairn-daemon/src/lib.rs crates/cairn-daemon/tests/http.rs
git commit -m "feat(cli,daemon): use TantivyIndex in production"
```

---

### Task 5: Contract — `SearchResult` + `QueryResponse::SearchResults`

Additive: the new variant compiles without any consumer change (the dispatcher still emits `Paths` until Task 6).

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- (Regenerated) `crates/cairn-contract/bindings/*.ts`

- [ ] **Step 1: Add the `SearchResult` DTO**

In `crates/cairn-contract/src/lib.rs`, add near the other result structs (e.g. after `NoteSummary`):
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
    /// `(start, end)` byte ranges within `snippet` that matched.
    pub highlights: Vec<(u32, u32)>,
}
```
`SearchResult` intentionally omits `Eq` (it holds `f32`).

- [ ] **Step 2: Add the `SearchResults` variant and drop `Eq` from `QueryResponse`**

In the `QueryResponse` enum's derive line, remove `Eq` (keep `Debug, Clone, PartialEq, Serialize, Deserialize, TS`). Then add the variant (e.g. after `Paths`):
```rust
    /// Ranked search results (response to `Search`).
    SearchResults {
        /// Best match first.
        results: Vec<SearchResult>,
    },
```

- [ ] **Step 3: Add a contract round-trip test**

In the `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn search_results_roundtrip() {
        let r = QueryResponse::SearchResults {
            results: vec![SearchResult {
                path: "a.md".into(),
                score: 1.5,
                snippet: "hello target".into(),
                highlights: vec![(6, 12)],
            }],
        };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"type\":\"search_results\""));
        assert_eq!(serde_json::from_str::<QueryResponse>(&j).unwrap(), r);
    }
```

- [ ] **Step 4: Run the contract tests (this regenerates the TS bindings)**

Run: `cargo test -p cairn-contract`
Expected: pass. ts-rs writes `crates/cairn-contract/bindings/SearchResult.ts` and updates `QueryResponse.ts`.

- [ ] **Step 5: Verify the generated binding**

Run: `grep -n "search_results\|SearchResult" crates/cairn-contract/bindings/QueryResponse.ts crates/cairn-contract/bindings/SearchResult.ts`
Expected: `QueryResponse.ts` includes a `{ "type": "search_results", results: Array<SearchResult> }` arm; `SearchResult.ts` exists with `path/score/snippet/highlights`, where `highlights` is `Array<[number, number]>`.

- [ ] **Step 6: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: green.
```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-contract/bindings/
git commit -m "feat(contract): SearchResult + QueryResponse::SearchResults"
```

---

### Task 6: Switch consumers to `SearchResults`

Atomic wire-shape switch: the dispatcher, CLI, and daemon move from `Paths` to `SearchResults` together, with their tests.

**Files:**
- Modify: `crates/cairn-service/src/lib.rs`
- Modify: `crates/cairn-cli/src/main.rs`
- Modify: `crates/cairn-cli/tests/cli.rs`
- Modify: `crates/cairn-daemon/tests/http.rs`

- [ ] **Step 1: Dispatcher — map `Search` to `SearchResults`**

In `crates/cairn-service/src/lib.rs`, add `SearchResult` to the `cairn_contract` import list. Replace the `Query::Search` arm in `dispatch_query`:
```rust
        Query::Search { query } => {
            let results = engine
                .search(query)?
                .into_iter()
                .map(|h| SearchResult {
                    path: h.path.as_str().to_string(),
                    score: h.score,
                    snippet: h.snippet,
                    highlights: h.highlights,
                })
                .collect();
            Ok(QueryResponse::SearchResults { results })
        }
```

- [ ] **Step 2: Update the dispatcher's search test**

In the same file's tests, the `write_commit_and_query_roundtrip` test asserts search returns `QueryResponse::Paths { paths: vec!["a.md"] }`. Replace that block with a `SearchResults` assertion:
```rust
        let search = dispatch_query(
            &eng,
            &Query::Search {
                query: "target".into(),
            },
        )
        .unwrap();
        match search {
            QueryResponse::SearchResults { results } => {
                assert!(results.iter().any(|r| r.path == "a.md"));
            }
            other => panic!("expected SearchResults, got {other:?}"),
        }
```

- [ ] **Step 3: CLI — print path + snippet**

In `crates/cairn-cli/src/main.rs`, replace the `Command::Search` arm:
```rust
        Command::Search { query } => {
            if let QueryResponse::SearchResults { results } =
                dispatch_query(&engine, &WireQuery::Search { query }).map_err(|e| e.to_string())?
            {
                for r in results {
                    println!("{}", r.path);
                    if !r.snippet.is_empty() {
                        println!("    {}", r.snippet);
                    }
                }
            }
        }
```

- [ ] **Step 4: Update the CLI search integration test**

In `crates/cairn-cli/tests/cli.rs`, the existing `write_search_backlinks_commit_flow` already asserts `search target` → contains `b.md`; that still holds (path line). Add a focused test for the snippet output:
```rust
#[test]
fn search_prints_path_and_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    cairn(dir).arg("init").assert().success();
    cairn(dir)
        .args(["write", "a.md", "the borrow checker enforces ownership"])
        .assert()
        .success();
    cairn(dir)
        .args(["search", "ownership"])
        .assert()
        .success()
        .stdout(contains("a.md"))
        .stdout(contains("ownership"));
}
```

- [ ] **Step 5: Update the daemon HTTP search test**

In `crates/cairn-daemon/tests/http.rs`, in `write_then_search_over_http`, change the search assertions from the `paths` shape to `search_results`:
```rust
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "search_results");
    assert_eq!(body["results"][0]["path"], "a.md");
```

- [ ] **Step 6: Run the affected crates' tests**

Run: `cargo test -p cairn-service -p cairn-cli -p cairn-daemon`
Expected: pass.

- [ ] **Step 7: Whole-workspace gate + commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: green.
```bash
git add crates/cairn-service/src/lib.rs crates/cairn-cli/src/main.rs crates/cairn-cli/tests/cli.rs crates/cairn-daemon/tests/http.rs
git commit -m "feat: return ranked SearchResults (score + snippet) from search"
```

---

### Task 7: Handoff doc + final gate

**Files:**
- Modify: `docs/handoffs/2026-06-01-ui-session-handoff.md`

- [ ] **Step 1: Update the contract TS block**

In `docs/handoffs/2026-06-01-ui-session-handoff.md`, in the `QueryResponse` TS block, change the `search` mapping line. Replace:
```ts
  | { type: "paths"; paths: string[] }          // <- search, get_backlinks, notes_by_tag
```
with:
```ts
  | { type: "paths"; paths: string[] }          // <- get_backlinks, notes_by_tag
  | { type: "search_results"; results: SearchResult[] } // <- search (ranked)
```
And add the interface near `NoteSummary`/`GraphEdge`:
```ts
interface SearchResult { path: string; score: number; snippet: string; highlights: [number, number][] }
```

- [ ] **Step 2: Update the query→response table**

In the "Query | success response" table, change the `search` row from `paths { paths }` to `search_results { results }` (leave `get_backlinks` and `notes_by_tag` as `paths`).

- [ ] **Step 3: Update the capabilities + curl examples**

In §2, update the Search capability line to note it is now ranked full-text with snippets:
```
- **Search:** n-gram full-text over body + path, BM25-ranked, with per-result score
  and highlighted snippet (Tantivy, in-memory index rebuilt on startup).
```
Update the `search` curl example response in §4a (if present) from a `paths` body to a `search_results` body, e.g.:
```
# {"type":"search_results","results":[{"path":"a.md","score":1.2,"snippet":"…","highlights":[[0,5]]}]}
```

- [ ] **Step 4: Final whole-workspace gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add docs/handoffs/2026-06-01-ui-session-handoff.md
git commit -m "docs(handoff): document ranked search_results shape"
```

---

## Notes for the implementer

- **Tantivy query semantics:** the adapter phrase-quotes the query over n-gram-tokenized fields, so a contiguous substring scores highest. Scattered n-grams can still match weakly — acceptable for a note search bar (documented out-of-scope to refine).
- **`order_by_score()` is mandatory** in Tantivy 0.26 — `TopDocs::with_limit(n)` alone does not implement `Collector`.
- **Snippet field must be `STORED`** — `SnippetGenerator` reads the stored `body`.
- **Do not re-sort** Tantivy results by path; preserve BM25 order.
- **`InMemoryIndex` stays** — it is the fast, deterministic double for the `cairn-app` and `cairn-service` unit tests; only production (CLI/daemon) and the daemon HTTP test use `TantivyIndex`.
- **MSRV:** the first build after Task 1 installs the 1.88 toolchain via `rust-toolchain.toml`.
