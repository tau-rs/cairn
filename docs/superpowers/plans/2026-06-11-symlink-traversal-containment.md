# Symlink Traversal / TOCTOU Containment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop a symlink planted inside a cairn from letting reads/writes/renames/deletes escape the cairn root, by canonicalizing the resolved target and verifying containment under the canonicalized root before each filesystem op.

**Architecture:** Add a canonicalized root to `LocalFsStore` (computed once at `open`). Replace the lexical-only `safe_full` with a `resolve` helper that (1) keeps the existing `is_safe_rel` lexical guard, (2) refuses a final component that is itself a symlink (O_NOFOLLOW-style leaf handling), and (3) canonicalizes the deepest on-disk ancestor of the target and requires it to stay under the canonical root. Route `read`, `write`, `delete`, and `rename` through `resolve`. This is defense-in-depth at the store layer; the lexical dot-segment fix from `01-cairn-rce` (in `NotePath::new` / `is_safe_rel`) is untouched.

**Tech Stack:** Rust, `std::fs`, `cairn-infra` crate, `tempfile` dev-dep, `#[cfg(unix)]` symlink tests via `std::os::unix::fs::symlink`.

---

### Task 1: Symlink containment in `LocalFsStore`

**Files:**
- Modify: `crates/cairn-infra/src/localfs.rs` (struct + `open` + `safe_full`→`resolve` + `read`/`write`/`delete`/`rename`)
- Test: `crates/cairn-infra/src/localfs.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
#[cfg(unix)]
#[test]
fn symlink_escaping_root_cannot_be_read_or_written() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "TOPSECRET").unwrap();

    let mut store = LocalFsStore::open(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
    // A symlink planted inside the cairn pointing outside the root.
    symlink(outside.path(), tmp.path().join("notes/escape")).unwrap();

    // Reading through the symlink must not escape the root.
    let r = NotePath::new("notes/escape/secret.md").unwrap();
    assert!(store.read(&r).is_err(), "read escaped the cairn via symlink");

    // Writing through the symlink must not escape the root, and must not
    // land a file outside the cairn.
    let w = NotePath::new("notes/escape/pwned.md").unwrap();
    assert!(store.write(&w, "x").is_err(), "write escaped the cairn via symlink");
    assert!(
        !outside.path().join("pwned.md").exists(),
        "write landed outside the cairn"
    );
}

#[cfg(unix)]
#[test]
fn in_root_symlink_resolves_correctly() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let mut store = LocalFsStore::open(tmp.path()).unwrap();
    let real = NotePath::new("real/a.md").unwrap();
    store.write(&real, "hello").unwrap();
    // A directory symlink that stays inside the root resolves normally.
    symlink(tmp.path().join("real"), tmp.path().join("link")).unwrap();
    let via = NotePath::new("link/a.md").unwrap();
    assert_eq!(store.read(&via).unwrap(), "hello");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-infra symlink_escaping_root_cannot_be_read_or_written`
Expected: FAIL — current `read`/`write` follow the symlink (`read` returns `Ok("TOPSECRET")`, `write` succeeds and creates a file outside the cairn).

- [ ] **Step 3: Add a canonical root and implement `resolve`**

Add `canonical_root: PathBuf` to the struct; set it in `open` after `create_dir_all`:

```rust
pub fn open(root: impl AsRef<Path>) -> Result<Self, PortError> {
    let root = root.as_ref().to_path_buf();
    fs::create_dir_all(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
    let canonical_root =
        fs::canonicalize(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
    Ok(Self {
        root,
        canonical_root,
    })
}
```

Replace `safe_full` with `resolve` (keep `full` and `is_safe_rel` as-is):

```rust
/// Resolve `path` under the root, following and validating symlinks so a
/// symlink planted inside the cairn cannot redirect an operation outside the
/// root (and closing the TOCTOU gap left by purely lexical validation).
///
/// 1. Lexical guard ([`is_safe_rel`]) — defense in depth behind `NotePath`.
/// 2. The final component must not itself be a symlink (O_NOFOLLOW-style): a
///    leaf symlink could redirect a write/delete/rename through to another
///    file.
/// 3. The deepest ancestor that exists on disk is canonicalized — resolving
///    every symlink in that prefix — and must stay under the canonical root.
///    Not-yet-existing tail components cannot be symlinks, so the original
///    target is returned for the caller to create.
fn resolve(&self, path: &NotePath) -> Result<PathBuf, PortError> {
    if !is_safe_rel(path.as_str()) {
        return Err(PortError::Adapter(format!(
            "unsafe note path: {}",
            path.as_str()
        )));
    }
    let full = self.full(path);

    if full
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(PortError::Adapter(format!(
            "note path is a symlink: {}",
            path.as_str()
        )));
    }

    // Walk up to the deepest entry that exists on disk (lstat, so a broken
    // symlink counts as existing and is caught by the canonicalize below).
    let mut ancestor = full.as_path();
    while ancestor.symlink_metadata().is_err() {
        ancestor = ancestor.parent().ok_or_else(|| {
            PortError::Adapter(format!("cannot resolve note path: {}", path.as_str()))
        })?;
    }
    let canon = fs::canonicalize(ancestor).map_err(|e| PortError::Adapter(e.to_string()))?;
    if !canon.starts_with(&self.canonical_root) {
        return Err(PortError::Adapter(format!(
            "note path escapes cairn root: {}",
            path.as_str()
        )));
    }
    Ok(full)
}
```

- [ ] **Step 4: Route the four ops through `resolve`**

`read` now resolves first (a missing note still maps to `NotFound`):

```rust
fn read(&self, path: &NotePath) -> Result<String, PortError> {
    let full = self.resolve(path)?;
    fs::read_to_string(full).map_err(|_| PortError::NotFound(path.as_str().to_string()))
}
```

In `write`, `delete`, and `rename`, replace each `self.safe_full(...)` call with `self.resolve(...)` (signatures and the rest of each body unchanged).

- [ ] **Step 5: Run the new tests to verify they pass**

Run: `cargo test -p cairn-infra symlink`
Expected: PASS (both symlink tests).

- [ ] **Step 6: Run the full crate test suites**

Run: `cargo test -p cairn-infra -p cairn-domain`
Expected: PASS — existing tests (`write_read_list_roundtrip`, `rename_moves_into_subdir_and_refuses_clobber`, `is_safe_rel_*`, etc.) still green.

- [ ] **Step 7: Lint**

Run: `cargo clippy -p cairn-infra --all-targets`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-infra/src/localfs.rs docs/superpowers/plans/2026-06-11-symlink-traversal-containment.md
git commit -m "fix(security): canonicalize note paths to contain symlink escapes (audit S?)"
```
