# Plugin Content Hashing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin a SHA-256 of each trusted plugin directory's full contents and refuse to spawn a trusted dir whose bytes drifted from the pin.

**Architecture:** Extends the #39 trust gate. A new `PinnedHash` type (own module in `cairn-infra`) hashes a directory tree with a canonical, framed construction. `TrustedPlugins` becomes a `dir_name -> Option<PinnedHash>` map. In `load_with_timeout`, after the existing name trust-gate and before spawn, the dir tree is hashed and matched against the pin: match → spawn, drift → refuse, unpinned → spawn + warn, symlink/non-regular → refuse. The `cairn.toml [[plugins.trusted]]` schema becomes an array of tables with an optional `hash`, parsed untagged so legacy bare strings still load.

**Tech Stack:** Rust, `sha2` crate (new), `serde` untagged enum, `toml`, existing `PortError`.

**Spec:** `docs/superpowers/specs/2026-06-12-plugin-content-hashing-design.md`

---

## File Structure

- **Create** `crates/cairn-infra/src/plugin_hash.rs` — the `PinnedHash` newtype: `parse`, `Display`, the canonical `hash_files` construction, and `of_dir` (tree walk + symlink/non-regular rejection). Self-contained, unit-tested in isolation.
- **Modify** `crates/cairn-infra/Cargo.toml` — add `sha2` dependency.
- **Modify** `Cargo.toml` (workspace) — add `sha2` to `[workspace.dependencies]`.
- **Modify** `crates/cairn-infra/src/plugin_host.rs` — `TrustedPlugins` set → map; add `from_entries`/`get`; wire the hash gate into `load_with_timeout`.
- **Modify** `crates/cairn-infra/src/lib.rs` — re-export `PinnedHash`.
- **Modify** `crates/cairn-daemon/src/config.rs` — `trusted: Vec<String>` → `Vec<TrustedEntry>` (untagged `String | {dir, hash?}`), with a normalizer to `(String, Option<String>)`.
- **Modify** `crates/cairn-daemon/src/main.rs` — build `TrustedPlugins::from_entries(...)`, aborting startup on a malformed pin.
- **Modify** `crates/cairn-plugin-example/tests/host.rs` — host-level pin/drift/symlink tests against the example plugin.

Existing call sites of `TrustedPlugins::from_ids` (`main.rs:90`, `host.rs:21`/`:33`, the `plugin_host.rs` unit tests) keep working: `from_ids` is retained and yields unpinned (`None`) entries.

---

## Task 1: Add the `sha2` dependency

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/cairn-infra/Cargo.toml` (`[dependencies]`)

- [ ] **Step 1: Add `sha2` to the workspace dependency table**

In `Cargo.toml`, under `[workspace.dependencies]`, add alongside the existing entries (e.g. after the `toml = "1"` line):

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Add `sha2` to cairn-infra**

In `crates/cairn-infra/Cargo.toml`, under `[dependencies]`, add after `toml = { workspace = true }`:

```toml
sha2 = { workspace = true }
```

- [ ] **Step 3: Verify it resolves**

Run: `cargo build -p cairn-infra`
Expected: builds (sha2 downloaded/compiled, no code using it yet).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/cairn-infra/Cargo.toml Cargo.lock
git commit -m "build(infra): add sha2 dependency for plugin content hashing"
```

---

## Task 2: `PinnedHash` newtype — parse + Display

**Files:**
- Create: `crates/cairn-infra/src/plugin_hash.rs`
- Modify: `crates/cairn-infra/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/cairn-infra/src/plugin_hash.rs` with:

```rust
//! Content hash of a plugin directory tree, pinned in the trust list.
//!
//! A pin is the string `sha256:<64 lowercase hex>`. The explicit algorithm
//! prefix is part of the stored value so a future construction change is a new
//! prefix (surfaced as a mismatch the user can act on), never a silent
//! wrong-compare. The hashing construction under the `sha256:` prefix is a
//! stability contract and must not change once pins exist in the wild.

use std::path::Path;

use cairn_ports::PortError;

const PREFIX: &str = "sha256:";
const HEX_LEN: usize = 64; // 32 bytes of SHA-256, lowercase hex

/// A pinned content hash, `sha256:<64 hex>`. Compare by value to detect drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHash(String);

impl PinnedHash {
    /// Parse a stored pin. Rejects unknown prefixes, wrong length, non-hex.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on any malformed value (caller surfaces it as a
    /// fail-fast config error).
    pub fn parse(s: &str) -> Result<Self, PortError> {
        let hex = s.strip_prefix(PREFIX).ok_or_else(|| {
            PortError::Adapter(format!("plugin hash {s:?} missing \"{PREFIX}\" prefix").into())
        })?;
        if hex.len() != HEX_LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(PortError::Adapter(
                format!("plugin hash {s:?} must be \"{PREFIX}\" + {HEX_LEN} lowercase hex chars")
                    .into(),
            ));
        }
        Ok(Self(s.to_string()))
    }
}

impl std::fmt::Display for PinnedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_canonical_pin() {
        let s = format!("sha256:{}", "a".repeat(64));
        assert_eq!(PinnedHash::parse(&s).unwrap().to_string(), s);
    }

    #[test]
    fn parse_rejects_bad_pins() {
        assert!(PinnedHash::parse(&"a".repeat(64)).is_err()); // no prefix
        assert!(PinnedHash::parse("sha256:abc").is_err()); // too short
        assert!(PinnedHash::parse(&format!("sha256:{}", "a".repeat(63))).is_err()); // 63
        assert!(PinnedHash::parse(&format!("sha256:{}", "A".repeat(64))).is_err()); // uppercase
        assert!(PinnedHash::parse(&format!("sha256:{}", "g".repeat(64))).is_err()); // non-hex
        assert!(PinnedHash::parse(&format!("blake3:{}", "a".repeat(64))).is_err()); // wrong algo
    }
}
```

- [ ] **Step 2: Wire the module into the crate**

In `crates/cairn-infra/src/lib.rs`, add the module declaration near the other `mod` lines and extend the `plugin_host` re-export line. Add:

```rust
mod plugin_hash;
```

and add `PinnedHash` to the public surface by adding this line next to the existing `pub use plugin_host::...` (line 15):

```rust
pub use plugin_hash::PinnedHash;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_hash`
Expected: `parse_accepts_canonical_pin` and `parse_rejects_bad_pins` PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-infra/src/plugin_hash.rs crates/cairn-infra/src/lib.rs
git commit -m "feat(infra): PinnedHash newtype with parse + Display"
```

---

## Task 3: Canonical framed hashing construction

**Files:**
- Modify: `crates/cairn-infra/src/plugin_hash.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `plugin_hash.rs`:

```rust
#[test]
fn hash_is_deterministic() {
    let files = vec![("a.txt".to_string(), b"x".to_vec())];
    assert_eq!(hash_files(files.clone()), hash_files(files));
}

#[test]
fn hash_is_order_independent() {
    let asc = vec![
        ("a.txt".to_string(), b"1".to_vec()),
        ("b.txt".to_string(), b"2".to_vec()),
    ];
    let desc = vec![
        ("b.txt".to_string(), b"2".to_vec()),
        ("a.txt".to_string(), b"1".to_vec()),
    ];
    assert_eq!(hash_files(asc), hash_files(desc));
}

#[test]
fn framing_prevents_boundary_collision() {
    // Same concatenated bytes, different split between path and contents.
    // Without the separator + length framing these would collide.
    let a = vec![("ab".to_string(), b"c".to_vec())];
    let b = vec![("a".to_string(), b"bc".to_vec())];
    assert_ne!(hash_files(a), hash_files(b));
}

#[test]
fn distinct_contents_distinct_hash() {
    let a = vec![("f".to_string(), b"one".to_vec())];
    let b = vec![("f".to_string(), b"two".to_vec())];
    assert_ne!(hash_files(a), hash_files(b));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra plugin_hash`
Expected: FAIL — `hash_files` not found.

- [ ] **Step 3: Implement the construction**

Add to `plugin_hash.rs` (after the imports, before the `impl PinnedHash` block). Note the `Sha256`/`Digest` import goes at the top with the other `use` lines:

```rust
use sha2::{Digest, Sha256};
```

```rust
/// Hash a list of `(relative_path, bytes)` into a [`PinnedHash`].
///
/// Canonical construction (a stability contract — see module docs): sort by
/// relative path (byte order), then for each file feed `path`, a `0x00`
/// separator (cannot appear in a path), the byte length as little-endian u64,
/// and the bytes. The separator + length framing makes the serialization
/// unambiguous, so no two distinct trees share a hash.
fn hash_files(mut files: Vec<(String, Vec<u8>)>) -> PinnedHash {
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (path, bytes) in &files {
        hasher.update(path.as_bytes());
        hasher.update([0x00]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(PREFIX.len() + HEX_LEN);
    hex.push_str(PREFIX);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    PinnedHash(hex)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_hash`
Expected: all six `plugin_hash` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_hash.rs
git commit -m "feat(infra): canonical framed dir-tree hashing construction"
```

---

## Task 4: `PinnedHash::of_dir` — tree walk, reject symlinks/non-regular

**Files:**
- Modify: `crates/cairn-infra/src/plugin_hash.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module (and add `use std::path::PathBuf;` is not needed — use `tempfile`, already a dev-dependency):

```rust
#[test]
fn of_dir_matches_manual_hash() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("sub").join("b.txt"), b"world").unwrap();

    let expected = hash_files(vec![
        ("a.txt".to_string(), b"hello".to_vec()),
        ("sub/b.txt".to_string(), b"world".to_vec()),
    ]);
    assert_eq!(PinnedHash::of_dir(tmp.path()).unwrap(), expected);
}

#[test]
fn of_dir_detects_content_drift() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"before").unwrap();
    let h1 = PinnedHash::of_dir(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"after").unwrap();
    let h2 = PinnedHash::of_dir(tmp.path()).unwrap();
    assert_ne!(h1, h2);
}

#[cfg(unix)]
#[test]
fn of_dir_refuses_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("real.txt"), b"x").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    assert!(PinnedHash::of_dir(tmp.path()).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra plugin_hash`
Expected: FAIL — `of_dir` not found.

- [ ] **Step 3: Implement `of_dir`**

Add to the `impl PinnedHash` block:

```rust
    /// Hash a plugin directory tree (every regular file, recursively).
    ///
    /// Relative paths are normalized to `/` separators so the pin is stable
    /// across platforms. Symlinks and other non-regular files are **refused**
    /// (not followed): following one would re-open the directory-escape hole
    /// this feature closes.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a symlink/non-regular file, a non-UTF-8 path,
    /// or any IO error reading the tree.
    pub fn of_dir(dir: &Path) -> Result<Self, PortError> {
        let mut files = Vec::new();
        collect_files(dir, dir, &mut files)?;
        Ok(hash_files(files))
    }
```

And add this free function below `hash_files`:

```rust
/// Recursively gather `(relative_path, bytes)` under `root`. `current` is the
/// directory presently being walked. Refuses symlinks and non-regular files.
fn collect_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), PortError> {
    let adapt = |e: std::io::Error| PortError::Adapter(Box::new(e));
    for entry in std::fs::read_dir(current).map_err(adapt)? {
        let entry = entry.map_err(adapt)?;
        let path = entry.path();
        // `file_type()` from `read_dir` does NOT follow symlinks, so this
        // detects a symlink itself rather than its target.
        let ft = entry.file_type().map_err(adapt)?;
        if ft.is_symlink() {
            return Err(PortError::Adapter(
                format!("contains a symlink ({}); refusing", path.display()).into(),
            ));
        }
        if ft.is_dir() {
            collect_files(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path.strip_prefix(root).map_err(|_| {
                PortError::Adapter(format!("path {} escaped plugin dir", path.display()).into())
            })?;
            // Join components with `/` for a platform-stable relative path.
            let mut norm = String::new();
            for comp in rel.components() {
                let part = comp.as_os_str().to_str().ok_or_else(|| {
                    PortError::Adapter(
                        format!("non-UTF-8 path under {}; refusing", root.display()).into(),
                    )
                })?;
                if !norm.is_empty() {
                    norm.push('/');
                }
                norm.push_str(part);
            }
            let bytes = std::fs::read(&path).map_err(adapt)?;
            out.push((norm, bytes));
        } else {
            return Err(PortError::Adapter(
                format!("contains a non-regular file ({}); refusing", path.display()).into(),
            ));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_hash`
Expected: all `plugin_hash` tests PASS (including `of_dir_*`).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_hash.rs
git commit -m "feat(infra): PinnedHash::of_dir walks tree, refuses symlinks"
```

---

## Task 5: `TrustedPlugins` becomes a `dir -> Option<PinnedHash>` map

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs:59-81` (the `TrustedPlugins` type + impl)

- [ ] **Step 1: Write the failing tests**

In `plugin_host.rs`, replace the existing `trusted_set_membership` test (lines ~634-641) with:

```rust
    #[test]
    fn from_ids_yields_unpinned_entries() {
        let trusted = TrustedPlugins::from_ids(["a".to_string(), "b".to_string()]);
        assert!(matches!(trusted.get("a"), Some(None)));
        assert!(matches!(trusted.get("b"), Some(None)));
        assert!(trusted.get("c").is_none());
        assert!(TrustedPlugins::none().get("a").is_none());
    }

    #[test]
    fn from_entries_parses_pins_and_rejects_bad() {
        let good = TrustedPlugins::from_entries([(
            "a".to_string(),
            Some(format!("sha256:{}", "a".repeat(64))),
        )])
        .unwrap();
        assert!(matches!(good.get("a"), Some(Some(_))));

        assert!(TrustedPlugins::from_entries([("a".to_string(), Some("bogus".to_string()))]).is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra plugin_host`
Expected: FAIL — `get`/`from_entries` not found, `HashSet` mismatch.

- [ ] **Step 3: Replace the type and impl**

In `plugin_host.rs`, change the `use std::collections::HashSet;` line (line 3) to:

```rust
use std::collections::HashMap;
```

Add to the existing `cairn_ports` / crate imports the `PinnedHash` type (it lives in the same crate). At the top of `plugin_host.rs`, add:

```rust
use crate::PinnedHash;
```

Replace the `TrustedPlugins` definition and impl (lines 59-81) with:

```rust
/// The set of plugin **directory names** the user has explicitly trusted, each
/// with an optional pinned content hash. A plugin under
/// `<cairn>/.cairn/plugins/<dir>` is spawned only if `<dir>` is a key here; if
/// its value is `Some(pin)`, the directory's contents must hash to `pin` or it
/// is refused (drift). `None` = trusted but unpinned (spawns with a warning).
/// An empty map trusts nothing.
#[derive(Debug, Default, Clone)]
pub struct TrustedPlugins(HashMap<String, Option<PinnedHash>>);

impl TrustedPlugins {
    /// A map that trusts no plugin (default-deny).
    pub fn none() -> Self {
        Self::default()
    }

    /// Build from directory names, all unpinned. Retained for callers (and
    /// tests) that only express name trust.
    pub fn from_ids<I: IntoIterator<Item = String>>(ids: I) -> Self {
        Self(ids.into_iter().map(|id| (id, None)).collect())
    }

    /// Build from `(dir_name, optional_pin_string)` pairs, parsing each pin.
    ///
    /// # Errors
    /// [`PortError::Adapter`] if any pin string is malformed (fail-fast: a
    /// typo'd pin must not silently degrade to "unpinned").
    pub fn from_entries<I: IntoIterator<Item = (String, Option<String>)>>(
        entries: I,
    ) -> Result<Self, PortError> {
        let mut map = HashMap::new();
        for (dir, pin) in entries {
            let parsed = pin.map(|p| PinnedHash::parse(&p)).transpose()?;
            map.insert(dir, parsed);
        }
        Ok(Self(map))
    }

    /// Trust + pin for a directory name. Outer `None` ⇒ not trusted; inner
    /// `None` ⇒ trusted but unpinned; `Some(&pin)` ⇒ pinned.
    pub fn get(&self, dir_name: &str) -> Option<&Option<PinnedHash>> {
        self.0.get(dir_name)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra plugin_host::tests::from_ids_yields_unpinned_entries plugin_host::tests::from_entries_parses_pins_and_rejects_bad`
Expected: PASS. (Other `plugin_host` tests still compile against `from_ids`; the load gate is updated in Task 6.)

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs
git commit -m "feat(infra): TrustedPlugins carries an optional pin per dir"
```

---

## Task 6: Wire the hash gate into `load_with_timeout`

**Files:**
- Modify: `crates/cairn-infra/src/plugin_host.rs:397-424` (the per-entry loop)
- Modify: `crates/cairn-plugin-example/tests/host.rs` (host-level tests)

- [ ] **Step 1: Write the failing host tests**

In `crates/cairn-plugin-example/tests/host.rs`, add a helper and tests. First add this helper near `load_example` (it needs the example plugin dir path and the example binary; reuse the existing `write_manifest`):

```rust
/// Path to the built example plugin binary (same one the existing tests spawn).
fn example_bin() -> std::path::PathBuf {
    // The existing tests build/locate the example binary; reuse that path.
    // See `load_example` setup above for how the plugin dir is populated.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cairn-plugin-example"))
}

/// Populate `<tmp>/.cairn/plugins/example` with a valid manifest + binary and
/// return the plugin dir.
fn setup_example_dir(tmp: &std::path::Path) -> std::path::PathBuf {
    let pdir = tmp.join(".cairn").join("plugins").join("example");
    write_manifest(&pdir, example_bin().to_str().unwrap(), "");
    pdir
}
```

> NOTE: If the existing `host.rs` already has an equivalent setup helper (check how `load_example` populates the dir before this task), reuse it instead of adding `setup_example_dir`, and adapt the tests below to it. The existing `write_manifest` writes `id="example"`, matching the `example` dir name.

Then add the tests:

```rust
use cairn_infra::PinnedHash;

#[test]
fn pinned_matching_hash_spawns() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    let pin = PinnedHash::of_dir(&pdir).unwrap().to_string();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(pin))]).unwrap();
    let host = ProcessPluginHost::load(&dir, &trusted).unwrap();
    assert_eq!(host.plugins().len(), 1);
}

#[test]
fn drifted_hash_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    let pin = PinnedHash::of_dir(&pdir).unwrap().to_string();
    // Tamper: add a file so the tree no longer matches the pin.
    std::fs::write(pdir.join("evil.txt"), b"tampered").unwrap();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted =
        TrustedPlugins::from_entries([("example".to_string(), Some(pin))]).unwrap();
    let host = ProcessPluginHost::load(&dir, &trusted).unwrap();
    assert!(host.plugins().is_empty());
}

#[test]
fn unpinned_trusted_spawns() {
    let tmp = tempfile::tempdir().unwrap();
    setup_example_dir(tmp.path());
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_ids(["example".to_string()]);
    let host = ProcessPluginHost::load(&dir, &trusted).unwrap();
    assert_eq!(host.plugins().len(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_in_trusted_dir_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = setup_example_dir(tmp.path());
    std::os::unix::fs::symlink(pdir.join("manifest.toml"), pdir.join("link.toml")).unwrap();
    let dir = tmp.path().join(".cairn").join("plugins");
    let trusted = TrustedPlugins::from_ids(["example".to_string()]);
    let host = ProcessPluginHost::load(&dir, &trusted).unwrap();
    assert!(host.plugins().is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-plugin-example --test host pinned_matching_hash_spawns drifted_hash_refuses unpinned_trusted_spawns symlink_in_trusted_dir_refuses`
Expected: FAIL — the loop doesn't hash yet; `drifted`/`symlink` would currently spawn.

- [ ] **Step 3: Implement the gate**

In `plugin_host.rs`, replace the trust-check + spawn portion of the loop (the `if !trusted.contains(dir_name) { ... } ... match Self::spawn_plugin(...)` block, lines ~411-421) with:

```rust
            let pin = match trusted.get(dir_name) {
                None => {
                    eprintln!(
                        "plugin: skipping {dir_name} (not in [plugins] trusted; \
                         add \"{dir_name}\" to cairn.toml to enable)"
                    );
                    continue;
                }
                Some(pin) => pin,
            };
            // Trusted: hash the directory tree before spawning. A symlink /
            // non-regular file / IO error here is a refusal, not a panic.
            let computed = match PinnedHash::of_dir(&plugin_dir) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("plugin: refusing {dir_name}: {e}");
                    continue;
                }
            };
            match pin {
                Some(expected) if &computed != expected => {
                    eprintln!(
                        "plugin: refusing {dir_name}: contents changed (pinned {expected}, \
                         found {computed}); re-approve by updating hash in cairn.toml"
                    );
                    continue;
                }
                Some(_) => {} // pinned and matches: spawn below
                None => {
                    eprintln!(
                        "plugin: {dir_name} is trusted but unpinned; pin it by setting \
                         hash = \"{computed}\" in cairn.toml"
                    );
                }
            }
            match Self::spawn_plugin(&plugin_dir, timeout) {
                Ok(p) => loaded.push(p),
                Err(e) => eprintln!("plugin: skipping {}: {e}", plugin_dir.display()),
            }
```

- [ ] **Step 4: Run the full infra + example test suites**

Run: `cargo test -p cairn-infra -p cairn-plugin-example`
Expected: PASS — new gate tests pass; existing `~14` host tests still pass (they use `from_ids` → unpinned → spawn).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/plugin_host.rs crates/cairn-plugin-example/tests/host.rs
git commit -m "feat(infra): hash trusted plugin dir and refuse on drift"
```

---

## Task 7: `cairn.toml` schema — array of tables, untagged back-compat

**Files:**
- Modify: `crates/cairn-daemon/src/config.rs:22-36` (`PluginsConfig` + new `TrustedEntry`)

- [ ] **Step 1: Write the failing tests**

In `config.rs`, update the existing `plugins_trusted_parses` / `plugins_trusted_defaults_empty` tests (lines ~164-179) and add new cases. Replace them with:

```rust
    #[test]
    fn plugins_trusted_legacy_strings_parse() {
        let c: Config = toml::from_str("[plugins]\ntrusted = [\"a\", \"b\"]").unwrap();
        let entries: Vec<_> = c.plugins.trusted.iter().map(|e| e.normalize()).collect();
        assert_eq!(
            entries,
            vec![("a".to_string(), None), ("b".to_string(), None)]
        );
    }

    #[test]
    fn plugins_trusted_table_with_hash_parses() {
        let pin = format!("sha256:{}", "a".repeat(64));
        let toml = format!("[[plugins.trusted]]\ndir = \"a\"\nhash = \"{pin}\"\n");
        let c: Config = toml::from_str(&toml).unwrap();
        assert_eq!(c.plugins.trusted[0].normalize(), ("a".to_string(), Some(pin)));
    }

    #[test]
    fn plugins_trusted_table_without_hash_parses() {
        let c: Config = toml::from_str("[[plugins.trusted]]\ndir = \"a\"\n").unwrap();
        assert_eq!(c.plugins.trusted[0].normalize(), ("a".to_string(), None));
    }

    #[test]
    fn plugins_trusted_defaults_empty() {
        assert!(toml::from_str::<Config>("")
            .unwrap()
            .plugins
            .trusted
            .is_empty());
        assert!(toml::from_str::<Config>("[plugins]\ntimeout_secs = 5")
            .unwrap()
            .plugins
            .trusted
            .is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-daemon plugins_trusted`
Expected: FAIL — `TrustedEntry`/`normalize` not defined; `trusted` is still `Vec<String>`.

- [ ] **Step 3: Implement the schema**

In `config.rs`, change the `trusted` field in `PluginsConfig` (line 34-35) from:

```rust
    #[serde(default)]
    pub trusted: Vec<String>,
```

to:

```rust
    #[serde(default)]
    pub trusted: Vec<TrustedEntry>,
```

Then add this type after `PluginsConfig` (before `IndexConfig`):

```rust
/// One entry in `[plugins].trusted`. Parsed untagged so both the legacy bare
/// string form (`trusted = ["name"]`) and the table form
/// (`[[plugins.trusted]] dir = "name" hash = "sha256:..."`) are accepted. A
/// bare string and a table with `hash` omitted both mean "trusted, unpinned".
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TrustedEntry {
    /// Legacy / shorthand: trust by directory name, no pin.
    Name(String),
    /// Table form: directory name plus an optional pinned content hash.
    Pinned {
        dir: String,
        #[serde(default)]
        hash: Option<String>,
    },
}

impl TrustedEntry {
    /// Reduce to `(dir_name, optional_pin_string)` for `TrustedPlugins`.
    pub fn normalize(&self) -> (String, Option<String>) {
        match self {
            TrustedEntry::Name(dir) => (dir.clone(), None),
            TrustedEntry::Pinned { dir, hash } => (dir.clone(), hash.clone()),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-daemon plugins_trusted`
Expected: all four `plugins_trusted_*` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/config.rs
git commit -m "feat(daemon): [[plugins.trusted]] array-of-tables with optional hash"
```

---

## Task 8: Build `TrustedPlugins` from config, fail fast on bad pin

**Files:**
- Modify: `crates/cairn-daemon/src/main.rs:88-97`

- [ ] **Step 1: Locate the wiring**

In `main.rs`, the current build (lines ~89-95) is:

```rust
    let plugins_dir = cli.cairn.join(".cairn").join("plugins");
    let trusted = cairn_infra::TrustedPlugins::from_ids(config.plugins.trusted.clone());
    if config.plugins.trusted.is_empty() {
        tracing::warn!(
            "plugins: none trusted (add [plugins].trusted = [\"<dir>\"] to {}/cairn.toml to enable)",
            ...
        );
    }
```

- [ ] **Step 2: Replace the `from_ids` call with `from_entries`**

Change the `let trusted = ...` line to build from normalized entries, aborting startup on a malformed pin (fail-fast per the spec). Replace lines ~90 with:

```rust
    let trusted = cairn_infra::TrustedPlugins::from_entries(
        config.plugins.trusted.iter().map(|e| e.normalize()),
    )
    .map_err(|e| {
        anyhow::anyhow!("invalid [plugins].trusted entry in cairn.toml: {e}")
    })?;
```

> NOTE: confirm `main` returns `anyhow::Result<()>` (it almost certainly does — it uses `?` elsewhere). If it returns a different error type, adapt the `map_err` to it. If the surrounding function is not fallible, propagate via the existing error path used for config load.

The `config.plugins.trusted.is_empty()` warning below stays unchanged (it checks the same `Vec`, now of `TrustedEntry`).

- [ ] **Step 3: Build and run the daemon test/build**

Run: `cargo build -p cairn-daemon && cargo test -p cairn-daemon`
Expected: builds and tests pass.

- [ ] **Step 4: Full workspace check**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
Expected: all green. (Mirrors the pre-commit hooks: test, clippy, fmt.)

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-daemon/src/main.rs
git commit -m "feat(daemon): build TrustedPlugins from config, fail fast on bad pin"
```

---

## Self-Review

**Spec coverage:**
- Hash scope C (whole tree) → Task 4 `collect_files` (recursive, all regular files). ✓
- SHA-256 + framed construction → Task 3 `hash_files`. ✓
- `sha256:` prefix in stored value → Task 2 `parse`/`Display`, Task 3 builds with `PREFIX`. ✓
- `/`-normalized sorted paths → Task 3 sort, Task 4 component-join. ✓
- Reject symlinks/non-regular → Task 4 `collect_files`. ✓
- Schema A + untagged back-compat → Task 7 `TrustedEntry`. ✓
- Spawn-time state machine (match/drift/unpinned/symlink) → Task 6. ✓
- Untrusted dirs never read → Task 6 keeps `trusted.get` → skip before `of_dir`. ✓
- Migration: legacy strings + omitted hash → unpinned spawn+warn → Tasks 6, 7. ✓
- Fail-fast on malformed pin → Task 5 `from_entries`, Task 8 `main`. ✓
- Tests 1-14 from spec → mapped across Tasks 2,3,4,5,6,7. ✓

**Placeholder scan:** No TBD/TODO in steps. Two `> NOTE` callouts (host.rs setup helper reuse, `main` error type) are conditional-adaptation guidance with concrete fallbacks, not placeholders.

**Type consistency:** `PinnedHash` (parse/of_dir/Display), `hash_files`, `collect_files`, `TrustedPlugins::{none,from_ids,from_entries,get}`, `TrustedEntry::{normalize}` are used consistently across tasks. `get` returns `Option<&Option<PinnedHash>>` and Task 6 matches it as `None`/`Some(None)`/`Some(Some(_))` correctly.

**Known limitation (not a task):** absolute `command` path escaping the hashed dir — documented in the spec as out of scope; no task, intentionally.
