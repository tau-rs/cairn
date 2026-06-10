# NotePath RCE Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Critical note-write → arbitrary-code-execution path (S1) by rejecting dot-leading path segments in `NotePath::new` and adding a defense-in-depth containment check in `LocalFsStore`.

**Architecture:** Two layers. (1) Domain invariant: `NotePath` is the single chokepoint every write surface (CLI, daemon `/command`, `fs:write` plugins) passes through, so hardening its constructor closes the vector everywhere at once. (2) Defense in depth: `LocalFsStore` mutation methods (`write`/`rename`/`delete`) re-verify the resolved path stays within `root` and never names a dot segment, so the store is safe even if a future caller bypasses `NotePath`. The plugin-loader trust boundary is explicitly **out of scope** (separate finding `03-cairn-plugin-trust`) — closing the write vector is sufficient to stop the RCE.

**Tech Stack:** Rust, `thiserror`, `std::path::Component`, `tempfile` (dev).

---

### Task 1: Reject dot-leading segments in `NotePath::new`

A cairn is "just markdown files" — notes are `.md` files at normal relative paths. No legitimate note has a path segment starting with `.` (verified: every `NotePath::new` callsite uses plain paths; `.cairn`/`.git` are reached directly via `root.join`, never through `NotePath`). Rejecting dot-leading segments blocks `.cairn/plugins/<x>/manifest.toml`, `.git/config`, `.`, and any hidden dotfile, while keeping `..` mapped to the existing `Escapes` error for backward compatibility.

**Files:**
- Modify: `crates/cairn-domain/src/note.rs:15-27` (`NotePath::new`), `:44-56` (`NotePathError`)
- Test: `crates/cairn-domain/src/note.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/cairn-domain/src/note.rs`:

```rust
#[test]
fn rejects_dot_leading_segments() {
    // The RCE vectors from the security audit (S1).
    assert_eq!(
        NotePath::new(".cairn/plugins/evil/manifest.toml"),
        Err(NotePathError::Hidden)
    );
    assert_eq!(NotePath::new(".git/config"), Err(NotePathError::Hidden));
    // A hidden segment anywhere in the path, not just the first.
    assert_eq!(NotePath::new("notes/.git/config"), Err(NotePathError::Hidden));
    assert_eq!(NotePath::new("a/.hidden.md"), Err(NotePathError::Hidden));
    // A lone "." (current-dir) segment.
    assert_eq!(NotePath::new("./a.md"), Err(NotePathError::Hidden));
    // Backslash-normalized variant still caught.
    assert_eq!(NotePath::new(r".cairn\x"), Err(NotePathError::Hidden));
    // ".." stays mapped to the pre-existing Escapes error.
    assert_eq!(NotePath::new("a/../b"), Err(NotePathError::Escapes));
    // Ordinary notes still accepted.
    assert_eq!(NotePath::new("dir/a.md").unwrap().as_str(), "dir/a.md");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-domain rejects_dot_leading_segments`
Expected: FAIL — `NotePathError::Hidden` does not exist (compile error), and the constructor accepts the dot paths.

- [ ] **Step 3: Add the `Hidden` error variant**

In `crates/cairn-domain/src/note.rs`, add to `enum NotePathError` (after `Escapes`):

```rust
    /// A path segment began with `.` (e.g. `.git`, `.cairn`, a dotfile).
    /// These are never notes and could escape into control directories.
    #[error("note path must not contain dot-leading segments")]
    Hidden,
```

- [ ] **Step 4: Harden the constructor**

Replace the body of `NotePath::new` (`crates/cairn-domain/src/note.rs:15-27`):

```rust
    pub fn new(raw: &str) -> Result<Self, NotePathError> {
        let norm = raw.replace('\\', "/");
        if norm.is_empty() {
            return Err(NotePathError::Empty);
        }
        if norm.starts_with('/') {
            return Err(NotePathError::Absolute);
        }
        for seg in norm.split('/') {
            if seg == ".." {
                return Err(NotePathError::Escapes);
            }
            if seg.starts_with('.') {
                return Err(NotePathError::Hidden);
            }
        }
        Ok(Self(norm))
    }
```

Also update the `# Errors` doc comment above `new` to mention dot-leading segments.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p cairn-domain`
Expected: PASS — new test green, existing `rejects_absolute_and_escaping_paths` still green (`../secret` → `Escapes`).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-domain/src/note.rs
git commit -m "fix(domain): reject dot-leading NotePath segments (RCE S1)"
```

---

### Task 2: Defense-in-depth containment check in `LocalFsStore`

Even with `NotePath` hardened, the store should never write outside `root` or into a control directory. Because hardened `NotePath::new` blocks hostile inputs, the guard's logic cannot be exercised through the public constructor — so factor it into a free function `is_safe_rel(&str)` that takes a raw relative path string and can be unit-tested directly with malicious strings. `safe_full` calls it; `write`/`rename`/`delete` route through `safe_full`.

**Files:**
- Modify: `crates/cairn-infra/src/localfs.rs:41-43` (add free fn + helper near `full`), `:83-114` (`write`/`rename`/`delete`)
- Test: `crates/cairn-infra/src/localfs.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/cairn-infra/src/localfs.rs`:

```rust
#[test]
fn is_safe_rel_rejects_control_and_escaping_paths() {
    // The RCE vectors (S1) — even if a caller bypasses NotePath::new.
    assert!(!is_safe_rel(".cairn/plugins/evil/manifest.toml"));
    assert!(!is_safe_rel(".git/config"));
    assert!(!is_safe_rel("notes/.git/config"));
    assert!(!is_safe_rel("a/../../etc/passwd"));
    assert!(!is_safe_rel("../escape"));
    assert!(!is_safe_rel("/absolute"));
    assert!(!is_safe_rel(""));
    // Backslash separators are treated as separators too.
    assert!(!is_safe_rel(r".cairn\x"));
    assert!(!is_safe_rel(r"a\..\..\x"));
    // Ordinary note paths pass.
    assert!(is_safe_rel("dir/a.md"));
    assert!(is_safe_rel("a.md"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-infra is_safe_rel`
Expected: FAIL — `is_safe_rel` does not exist (compile error).

- [ ] **Step 3: Add `is_safe_rel`, `safe_full`, and route mutations through it**

In `crates/cairn-infra/src/localfs.rs`, add this free function (module level, above `impl LocalFsStore`):

```rust
/// Whether `rel` (a relative path under the vault root) is safe to resolve:
/// non-empty, not absolute, and with no `..` or dot-leading segment. Defense
/// in depth behind `NotePath::new` — a crafted path that bypassed domain
/// validation still cannot escape the root or name a control directory
/// (`.cairn`, `.git`) or dotfile. Splits on both separators so a stray
/// backslash cannot smuggle a segment past the check.
fn is_safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.starts_with('\\')
        && rel
            .split(['/', '\\'])
            .all(|seg| seg != ".." && !seg.starts_with('.'))
}
```

Add this method to `impl LocalFsStore` (next to `full`):

```rust
    /// Resolve `path` under the root, refusing anything `is_safe_rel` rejects.
    /// Used by every mutating operation; read-only paths use `full`.
    fn safe_full(&self, path: &NotePath) -> Result<PathBuf, PortError> {
        if !is_safe_rel(path.as_str()) {
            return Err(PortError::Adapter(format!(
                "unsafe note path: {}",
                path.as_str()
            )));
        }
        Ok(self.full(path))
    }
```

Then in `write`, `rename`, and `delete`, replace `self.full(...)` with `self.safe_full(...)?`:
- `write`: `let full = self.safe_full(path)?;`
- `rename`: `let src = self.safe_full(from)?;` and `let dst = self.safe_full(to)?;`
- `delete`: `fs::remove_file(self.safe_full(path)?)`

(Leave read-only `read`/`stamp` on `self.full` — they cannot create or plant anything.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p cairn-infra`
Expected: PASS — all existing store tests (`write_read_list_roundtrip`, `rename_*`, etc.) still green, new test green.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-infra/src/localfs.rs
git commit -m "fix(infra): defense-in-depth path containment in LocalFsStore (RCE S1)"
```

---

### Task 3: Full-suite verification

- [ ] **Step 1: Run the whole workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no failures.

- [ ] **Step 2: Lint**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

---

## Self-Review

- **Spec coverage:** S1 recommendation has two halves — (a) harden `NotePath::new` to reject dot-leading segments → Task 1; (b) `LocalFsStore` assert resolved path stays within root / excludes control dirs before write/rename/delete → Task 2. Both covered. Plugin-loader change deliberately excluded per brief scope constraint.
- **Regression variants closed:** `.cairn/...`, `.git/config`, nested `notes/.git/...`, dotfile `a/.hidden.md`, `./a.md`, backslash `.cairn\x`, and `..` (still `Escapes`) — all asserted in Task 1.
- **Error style:** new `NotePathError::Hidden` mirrors existing `thiserror` variants; store guard reuses `PortError::Adapter` (the crate's catch-all), consistent with surrounding code.
- **Type consistency:** `safe_full` returns `Result<PathBuf, PortError>`, matching `full`'s return shape plus the error channel already used throughout the file.
