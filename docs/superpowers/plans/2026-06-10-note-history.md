# Git-backed Note History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** View a note's git commit history, read its content at a past revision, and restore an old version.

**Architecture:** Extend the `Vcs` port with `history`/`show` (git2 in `GitVcs`); the engine adds `note_history`/`note_at`/`restore_note` (restore = `show` + the existing `write_note`); the contract gains `NoteHistory`/`NoteAt` queries + a `RestoreNote` command + a ts-rs `Revision` DTO; dispatch + CLI expose them. Daemon flows through unchanged. Follows the existing `SearchHit`(port)→`SearchResult`(contract) mapping pattern.

**Tech Stack:** Rust (workspace, MSRV 1.88, `forbid(unsafe_code)`), git2, serde + ts-rs, clap, nextest, clippy `-D warnings`, 3-OS CI.

**Spec:** `docs/superpowers/specs/2026-06-10-note-history-design.md`

---

## File Structure

| File | Responsibility | Task |
|------|----------------|------|
| `crates/cairn-ports/src/lib.rs` | `Revision` type + `Vcs::history`/`show` | 1 |
| `crates/cairn-infra/src/git.rs` | `GitVcs::history`/`show` (git2) + git tests | 1 |
| `crates/cairn-app/src/lib.rs` | `note_history`/`note_at`/`restore_note` + test | 2 |
| `crates/cairn-contract/src/lib.rs` | `Revision` DTO, `NoteHistory`/`NoteAt`, `History`, `RestoreNote` | 3 |
| `crates/cairn-service/src/lib.rs` | dispatch arms + tests | 3 |
| `crates/cairn-cli/src/main.rs` | `history`/`show`/`restore` subcommands | 4 |

**Unchanged:** the plugin host, watcher, daemon wiring (flows through dispatch).

---

## Task 1: `Vcs` port `history`/`show` + `GitVcs` implementation

**Files:**
- Modify: `crates/cairn-ports/src/lib.rs`
- Modify: `crates/cairn-infra/src/git.rs`

`GitVcs` is the only `Vcs` impl, so the trait + impl land together (the whole workspace still compiles — other crates use `Vcs` generically). TDD the git2 methods.

- [ ] **Step 1: Write the failing GitVcs tests**

In the `#[cfg(test)] mod tests` block of `crates/cairn-infra/src/git.rs`, add:

```rust
    #[test]
    fn history_lists_commits_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("add a v1").unwrap();
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("update a v2").unwrap();
        // An unrelated note's commit must NOT appear in a.md's history.
        fs::write(tmp.path().join("b.md"), "b").unwrap();
        vcs.commit_all("add b").unwrap();

        let hist = vcs.history("a.md").unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].message, "update a v2"); // newest first
        assert_eq!(hist[1].message, "add a v1");
        assert_eq!(hist[0].id.len(), 7);
    }

    #[test]
    fn history_empty_for_uncommitted() {
        let tmp = tempfile::tempdir().unwrap();
        let vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        // No commits at all.
        assert!(vcs.history("a.md").unwrap().is_empty());
        // A file present but never committed (still no commits in the repo).
        fs::write(tmp.path().join("a.md"), "hi").unwrap();
        assert!(vcs.history("a.md").unwrap().is_empty());
    }

    #[test]
    fn show_returns_content_at_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vcs = GitVcs::open_or_init(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.md"), "v1").unwrap();
        vcs.commit_all("v1").unwrap();
        fs::write(tmp.path().join("a.md"), "v2").unwrap();
        vcs.commit_all("v2").unwrap();

        let hist = vcs.history("a.md").unwrap();
        let old = hist[1].id.clone(); // the v1 commit
        assert_eq!(vcs.show("a.md", &old).unwrap(), "v1");
        assert_eq!(vcs.show("a.md", "HEAD").unwrap(), "v2");
        // Unknown path at a revision -> NotFound.
        assert!(matches!(vcs.show("nope.md", "HEAD"), Err(PortError::NotFound(_))));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-infra --lib git::tests::history`
Expected: COMPILE failure — `Revision` / `Vcs::history` / `Vcs::show` don't exist.

- [ ] **Step 3: Add `Revision` + the two `Vcs` methods in ports**

In `crates/cairn-ports/src/lib.rs`, add the `Revision` struct just before `pub trait Vcs`:

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
```

Add the two methods to the `pub trait Vcs` block (after `commit_all`):

```rust
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
```

- [ ] **Step 4: Implement `history`/`show` in `GitVcs`**

In `crates/cairn-infra/src/git.rs`, add `Revision` to the `cairn_ports` import (it becomes `use cairn_ports::{PortError, Revision, Vcs};`). Add a small error helper near the top (after the imports):

```rust
fn adapt<E: std::fmt::Display>(e: E) -> PortError {
    PortError::Adapter(e.to_string())
}

/// Whether `commit` added/changed/removed the blob at `path` (vs its parents).
fn commit_touched_path(
    commit: &git2::Commit,
    path: &Path,
) -> Result<bool, git2::Error> {
    let cur = commit.tree()?.get_path(path).ok().map(|e| e.id());
    if commit.parent_count() == 0 {
        return Ok(cur.is_some()); // root commit: touched iff the path exists
    }
    for i in 0..commit.parent_count() {
        let parent = commit.parent(i)?;
        let prev = parent.tree()?.get_path(path).ok().map(|e| e.id());
        if prev != cur {
            return Ok(true);
        }
    }
    Ok(false)
}
```

Add the two methods inside the `impl Vcs for GitVcs` block (after `commit_all`):

```rust
    fn history(&self, path: &str) -> Result<Vec<Revision>, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let mut walk = repo.revwalk().map_err(adapt)?;
        // No HEAD (empty repo) -> no history.
        if walk.push_head().is_err() {
            return Ok(Vec::new());
        }
        walk.set_sorting(git2::Sort::TIME).map_err(adapt)?;
        let p = Path::new(path);
        let mut revs = Vec::new();
        for oid in walk {
            let oid = oid.map_err(adapt)?;
            let commit = repo.find_commit(oid).map_err(adapt)?;
            if commit_touched_path(&commit, p).map_err(adapt)? {
                revs.push(Revision {
                    id: oid.to_string()[..7].to_string(),
                    message: commit.summary().unwrap_or("").to_string(),
                    timestamp_secs: commit.time().seconds(),
                    author: commit.author().name().unwrap_or("").to_string(),
                });
            }
        }
        Ok(revs)
    }

    fn show(&self, path: &str, revision: &str) -> Result<String, PortError> {
        let repo = Repository::open(&self.root).map_err(adapt)?;
        let obj = repo.revparse_single(revision).map_err(adapt)?;
        let tree = obj.peel_to_commit().map_err(adapt)?.tree().map_err(adapt)?;
        let entry = tree
            .get_path(Path::new(path))
            .map_err(|_| PortError::NotFound(format!("{path} at {revision}")))?;
        let blob = entry
            .to_object(&repo)
            .map_err(adapt)?
            .peel_to_blob()
            .map_err(|_| PortError::NotFound(format!("{path} at {revision} is not a file")))?;
        Ok(String::from_utf8_lossy(blob.content()).into_owned())
    }
```

(The existing `commit_all` and its inline `map_err`s are unchanged — do not refactor them to use `adapt` in this task; just add the helper for the new methods.)

- [ ] **Step 5: Run the tests**

Run: `cargo test -p cairn-infra --lib`
Expected: PASS — the 3 new git tests + the existing `commit_all` tests.

- [ ] **Step 6: Full suite + lint + fmt**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt`.
Expected: all green (other crates use `Vcs` generically, so the trait growth doesn't break them).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-ports/src/lib.rs crates/cairn-infra/src/git.rs
git commit -m "feat(vcs): Vcs::history + show (git-backed note history)"
```

---

## Task 2: Engine `note_history`/`note_at`/`restore_note`

**Files:**
- Modify: `crates/cairn-app/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block of `crates/cairn-app/src/lib.rs`, add:

```rust
    #[test]
    fn restore_writes_old_content_and_emits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let a = NotePath::new("a.md").unwrap();
        let mut events = Vec::new();
        eng.write_note(&a, "v1", &mut events).unwrap();
        eng.commit("v1", &mut events).unwrap();
        eng.write_note(&a, "v2", &mut events).unwrap();
        eng.commit("v2", &mut events).unwrap();

        let hist = eng.note_history(&a).unwrap();
        assert_eq!(hist.len(), 2);
        let v1_rev = hist[1].id.clone(); // oldest = v1
        assert_eq!(eng.note_at(&a, &v1_rev).unwrap(), "v1");

        events.clear();
        eng.restore_note(&a, &v1_rev, &mut events).unwrap();
        assert_eq!(eng.read_note(&a).unwrap(), "v1");
        assert!(events.contains(&Event::NoteChanged(a.clone())));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-app restore_writes_old_content`
Expected: COMPILE failure — `note_history`/`note_at`/`restore_note` don't exist.

- [ ] **Step 3: Add the engine methods**

In `crates/cairn-app/src/lib.rs`, add `Revision` to the `cairn_ports` import (keep all existing names). Then add to the `impl<S: VaultStore, I: SearchIndex, V: Vcs> Engine<S, I, V>` block (near `commit`):

```rust
    /// A note's commit history (newest first).
    ///
    /// # Errors
    /// Returns [`PortError`] if the VCS adapter fails.
    pub fn note_history(&self, path: &NotePath) -> Result<Vec<Revision>, PortError> {
        self.vcs.history(path.as_str())
    }

    /// A note's contents at a past revision.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note didn't exist at that revision;
    /// [`PortError`] on a VCS failure.
    pub fn note_at(&self, path: &NotePath, revision: &str) -> Result<String, PortError> {
        self.vcs.show(path.as_str(), revision)
    }

    /// Restore a note to a past revision: write that revision's contents as the
    /// current note (a pending change to commit later). Emits `NoteChanged`.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note didn't exist at that revision;
    /// [`PortError`] on a VCS or storage failure.
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

- [ ] **Step 4: Run the test**

Run: `cargo test -p cairn-app restore_writes_old_content`
Expected: PASS.

- [ ] **Step 5: Full suite + lint + fmt**

Run: `cargo test -p cairn-app` then `cargo clippy -p cairn-app --all-targets -- -D warnings` then `cargo fmt`.
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-app/src/lib.rs
git commit -m "feat(app): Engine note_history/note_at/restore_note"
```

---

## Task 3: Contract DTOs + dispatch

**Files:**
- Modify: `crates/cairn-contract/src/lib.rs`
- Modify: `crates/cairn-service/src/lib.rs`

- [ ] **Step 1: Write the failing dispatch tests**

In the `#[cfg(test)] mod tests` block of `crates/cairn-service/src/lib.rs`, add. The tests already have a local `fn engine(dir: &std::path::Path) -> Engine<LocalFsStore, InMemoryIndex, GitVcs>` helper, `AppEvent` (= `cairn_app::Event`) is imported, and the contract `Command`/`Query`/`QueryResponse` are in scope:

```rust
    #[test]
    fn history_show_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut eng = engine(tmp.path());
        let mut sink: Vec<AppEvent> = Vec::new();
        dispatch_command(&mut eng, &Command::WriteNote { path: "a.md".into(), contents: "v1".into() }, &mut sink).unwrap();
        dispatch_command(&mut eng, &Command::Commit { message: "v1".into() }, &mut sink).unwrap();
        dispatch_command(&mut eng, &Command::WriteNote { path: "a.md".into(), contents: "v2".into() }, &mut sink).unwrap();
        dispatch_command(&mut eng, &Command::Commit { message: "v2".into() }, &mut sink).unwrap();

        let revisions = match dispatch_query(&eng, &Query::NoteHistory { path: "a.md".into() }).unwrap() {
            QueryResponse::History { revisions } => revisions,
            other => panic!("expected History, got {other:?}"),
        };
        assert_eq!(revisions.len(), 2);
        let v1 = revisions[1].id.clone();

        // NoteAt returns the content at that revision (reuses the Note response).
        match dispatch_query(&eng, &Query::NoteAt { path: "a.md".into(), revision: v1.clone() }).unwrap() {
            QueryResponse::Note { contents } => assert_eq!(contents, "v1"),
            other => panic!("expected Note, got {other:?}"),
        }

        // RestoreNote writes v1 back.
        let mut sink2: Vec<AppEvent> = Vec::new();
        dispatch_command(&mut eng, &Command::RestoreNote { path: "a.md".into(), revision: v1 }, &mut sink2).unwrap();
        match dispatch_query(&eng, &Query::GetNote { path: "a.md".into() }).unwrap() {
            QueryResponse::Note { contents } => assert_eq!(contents, "v1"),
            other => panic!("expected Note, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-service history_show_restore`
Expected: COMPILE failure — `Query::NoteHistory`/`NoteAt`, `QueryResponse::History`, `Command::RestoreNote` don't exist.

- [ ] **Step 3: Add the contract DTOs + variants**

In `crates/cairn-contract/src/lib.rs`, add the `Revision` DTO (mirror `SearchResult`'s derives) near the other response DTOs:

```rust
/// One commit in a note's history (response element of `NoteHistory`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Revision {
    /// Short commit id.
    pub id: String,
    /// Commit summary (first line).
    pub message: String,
    /// Commit time, seconds since the Unix epoch.
    pub timestamp_secs: i64,
    /// Author name.
    pub author: String,
}
```

Add to the `Query` enum:

```rust
    /// A note's commit history (newest first).
    NoteHistory {
        /// Relative note path.
        path: String,
    },
    /// A note's contents at a past revision.
    NoteAt {
        /// Relative note path.
        path: String,
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
    },
```

Add to the `QueryResponse` enum:

```rust
    /// A note's commit history (response to `NoteHistory`).
    History {
        /// One per commit, newest first.
        revisions: Vec<Revision>,
    },
```

Add to the `Command` enum:

```rust
    /// Restore a note to a past revision (writes that version as current).
    RestoreNote {
        /// Relative note path.
        path: String,
        /// A git revspec to restore from.
        revision: String,
    },
```

- [ ] **Step 4: Add the dispatch arms in the service**

In `crates/cairn-service/src/lib.rs`, add to `dispatch_query`'s match (after the `GetNote` arm):

```rust
        Query::NoteHistory { path } => {
            let p = parse_path(path)?;
            let revisions = engine
                .note_history(&p)?
                .into_iter()
                .map(|r| Revision {
                    id: r.id,
                    message: r.message,
                    timestamp_secs: r.timestamp_secs,
                    author: r.author,
                })
                .collect();
            Ok(QueryResponse::History { revisions })
        }
        Query::NoteAt { path, revision } => {
            let p = parse_path(path)?;
            let contents = engine.note_at(&p, revision)?;
            Ok(QueryResponse::Note { contents })
        }
```

Add to `dispatch_command`'s match (after the `Commit` arm):

```rust
        Command::RestoreNote { path, revision } => {
            let p = parse_path(path)?;
            engine.restore_note(&p, revision, sink)?;
            Ok(CommandResponse::Done)
        }
```

Add `Revision` to the `cairn_contract` import at the top of `cairn-service/src/lib.rs` (the contract `Revision`, used in the `map` above). `cairn_ports::Revision` is reached via the engine's return type and is not named, so there's no name clash.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p cairn-service` then `cargo test -p cairn-contract`
Expected: PASS — the new round-trip test + all existing service/contract tests, and the ts-rs `Revision` binding exports.

- [ ] **Step 6: Full suite + lint + fmt**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt`.
Expected: green. (Adding `QueryResponse::History` may break an exhaustive `match` if one exists outside dispatch — if the compiler flags one, add a `History` arm; the CLI uses `if let`, so it is unaffected.)

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-contract/src/lib.rs crates/cairn-service/src/lib.rs
git commit -m "feat(contract): NoteHistory/NoteAt queries + RestoreNote command + Revision DTO"
```

---

## Task 4: CLI `history`/`show`/`restore`

**Files:**
- Modify: `crates/cairn-cli/src/main.rs`

- [ ] **Step 1: Add the subcommands**

In `crates/cairn-cli/src/main.rs`, add to the `enum Command` (after `Tagged`):

```rust
    /// Show a note's commit history (newest first).
    History {
        /// Relative note path.
        path: String,
    },
    /// Print a note's contents at a past revision.
    Show {
        /// Relative note path.
        path: String,
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
    },
    /// Restore a note to a past revision (writes that version as current).
    Restore {
        /// Relative note path.
        path: String,
        /// A git revspec to restore from.
        revision: String,
    },
```

- [ ] **Step 2: Add the dispatch arms**

In `run()`'s `match cli.command { ... }`, add (after the `Backlinks` arm):

```rust
        Command::History { path } => {
            if let QueryResponse::History { revisions } =
                dispatch_query(&engine, &WireQuery::NoteHistory { path }).map_err(|e| e.to_string())?
            {
                for r in revisions {
                    println!("{}  {}", r.id, r.message);
                }
            }
        }
        Command::Show { path, revision } => {
            if let QueryResponse::Note { contents } =
                dispatch_query(&engine, &WireQuery::NoteAt { path, revision })
                    .map_err(|e| e.to_string())?
            {
                print!("{contents}");
            }
        }
        Command::Restore { path, revision } => {
            let resp = dispatch_command(
                &mut engine,
                &WireCommand::RestoreNote {
                    path: path.clone(),
                    revision: revision.clone(),
                },
                &mut events,
            )
            .map_err(|e| e.to_string())?;
            debug_assert!(matches!(resp, CommandResponse::Done));
            println!("restored {path} from {revision}");
        }
```

(`WireQuery`/`WireCommand` are the existing aliases for the contract `Query`/`Command` in `main.rs`; `QueryResponse`/`CommandResponse` are already imported.)

- [ ] **Step 3: Build + run the CLI tests**

Run: `cargo build -p cairn-cli` then `cargo test -p cairn-cli`
Expected: compiles; existing CLI integration tests pass (the new subcommands don't affect them).

- [ ] **Step 4: Manual smoke check (optional but recommended)**

Run from a temp cairn:
```bash
cd "$(mktemp -d)" && cargo run -q -p cairn-cli -- --cairn . init >/dev/null
cargo run -q -p cairn-cli -- --cairn . write a.md "v1"
cargo run -q -p cairn-cli -- --cairn . commit "v1"
cargo run -q -p cairn-cli -- --cairn . write a.md "v2"
cargo run -q -p cairn-cli -- --cairn . commit "v2"
cargo run -q -p cairn-cli -- --cairn . history a.md      # two lines, newest first
cargo run -q -p cairn-cli -- --cairn . restore a.md "$(cargo run -q -p cairn-cli -- --cairn . history a.md | tail -1 | cut -d' ' -f1)"
cargo run -q -p cairn-cli -- --cairn . read a.md          # prints v1
```
Expected: history lists two commits; restore brings back `v1`.

- [ ] **Step 5: Full workspace suite + lint + fmt + lock**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --check` then `cargo build --workspace --locked`.
Expected: all green, no warnings, fmt clean, lock consistent (no new deps).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-cli/src/main.rs
git commit -m "feat(cli): history / show / restore subcommands"
```

---

## Notes for the implementer

- **`GitVcs` is the only `Vcs` impl** — adding `history`/`show` to the trait only requires implementing them once. Other crates use `Vcs` generically (`<V: Vcs>`), so the trait growth doesn't break them.
- **Restore is a working-tree write**, not a git checkout: `restore_note` = `vcs.show` (read old content) + `write_note` (persist + reindex + cache + emit `NoteChanged`). The user commits it later via the existing `commit`.
- **Two `Revision` types** (ports + contract) by design, mapped in `dispatch_query` (same as `SearchHit`→`SearchResult`). Keep only the *contract* `Revision` imported by name in `cairn-service`; the ports one is reached via the engine return type.
- **`note_at` reuses `QueryResponse::Note`** — no new response variant for it.
- **CLI prints `<id>  <message>`** (no date formatting → no date dependency); `timestamp_secs`/`author` ride in the DTO for the daemon/UI.
- **fmt:** run `cargo fmt` before committing each task (CI rustfmt is strict).
- **Don't touch** the plugin host, watcher, or daemon (it flows through dispatch unchanged).
```
