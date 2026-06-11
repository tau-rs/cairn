# Content-hash persistence — stable, versioned hash

**Audit finding:** Design (Medium) — *`content_hash` is documented "do not persist"
but is persisted and reloaded* (`audit/design.md`).

## Problem

`Note::content_hash()` (`crates/cairn-domain/src/note.rs:138-149`) uses
`DefaultHasher` (SipHash) and its doc explicitly states the value is "not stable
across Rust versions or process restarts — do not persist it or compare hashes
across processes."

Yet the engine does exactly that:

- `save_state` (`crates/cairn-app/src/lib.rs:194-213`) serializes the memo hash
  into `.cairn/state.json`.
- `reconcile_warm` (`lib.rs:155-192`) loads it back into `self.memo`. For a note
  whose `(mtime, len)` stamp is unchanged it **trusts the persisted hash without
  re-reading the file** — this is the warm-start fast path.
- `apply_change` / `apply_write` (`lib.rs:244, 299`) later compare a *fresh*
  `content_hash()` against that persisted value for change dedup.

**Damage.** After a Rust/std upgrade, SipHash output changes, so a persisted hash
no longer matches a fresh hash of *identical* content. When a touched-but-unchanged
note fires a watcher event (stamp differs, content same), the fresh≠persisted
comparison fails → a spurious re-index and a false `NoteChanged` event. The stamp
guard masks the common case, so the regression is silent. There are no false
negatives (a real change is never skipped); the failure mode is false positives /
weakened dedup. The code contradicts its own documented contract.

## Approaches considered

- **(a) Drop persistence.** Stop writing the hash; recompute or leave the memo
  empty on load. Smallest deletion, but it *regresses warm-start dedup that works
  today within a single Rust version*: recomputing means re-reading every file
  (kills the fast path), and leaving the memo empty eats a spurious re-index on
  each note's first post-startup change. It trades a cross-version-only bug for an
  always-present minor regression.

- **(b) Stable, versioned hash. ← chosen.** Make the hash actually portable, so
  persisting it is correct. Keeps full warm-start dedup and makes the contract
  honest instead of removing the feature.

## Design (option b)

### Stable hash in `cairn-domain`

Replace `DefaultHasher` in `content_hash()` with an inline **FNV-1a 64-bit** hash
over the note's content. Deterministic by construction, no new dependency, stable
across Rust versions and processes. Hash, in order:

1. a presence byte for `frontmatter` (`1` for `Some`, `0` for `None`), so an empty
   frontmatter block does not collide with no frontmatter,
2. the frontmatter bytes (if present),
3. the body bytes.

Update the doc comment: the hash **is** stable and may be persisted, but it is
non-cryptographic and must not be used for security. The existing
`content_hash_is_stable_and_sensitive` test still applies; add a known-answer
(golden) assertion pinning the FNV output for a fixed input so an accidental
algorithm change is caught.

### Versioned state in `cairn-app`

Add a `schema_version` field to `StatePayload`, set to a current constant (e.g.
`const STATE_SCHEMA_VERSION: u32 = 1`). The version tags the *hash regime*: bump it
whenever the hashing algorithm changes, invalidating stale persisted hashes.

`parse_state` rejects (`Err(())`) a payload whose `schema_version` is missing or
not equal to the current constant. `reconcile` already maps `Err(())` →
`reconcile_cold`, i.e. a full rebuild. So a `state.json` from an older/different
hash regime is **reject-and-rebuilt, never crashes** — backward-tolerant by reusing
the existing cold-start fallback.

`save_state` writes the current `schema_version`.

### Data flow (unchanged otherwise)

Warm start with a matching version restores `memo` + `stamps` exactly as today and
the fast path (no read for unchanged stamps) is preserved. Because the hash is now
stable, the persisted memo value matches a fresh hash of identical content across
restarts and Rust upgrades, so dedup stays correct.

## Testing (TDD — failing test first)

1. **Regime invalidation (failing first):** a `state.json` whose `schema_version`
   does not match the current constant is rejected by `parse_state`, so `reconcile`
   falls back to a full rebuild rather than seeding a stale memo. Today there is no
   version field, so the test fails until the field + check exist.
2. **Stable round-trip:** the FNV hash is identical for identical content across
   independent `Note` values; a golden known-answer test pins the algorithm.
3. **Backward tolerance:** a legacy `state.json` with no `schema_version` field
   parses to `Err(())` → rebuild, no panic.

## Scope

One finding, one PR. In scope: `content_hash` algorithm swap + doc, `state.json`
`schema_version` field + reject-and-rebuild load path, tests. Out of scope: any
other state.json fields, watcher/index changes, unrelated refactors.
