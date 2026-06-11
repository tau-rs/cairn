# Error Fidelity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop two diagnostics bugs from hiding real failures: `LocalFsStore::read` collapsing every IO error to `NotFound`, and a corrupt `state.json` being silently discarded on startup.

**Architecture:** Fix (1) is local to the `read` adapter — match on `io::ErrorKind` exactly like `delete`/`stamp` already do. Fix (2) adds a best-effort `quarantine_meta` port so the engine can preserve a rejected `state.json` (rename to `.corrupt`), changes `parse_state` to return the failure reason instead of `()`, and emits an `eprintln!` warning (the repo's de-facto logging facility) before falling back to a cold rebuild. The happy path is untouched.

**Tech Stack:** Rust, `std::fs`, `serde_json`, `tempfile` (dev). No new dependencies.

---

## Context for the implementer

- The audit finding lives at `audit/diagnostics.md` G2 (read) and G3 (state-parse).
- `34-cairn-structured-logging` has NOT landed: there is no `tracing`/`log` crate in this repo. The existing logging facility is `eprintln!` to stderr (see `crates/cairn-daemon/src/main.rs`, `plugin_host.rs`). Use `eprintln!`.
- `parse_state` is also edited by session `33-cairn-content-hash-persistence`. Keep this change minimal and localized; the merge queue will rebase.
- Two `VaultStore` impls exist: `LocalFsStore` (`crates/cairn-infra/src/localfs.rs:108`) and the test mock `CountingStore` (`crates/cairn-app/src/lib.rs:623`). Both must implement any new trait method.
- The corrupt `state.json` is at `<root>/.cairn/state.json`, owned by the adapter — the engine doesn't know that path, so quarantine and the path string it logs must come from the port.

---

## Task 1: `read` reports IO errors faithfully

**Files:**
- Modify: `crates/cairn-infra/src/localfs.rs:109-112` (`LocalFsStore::read`)
- Test: same file's `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing test**

Add to the tests module in `crates/cairn-infra/src/localfs.rs`:

```rust
#[test]
fn read_unreadable_path_is_not_not_found() {
    // A path that exists but cannot be read as a note (here: a directory
    // sitting where a note file would be) must surface as a real adapter
    // error, never masquerade as a missing note.
    let tmp = tempfile::tempdir().unwrap();
    let store = LocalFsStore::open(tmp.path()).unwrap();
    let p = NotePath::new("a.md").unwrap();
    std::fs::create_dir(tmp.path().join("a.md")).unwrap();

    let err = store.read(&p).unwrap_err();
    assert!(
        matches!(err, PortError::Adapter(_)),
        "expected Adapter, got {err:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra read_unreadable_path_is_not_not_found`
Expected: FAIL — current `read` maps the directory read error to `PortError::NotFound`.

- [ ] **Step 3: Write minimal implementation**

Replace `LocalFsStore::read` (`crates/cairn-infra/src/localfs.rs:109-112`):

```rust
    fn read(&self, path: &NotePath) -> Result<String, PortError> {
        fs::read_to_string(self.full(path)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PortError::NotFound(path.as_str().to_string())
            } else {
                PortError::Adapter(e.to_string())
            }
        })
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra`
Expected: PASS — new test passes; `read_missing_is_not_found` and `write_read_list_roundtrip` still pass (a genuinely absent file still yields `NotFound`).

- [ ] **Step 5: Commit** (deferred — single commit for the coupled pair at the end)

---

## Task 2: Add `quarantine_meta` port to preserve a rejected `state.json`

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs` (`VaultStore` trait, after `write_meta`)
- Modify: `crates/cairn-infra/src/localfs.rs` (impl on `LocalFsStore`, near `read_meta`/`write_meta`)
- Modify: `crates/cairn-app/src/lib.rs:623` (`CountingStore` test mock — delegate)
- Test: `crates/cairn-infra/src/localfs.rs` tests module

- [ ] **Step 1: Write the failing test**

Add to the tests module in `crates/cairn-infra/src/localfs.rs`:

```rust
#[test]
fn quarantine_meta_moves_state_aside_and_is_noop_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let store = LocalFsStore::open(tmp.path()).unwrap();

    // Nothing to move yet.
    assert_eq!(store.quarantine_meta().unwrap(), None);

    store.write_meta("corrupt{").unwrap();
    let moved = store.quarantine_meta().unwrap().expect("a path");
    assert!(moved.ends_with("state.json.corrupt"), "got {moved}");

    // Original bytes preserved at the new path; state.json no longer present.
    let corrupt = tmp.path().join(".cairn").join("state.json.corrupt");
    assert_eq!(std::fs::read_to_string(&corrupt).unwrap(), "corrupt{");
    assert!(store.read_meta().unwrap().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra quarantine_meta_moves_state_aside`
Expected: FAIL to COMPILE — `quarantine_meta` does not exist.

- [ ] **Step 3a: Declare the port method**

Add to the `VaultStore` trait in `crates/cairn-ports/src/lib.rs`, after `write_meta`:

```rust
    /// Move a rejected metadata blob aside so it is not lost to a fresh write,
    /// renaming `<root>/.cairn/state.json` to `state.json.corrupt`. Best-effort
    /// diagnostics aid. Returns the destination path (for logging), or
    /// `Ok(None)` if there was nothing to move.
    ///
    /// # Errors
    /// `Adapter` on an IO failure during the rename.
    fn quarantine_meta(&self) -> Result<Option<String>, PortError>;
```

- [ ] **Step 3b: Implement on `LocalFsStore`**

Add after `write_meta` in `crates/cairn-infra/src/localfs.rs`:

```rust
    fn quarantine_meta(&self) -> Result<Option<String>, PortError> {
        let src = self.root.join(".cairn").join("state.json");
        if !src.exists() {
            return Ok(None);
        }
        let dst = src.with_extension("json.corrupt");
        fs::rename(&src, &dst).map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(Some(dst.display().to_string()))
    }
```

- [ ] **Step 3c: Implement on the `CountingStore` test mock**

Add to `impl VaultStore for CountingStore` in `crates/cairn-app/src/lib.rs` (alongside the other delegating methods):

```rust
        fn quarantine_meta(&self) -> Result<Option<String>, PortError> {
            self.inner.quarantine_meta()
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra quarantine_meta_moves_state_aside && cargo build`
Expected: PASS and the whole workspace compiles (both `VaultStore` impls satisfy the trait).

- [ ] **Step 5: Commit** (deferred)

---

## Task 3: Warn + preserve on a rejected `state.json`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs:138-146` (`reconcile`)
- Modify: `crates/cairn-app/src/lib.rs:590` (`parse_state` signature `Result<_, ()>` → `Result<_, String>`)
- Add: a pure formatter `state_rejected_warning` in `crates/cairn-app/src/lib.rs`
- Test: `crates/cairn-app/src/lib.rs` tests module

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `crates/cairn-app/src/lib.rs`:

```rust
    #[test]
    fn warning_message_names_reason_and_preserved_path() {
        // The emitted (stderr) warning text — asserted here as captured output
        // via the pure formatter that feeds eprintln!.
        let msg = state_rejected_warning("expected value at line 1", Some("/v/.cairn/state.json.corrupt"));
        assert!(msg.contains("state.json"));
        assert!(msg.contains("expected value at line 1"));
        assert!(msg.contains("/v/.cairn/state.json.corrupt"));

        let msg_none = state_rejected_warning("bad", None);
        assert!(msg_none.contains("bad"));
        assert!(msg_none.contains("rebuild"));
    }

    #[test]
    fn corrupt_state_is_preserved_and_rebuild_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("a.md"), "hello").unwrap();
        // Plant a corrupt state.json the warm path cannot parse.
        store.write_meta("{ not valid json").unwrap();

        let mut eng = Engine::new(
            store,
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        let mut ev = Vec::new();
        eng.reconcile(&mut ev).unwrap(); // must not error; falls back to cold rebuild

        // The note got indexed by the cold rebuild.
        assert_eq!(ev, vec![Event::Reindexed(1)]);
        // The corrupt file was preserved, not silently dropped.
        let corrupt = tmp.path().join(".cairn").join("state.json.corrupt");
        assert_eq!(std::fs::read_to_string(&corrupt).unwrap(), "{ not valid json");
        // A fresh, valid state.json was written by the rebuild.
        let fresh = tmp.path().join(".cairn").join("state.json");
        assert!(parse_state(&std::fs::read_to_string(&fresh).unwrap()).is_ok());
    }

    #[test]
    fn parse_state_returns_reason_on_bad_json() {
        let err = parse_state("{ not json").unwrap_err();
        assert!(!err.is_empty(), "reason must be non-empty");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-app warning_message_names_reason_and_preserved_path corrupt_state_is_preserved_and_rebuild_succeeds parse_state_returns_reason_on_bad_json`
Expected: FAIL to COMPILE — `state_rejected_warning` undefined and `parse_state` returns `Result<_, ()>` (so `.unwrap_err()` is `()`).

- [ ] **Step 3a: Make `parse_state` carry the reason**

Replace `parse_state` (`crates/cairn-app/src/lib.rs:590`):

```rust
fn parse_state(json: &str) -> Result<RestoredState, String> {
    let payload: StatePayload = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut map = HashMap::with_capacity(payload.entries.len());
    for e in payload.entries {
        let path = NotePath::new(&e.path).map_err(|err| format!("invalid note path {}: {err}", e.path))?;
        let modified = UNIX_EPOCH + Duration::new(e.mtime_secs, e.mtime_nanos);
        map.insert(
            path,
            (
                e.hash,
                FileStamp {
                    modified,
                    len: e.len,
                },
            ),
        );
    }
    Ok(map)
}
```

- [ ] **Step 3b: Add the pure warning formatter**

Add near `parse_state` in `crates/cairn-app/src/lib.rs`:

```rust
/// The warning text emitted when a persisted `state.json` is rejected. Pure so
/// it is unit-testable as captured output; the caller writes it to stderr.
fn state_rejected_warning(reason: &str, preserved: Option<&str>) -> String {
    match preserved {
        Some(dest) => format!(
            "warning: persisted state.json rejected ({reason}); preserved at {dest}, rebuilding index"
        ),
        None => format!("warning: persisted state.json rejected ({reason}); rebuilding index"),
    }
}
```

- [ ] **Step 3c: Wire warn + quarantine into `reconcile`**

Replace `reconcile` (`crates/cairn-app/src/lib.rs:138-146`):

```rust
    pub fn reconcile(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        match self.store.read_meta()? {
            Some(json) => match parse_state(&json) {
                Ok(restored) => self.reconcile_warm(restored, sink),
                Err(reason) => {
                    // Preserve the corrupt blob (best-effort) so it is not lost,
                    // then warn before abandoning the warm-start path.
                    let preserved = self.store.quarantine_meta().unwrap_or(None);
                    eprintln!("{}", state_rejected_warning(&reason, preserved.as_deref()));
                    self.reconcile_cold(sink)
                }
            },
            None => self.reconcile_cold(sink),
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-app`
Expected: PASS — new tests pass; existing reconcile/warm-start tests unaffected (happy path unchanged).

- [ ] **Step 5: Commit** (deferred)

---

## Task 4: Verify, review, ship

- [ ] **Step 1: Full suite + clippy**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 2: Capture the real warning output**

Run: `cargo test -p cairn-app corrupt_state_is_preserved_and_rebuild_succeeds -- --nocapture 2>&1 | grep "warning: persisted state.json rejected"`
Expected: the real stderr warning line is printed (records the actual diagnostic the brief asks to capture).

- [ ] **Step 3: requesting-code-review skill**

- [ ] **Step 4: Single commit** (Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>), push, `gh pr create -R tau-rs/cairn --base main`, cite G2 + G3. STOP — no merge.
