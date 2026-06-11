# CLI / UX Polish Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax. Executed inline in this session with TDD.

**Goal:** Fix three CLI papercuts — redundant full-vault reindex on every command (D2), silently non-idempotent `init` (D9), and silent empty results for sub-2-char queries (D11).

**Architecture:** All three are surfaced at the composition root (`cairn-cli/src/main.rs`) via small pure helpers that are unit-tested directly, plus one new `pub const` in `cairn-infra` so the query-length threshold has a single source of truth. No engine, daemon, contract, or search-semantics changes.

**Tech Stack:** Rust, clap, Tantivy (n-gram index), cargo/just test harness.

---

### Task 1: D2 — gate reindex behind a `needs_index` predicate

**Files:**
- Modify: `crates/cairn-cli/src/main.rs` (`run`, ~line 153-155) + new `needs_index`
- Test: `crates/cairn-cli/src/main.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing test** — `needs_index` true only for `Search`:

```rust
#[test]
fn only_search_needs_the_index() {
    assert!(needs_index(&Command::Search { query: "x".into() }));
    assert!(!needs_index(&Command::Read { path: "a.md".into() }));
    assert!(!needs_index(&Command::Commit { message: "m".into() }));
    assert!(!needs_index(&Command::Backlinks { path: "a.md".into() }));
    assert!(!needs_index(&Command::Init));
}
```

- [ ] **Step 2:** `cargo test -p cairn-cli only_search_needs_the_index` → FAIL (no `needs_index`).

- [ ] **Step 3: Implement**

```rust
/// Whether a command consults the full-text search index. Only `search` does;
/// every other command reads the store or the lazy notes-cache directly, so we
/// skip the O(vault) index build for them (audit D2).
fn needs_index(command: &Command) -> bool {
    matches!(command, Command::Search { .. })
}
```

And gate the reindex call in `run`:

```rust
    let mut engine = build_engine(&root)?;
    // Build the search index only for commands that query it (audit D2): a
    // full reindex is O(vault) work and a full disk read, wasted on a `read`,
    // `commit`, or `backlinks`.
    if needs_index(&cli.command) {
        engine.reindex(&mut events).map_err(|e| e.to_string())?;
    }
```

- [ ] **Step 4:** `cargo test -p cairn-cli only_search_needs_the_index` → PASS.

- [ ] **Step 5: Commit.**

### Task 2: D9 — distinct "already a cairn" init message

**Files:**
- Modify: `crates/cairn-cli/src/main.rs` (`run` guard + Init arm) + new `init_message`
- Test: `crates/cairn-cli/src/main.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn init_message_distinguishes_fresh_from_existing() {
    let p = Path::new("/tmp/v");
    assert_eq!(init_message(false, p), "initialized cairn at /tmp/v");
    assert_eq!(init_message(true, p), "already a cairn at /tmp/v");
}
```

- [ ] **Step 2:** `cargo test -p cairn-cli init_message_distinguishes` → FAIL.

- [ ] **Step 3: Implement** the pure formatter:

```rust
/// The message `init` prints. Distinguishes a freshly created cairn from a
/// re-run on an existing one so `init` is no longer silently a no-op (audit D9).
fn init_message(already: bool, root: &Path) -> String {
    if already {
        format!("already a cairn at {}", root.display())
    } else {
        format!("initialized cairn at {}", root.display())
    }
}
```

Capture `.git` existence once at the top of `run` (before `build_engine` runs `open_or_init`), reuse it in the guard and the Init arm:

```rust
    // `.git` is the cairn marker. Capture it before `build_engine`'s
    // `open_or_init` would create it, so `init` can tell created from no-op.
    let is_cairn = root.join(".git").exists();
    if !matches!(cli.command, Command::Init) && !is_cairn {
        return Err(format!(
            "not a cairn at {0} (run `cairn --cairn {0} init` first)",
            root.display()
        ));
    }
```

```rust
        Command::Init => {
            println!("{}", init_message(is_cairn, &root));
        }
```

- [ ] **Step 4:** `cargo test -p cairn-cli init_message_distinguishes` → PASS.

- [ ] **Step 5: Commit.**

### Task 3: D11 — surface a short-query hint

**Files:**
- Modify: `crates/cairn-infra/src/tantivy_index.rs` (expose `MIN_QUERY_CHARS`)
- Modify: `crates/cairn-infra/src/lib.rs` (re-export)
- Modify: `crates/cairn-cli/src/main.rs` (`Search` arm + new `short_query_hint`)
- Test: `crates/cairn-cli/src/main.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn short_query_yields_hint() {
    assert!(short_query_hint("a").is_some());
    assert!(short_query_hint("  ").is_some());
    assert!(short_query_hint("").is_some());
    assert!(short_query_hint("ab").is_none());
    assert!(short_query_hint("hello").is_none());
}
```

- [ ] **Step 2:** `cargo test -p cairn-cli short_query_yields_hint` → FAIL.

- [ ] **Step 3: Implement.** In `tantivy_index.rs`, expose the threshold and use it in the guard:

```rust
/// Smallest query (in characters, after trimming) the n-gram index can match.
/// Shorter queries match no n-gram and always return empty; callers should
/// surface a hint rather than a bare empty result (audit D11).
pub const MIN_QUERY_CHARS: usize = NGRAM_MIN;
```

(use `MIN_QUERY_CHARS` in the `search` length guard in place of `NGRAM_MIN`.)

Re-export in `cairn-infra/src/lib.rs`:

```rust
pub use tantivy_index::{TantivyIndex, MIN_QUERY_CHARS};
```

In `main.rs`, import it and add the helper:

```rust
use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex, MIN_QUERY_CHARS};

/// A hint to print when a search query is too short for the n-gram index to
/// match anything. `None` when the query is long enough. Mirrors the index's
/// own `trim().chars().count()` rejection so the hint fires exactly when the
/// index returns empty for being too short (audit D11).
fn short_query_hint(query: &str) -> Option<String> {
    if query.trim().chars().count() < MIN_QUERY_CHARS {
        Some(format!(
            "hint: query is shorter than the {MIN_QUERY_CHARS}-character minimum; no results"
        ))
    } else {
        None
    }
}
```

Wire into the `Search` arm:

```rust
        Command::Search { query } => {
            if let Some(hint) = short_query_hint(&query) {
                eprintln!("{hint}");
            }
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

- [ ] **Step 4:** `cargo test -p cairn-cli short_query_yields_hint` → PASS.

- [ ] **Step 5: Commit.**

### Task 4: Verify + review + PR

- [ ] Run full suite (`just test` or `cargo test --workspace`); paste real output.
- [ ] Manual smoke: fresh `init` vs re-`init`; `search a`.
- [ ] `requesting-code-review`.
- [ ] Push, `gh pr create -R tau-rs/cairn --base main`, cite D2/D9/D11. STOP — no merge.
