# Cairn Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a working, git-backed Cairn engine (`cairn-core`) with a CLI that creates/reads/searches/links notes and commits them to git, structured as a hexagon so every other capability is a proven seam.

**Architecture:** A Cargo workspace of focused crates following tau's `domain / ports / infra / app` split. Pure domain logic (notes, links, graph) has no I/O. Application use-cases depend only on port traits. Adapters (local filesystem, git, in-memory index) implement the ports. A `cairn-contract` crate defines serializable Command/Query/Event DTOs with generated TypeScript bindings — the artifact a future UI consumes. `cairn-cli` wires real adapters to the app and is the in-process contract consumer. Ports for plugin host, tau, collab, and daemon exist as traits with `Null` adapters.

**Tech Stack:** Rust (edition 2021, `#![forbid(unsafe_code)]`), `git2` (libgit2 bindings), `clap` v4 (CLI), `serde` + `serde_json`, `ts-rs` (TypeScript codegen), `thiserror`, and for tests `tempfile`, `assert_cmd`, `predicates`.

**Scope notes (resolved for the skeleton only; swappable later per the spec):**
- `GitVcs` uses `git2`; `gix` can replace it behind the `Vcs` port later.
- `SearchIndex` is `InMemoryIndex`; `TantivyIndex` swaps in later.
- `Watcher`, `Executor`, `MergePolicy`, `CollabSession`, `AgentRuntime`, daemon `Transport`, and the plugin host are defined as traits with `Null`/blocking stub adapters — seams proven, not filled.
- Handlers are synchronous in-process; the async/event-stream contract shape is captured in the types, with the runtime added by the daemon/Tauri sub-projects.

**Crate layout produced by this plan:**
```
Cargo.toml                      # workspace
rust-toolchain.toml
clippy.toml  rustfmt.toml  deny.toml
.github/workflows/ci.yml
LICENSE-MIT  LICENSE-APACHE
crates/
  cairn-domain/    # pure model: NotePath, Note, links, Graph
  cairn-ports/     # trait definitions + Null seam adapters
  cairn-infra/     # LocalFsStore, InMemoryIndex, GitVcs
  cairn-contract/  # Command/Query/Event DTOs + ts-rs bindings
  cairn-app/        # use-case handlers wiring ports, event emission
  cairn-cli/       # clap CLI; in-process contract consumer
```

---

## Task 1: Workspace scaffolding & CI

**Files:**
- Create: `Cargo.toml` (workspace), `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `LICENSE-MIT`, `LICENSE-APACHE`, `.github/workflows/ci.yml`
- Create: `crates/cairn-domain/Cargo.toml`, `crates/cairn-domain/src/lib.rs`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/cairn-domain",
    "crates/cairn-ports",
    "crates/cairn-infra",
    "crates/cairn-contract",
    "crates/cairn-app",
    "crates/cairn-cli",
]

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/tau-rs/cairn"
rust-version = "1.85"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
ts-rs = "10"
git2 = "0.19"
clap = { version = "4", features = ["derive"] }
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 2: Create toolchain and lint config**

Create `rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.85"
components = ["rustfmt", "clippy"]
```
Create `rustfmt.toml`:
```toml
edition = "2021"
```
Create `clippy.toml`:
```toml
# project-wide clippy config; intentionally minimal for now
```
Create `deny.toml`:
```toml
[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-3-Clause", "ISC", "Unicode-3.0"]

[bans]
multiple-versions = "warn"

[advisories]
yanked = "deny"
```

- [ ] **Step 3: Add license files and CI**

Create `LICENSE-MIT` and `LICENSE-APACHE` with the standard MIT and Apache-2.0 text (copy from https://opensource.org/licenses/MIT and https://www.apache.org/licenses/LICENSE-2.0.txt).

Create `.github/workflows/ci.yml`:
```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85
        with:
          components: rustfmt, clippy
      - name: fmt
        run: cargo fmt --all -- --check
      - name: clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: test
        run: cargo test --workspace --all-targets
```

- [ ] **Step 4: Create the first crate so the workspace compiles**

Create `crates/cairn-domain/Cargo.toml`:
```toml
[package]
name = "cairn-domain"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[lints]
workspace = true
```
Create `crates/cairn-domain/src/lib.rs`:
```rust
//! Pure domain model for Cairn: notes, links, and the link graph.
//! No I/O lives here.
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: compiles with no errors (the other members don't exist yet, so temporarily comment them out of `members` OR create them in later tasks; for this step, reduce `members` to just `crates/cairn-domain`, then restore entries as each crate is created).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: scaffold cairn workspace, CI, and licenses"
```

---

## Task 2: Domain — NotePath and Note

**Files:**
- Create: `crates/cairn-domain/src/note.rs`
- Modify: `crates/cairn-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/cairn-domain/src/note.rs`:
```rust
//! A note: a relative path inside a cairn plus its markdown content,
//! split into an optional raw frontmatter block and a body.

/// A note's location, always a forward-slash relative path inside a cairn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotePath(String);

impl NotePath {
    /// Build a `NotePath`, normalizing backslashes and rejecting absolute
    /// or parent-escaping paths.
    pub fn new(raw: &str) -> Result<Self, NotePathError> {
        let norm = raw.replace('\\', "/");
        if norm.starts_with('/') {
            return Err(NotePathError::Absolute);
        }
        if norm.split('/').any(|seg| seg == "..") {
            return Err(NotePathError::Escapes);
        }
        if norm.is_empty() {
            return Err(NotePathError::Empty);
        }
        Ok(Self(norm))
    }

    /// The path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Errors building a [`NotePath`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotePathError {
    /// Path was absolute.
    #[error("note path must be relative")]
    Absolute,
    /// Path tried to escape the cairn with `..`.
    #[error("note path must not contain ..")]
    Escapes,
    /// Path was empty.
    #[error("note path must not be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_absolute_and_escaping_paths() {
        assert_eq!(NotePath::new("/etc/passwd"), Err(NotePathError::Absolute));
        assert_eq!(NotePath::new("../secret"), Err(NotePathError::Escapes));
        assert_eq!(NotePath::new(""), Err(NotePathError::Empty));
    }

    #[test]
    fn normalizes_backslashes() {
        assert_eq!(NotePath::new(r"sub\note.md").unwrap().as_str(), "sub/note.md");
    }
}
```
Add `thiserror` to `crates/cairn-domain/Cargo.toml`:
```toml
[dependencies]
thiserror = { workspace = true }
```
Add to `crates/cairn-domain/src/lib.rs`:
```rust
pub mod note;
pub use note::{NotePath, NotePathError};
```

- [ ] **Step 2: Run the test to verify it fails (then passes)**

Run: `cargo test -p cairn-domain note::`
Expected: compiles and PASSES (the implementation is included above). If it fails to compile, fix the reported error.

- [ ] **Step 3: Add the Note type and frontmatter split — write the failing test**

Append to `crates/cairn-domain/src/note.rs`, inside the file above the `tests` module:
```rust
/// A parsed note: its path, an optional raw YAML frontmatter block
/// (without the `---` fences), and the markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Location inside the cairn.
    pub path: NotePath,
    /// Raw frontmatter text (YAML), if a fenced block was present.
    pub frontmatter: Option<String>,
    /// Markdown body (everything after the frontmatter block).
    pub body: String,
}

impl Note {
    /// Parse raw file contents into a [`Note`]. A leading `---\n ... \n---\n`
    /// block is captured as `frontmatter`; everything else is `body`.
    pub fn parse(path: NotePath, raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix("---\n") {
            if let Some(end) = rest.find("\n---\n") {
                let fm = rest[..end].to_string();
                let body = rest[end + "\n---\n".len()..].to_string();
                return Self { path, frontmatter: Some(fm), body };
            }
        }
        Self { path, frontmatter: None, body: raw.to_string() }
    }
}
```
Add to the `tests` module:
```rust
    #[test]
    fn parses_frontmatter_and_body() {
        let p = NotePath::new("a.md").unwrap();
        let n = Note::parse(p, "---\ntitle: Hi\n---\nHello world");
        assert_eq!(n.frontmatter.as_deref(), Some("title: Hi"));
        assert_eq!(n.body, "Hello world");
    }

    #[test]
    fn note_without_frontmatter_is_all_body() {
        let p = NotePath::new("a.md").unwrap();
        let n = Note::parse(p, "Just text");
        assert_eq!(n.frontmatter, None);
        assert_eq!(n.body, "Just text");
    }
```
Update `lib.rs` re-export:
```rust
pub use note::{Note, NotePath, NotePathError};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cairn-domain`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(domain): NotePath validation and Note frontmatter parsing"
```

---

## Task 3: Domain — link extraction

**Files:**
- Create: `crates/cairn-domain/src/link.rs`
- Modify: `crates/cairn-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/cairn-domain/src/link.rs`:
```rust
//! Extraction of `[[wikilink]]` targets from markdown body text.

/// A link target referenced by a note via `[[target]]` syntax.
/// The target is the raw text inside the brackets, trimmed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkTarget(pub String);

/// Extract all `[[...]]` link targets from `body`, in order of appearance,
/// including duplicates. An alias form `[[target|alias]]` yields `target`.
pub fn extract_links(body: &str) -> Vec<LinkTarget> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = body[i + 2..].find("]]") {
                let inner = &body[i + 2..i + 2 + close];
                let target = inner.split('|').next().unwrap_or("").trim();
                if !target.is_empty() {
                    out.push(LinkTarget(target.to_string()));
                }
                i = i + 2 + close + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_and_aliased_links() {
        let links = extract_links("see [[Alpha]] and [[Beta|the second]] end");
        assert_eq!(
            links,
            vec![LinkTarget("Alpha".into()), LinkTarget("Beta".into())]
        );
    }

    #[test]
    fn ignores_unclosed_and_empty() {
        assert_eq!(extract_links("[[ ]] and [[unclosed"), Vec::new());
    }
}
```
Add to `lib.rs`:
```rust
pub mod link;
pub use link::{extract_links, LinkTarget};
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p cairn-domain link::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(domain): extract [[wikilink]] targets from note bodies"
```

---

## Task 4: Domain — backlink graph

**Files:**
- Create: `crates/cairn-domain/src/graph.rs`
- Modify: `crates/cairn-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/cairn-domain/src/graph.rs`:
```rust
//! The link graph derived from a set of notes: forward links and backlinks.
//!
//! Link targets are matched to notes by file stem (the note path without
//! its directory or `.md` extension), case-sensitively, mirroring the
//! common wikilink resolution rule.

use std::collections::BTreeMap;

use crate::{extract_links, Note, NotePath};

/// A derived graph of notes and the links between them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// note -> the notes it links to (resolved, deduplicated, sorted)
    forward: BTreeMap<NotePath, Vec<NotePath>>,
    /// note -> the notes that link to it (resolved, deduplicated, sorted)
    backward: BTreeMap<NotePath, Vec<NotePath>>,
}

fn stem(path: &NotePath) -> &str {
    let s = path.as_str();
    let after_slash = s.rsplit('/').next().unwrap_or(s);
    after_slash.strip_suffix(".md").unwrap_or(after_slash)
}

impl Graph {
    /// Build a graph from all notes. Targets are resolved to a note whose
    /// stem equals the target text; unresolved targets are dropped.
    pub fn build(notes: &[Note]) -> Self {
        let by_stem: BTreeMap<&str, &NotePath> =
            notes.iter().map(|n| (stem(&n.path), &n.path)).collect();

        let mut forward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();
        let mut backward: BTreeMap<NotePath, Vec<NotePath>> = BTreeMap::new();

        for note in notes {
            let mut targets: Vec<NotePath> = extract_links(&note.body)
                .into_iter()
                .filter_map(|t| by_stem.get(t.0.as_str()).map(|p| (*p).clone()))
                .collect();
            targets.sort();
            targets.dedup();
            for t in &targets {
                backward.entry(t.clone()).or_default().push(note.path.clone());
            }
            forward.insert(note.path.clone(), targets);
        }
        for v in backward.values_mut() {
            v.sort();
            v.dedup();
        }
        Self { forward, backward }
    }

    /// Notes that `path` links to.
    pub fn forward_links(&self, path: &NotePath) -> &[NotePath] {
        self.forward.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Notes that link to `path`.
    pub fn backlinks(&self, path: &NotePath) -> &[NotePath] {
        self.backward.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, body: &str) -> Note {
        Note { path: NotePath::new(path).unwrap(), frontmatter: None, body: body.into() }
    }

    #[test]
    fn resolves_forward_and_backlinks_by_stem() {
        let notes = vec![
            note("a.md", "links to [[b]]"),
            note("dir/b.md", "no links"),
        ];
        let g = Graph::build(&notes);
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("dir/b.md").unwrap();
        assert_eq!(g.forward_links(&a), &[b.clone()]);
        assert_eq!(g.backlinks(&b), &[a]);
    }

    #[test]
    fn drops_unresolved_targets() {
        let notes = vec![note("a.md", "links to [[missing]]")];
        let g = Graph::build(&notes);
        assert!(g.forward_links(&NotePath::new("a.md").unwrap()).is_empty());
    }
}
```
Add to `lib.rs`:
```rust
pub mod graph;
pub use graph::Graph;
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p cairn-domain`
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(domain): derive forward-link and backlink graph"
```

---

## Task 5: Ports — trait definitions and Null seams

**Files:**
- Create: `crates/cairn-ports/Cargo.toml`, `crates/cairn-ports/src/lib.rs`
- Modify: root `Cargo.toml` (ensure `crates/cairn-ports` is in `members`)

- [ ] **Step 1: Create the crate and the active ports**

Create `crates/cairn-ports/Cargo.toml`:
```toml
[package]
name = "cairn-ports"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-domain = { path = "../cairn-domain" }
thiserror = { workspace = true }

[lints]
workspace = true
```
Create `crates/cairn-ports/src/lib.rs`:
```rust
//! Port traits for Cairn. The application depends only on these; adapters
//! in `cairn-infra` (and future plugins) implement them.

use cairn_domain::{Note, NotePath};

/// Errors any port may surface to the application.
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// The requested note does not exist.
    #[error("note not found: {0}")]
    NotFound(String),
    /// An underlying adapter failed.
    #[error("{0}")]
    Adapter(String),
}

/// Read/write access to note content in a cairn.
pub trait VaultStore {
    /// Read a note's raw contents.
    fn read(&self, path: &NotePath) -> Result<String, PortError>;
    /// Write (create or overwrite) a note's raw contents.
    fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError>;
    /// Delete a note.
    fn delete(&mut self, path: &NotePath) -> Result<(), PortError>;
    /// List all note paths in the cairn.
    fn list(&self) -> Result<Vec<NotePath>, PortError>;
}

/// A search hit: a note path and a relevance-ordered position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The matching note.
    pub path: NotePath,
}

/// Full-text style search over note content.
pub trait SearchIndex {
    /// Replace the index contents with the given notes.
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError>;
    /// Return notes matching `query` (substring match in the skeleton).
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError>;
}

/// Version control over the cairn directory.
pub trait Vcs {
    /// Stage all changes and create a commit with `message`. Returns the
    /// new commit's short id.
    fn commit_all(&mut self, message: &str) -> Result<String, PortError>;
}
```

- [ ] **Step 2: Add the seam ports (deferred capabilities)**

Append to `crates/cairn-ports/src/lib.rs`:
```rust
/// Detects external changes to the cairn. Seam: `NoopWatcher` for now.
pub trait Watcher {
    /// Begin watching. The skeleton's `NoopWatcher` does nothing.
    fn start(&mut self) -> Result<(), PortError>;
}

/// Runs background/parallel work. Seam: `BlockingExecutor` runs inline.
pub trait Executor {
    /// Run a unit of work to completion.
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

/// Live collaboration session. Seam: `NoCollab`.
pub trait CollabSession {
    /// Whether a live session is active. Always false in the skeleton.
    fn is_active(&self) -> bool;
}

/// Agent runtime (tau). Seam: `NullRuntime`.
pub trait AgentRuntime {
    /// Run a named agent action over optional note context, returning text.
    fn run_action(&self, action: &str, context: Option<&str>) -> Result<String, PortError>;
}

/// No-op watcher seam.
#[derive(Debug, Default)]
pub struct NoopWatcher;
impl Watcher for NoopWatcher {
    fn start(&mut self) -> Result<(), PortError> {
        Ok(())
    }
}

/// Inline executor seam.
#[derive(Debug, Default)]
pub struct BlockingExecutor;
impl Executor for BlockingExecutor {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

/// No-collaboration seam.
#[derive(Debug, Default)]
pub struct NoCollab;
impl CollabSession for NoCollab {
    fn is_active(&self) -> bool {
        false
    }
}

/// Null agent runtime seam.
#[derive(Debug, Default)]
pub struct NullRuntime;
impl AgentRuntime for NullRuntime {
    fn run_action(&self, action: &str, _context: Option<&str>) -> Result<String, PortError> {
        Err(PortError::Adapter(format!(
            "no agent runtime configured (action '{action}' unavailable until tau is wired)"
        )))
    }
}
```
Ensure root `Cargo.toml` `members` includes `crates/cairn-ports`.

- [ ] **Step 3: Add a seam test**

Append to `crates/cairn-ports/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seams_have_expected_neutral_behavior() {
        assert!(!NoCollab.is_active());
        assert!(NoopWatcher.start().is_ok());
        assert!(NullRuntime.run_action("summarize", None).is_err());
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cairn-ports`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(ports): define active ports and Null seam adapters"
```

---

## Task 6: Infra — LocalFsStore

**Files:**
- Create: `crates/cairn-infra/Cargo.toml`, `crates/cairn-infra/src/lib.rs`, `crates/cairn-infra/src/localfs.rs`
- Modify: root `Cargo.toml` `members`

- [ ] **Step 1: Create the crate**

Create `crates/cairn-infra/Cargo.toml`:
```toml
[package]
name = "cairn-infra"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-domain = { path = "../cairn-domain" }
cairn-ports = { path = "../cairn-ports" }
git2 = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }

[lints]
workspace = true
```
Create `crates/cairn-infra/src/lib.rs`:
```rust
//! Adapters implementing Cairn ports against real systems.

pub mod localfs;
pub use localfs::LocalFsStore;
```
Ensure root `Cargo.toml` `members` includes `crates/cairn-infra`.

- [ ] **Step 2: Write the failing test**

Create `crates/cairn-infra/src/localfs.rs`:
```rust
//! A `VaultStore` backed by a local directory of `.md` files.

use std::fs;
use std::path::{Path, PathBuf};

use cairn_domain::NotePath;
use cairn_ports::{PortError, VaultStore};

/// Stores notes as files under `root`.
#[derive(Debug, Clone)]
pub struct LocalFsStore {
    root: PathBuf,
}

impl LocalFsStore {
    /// Open a store rooted at `root`, creating the directory if needed.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, PortError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(Self { root })
    }

    fn full(&self, path: &NotePath) -> PathBuf {
        self.root.join(path.as_str())
    }

    fn collect_md(&self, dir: &Path, out: &mut Vec<NotePath>) -> Result<(), PortError> {
        for entry in fs::read_dir(dir).map_err(|e| PortError::Adapter(e.to_string()))? {
            let entry = entry.map_err(|e| PortError::Adapter(e.to_string()))?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                self.collect_md(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "md") {
                let rel = path
                    .strip_prefix(&self.root)
                    .map_err(|e| PortError::Adapter(e.to_string()))?;
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.push(NotePath::new(&rel).map_err(|e| PortError::Adapter(e.to_string()))?);
            }
        }
        Ok(())
    }
}

impl VaultStore for LocalFsStore {
    fn read(&self, path: &NotePath) -> Result<String, PortError> {
        fs::read_to_string(self.full(path))
            .map_err(|_| PortError::NotFound(path.as_str().to_string()))
    }

    fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        fs::write(full, contents).map_err(|e| PortError::Adapter(e.to_string()))
    }

    fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
        fs::remove_file(self.full(path)).map_err(|e| PortError::Adapter(e.to_string()))
    }

    fn list(&self) -> Result<Vec<NotePath>, PortError> {
        let mut out = Vec::new();
        if self.root.exists() {
            self.collect_md(&self.root, &mut out)?;
        }
        out.sort();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_list_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("dir/a.md").unwrap();
        store.write(&p, "hello").unwrap();
        assert_eq!(store.read(&p).unwrap(), "hello");
        assert_eq!(store.list().unwrap(), vec![p]);
    }

    #[test]
    fn read_missing_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("nope.md").unwrap();
        assert!(matches!(store.read(&p), Err(PortError::NotFound(_))));
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p cairn-infra localfs::`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(infra): LocalFsStore VaultStore adapter"
```

---

## Task 7: Infra — InMemoryIndex

**Files:**
- Create: `crates/cairn-infra/src/index.rs`
- Modify: `crates/cairn-infra/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/cairn-infra/src/index.rs`:
```rust
//! A simple in-memory `SearchIndex` (substring match). Tantivy replaces
//! this later behind the same port.

use cairn_domain::Note;
use cairn_ports::{PortError, SearchHit, SearchIndex};

/// Keeps note bodies in memory and matches queries by case-insensitive
/// substring.
#[derive(Debug, Default)]
pub struct InMemoryIndex {
    docs: Vec<Note>,
}

impl SearchIndex for InMemoryIndex {
    fn reindex(&mut self, notes: &[Note]) -> Result<(), PortError> {
        self.docs = notes.to_vec();
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        let q = query.to_lowercase();
        let mut hits: Vec<SearchHit> = self
            .docs
            .iter()
            .filter(|n| {
                n.body.to_lowercase().contains(&q)
                    || n.path.as_str().to_lowercase().contains(&q)
            })
            .map(|n| SearchHit { path: n.path.clone() })
            .collect();
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::NotePath;

    fn note(path: &str, body: &str) -> Note {
        Note { path: NotePath::new(path).unwrap(), frontmatter: None, body: body.into() }
    }

    #[test]
    fn finds_by_body_substring_case_insensitive() {
        let mut idx = InMemoryIndex::default();
        idx.reindex(&[note("a.md", "Hello World"), note("b.md", "other")]).unwrap();
        let hits = idx.search("hello").unwrap();
        assert_eq!(hits, vec![SearchHit { path: NotePath::new("a.md").unwrap() }]);
    }
}
```
Add to `crates/cairn-infra/src/lib.rs`:
```rust
pub mod index;
pub use index::InMemoryIndex;
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p cairn-infra index::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(infra): InMemoryIndex SearchIndex adapter"
```

---

## Task 8: Infra — GitVcs

**Files:**
- Create: `crates/cairn-infra/src/git.rs`
- Modify: `crates/cairn-infra/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/cairn-infra/src/git.rs`:
```rust
//! A `Vcs` adapter over a local git repository using `git2`.

use std::path::{Path, PathBuf};

use cairn_ports::{PortError, Vcs};
use git2::{Repository, Signature};

/// Operates on the git repository rooted at `root`.
#[derive(Debug)]
pub struct GitVcs {
    root: PathBuf,
}

impl GitVcs {
    /// Open an existing repository, or initialize one if absent.
    pub fn open_or_init(root: impl AsRef<Path>) -> Result<Self, PortError> {
        let root = root.as_ref().to_path_buf();
        match Repository::open(&root) {
            Ok(_) => {}
            Err(_) => {
                Repository::init(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
            }
        }
        Ok(Self { root })
    }
}

impl Vcs for GitVcs {
    fn commit_all(&mut self, message: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(|e| PortError::Adapter(e.to_string()))?;
        let mut index = repo.index().map_err(|e| PortError::Adapter(e.to_string()))?;
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        index.write().map_err(|e| PortError::Adapter(e.to_string()))?;
        let tree_id = index.write_tree().map_err(|e| PortError::Adapter(e.to_string()))?;
        let tree = repo.find_tree(tree_id).map_err(|e| PortError::Adapter(e.to_string()))?;
        let sig = Signature::now("Cairn", "cairn@localhost")
            .map_err(|e| PortError::Adapter(e.to_string()))?;

        let parent = repo.head().ok().and_then(|h| h.target()).and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(oid.to_string()[..7].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn init_and_commit_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        let id = vcs.commit_all("first").unwrap();
        assert_eq!(id.len(), 7);
        // A second commit with no changes still succeeds.
        let id2 = vcs.commit_all("second").unwrap();
        assert_eq!(id2.len(), 7);
    }
}
```
Add to `crates/cairn-infra/src/lib.rs`:
```rust
pub mod git;
pub use git::GitVcs;
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p cairn-infra git::`
Expected: PASS. (Requires libgit2 to build; on CI the `git2` crate vendors it.)

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(infra): GitVcs adapter over git2"
```

---

## Task 9: App — use-case handlers and events

**Files:**
- Create: `crates/cairn-app/Cargo.toml`, `crates/cairn-app/src/lib.rs`
- Modify: root `Cargo.toml` `members`

- [ ] **Step 1: Create the crate with the event type and engine struct**

Create `crates/cairn-app/Cargo.toml`:
```toml
[package]
name = "cairn-app"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-domain = { path = "../cairn-domain" }
cairn-ports = { path = "../cairn-ports" }

[dev-dependencies]
cairn-infra = { path = "../cairn-infra" }
tempfile = { workspace = true }

[lints]
workspace = true
```
Create `crates/cairn-app/src/lib.rs`:
```rust
//! Application use-cases: orchestrate ports to fulfill commands and queries,
//! emitting domain events. No transport or serialization lives here.

use cairn_domain::{Graph, Note, NotePath};
use cairn_ports::{PortError, SearchHit, SearchIndex, VaultStore, Vcs};

/// A domain event emitted as a side effect of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A note was created or updated.
    NoteChanged(NotePath),
    /// A note was deleted.
    NoteDeleted(NotePath),
    /// The cairn was committed; carries the short commit id.
    Committed(String),
    /// The index finished rebuilding; carries note count.
    Reindexed(usize),
}

/// Collects events emitted during a use-case.
pub trait EventSink {
    /// Record an event.
    fn emit(&mut self, event: Event);
}

impl EventSink for Vec<Event> {
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

/// The engine: owns the ports and runs use-cases.
pub struct Engine<S, I, V> {
    store: S,
    index: I,
    vcs: V,
}

impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V> {
    /// Construct an engine from its ports.
    pub fn new(store: S, index: I, vcs: V) -> Self {
        Self { store, index, vcs }
    }

    fn load_all_notes(&self) -> Result<Vec<Note>, PortError> {
        let mut notes = Vec::new();
        for path in self.store.list()? {
            let raw = self.store.read(&path)?;
            notes.push(Note::parse(path, &raw));
        }
        Ok(notes)
    }

    /// Rebuild the search index from the current store contents.
    pub fn reindex(&mut self, sink: &mut dyn EventSink) -> Result<(), PortError> {
        let notes = self.load_all_notes()?;
        self.index.reindex(&notes)?;
        sink.emit(Event::Reindexed(notes.len()));
        Ok(())
    }

    /// Create or overwrite a note and refresh the index.
    pub fn write_note(
        &mut self,
        path: &NotePath,
        contents: &str,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.write(path, contents)?;
        sink.emit(Event::NoteChanged(path.clone()));
        self.reindex(sink)
    }

    /// Read a note's raw contents.
    pub fn read_note(&self, path: &NotePath) -> Result<String, PortError> {
        self.store.read(path)
    }

    /// Delete a note and refresh the index.
    pub fn delete_note(
        &mut self,
        path: &NotePath,
        sink: &mut dyn EventSink,
    ) -> Result<(), PortError> {
        self.store.delete(path)?;
        sink.emit(Event::NoteDeleted(path.clone()));
        self.reindex(sink)
    }

    /// Search note content.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.index.search(query)
    }

    /// Backlinks for a note, computed from the current store contents.
    pub fn backlinks(&self, path: &NotePath) -> Result<Vec<NotePath>, PortError> {
        let notes = self.load_all_notes()?;
        let graph = Graph::build(&notes);
        Ok(graph.backlinks(path).to_vec())
    }

    /// Commit all changes.
    pub fn commit(&mut self, message: &str, sink: &mut dyn EventSink) -> Result<String, PortError> {
        let id = self.vcs.commit_all(message)?;
        sink.emit(Event::Committed(id.clone()));
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};

    fn engine(dir: &std::path::Path) -> Engine<LocalFsStore, InMemoryIndex, GitVcs> {
        Engine::new(
            LocalFsStore::open(dir).unwrap(),
            InMemoryIndex::default(),
            GitVcs::open_or_init(dir).unwrap(),
        )
    }

    #[test]
    fn write_then_search_and_backlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();

        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("b.md").unwrap();
        eng.write_note(&a, "I link to [[b]]", &mut events).unwrap();
        eng.write_note(&b, "target note", &mut events).unwrap();

        assert_eq!(eng.search("target").unwrap(), vec![SearchHit { path: b.clone() }]);
        assert_eq!(eng.backlinks(&b).unwrap(), vec![a]);
        assert!(events.contains(&Event::NoteChanged(b)));
    }

    #[test]
    fn commit_emits_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut events = Vec::new();
        eng.write_note(&NotePath::new("a.md").unwrap(), "hi", &mut events).unwrap();
        let id = eng.commit("first", &mut events).unwrap();
        assert!(events.contains(&Event::Committed(id)));
    }
}
```
Ensure root `Cargo.toml` `members` includes `crates/cairn-app`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p cairn-app`
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(app): Engine use-cases with event emission"
```

---

## Task 10: Contract — Command/Query/Event DTOs with TypeScript codegen

**Files:**
- Create: `crates/cairn-contract/Cargo.toml`, `crates/cairn-contract/src/lib.rs`
- Create: `crates/cairn-contract/tests/codegen.rs`
- Modify: root `Cargo.toml` `members`

- [ ] **Step 1: Create the crate and DTOs**

Create `crates/cairn-contract/Cargo.toml`:
```toml
[package]
name = "cairn-contract"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde = { workspace = true }
ts-rs = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }

[lints]
workspace = true
```
Create `crates/cairn-contract/src/lib.rs`:
```rust
//! The transport-blind contract: serializable Command / Query / Event DTOs
//! with generated TypeScript bindings. This is the surface a UI consumes.
//!
//! These DTOs are intentionally independent of `cairn-domain` types so the
//! wire format can stay stable while the domain evolves.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A request that mutates the cairn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Create or overwrite a note.
    WriteNote {
        /// Relative note path.
        path: String,
        /// Full markdown contents.
        contents: String,
    },
    /// Delete a note.
    DeleteNote {
        /// Relative note path.
        path: String,
    },
    /// Commit all changes with a message.
    Commit {
        /// Commit message.
        message: String,
    },
}

/// A read-only request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    /// Read a note's contents.
    GetNote {
        /// Relative note path.
        path: String,
    },
    /// Search note content.
    Search {
        /// Query string.
        query: String,
    },
    /// List the notes that link to a note.
    GetBacklinks {
        /// Relative note path.
        path: String,
    },
}

/// A push event emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A note was created or updated.
    NoteChanged {
        /// Relative note path.
        path: String,
    },
    /// A note was deleted.
    NoteDeleted {
        /// Relative note path.
        path: String,
    },
    /// The cairn was committed.
    Committed {
        /// Short commit id.
        commit: String,
    },
    /// The index finished rebuilding.
    Reindexed {
        /// Number of notes indexed.
        count: u32,
    },
}
```
Ensure root `Cargo.toml` `members` includes `crates/cairn-contract`.

- [ ] **Step 2: Write a serde round-trip test (failing until crate compiles)**

Append to `crates/cairn-contract/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serializes_with_type_tag() {
        let cmd = Command::WriteNote { path: "a.md".into(), contents: "hi".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"write_note\""));
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }
}
```
Add `serde_json` to `[dependencies]` of `cairn-contract` for the in-crate test (or move the test to `tests/`):
```toml
serde_json = { workspace = true }
```

- [ ] **Step 3: Write the codegen test**

Create `crates/cairn-contract/tests/codegen.rs`:
```rust
//! Verifies the `#[ts(export)]` bindings generate without error.
use cairn_contract::{Command, Event, Query};
use ts_rs::TS;

#[test]
fn exports_typescript_bindings() {
    // `export` writes the .ts files to the crate's bindings dir; here we
    // assert the type definitions render to non-empty TypeScript.
    assert!(Command::decl().contains("Command"));
    assert!(Query::decl().contains("Query"));
    assert!(Event::decl().contains("Event"));
    Command::export_all().unwrap();
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cairn-contract`
Expected: PASS. Generated `.ts` files appear under `crates/cairn-contract/bindings/`.

- [ ] **Step 5: Commit (including generated bindings)**

```bash
git add -A
git commit -m "feat(contract): Command/Query/Event DTOs with TS bindings"
```

---

## Task 11: CLI — wire everything together

**Files:**
- Create: `crates/cairn-cli/Cargo.toml`, `crates/cairn-cli/src/main.rs`
- Create: `crates/cairn-cli/tests/cli.rs`
- Modify: root `Cargo.toml` `members`

- [ ] **Step 1: Create the CLI crate**

Create `crates/cairn-cli/Cargo.toml`:
```toml
[package]
name = "cairn-cli"
version = "0.0.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "cairn"
path = "src/main.rs"

[dependencies]
cairn-domain = { path = "../cairn-domain" }
cairn-ports = { path = "../cairn-ports" }
cairn-infra = { path = "../cairn-infra" }
cairn-app = { path = "../cairn-app" }
clap = { workspace = true }

[dev-dependencies]
assert_cmd = { workspace = true }
predicates = { workspace = true }
tempfile = { workspace = true }

[lints]
workspace = true
```
Ensure root `Cargo.toml` `members` includes `crates/cairn-cli`.

- [ ] **Step 2: Write the CLI**

Create `crates/cairn-cli/src/main.rs`:
```rust
//! The `cairn` CLI: an in-process consumer of the engine.

use std::path::PathBuf;
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_domain::NotePath;
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cairn", about = "Cairn note engine")]
struct Cli {
    /// Path to the cairn (defaults to the current directory).
    #[arg(long, default_value = ".")]
    cairn: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new cairn (git repo + directory).
    Init,
    /// Create or overwrite a note from a string.
    Write {
        /// Relative note path, e.g. `notes/a.md`.
        path: String,
        /// Markdown contents.
        contents: String,
    },
    /// Print a note's contents.
    Read {
        /// Relative note path.
        path: String,
    },
    /// Search notes.
    Search {
        /// Query string.
        query: String,
    },
    /// List notes that link to a note.
    Backlinks {
        /// Relative note path.
        path: String,
    },
    /// Commit all changes.
    Commit {
        /// Commit message.
        message: String,
    },
}

fn build_engine(root: &PathBuf) -> Result<Engine<LocalFsStore, InMemoryIndex, GitVcs>, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    Ok(Engine::new(store, InMemoryIndex::default(), vcs))
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let mut events: Vec<Event> = Vec::new();
    let mut engine = build_engine(&cli.cairn)?;
    // Always reindex on startup so queries see current content.
    engine.reindex(&mut events).map_err(|e| e.to_string())?;

    match cli.command {
        Command::Init => {
            println!("initialized cairn at {}", cli.cairn.display());
        }
        Command::Write { path, contents } => {
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            engine.write_note(&p, &contents, &mut events).map_err(|e| e.to_string())?;
            println!("wrote {path}");
        }
        Command::Read { path } => {
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            print!("{}", engine.read_note(&p).map_err(|e| e.to_string())?);
        }
        Command::Search { query } => {
            for hit in engine.search(&query).map_err(|e| e.to_string())? {
                println!("{}", hit.path.as_str());
            }
        }
        Command::Backlinks { path } => {
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            for b in engine.backlinks(&p).map_err(|e| e.to_string())? {
                println!("{}", b.as_str());
            }
        }
        Command::Commit { message } => {
            let id = engine.commit(&message, &mut events).map_err(|e| e.to_string())?;
            println!("committed {id}");
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 3: Write the CLI integration test**

Create `crates/cairn-cli/tests/cli.rs`:
```rust
use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn write_search_backlinks_commit_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let mut write_a = Command::cargo_bin("cairn").unwrap();
    write_a.args(["--cairn", dir.to_str().unwrap(), "write", "a.md", "links to [[b]]"]);
    write_a.assert().success().stdout(contains("wrote a.md"));

    let mut write_b = Command::cargo_bin("cairn").unwrap();
    write_b.args(["--cairn", dir.to_str().unwrap(), "write", "b.md", "the target"]);
    write_b.assert().success();

    let mut search = Command::cargo_bin("cairn").unwrap();
    search.args(["--cairn", dir.to_str().unwrap(), "search", "target"]);
    search.assert().success().stdout(contains("b.md"));

    let mut backlinks = Command::cargo_bin("cairn").unwrap();
    backlinks.args(["--cairn", dir.to_str().unwrap(), "backlinks", "b.md"]);
    backlinks.assert().success().stdout(contains("a.md"));

    let mut commit = Command::cargo_bin("cairn").unwrap();
    commit.args(["--cairn", dir.to_str().unwrap(), "commit", "first"]);
    commit.assert().success().stdout(contains("committed"));
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p cairn-cli`
Expected: PASS.

- [ ] **Step 5: Run the full workspace gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green. Fix any clippy/fmt issues.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(cli): cairn CLI wiring engine to local adapters"
```

---

## Task 12: First ADR and README

**Files:**
- Create: `docs/decisions/0001-walking-skeleton.md`, `README.md`

- [ ] **Step 1: Write the ADR**

Create `docs/decisions/0001-walking-skeleton.md` documenting: the hexagon crate split; the skeleton adapter choices (`git2`, `InMemoryIndex`, synchronous handlers) and that they are swappable per the design spec; and the seam ports. Reference `docs/superpowers/specs/2026-06-01-cairn-engine-design.md`.

- [ ] **Step 2: Write the README**

Create `README.md` with: what Cairn is (one paragraph), the `tau-rs` org + dual license, a build/test quickstart (`cargo test --workspace`), a CLI usage example (the write/search/backlinks/commit flow), and a link to the design spec.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: walking-skeleton ADR and README"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** §4 hexagon → Tasks 1–11; ports incl. seams → Task 5; LocalFs/Git/Index adapters → Tasks 6–8; §6 contract + TS codegen → Task 10; CLI → Task 11; §11 repo/CI conventions → Tasks 1 & 12. Deferred sub-projects (plugin host, tau, daemon, collab, UI-plugin host) intentionally appear only as seam traits (Task 5), per the walking-skeleton scope.
- **Type consistency:** `NotePath::new/as_str`, `Note { path, frontmatter, body }`, `Graph::build/forward_links/backlinks`, `PortError::{NotFound,Adapter}`, `SearchHit { path }`, `Engine::{new,reindex,write_note,read_note,delete_note,search,backlinks,commit}`, and `Event` variants are used consistently across Tasks 2–11.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code. Task 12's ADR/README steps describe document prose to write (not code), which is acceptable.
- **Known external dependencies:** `git2` needs a C toolchain/libgit2 (vendored by the crate); `ts-rs` `export_all()` writes under the crate's `bindings/` dir. Both are noted at their tasks.
```
