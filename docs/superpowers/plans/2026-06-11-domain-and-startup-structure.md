# Domain Validation & Startup Structure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden `NotePath` against Windows drive/UNC roots (S7), de-duplicate the
copy-pasted "open an existing cairn + build engine" startup between CLI and daemon
(D8), and evaluate collapsing the engine port generics (D5).

**Architecture:** S7 is a lexical, platform-independent guard inside
`NotePath::new`. D8 extracts a new `cairn-startup` composition-root crate (depends
on `cairn-app` + `cairn-infra`) holding the `.git` "is this a cairn?" check and the
in-memory engine builder; CLI and daemon route through it. D5 is evaluated and
**deferred** (see rationale) to keep this PR coherent and low-risk.

**Tech Stack:** Rust, hexagonal crates, thiserror at boundaries, TDD.

---

## Design decisions

- **S7 uses lexical checks, NOT `std::path::Component`.** The brief suggests
  `Component` semantics, but `Path::new("C:\\secret")` parses `C:` as a *normal*
  component when compiled on a non-Windows host — so `Component` would NOT reject a
  Windows drive path on the Linux CI/dev box. A cairn's notes are portable across
  platforms, so the rejection must be platform-independent. We reject lexically:
  a leading `<ascii-letter>:` drive prefix. UNC roots (`\\host\share`) already fall
  out via the existing leading-`/` check after backslash normalization
  (`//host/share`); we add a test to lock that in. Reuse the existing
  `NotePathError::Absolute` variant — a drive/UNC root *is* absolute.

- **D8 lands in a new `cairn-startup` crate, NOT `cairn-app`.** `cairn-app` is the
  inner hexagon and must not depend on `cairn-infra` (concrete adapters) — that
  would point a dependency outward. Engine *construction* wires concrete adapters,
  so it belongs in a composition-root layer. `cairn-startup` depends on
  `cairn-app` + `cairn-infra` + `cairn-domain`, owns the `CairnEngine` type alias
  (moved from `cairn-daemon`), and exposes `ensure_cairn` + `build_engine`.

- **D5 is IMPLEMENTED (per user direction "do all in the spec").** All three ports
  (`VaultStore`, `SearchIndex`, `Vcs`) are object-safe (verified: `&self`/`&mut self`
  receivers, concrete args, no generic methods / `Self` returns / associated items),
  so the collapse is sound. `Engine` becomes non-generic, holding
  `Box<dyn VaultStore + Send>` / `Box<dyn SearchIndex + Send>` / `Box<dyn Vcs + Send>`
  (mirroring the existing `Box<dyn PluginHost>`). `Engine::new` stays generic over
  `impl Port + Send + 'static` and boxes internally, so the dozens of
  `Engine::new(store, index, vcs)` call sites compile unchanged; only explicit
  `Engine<…,…,…>` *type annotations* (test helpers, `dispatch_*` signatures, the
  daemon's `CairnEngine` alias) collapse to bare `Engine`. The `+ Send` boxes
  preserve `Engine: Send`, which the daemon's `Arc<Mutex<…>>` requires. The
  now-pointless `CairnEngine` alias is removed (it was the very "pin all three
  generics to one tuple" D5 targets). Existing suite proves no behavioral change.

---

## Task 1: S7 — reject Windows drive/UNC roots in `NotePath::new`

**Files:**
- Modify/Test: `crates/cairn-domain/src/note.rs`

- [ ] **Step 1: Write failing tests** in the existing `tests` module of `note.rs`,
  alongside `rejects_absolute_and_escaping_paths`:

```rust
    #[test]
    fn rejects_windows_drive_and_unc_roots() {
        // Drive-absolute: `C:\secret` -> normalized `C:/secret`. The pre-existing
        // `starts_with('/')` check misses this; a lexical drive-prefix guard catches it.
        assert_eq!(NotePath::new(r"C:\secret"), Err(NotePathError::Absolute));
        // Drive-relative (`C:secret`, no separator) is equally not a note path.
        assert_eq!(NotePath::new("C:secret"), Err(NotePathError::Absolute));
        // Lowercase drive letter.
        assert_eq!(NotePath::new(r"d:\x"), Err(NotePathError::Absolute));
        // UNC root: `\\host\share` -> `//host/share`, already absolute. Locked in here.
        assert_eq!(NotePath::new(r"\\host\share"), Err(NotePathError::Absolute));
        // A colon mid-segment is NOT a drive prefix and stays accepted (only a
        // leading `<letter>:` is a drive spec).
        assert_eq!(NotePath::new("a/b.md").unwrap().as_str(), "a/b.md");
    }
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p cairn-domain rejects_windows_drive_and_unc_roots`
Expected: FAIL — `C:secret` / `C:\secret` currently `Ok`.

- [ ] **Step 3: Implement** — add the guard after the `starts_with('/')` check in
  `NotePath::new`:

```rust
        if norm.starts_with('/') {
            return Err(NotePathError::Absolute);
        }
        // A Windows drive prefix (`C:\foo` -> `C:/foo`, or drive-relative `C:foo`).
        // `std::path::Component` would not catch this on a non-Windows host (it
        // parses `C:` as a normal component there), so check lexically — a cairn
        // authored on any platform must reject the same paths.
        let bytes = norm.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(NotePathError::Absolute);
        }
```

- [ ] **Step 4: Run, expect PASS** (and the full domain suite)

Run: `cargo test -p cairn-domain`
Expected: PASS.

- [ ] **Step 5: Commit** (`feat(domain): reject Windows drive/UNC note paths (audit S7)`)

---

## Task 2: D8 — create `cairn-startup` crate

**Files:**
- Create: `crates/cairn-startup/Cargo.toml`
- Create: `crates/cairn-startup/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`)

- [ ] **Step 1:** Add `"crates/cairn-startup"` to workspace `members` in root `Cargo.toml`.

- [ ] **Step 2:** Create `crates/cairn-startup/Cargo.toml`:

```toml
[package]
name = "cairn-startup"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-app = { path = "../cairn-app" }
cairn-infra = { path = "../cairn-infra" }

[dev-dependencies]
tempfile = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 3: Write `src/lib.rs`** with the shared helpers + a failing-first test
  (the test compiles only once the crate exists, so it doubles as the build gate):

```rust
//! Composition-root helpers shared by the `cairn` CLI and `cairn-daemon`:
//! detecting an existing cairn and constructing the engine from concrete adapters.

use std::path::Path;

use cairn_app::Engine;
use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};

/// The concrete engine both binaries run: local-filesystem store, Tantivy index,
/// git VCS. (Moved here from `cairn-daemon` so it has a single home.)
pub type CairnEngine = Engine<LocalFsStore, TantivyIndex, GitVcs>;

/// Failures starting up against a cairn directory.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// `root` is not an initialized cairn (no `.git`).
    #[error("not a cairn at {path} (run `cairn --cairn {path} init` first)")]
    NotACairn { path: String },
    /// A concrete adapter failed to open.
    #[error("{0}")]
    Build(String),
}

/// True if `root` looks like an initialized cairn. `.git` is a directory in a
/// normal repo but a file in worktrees/submodules, so test existence, not type.
#[must_use]
pub fn is_cairn(root: &Path) -> bool {
    root.join(".git").exists()
}

/// Error unless `root` is an existing cairn. Only `cairn init` may create one, so
/// callers gate every other command on this rather than silently `git init`-ing.
///
/// # Errors
/// [`StartupError::NotACairn`] if `root` has no `.git`.
pub fn ensure_cairn(root: &Path) -> Result<(), StartupError> {
    if is_cairn(root) {
        Ok(())
    } else {
        Err(StartupError::NotACairn {
            path: root.display().to_string(),
        })
    }
}

/// Build the in-memory-index engine from a cairn `root` (store + git + ephemeral
/// Tantivy index). The daemon's persistent path constructs its engine separately.
///
/// # Errors
/// [`StartupError::Build`] if any adapter fails to open.
pub fn build_engine(root: &Path) -> Result<CairnEngine, StartupError> {
    let store = LocalFsStore::open(root).map_err(|e| StartupError::Build(e.to_string()))?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| StartupError::Build(e.to_string()))?;
    let index = TantivyIndex::in_memory().map_err(|e| StartupError::Build(e.to_string()))?;
    Ok(Engine::new(store, index, vcs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_cairn_distinguishes_initialized_and_not() {
        let tmp = tempfile::tempdir().unwrap();
        // A bare directory is not a cairn.
        assert!(!is_cairn(tmp.path()));
        let err = ensure_cairn(tmp.path()).unwrap_err();
        assert!(matches!(err, StartupError::NotACairn { .. }));
        assert!(err.to_string().contains("not a cairn"));

        // After a git init, it is — and the engine builds against it.
        GitVcs::open_or_init(tmp.path()).unwrap();
        assert!(is_cairn(tmp.path()));
        ensure_cairn(tmp.path()).unwrap();
        build_engine(tmp.path()).unwrap();
    }
}
```

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p cairn-startup`
Expected: PASS (2 cases in one test).

- [ ] **Step 5: Commit** (`feat(startup): add cairn-startup composition-root crate (audit D8)`)

---

## Task 3: D8 — route CLI and daemon through `cairn-startup`

**Files:**
- Modify: `crates/cairn-cli/Cargo.toml`, `crates/cairn-cli/src/main.rs`
- Modify: `crates/cairn-daemon/Cargo.toml`, `crates/cairn-daemon/src/main.rs`,
  `crates/cairn-daemon/src/lib.rs`

- [ ] **Step 1 (CLI Cargo):** add `cairn-startup = { path = "../cairn-startup" }` to
  `crates/cairn-cli/Cargo.toml` `[dependencies]`.

- [ ] **Step 2 (CLI main):** delete the local `build_engine` fn
  (`crates/cairn-cli/src/main.rs:131-136`) and replace the imports/usage:
  - Drop now-unused infra imports of `GitVcs, LocalFsStore, TantivyIndex` from the
    `cairn_infra` use (keep `NotifyWatcher`); drop the `Engine` import if unused.
  - Add `use cairn_startup::{build_engine, ensure_cairn};`.
  - Replace the inline `.git` check (lines 146-151) with:

```rust
    if !matches!(cli.command, Command::Init) {
        ensure_cairn(&root).map_err(|e| e.to_string())?;
    }
```

  - `let mut engine = build_engine(&root).map_err(|e| e.to_string())?;`

- [ ] **Step 3 (daemon Cargo):** add `cairn-startup = { path = "../cairn-startup" }`.

- [ ] **Step 4 (daemon lib):** replace the local `CairnEngine` definition
  (`crates/cairn-daemon/src/lib.rs:33-34`) with a re-export:

```rust
pub use cairn_startup::CairnEngine;
```

  Remove the now-unused `Engine` / `GitVcs, LocalFsStore, TantivyIndex` imports from
  lib.rs **only if** they become unused (the test module still constructs engines —
  keep imports it needs; adjust to satisfy the compiler/clippy).

- [ ] **Step 5 (daemon main):** delete the local `build_engine` fn
  (`crates/cairn-daemon/src/main.rs:40-45`), `use cairn_startup::{build_engine, ensure_cairn};`,
  replace the inline `.git` check (lines 49-56) with
  `ensure_cairn(&cli.cairn).map_err(|e| e.to_string())?;`, and in the `else`
  (no-persist) branch call `build_engine(&cli.cairn).map_err(|e| e.to_string())?`.
  Trim now-unused `cairn_infra` imports in main (keep `GitVcs, LocalFsStore,
  TantivyIndex` — the persistent branch still uses them directly; keep
  `NotifyWatcher`).

- [ ] **Step 6: Build + full workspace test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS, including `crates/cairn-cli/tests/cli.rs`
(`commands_require_an_initialized_cairn`) and the daemon suite — these are the
behavioral proof that "open a cairn" / "not a cairn" still work end-to-end.

- [ ] **Step 7: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit** (`refactor(startup): route CLI + daemon through cairn-startup (audit D8)`)

---

## Task 4: Verification & review

- [ ] `cargo test --workspace` green (capture real output).
- [ ] `cargo build --workspace` — both `cairn` and `cairn-daemon` binaries build.
- [ ] Manual smoke: `cairn --cairn /tmp/x init` then `cairn --cairn /tmp/x list`
  succeeds; `cairn --cairn /tmp/empty list` prints `not a cairn`.
- [ ] `requesting-code-review`.
- [ ] Commit, push, `gh pr create -R tau-rs/cairn --base main`, cite S7/D8 and the
  D5 deferral. STOP — no merge.
```
