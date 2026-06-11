# Content-hash persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Note::content_hash()` stable and persistable, and version the persisted `state.json` so a stale hash regime is rebuilt rather than silently trusted.

**Architecture:** Swap `DefaultHasher` (SipHash, non-portable) for an inline FNV-1a 64-bit hash over a length-delimited encoding of the note's content. Add a `schema_version` field to `StatePayload`; `parse_state` rejects a missing/mismatched version, and the existing `reconcile` path already falls back to a full cold rebuild on `Err(())`.

**Tech Stack:** Rust, `cairn-domain` (`note.rs`), `cairn-app` (`lib.rs`), `serde`, `cargo test`.

---

### Task 1: Stable FNV-1a content hash in `cairn-domain`

**Files:**
- Modify: `crates/cairn-domain/src/note.rs:138-149` (`content_hash` + doc)
- Test: `crates/cairn-domain/src/note.rs` (tests module, near line 292)

- [ ] **Step 1: Write the failing golden test**

Add this test to the `tests` module in `note.rs` (next to `content_hash_is_stable_and_sensitive`):

```rust
#[test]
fn content_hash_is_a_fixed_fnv1a_known_answer() {
    // FNV-1a 64 over: presence byte 0x00 (no frontmatter) + body bytes "body".
    // Independently computed; pins the algorithm so an accidental change is caught.
    let p = NotePath::new("a.md").unwrap();
    let n = Note::parse(p, "body");
    assert_eq!(n.content_hash(), 0xc2eac2be539f2a97);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cairn-domain content_hash_is_a_fixed_fnv1a_known_answer`
Expected: FAIL — `DefaultHasher` produces a different value than the FNV-1a known answer.

- [ ] **Step 3: Replace the hash implementation and update the doc**

Replace `note.rs:138-149` (the doc comment + `content_hash` body) with:

```rust
    /// A non-cryptographic hash of the note's content (frontmatter + body),
    /// for change detection / memoization and the on-disk index in
    /// `.cairn/state.json`. Stable across Rust versions and processes (FNV-1a
    /// 64), so it may be persisted. Not for security: it is not collision-
    /// resistant against an adversary. If the algorithm ever changes, bump
    /// `STATE_SCHEMA_VERSION` in `cairn-app` so stale persisted hashes are
    /// rebuilt rather than trusted.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        // FNV-1a 64. Length-delimit the frontmatter so that, e.g.,
        // (frontmatter "a", body "b") and (frontmatter "ab", body "") differ.
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = OFFSET;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                h ^= u64::from(b);
                h = h.wrapping_mul(PRIME);
            }
        };
        match &self.frontmatter {
            Some(fm) => {
                feed(&[1]);
                feed(&(fm.len() as u64).to_le_bytes());
                feed(fm.as_bytes());
            }
            None => feed(&[0]),
        }
        feed(self.body.as_bytes());
        h
    }
```

- [ ] **Step 4: Run the domain tests to verify green**

Run: `cargo test -p cairn-domain`
Expected: PASS — the new golden test plus the existing `content_hash_is_stable_and_sensitive` both pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-domain/src/note.rs
git commit -m "fix(domain): stable FNV-1a content_hash, safe to persist (audit Medium)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Version `state.json` and reject a stale hash regime in `cairn-app`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs:13-25` (add `STATE_SCHEMA_VERSION` + `schema_version` field)
- Modify: `crates/cairn-app/src/lib.rs:194-213` (`save_state` writes the version)
- Modify: `crates/cairn-app/src/lib.rs:590-608` (`parse_state` checks the version)
- Test: `crates/cairn-app/src/lib.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add these tests to the `tests` module in `lib.rs` (near `reconcile_warm_skips_unchanged_and_catches_changes`). `parse_state`, `StatePayload`, `StateEntry`, and `STATE_SCHEMA_VERSION` are in the parent module and reachable via `super::*`:

```rust
    #[test]
    fn parse_state_rejects_mismatched_schema_version() {
        // A payload from a different (future) hash regime must not seed memo.
        let json = serde_json::json!({
            "schema_version": STATE_SCHEMA_VERSION + 1,
            "entries": []
        })
        .to_string();
        assert!(parse_state(&json).is_err());
    }

    #[test]
    fn parse_state_rejects_legacy_state_without_version() {
        // Pre-versioning state.json (no schema_version field) is rebuilt, not trusted.
        let json = r#"{"entries":[]}"#;
        assert!(parse_state(json).is_err());
    }

    #[test]
    fn parse_state_accepts_current_version() {
        let json = serde_json::json!({
            "schema_version": STATE_SCHEMA_VERSION,
            "entries": []
        })
        .to_string();
        assert!(parse_state(&json).is_ok());
    }

    #[test]
    fn stale_state_json_triggers_full_rebuild() {
        // End-to-end: a state.json from a different regime must rebuild the
        // index (re-read every note) rather than warm-start off stale hashes.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "alpha body").unwrap();
        std::fs::write(tmp.path().join("b.md"), "beta body").unwrap();

        // First run writes a current-version state.json.
        {
            let mut eng = engine(tmp.path());
            eng.reconcile(&mut Vec::new()).unwrap();
        }

        // Rewrite state.json with a bumped schema_version (simulated future regime).
        let store = LocalFsStore::open(tmp.path()).unwrap();
        let raw = store.read_meta().unwrap().unwrap();
        let mut payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
        payload["schema_version"] =
            serde_json::json!(payload["schema_version"].as_u64().unwrap() + 1);
        store.write_meta(&payload.to_string()).unwrap();

        // Reconcile again with a read-counting store: a rebuild re-reads both notes.
        let reads = Arc::new(AtomicUsize::new(0));
        let mut eng = Engine::new(
            CountingStore {
                inner: LocalFsStore::open(tmp.path()).unwrap(),
                reads: reads.clone(),
            },
            InMemoryIndex::default(),
            GitVcs::open_or_init(tmp.path()).unwrap(),
        );
        eng.reconcile(&mut Vec::new()).unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "stale schema_version forces a full rebuild that re-reads every note"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-app parse_state stale_state_json_triggers_full_rebuild`
Expected: FAIL — `STATE_SCHEMA_VERSION` is undefined (compile error), and once that is referenced the version checks do not yet exist. (`parse_state_rejects_*` would otherwise pass on the empty-entries path only after the field exists.)

- [ ] **Step 3: Add the version constant and struct field**

Replace `lib.rs:13-25` (the two struct definitions) with:

```rust
/// Schema version of `.cairn/state.json`. Tags the hash regime: bump this
/// whenever `Note::content_hash`'s algorithm changes so stale persisted hashes
/// are rebuilt (cold) rather than silently trusted.
const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct StateEntry {
    path: String,
    hash: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    len: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StatePayload {
    #[serde(default)]
    schema_version: u32,
    entries: Vec<StateEntry>,
}
```

- [ ] **Step 4: Write the version on save**

In `save_state` (`lib.rs:210`), change the payload construction to include the version:

```rust
        let json = serde_json::to_string(&StatePayload {
            schema_version: STATE_SCHEMA_VERSION,
            entries,
        })
        .map_err(|e| PortError::Adapter(e.to_string()))?;
```

- [ ] **Step 5: Check the version on load**

In `parse_state` (`lib.rs:590-591`), reject a mismatched version right after deserializing:

```rust
fn parse_state(json: &str) -> Result<RestoredState, ()> {
    let payload: StatePayload = serde_json::from_str(json).map_err(|_| ())?;
    if payload.schema_version != STATE_SCHEMA_VERSION {
        return Err(()); // different/absent hash regime → reconcile_cold rebuilds
    }
    let mut map = HashMap::with_capacity(payload.entries.len());
```

(The rest of `parse_state` is unchanged.)

- [ ] **Step 6: Run the app tests to verify green**

Run: `cargo test -p cairn-app`
Expected: PASS — the four new tests pass and the existing `reconcile_*` tests (which now round-trip through the versioned payload) still pass.

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-app/src/lib.rs
git commit -m "fix(app): version state.json hash regime, reject-and-rebuild stale (audit Medium)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Full verification

- [ ] **Step 1: Run the full workspace test + lint**

Run: `cargo test -p cairn-domain -p cairn-app && cargo clippy -p cairn-domain -p cairn-app --all-targets -- -D warnings && cargo fmt --check`
Expected: all tests pass, no clippy warnings, formatting clean.

(If `cargo fmt --check` reports diffs, run `cargo fmt` and amend the relevant commit.)
