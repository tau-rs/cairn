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
- Manual DoD check: DONE 2026-08-27 against `ad97b05` — all three edit
  surfaces plus the backstop verified end-to-end on a scratch vault; the `/mcp`
  surface added later followed on 2026-08-28 against `c147547`. See the run
  notes below.

### Manual verification — 2026-08-27

Scratch vault (`cairn init` + baseline commit), daemon on
`cairn-daemon --cairn <vault> --port <port>`. Startup log confirmed the
defaults: `seal: auto-committing sessions after 2s idle (1200s backstop)`.
Scratch vaults are gone; these notes are the record.

**Run 1** — surfaces 1–7, one vault, in order:

```
0e8635d  Edit "Beta Note" § Beta Note (+9 words)                            [7] push-decoupling probe
c5bb56b  Edit "Alpha Note" § Intro (+20/−9 words)                           [4] web app (Playwright/Chromium)
56f9b08  (tag: cairn/deuxième-étape-v2) Edit "Gamma Note" § Gamma Note (+9 words)  [3] CLI
c908004  Edit "Beta Note" § Beta Note (+9 words)
7076a8b  Update 3 notes: "Alpha Note", "Beta Note", "Gamma Note"            [2] 3-note burst → ONE commit
703aa95  Edit "Alpha Note" § Details (+16 words)                            [1] vim
62ab7be  baseline: three scratch notes
```

1. **External editor (vim)**: one commit, correct `§ heading`, exact word
   count.
2. **Burst of 3 notes <2s apart**: ONE rollup commit, not three.
3. **CLI**: `commit` with no message → `committed 56f9b08` + generated
   message; immediately again on the clean tree → `nothing to commit`, exit 0,
   commit count unchanged. No empty commit.
4. **Web app** (cairn-web-ui `origin/main@7c4a8f9`, daemon backend): one
   commit; Versions panel rendered `Edit "Alpha Note" § Intro (+20/−9 words)`
   with the delta; status bar `Last version: just now · +20 words`.
5. **Named versions**: annotated tag `cairn/première-étape-v1` whose message
   is exactly `Première étape v1`; re-naming the same commit replaced it with
   `cairn/deuxième-étape-v2` (old tag gone); reusing that name on a different
   commit → HTTP 400 `invalid_request`, "already labels commit 56f9b08".
   `vault_history` joins `name` + nested `summary` onto the rows correctly.
6. **No-op safety**: `touch` plus a byte-identical rewrite → no commit.
7. **Push decoupled**: a real bare remote added mid-run; after another seal the
   remote's `for-each-ref` was still empty (0 refs, no branches, no tags).

**Run 2** — the 20-minute backstop (E3 trigger 2), second vault with
`[sync] backstop_minutes = 1` (daemon logged `2s idle (60s backstop)`). A
never-idle burst: one append every 0.7s (inside the 2s idle window) for 100s.

```
t=0     burst starts
t=61s   *** NEW COMMIT while burst still running: e2a444e Edit "Marathon Note" § Log (+405 words)
t=100s  burst ends after 132 edits
t=106s  idle seal after burst: 49d5e58 Edit "Marathon Note" § Log (+255 words)
```

Backstop sealed mid-session at the 60s deadline while edits kept flowing; the
idle seal closed the remainder 2s after the last edit. Word counts reconcile
exactly: 405/5 = 81 lines + 255/5 = 51 lines = 132 = edits made.

### Manual verification — 2026-08-28, `/mcp` surface

The MCP entry point to this feature (`commit` with an optional `message`,
`name_version`) landed after the run above, so it is covered separately.
Third scratch vault against `c147547`, driven over `POST /mcp` with
`Authorization: Bearer $(cat <vault>/.cairn/token)`. Auto-commit left ON (the
shipped default) but with `[sync] idle_seconds = 3600`, `backstop_minutes = 600`
so the explicit MCP `commit` is the only thing that seals — the idle seal is
already covered by Run 1.

```
52f8866  (tag: cairn/mcp-étape-v2) Message explicite via MCP   [3] explicit message
3bd1904  Add "MCP Note" (+14 words)                            [2] generated message
48625ab  baseline: mcp scratch vault
```

1. **Write gating** (`--mcp-write` absent): `tools/list` returned exactly the 8
   read tools, no write tools; a direct `tools/call name_version` was refused
   with JSON-RPC `-32601`, `unknown tool: name_version (daemon is read-only;
   start with --mcp-write)`.
2. **`commit` with no message** → `committed 3bd1904`, message generated by the
   engine in the E2 format: `Add "MCP Note" (+14 words)`.
3. **`commit` with a message** → `Message explicite via MCP` verbatim as the
   subject, no generated text appended.
4. **`commit` on a clean tree** → `nothing to commit`, `isError:false`, commit
   count unchanged at 3. No empty commit.
5. **`name_version`** → annotated tag (`cat-file -t` = `tag`)
   `cairn/mcp-étape-v1` whose message is exactly `MCP étape v1`; re-naming the
   same commit replaced it with `cairn/mcp-étape-v2`, old tag gone.
6. **Collision on a different commit** → tool-level failure (`isError:true`,
   not a JSON-RPC error): `invalid request: name "MCP étape v2" already labels
   commit 52f8866`; tag list unchanged.

Unicode survives the round trip in both the tag ref and the tag message. The
`/mcp` refusal path returns a protocol error, while a business-rule violation
returns `isError:true` — the split ADR-0013 specifies.

## Out of scope

- Push/sync policy (explicitly decoupled).
- History rewriting of pre-existing noisy commits.
- UI rendering of summaries/names (parallel UI track consumes the bindings).
