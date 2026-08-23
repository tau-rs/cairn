# Engine auto-commit & versioning — design

Date: 2026-08-22
Status: approved
Tracks: C0 (contract), E1 (diff summary), E2 (message generator), E3 (seal
generalization), E4 (named versions), E5 (config)

## Problem

A cairn vault is a git repository, and `git log` doubles as the version-history
panel users see. Today commit policy and messages are scattered across callers:
the UI commits with whatever it hardcodes, the daemon's file-watcher commits
`"cairn: sync external edits"` (opt-in, default off), and collab flushes commit
`"cairn: collab sync {path}"`. Edits made through the client API are only
committed when the UI decides to. The result is a history that is part noise,
part missing.

The engine is the only party that sees every change source and holds the git
diff, so commit policy and message generation move into the engine.

**Strategy (locked upstream): flat, linear history — one commit per sealed
editing session, engine-generated deterministic message. No squash, no shadow
ref, no history rewriting.**

## Decisions (settled during brainstorming)

1. **`Revision` enrichment**: nested `summary: Option<ChangeSummary>` plus
   `name: Option<String>` (the named-version label). `name` replaces the
   originally-specced `is_named: bool` — carrying the label is strictly more
   informative (`is_named ≡ name != null`) and the UI wants to display it
   anyway. `Option` on `summary` lets the engine skip the per-row diff cost
   beyond what a history page shows, without a contract change.
2. **Seal-now on a clean tree**: new additive `CommandResponse::NothingToCommit`.
   Empty commits are gone across the board — an explicit `message: Some(..)` on
   a clean tree also gets `NothingToCommit`. `Event::Committed` fires only when
   a commit exists.
3. **Named versions**: annotated git tag `refs/tags/cairn/<slug>`; the tag
   message holds the exact display name (refs can't carry spaces/unicode).
   One name per commit (re-naming replaces), one commit per name (reuse on a
   different commit → `InvalidRequest`, never a silent move).
4. **Word counting**: Unicode-whitespace-split token count over added/removed
   diff-line content.
5. **Defaults**: auto-commit ON, idle 2 s, long-session backstop 20 min.

## C0 — contract seam (do first; UI track mirrors these shapes)

`crates/cairn-contract/src/lib.rs`, ts-rs regenerated and published early.
All changes additive / serde-defaulted; existing payloads keep parsing.

```rust
Command::Commit {
    /// None ⇒ the engine generates the message. "Seal now" is exactly a
    /// commit with no message.
    message: Option<String>,
}

Command::NameVersion {
    /// Commit id (short or full) to label.
    commit: String,
    /// Display name, any string.
    name: String,
}

pub enum CommandResponse {
    Done,
    Committed { commit: String },
    /// Commit requested but the working tree matched HEAD; nothing created.
    NothingToCommit,
    PluginResult { .. },
}

/// What a commit changed, in note terms.
pub struct ChangeSummary {
    pub files_changed: u32,
    pub words_added: u32,
    pub words_removed: u32,
}

pub struct Revision {
    pub id: String,
    pub message: String,
    pub timestamp_secs: i64,   // ts type = number, as today
    pub author: String,
    #[serde(default)]
    pub summary: Option<ChangeSummary>,
    #[serde(default)]
    pub name: Option<String>,
}
```

Generated TS shapes:

```ts
type ChangeSummary = { files_changed: number, words_added: number, words_removed: number };
type Revision = { id: string, message: string, timestamp_secs: number, author: string,
                  summary: ChangeSummary | null, name: string | null };
type CommandResponse = ... | { type: "nothing_to_commit" };
```

Backward compatibility of `Commit`: `message` deserializes from a present
string (old clients) or `null`/absent (new "seal now" clients) —
`#[serde(default)] Option<String>`.

`NameVersion` responds `Done`. Errors: unknown commit → `NotFound`; name
already used on a different commit → `InvalidRequest`.

## E1 — diff summary (port + adapter)

New types in `cairn-ports`, methods on the `Vcs` trait, implemented by
`GitVcs` (`crates/cairn-infra/src/git.rs`) with git2.

```rust
pub enum ChangeOp { Add, Edit, Rename { from: String }, Delete }

pub struct NoteChange {
    pub path: String,
    pub op: ChangeOp,
    /// Display title (frontmatter `title:` → first `# ` → file stem), from the
    /// new blob (old blob for `Delete`).
    pub title: String,
    /// First changed heading: nearest `#`-prefixed line in hunk context or
    /// added lines. `None` when no heading is identifiable.
    pub heading: Option<String>,
    pub words_added: u32,
    pub words_removed: u32,
}

pub struct DiffSummary {
    pub changes: Vec<NoteChange>,
    pub words_added: u32,   // totals across changes
    pub words_removed: u32,
}

trait Vcs {
    /// Working tree vs HEAD — what a seal would commit. Empty `changes` ⇒
    /// nothing to commit (byte-identical tree).
    fn pending_summary(&self) -> Result<DiffSummary, PortError>;
    /// Commit vs its first parent — for history-row enrichment.
    fn commit_summary(&self, revision: &str) -> Result<DiffSummary, PortError>;
}
```

Implementation notes:

- `pending_summary`: `diff_tree_to_workdir_with_index` against HEAD's tree
  (untracked included), `find_similar` enabled so renames classify as
  `Rename { from }` rather than Delete+Add.
- Word counts from diff line callbacks: `+` lines feed `words_added`, `-`
  lines feed `words_removed`, split on Unicode whitespace. `.md` files only;
  non-md changes still count as `changes` entries (op + path) but contribute
  0 words and use the file stem as title.
- Heading detection: per file, scan hunk header context lines (`@@ ... @@
  <context>`) and added lines for the last `#`-prefixed heading preceding the
  first change; single heading only (used when one file changed).
- `commit_summary` reuses the same walk over `diff_tree_to_tree(parent, commit)`.
  Root commit diffs against the empty tree.
- History enrichment: `vault_history` / `history` / `structural_revisions`
  fill `Revision.summary` for the newest rows only (cap: `min(limit, 50)`;
  `None` beyond). Named-version labels come from one pass over
  `refs/tags/cairn/*` building an oid→name map (no per-row cost).

## E2 — message generator (pure, `cairn-app`)

`fn commit_message(summary: &DiffSummary) -> String`. No I/O; fully
deterministic; unit-tested against fixture summaries.

- Single note: `{verb} "{title}"[ § {heading}] (+A/−R words)`
  - verbs: Add / Edit / Delete / `Rename "old" → "new"` (rename shows both
    titles; word counts appended only if content also changed)
  - `§ {heading}` only when E1 identified one
  - zero-count sides elided: `(+112 words)`, `(−8 words)`, `(+112/−8 words)`
  - Delete carries no word counts.
- Multi-note: `Update {N} notes: "{t1}", "{t2}", "{t3}"…` — titles capped at
  3, ellipsis beyond; N = `changes.len()`.
- Timestamps never appear in the subject (commit metadata already has them).

Replaces both hardcoded daemon messages (`"cairn: sync external edits"`,
`"cairn: collab sync {path}"`).

`Engine::commit` becomes `commit(message: Option<&str>, sink)`: `None` ⇒
compute `pending_summary`, empty ⇒ signal nothing-to-commit (typed, so the
service can map to `NothingToCommit`), else generate the message and commit.
`Some(msg)` keeps caller text verbatim but still applies the empty-tree guard.

## E3 — seal generalization (daemon)

One commit per sealed session, any edit source. Reuses the existing
`Coalescer` + `run_watch_loop_timeout` machinery (`cairn-service`); the change
is who feeds it and what fires on quiet.

- **Session start/extend**: external watcher changes (as today) AND client
  mutations (`WriteNote`, `DeleteNote`, `RenameNote`, `RestoreNote`, collab
  flush) mark the same dirty flag. Implementation: the daemon marks the
  coalescer after each successful mutating dispatch.
- **Seal triggers**:
  1. idle: `idle_seconds` (default 2 s) with no new change;
  2. backstop: session open `backstop_minutes` (default 20 min) without ever
     going idle — seal mid-session so a marathon edit still lands in bounded
     chunks (`run_watch_loop_timeout` grows a session-start clock);
  3. explicit: `Commit { message: None }` from any client seals immediately;
  4. shutdown flush (exists today).
- **Guard**: seal commits only when `pending_summary().changes` is non-empty.
  Whitespace-only edits DO commit (real content change); byte-identical trees
  never do.
- **Default flip**: `auto_commit` defaults ON.
- **Push decoupled**: sealing never pushes; any push/sync remains a separate
  concern, unchanged by this design.

The engine stays passive (no timers in `cairn-app`); all timing lives in the
daemon/service layer as today. The CLI `watch` path gets the same generated
messages via `Engine::commit(None, ..)`.

## E4 — named versions

`Vcs` additions, `GitVcs` implementation via git2 tag APIs:

```rust
trait Vcs {
    /// Create/replace the cairn name for `revision`. Slugifies the ref,
    /// stores `name` exactly in the annotated-tag message.
    fn name_version(&mut self, revision: &str, name: &str) -> Result<(), PortError>;
    /// oid (full) → display name, from refs/tags/cairn/*.
    fn named_versions(&self) -> Result<HashMap<String, String>, PortError>;
}
```

- Slug: lowercase, alphanumeric runs kept, everything else collapsed to `-`,
  trimmed; empty slug (all-symbol name) falls back to the commit id. Slug
  collisions between *different* display names on different commits are
  handled by suffixing (`-2`); the uniqueness invariant is on display names.
- Replace-on-same-commit: delete old `cairn/*` tag for that oid, write new.
- Reuse-on-different-commit: `AlreadyExists` → service maps to
  `InvalidRequest` with the holding commit id in the message.
- Tagger signature: same `signature_from_config` fallback as commits.
- History rows: `Revision.name` joined from `named_versions()`.

## E5 — config

`crates/cairn-daemon/src/config.rs`, `SyncConfig`:

```toml
[sync]
auto_commit = true        # was false
idle_seconds = 2          # replaces quiet_period_ms
backstop_minutes = 20     # new
```

`quiet_period_ms` remains accepted as a deprecated alias (serde alias +
precedence: explicit `idle_seconds` wins); a deprecation warning is logged
when it is set.

## Testing

- **E2 (pure)**: fixture `DiffSummary` → exact message strings for every op
  class, heading present/absent, count-eliding combinations, multi-note
  rollup + title cap.
- **E1**: temp-repo tests — add/edit/rename/delete classification, word
  counts, heading extraction, root-commit summary, empty summary on clean
  tree, non-md handling.
- **Seal decisions**: dirty/clean × idle/backstop/explicit/shutdown table on
  the coalescer + timeout loop (extending the existing tests); backstop fires
  during a never-idle burst.
- **Contract**: serde round-trips for `Commit { message: null }`, legacy
  string payloads, `NothingToCommit`, enriched `Revision` incl. absent new
  fields.
- **E4**: name round-trip incl. unicode/spaces, replace-on-same-commit,
  collision → error, slug-collision suffixing, join onto history rows.
- **Multi-source coherence** (integration, daemon level): engine write +
  external file write in separate sessions → two commits with correct
  generated messages; interleaved within one quiet window → one commit.
- Manual DoD check: web app + CLI + external editor against one vault.

## Out of scope

- Push/sync policy (explicitly decoupled).
- History rewriting of pre-existing noisy commits.
- UI rendering of summaries/names (parallel UI track consumes the bindings).
