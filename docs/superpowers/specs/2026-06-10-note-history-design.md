# Git-backed note history

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation

## Goal

Expose the git backing that is cairn's canonical at-rest store: view a note's
**commit history**, read its **content at a past revision**, and **restore** an old
version. Today the `Vcs` port only does `commit_all`; git's rich history is
invisible to the product. This is a fresh engine capability (not more plugins),
on-brand with "git is the canonical store".

Three user-facing operations:
- **history** — list the commits that touched a note (newest first).
- **show** — the note's content at a given revision.
- **restore** — write a past version back as the current note.

Spans the hexagon (port → adapter → engine → contract → dispatch → CLI), following
the existing `SearchHit`(port) → `SearchResult`(contract) pattern.

## Decisions (resolved during brainstorming)

- **All three in one slice.** History + show are read-only; restore is small (it's
  `show` + the existing `write_note`) and the obvious payoff, so they ship together.
- **Restore is a working-tree write, not a git checkout.** `restore_note` reads the
  old content and routes it through the existing `Engine::write_note` (persists,
  reindexes, updates caches, emits `NoteChanged`). It becomes a normal pending
  change the user commits later — consistent with the engine's write-then-commit
  model. No git index/checkout manipulation.
- **Revision is identified by a git revspec string** (short or full hash; `HEAD~1`
  also works) resolved via `revparse_single`.
- **No `--follow`** across renames this slice (history stops at a rename); diffing
  two revisions and branch/remote ops are out of scope.

## Components

### 1. `Vcs` port — `crates/cairn-ports/src/lib.rs`

A wire-agnostic `Revision` type (ports-level, like `SearchHit`) + two read-only
methods on `Vcs`:

```rust
/// One commit in a note's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Short commit id (7 chars).
    pub id: String,
    /// Commit summary (first line of the message).
    pub message: String,
    /// Commit time, seconds since the Unix epoch.
    pub timestamp_secs: i64,
    /// Author name.
    pub author: String,
}

pub trait Vcs {
    fn commit_all(&mut self, message: &str) -> Result<String, PortError>; // existing

    /// Commits that added/changed/removed `path`, newest first.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on a git failure. An empty repo or a never-committed
    /// note yields `Ok(vec![])`.
    fn history(&self, path: &str) -> Result<Vec<Revision>, PortError>;

    /// The note's contents at `revision` (a git revspec: short/full hash, `HEAD~1`…).
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the path doesn't exist at that revision;
    /// [`PortError::Adapter`] on a git failure (e.g. an unknown revision).
    fn show(&self, path: &str, revision: &str) -> Result<String, PortError>;
}
```

`GitVcs` is the **only** `Vcs` implementation (production and tests both use it),
so the two methods are added to the trait and implemented once, in `GitVcs` — no
test stubs to update, no default bodies needed.

### 2. `GitVcs` adapter — `crates/cairn-infra/src/git.rs` (git2)

- **`history`**: open the repo; if there is no HEAD (empty repo) → `Ok(vec![])`.
  Otherwise a `revwalk` from HEAD, `Sort::TIME`. For each commit, compare the blob
  at `path` in the commit's tree against each parent's tree (added/changed/removed
  → the commit touched it; a root commit that contains the path counts). Map each
  kept commit to a `Revision` — `id` = `oid.to_string()[..7]`, `message` =
  `commit.summary().unwrap_or("")`, `timestamp_secs` = `commit.time().seconds()`,
  `author` = `commit.author().name().unwrap_or("")`. Newest first (revwalk default
  from HEAD is newest→oldest with `Sort::TIME`).
- **`show`**: `repo.revparse_single(revision)?` → `peel_to_commit()?` → `tree()?` →
  `get_path(Path::new(path))` (→ `NotFound` mapped from git2's `NotFound`) →
  `to_object(&repo)?.peel_to_blob()?` → `String::from_utf8_lossy(blob.content())`.

A small private helper `commit_touched_path(&repo, &commit, path) -> Result<bool>`
does the tree-blob comparison against the parent(s).

### 3. Engine — `crates/cairn-app/src/lib.rs`

```rust
/// A note's commit history (newest first).
pub fn note_history(&self, path: &NotePath) -> Result<Vec<Revision>, PortError> {
    self.vcs.history(path.as_str())
}

/// A note's contents at a past revision.
pub fn note_at(&self, path: &NotePath, revision: &str) -> Result<String, PortError> {
    self.vcs.show(path.as_str(), revision)
}

/// Restore a note to a past revision: write that revision's contents as the
/// current note (a pending change to be committed later). Emits `NoteChanged`.
pub fn restore_note(
    &mut self,
    path: &NotePath,
    revision: &str,
    sink: &mut dyn EventSink,
) -> Result<(), PortError> {
    let contents = self.vcs.show(path.as_str(), revision)?;
    self.write_note(path, &contents, sink)
}
```

`Revision` is imported from `cairn_ports`. No new field on `Engine` (it already
holds `vcs`).

### 4. Contract — `crates/cairn-contract/src/lib.rs`

A ts-rs `Revision` DTO (mirrors `SearchResult`'s derives) + new query/command
variants:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Revision {
    pub id: String,
    pub message: String,
    pub timestamp_secs: i64,
    pub author: String,
}
```

- `Query::NoteHistory { path: String }` → `QueryResponse::History { revisions: Vec<Revision> }` (new response variant).
- `Query::NoteAt { path: String, revision: String }` → reuses `QueryResponse::Note { contents }`.
- `Command::RestoreNote { path: String, revision: String }` → `CommandResponse::Done`.

### 5. Dispatch — `crates/cairn-service/src/lib.rs`

- `dispatch_query` gains a `NoteHistory` arm (calls `engine.note_history`, maps each
  ports `Revision` → contract `Revision`, returns `History { revisions }`) and a
  `NoteAt` arm (calls `engine.note_at` → `Note { contents }`). Path parsing reuses
  the existing `parse_path` helper.
- `dispatch_command` gains a `RestoreNote` arm (calls `engine.restore_note(path,
  revision, sink)` → `Done`). The existing `From<PortError>` mapping
  (`NotFound`→`NotFound`/404, `Adapter`→`Internal`/500) applies — a bad revision or
  missing-at-rev surfaces correctly.

### 6. CLI — `crates/cairn-cli/src/main.rs`

Three subcommands (clap `Command` enum + dispatch arms):
- `cairn history <path>` → `Query::NoteHistory`; print one line per revision:
  `<id>  <message>` (one per line, newest first). The `timestamp_secs`/`author`
  fields ride in the DTO for the daemon/UI; the CLI omits date formatting to avoid
  pulling in a date dependency (the engine stays dependency-light).
- `cairn show <path> <revision>` → `Query::NoteAt`; print the contents.
- `cairn restore <path> <revision>` → `Command::RestoreNote`; print a confirmation
  (e.g. `restored <path> from <revision>`).

The daemon needs **no wiring change** — the new query/command DTOs flow through
`dispatch_query`/`dispatch_command` and serialize over HTTP automatically (the
`Revision` ts-rs binding is generated for the UI).

## Testing

| Test | Crate | Asserts |
|------|-------|---------|
| `history_lists_commits_newest_first` | cairn-infra (git.rs) | commit a note as "v1" then "v2" → `history` returns 2 revisions, newest (v2's commit) first, with the right messages |
| `history_empty_for_uncommitted` | cairn-infra | a note only in the working tree (or empty repo) → `Ok(vec![])` |
| `show_returns_content_at_revision` | cairn-infra | content at the older revision == "v1", != current "v2"; unknown path at rev → `NotFound` |
| `restore_writes_old_content_and_emits` | cairn-app | write v1 + commit, write v2 + commit, `restore_note` to v1's rev → `read_note` == v1 and a `NoteChanged` was emitted |
| dispatch round-trips | cairn-service | `NoteHistory`/`NoteAt` queries + `RestoreNote` command dispatch correctly (map shapes, `NotFound` on bad rev) |
| `Revision` binding | cairn-contract | the ts-rs export builds (existing binding test/codegen) |

## Out of scope

- `--follow` history across note renames (history stops at a rename this slice).
- Diffing two revisions; viewing the full graph/branches; remote operations.
- A dedicated "restore creates a commit" — restore is a pending change; the user
  commits via the existing `commit` command.

## Unchanged

The plugin host, watcher, search, daemon wiring (flows through dispatch), and all
other crates. `Engine` gains methods but no new fields; `GitVcs` gains methods but
its `commit_all` is untouched.
