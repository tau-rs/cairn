# Presence & Versions redesign — design

**Date:** 2026-08-21
**Status:** design approved (brainstorm), pending implementation plan
**Scope:** cairn-web-ui frontend only. No engine/daemon changes required for the
shippable work; multiplayer cursors are called out as an explicitly deferred,
engine-first growth path.

## Problem

Two user complaints about the current live-collaboration and save UI:

1. **The live/sync overlay is two mismatched floating cards** stacked in the
   bottom-right corner (`CollabPresencePill` at `bottom-40`, `LiveUpdatesBanner`
   at `bottom-28`). Different sizes, overlapping meanings, they float over note
   content, and they only appear reactively (invisible while healthy/idle), so
   there is no trustworthy persistent presence signal. Wording is technical and
   alarming ("live changes", "Reload", "data may be stale", red "Discard unsaved
   edits?").

2. **The "Commit" flow reads like a code editor, not a notes app.** Top-bar
   shows `Saved · uncommitted`, a `@c0001` short-hash badge, and a **Commit**
   button that opens a "Describe this change" message modal. Background
   auto-commit exists but fires on idle+interval timers, producing a wall of
   identical `cairn: update note.md` versions with no meaning.

The user also wants to **keep the per-file saved/unsaved dot they like** while
**adding real-time multi-person editing** — and asked for the two to coexist.

## Guiding principle — four signals, four questions, four zones

Grounded in prior-art research (Google Docs, Figma, Notion, VS Code + Live
Share, Obsidian-Git, Linear). "Saved" secretly conflates three states; the apps
that show a dirty dot *and* live collaboration keep them as independent signals
in separate screen zones. Cairn's exact analog is **VS Code + Live Share**.

| Signal | Question | Zone | Cairn today |
|---|---|---|---|
| **● dirty dot** | "Is *this file* flushed to disk?" | on the tab | exists — `TabStrip.tsx:91` (`bg-accent` dot on `t.dirty`) |
| **Presence** (avatars, cursors) | "*Who* is here?" | top-right + inline | `CollabPresencePill` (binary, anonymous) |
| **Sync** | "Do others *have* my edit?" | status bar / presence chip dot | `LiveUpdatesBanner` (ok/reconnecting/down) |
| **Version** | "Is it in *history*?" | status bar, quiet | `CommitBar` + `HistoryPane` |

Never merge them into one blob. Reserve the word **"Saved"** for the disk-flush
dot; call the git layer **"Versions"** (never "saved"), or users get retrained
badly. This separation is what lets a note legibly be "● unsaved, 2 people
editing, synced, in history" at once without contradiction.

## Part 1 — Unified presence cluster (UI-only, ships now)

Replace both corner cards with **one persistent element in the top-right of the
top bar**: an avatar row + a small status chip. Calm by default, louder only
when it matters. Never floats over note content.

### States (single element, swaps content/color)

| State | Render | Copy | Replaces |
|---|---|---|---|
| Connected, alone | green-dot chip | **Connected** | (nothing persistent today) |
| Others present | avatars + count chip | **N here** | — |
| Live activity | avatar(s) + chip | **Maya is editing…** (self-fades) | cryptic "Live edits" flash |
| Several editing | avatars w/ green "typing" pip + chip | **Maya, Sam editing…** (rolls to "+N" past two names) | — |
| Reconnecting | amber pulsing chip | **Reconnecting…** | `LiveUpdatesBanner` reconnecting |
| Offline | red chip + button | **Offline** · **Reconnect** | "Live updates unavailable — data may be stale." |
| Conflict | amber chip on avatar | **Maya also changed this** (click → dialog) | red "Discard unsaved edits?" wall |

### Conflict resolution (replaces `CollabReloadDialog`)

When the buffer is dirty **and** a foreign change arrives for the same note,
clicking the conflict chip opens a calm choice — **never a one-click way to lose
work**:

- Title: **"This note also changed on another device"**
- Buttons: **Keep my version** (default/safe) · **See their version**
- No "discard", no "reload", no danger-red primary.

("See their version" surfaces the remote content non-destructively — minimally a
read-only view of the incoming version; the exact compare affordance is an
implementation detail for the plan, but it must not overwrite the buffer in one
click.)

### Graceful degradation (today's wire is anonymous)

The collab wire is receive-only with **no peer identity** (`collabSlice`
`CollabPresence = { note, live, pendingCount }`; daemon join uses a random
read-only replica id). So on day one:

- Names/avatars are not available → show **"Someone is editing…"** and a generic
  avatar/count where a name would go.
- "N here" requires a roster the wire does not carry yet → until then the live
  state is binary ("Someone is editing…"). The component API must accept a
  `peers[]` list so that when the engine later provides identity + roster, the
  same component renders names/avatars/counts with no rewrite.

### Simultaneous editing — Level 1 now, built to grow into Level 2

- **Level 1 (ship):** active-editing avatars (green typing pip) + "who's
  editing" chip. Answers *how many* / *who*, not *where*. Works with today's
  wire (degrades to "Someone is editing…").
- **Level 2 (deferred, engine-first):** inline colored cursors with name tags in
  the text (Figma/Docs). Requires the engine/daemon to broadcast a presence
  **awareness** channel (peer identity + cursor positions) and new contract
  types. Out of scope here; the presence component and store shape are designed
  so cursors slot in later. See "Deferred / engine-first" below.

## Part 2 — "Commit" → human "Versions" (Model A + meaningful auto-commit)

Remove the manual **Commit** button, the commit-message modal
(`CommitDialog`), and the `@hash` badge. Everything autosaves and versions in
the background; a **Versions** browser lets you look back and restore. Keep the
per-tab dirty dot untouched (it is the disk-flush signal the user likes).

### Git history strategy — chosen: A (flat auto-stream) + named versions

The *shape* of the history (decided over alternatives C "squash trivial runs" and
D "shadow snapshot ref"):

- **Flat, linear, honest.** Each sealed session = one real commit on the main
  line. **No history rewriting** — not squash-on-push, not a shadow ref. This is
  deliberately the simplest and safest shape, and the only one that stays correct
  when commits also arrive from the watcher and concurrent writers (C and D fight
  with pushes and external commits).
- **"Makes sense" is a browser experience, not a raw-log one.** The raw `git log`
  is intentionally allowed to be long; the Versions browser makes it read clean by
  **grouping by note / session / day**. Every entry is individually meaningful
  because of the deterministic message (below), so grouping — not deletion — is
  how noise is tamed.
- **Named versions (git tags) are the curated layer.** "Name this version" tags a
  commit; the browser can filter to named-only for a Google-Docs-style milestone
  view. This is orthogonal to the flat substrate and gives the "clean history"
  feel without rewriting anything.
- **Deferred, not chosen:** retroactive squash of consecutive same-note unpushed
  commits (Strategy C) remains available as pure later polish if the raw log ever
  bothers the user. Not in v1.

### Meaningful auto-commit (replaces idle+interval tick noise)

Current `autoCommit()` + `rearmInterval()` fire on timers and write
`cairn: update {path}`. This is the exact failure mode every git-backed notes
tool gets dinged for (audit below). Replace with a **two-layer** model that
separates *saving* (disk flush, fast, frequent) from *versioning* (a commit,
slow, coherent) — because we deliberately keep those as different signals:

- **Layer 1 — autosave to the working tree** on a short debounce (~1–2s after
  the last keystroke). This is what clears the ● dirty dot. It is **not** a
  commit — it just flushes the buffer to the file. Frequent, silent.
- **Layer 2 — commit (seal a version)** only on a **long idle gap (~2–5 min)**,
  so one editing *session* = one version instead of many. Plus:
  1. **Boundary triggers:** commit on note-switch and window blur/close (seal
     the session when the user's attention moves).
  2. **Backstop checkpoint** (~20–30 min) so a marathon session that never goes
     idle still gets savepoints (Figma 30 min / Notion 10 min precedent).
  3. **Skip no-op / whitespace-only / metadata-only diffs** — never create an
     empty version (GitJournal #615 is the cautionary tale of not doing this).
  4. **Decouple commit cadence from push** — a chatty local stream must never
     immediately hit a remote.

> Reconciling the audit: tools like gitwatch (~1–2s debounce) and Obsidian's
> idle mode seal at seconds because for them *commit == save*. We split those,
> so the **seconds** debounce belongs to Layer 1 (autosave) and the **minutes**
> idle belongs to Layer 2 (commit/version). Both numbers are right, at different
> layers.

### Diff-derived version labels (deterministic, no LLM)

We own the note title, the op (add/edit/rename/delete from git status), the
changed headings, and the full diff — so we can produce a genuinely informative
subject for near-zero cost. Template:

> `{{op}} "{{title}}"[ § {{first changed heading}}] (+{{words}}/−{{words}})`
> multi-note fallback: `Update {{N}} notes: {{first titles…}}`

Concrete examples (vs. today's `cairn: update note.md`):

- `Edit "Q3 Roadmap" § Goals (+42/−3 words)`
- `Add note "Weekly Review"`
- `Rename "note.md" → "planning.md"`
- `Update 3 notes: Roadmap, Inbox, Ideas`

Relative time ("Today 3:41 PM") is rendered by the Versions browser from commit
metadata — **the timestamp never goes in the subject line** (that redundancy is
what makes Obsidian/gitwatch/Logseq histories a wall of dates).

### Optional LLM-authored messages (opt-in, default OFF)

The deterministic message is already good, so an LLM is a nicety, not the plan.
If offered, it must be **off by default** and prefer a **local model (Ollama)**
path — because (a) auto-commit fires constantly (per-keystroke-burst, not
per-PR), so a network round-trip on every seal adds latency and an offline
failure surface, and (b) streaming note contents to a third-party API is a bad
default for a private notes app. Format as our template or Conventional Commits
(OpenCommit/aider pattern; ~1s, <$0.001/commit on a small hosted model).

### Optional retroactive fold (local, unpushed only)

Never rewrite pushed history. As an optional tidy step, on push we may
`reset --soft`-fold a run of consecutive same-note **unpushed** auto-commits
into one — safe because it only touches local, unshared history. Nice-to-have,
not required for v1.

### Named checkpoints (optional overlay)

The only manual act, and it is optional and retroactive: **"Name this version"**
writes a git tag / annotated marker on a commit. Named versions render **bold**;
a **"Show named only"** filter hides the auto-stream (Google Docs model). No
commit button returns.

### Versions browser (upgrade `HistoryPane` / `HistoryList`)

`HistoryList.tsx` today renders one row per commit showing the raw `r.message`.
Upgrade to:

- **Group by session/day** with relative-time headers ("Today", "Yesterday
  3:41 PM"); collapse within-session auto-versions behind a disclosure.
- **Per-row change summary** (word delta, sections touched) from the diff.
- **Named-version emphasis** + "Show named only" filter.
- Restore already exists in `historySlice` / engine — reuse.

### Status bar (new, bottom edge)

New persistent bottom strip is the calm home for the vault-wide save/version/sync
state, chosen over keeping it in the top bar:

> `✓ Saved · Synced · 🕘 Versions ……… Last version: Today 3:41 PM · +124 words`

- **✓ Saved / Saving…** — disk-flush state (mirrors the tab dot, vault-level).
- **Synced / Syncing… / Offline — changes saved locally** — the sync axis;
  loud/reassuring only when offline.
- **🕘 Versions** — opens the versions browser.
- Last-version summary at the right.

## Components touched

- **New:** `StatusBar` (bottom strip); `PresenceCluster` (top-right, replaces
  `CollabPresencePill` + `LiveUpdatesBanner`); presence conflict dialog
  (replaces `CollabReloadDialog`).
- **Changed:** `App.tsx` (mount StatusBar, drop the two corner overlays),
  `TopBar.tsx` (mount PresenceCluster; remove `CommitBar`), `DialogHost.tsx`
  (drop `CommitDialog`; swap reload dialog for conflict dialog),
  `HistoryPane`/`HistoryList` (session grouping + summaries + named filter),
  `Settings.tsx` (retire/relabel the git-flavored auto-commit toggles),
  `collabSlice.ts` (peers[] shape, keep-vs-see conflict actions, decay/typing),
  `store.ts` (`commitManual`/`autoCommit`/`rearmInterval` → session-seal +
  diff-summary message; version-naming action).
- **Removed/retired:** `CommitBar.tsx`, `CommitDialog.tsx` (and their tests).

## Deferred / engine-first (not in this work)

Captured so the direction is not lost, but **out of scope**:

- **Level 2 live cursors + real identity/roster.** Needs an engine/daemon
  awareness channel: peer identity, cursor/selection positions broadcast to all
  replicas, and new contract types. Lives in the `tau-rs/cairn` engine repo,
  then the web UI consumes it. The `PresenceCluster` `peers[]` API and the
  inline-cursor layer are the seams left open for it.
- **Version retention/GC policy** (squash old auto-commits between named
  versions). Nice-to-have once the auto-stream exists.

## Implementation architecture (UI ⇄ engine split)

**Load-bearing decision:** commit *policy* (when to seal a version, and the
message) moves from the web frontend into the **engine**, because the engine is
the only party that sees every change source — this web client, the desktop app,
external editors via the watcher, and concurrent collaborators — and holds the
authoritative git diff. UI-driven commits can only ever label changes this UI
made. The UI becomes a consumer + intent-sender.

### What already exists engine-side (reuse, don't rebuild)

- **Seal/debounce machinery:** `cairn-service` `Coalescer` + `run_watch_loop_timeout`
  (`cairn-service/src/lib.rs:26-80`) already fold a burst into one commit after a
  `quiet_period_ms` gap; `AppState::commit_external_blocking`
  (`cairn-daemon/src/lib.rs:279-296`) already guards on `is_dirty`
  (skip-no-op). Today it is opt-in (`sync.auto_commit`, default off), wired only
  for external edits, and writes `"cairn: sync external edits"`.
- **Commit plumbing:** `Engine::commit` (`cairn-app/src/lib.rs:877`) →
  `GitVcs::commit_all` (git2, `cairn-infra/src/git.rs:96`), `add_all("*")` +
  HEAD commit with the caller's message verbatim, returns 7-char id.
- **History reads:** `history` / `vault_history` / `show` / `restore_note`
  already power the Versions browser.

### The one new engine capability: diff-summary

No word/line delta, changed-file list, or changed-heading exists today (only a
boolean `is_dirty`). New function (cairn-infra, git2): working-tree-vs-HEAD (and
tree-vs-tree for history rows) → `Diff::stats()` for `+N/−M`, hunk scan for the
first changed heading, status for op class (add/edit/rename/delete), path →
note title. Feeds both the auto-commit message generator and the Versions rows.

### Contract seam (define first; both tracks build against it)

Changes to `cairn-contract/src/lib.rs`, regenerate ts-rs, vendor into
`web/src/contract`:

1. `Command::Commit { message: String }` → `message: Option<String>` — omitted ⇒
   engine generates the deterministic message; "seal now" = commit with no
   message. Backward compatible.
2. `Command::NameVersion { commit: String, name: String }` — new; tags/stars a
   version.
3. `Revision` (history row) enriched with a change summary + `is_named`
   (structured fields, or baked into `message` for a smaller v1).

`CommandResponse::Committed { commit }` / `Event::Committed { commit }` are
unchanged — the UI already listens for `committed`.

### Implementation tree

```
Phase 0 — CONTRACT SEAM (blocks nothing for long; both tracks build on it)
└─ C0  cairn-contract: message: Option<String>; Command::NameVersion { commit, name };
       Revision += change-summary fields + is_named. Regenerate ts-rs.
       ├─► vendored into web/src/contract  (unblocks UI real-client integration)
       └─► UI mirrors the SAME shape in MockClient immediately (UI never waits)

ENGINE TRACK (tau-rs/cairn) — owns commit POLICY + MESSAGE          [parallel]
├─ E1  diff-summary (cairn-infra, git2): Diff::stats() → +N/−M words,
│       changed-file list, op class (add/edit/rename/delete), path→title,
│       first changed heading via hunk scan. (No word/line delta exists today.)
├─ E2  deterministic message generator from E1  ── depends on E1
│       `{op} "{title}"[ § {heading}] (+N/−M words)` · multi-note rollup.
│       Replaces "cairn: sync external edits" AND the UI's "cairn: update …".
├─ E3  generalize the seal  ── reuses existing Coalescer/quiet_period + is_dirty
│       • cover client writes too (not just external edits)
│       • ON BY DEFAULT (flip sync.auto_commit default true)
│       • Layer-2 timing: idle-gap seal + long-session backstop
│       • honor a boundary "seal now" hint from the UI
│       • keep skip-no-op; DECOUPLE commit cadence from push
│       Strategy A: flat linear history, ONE commit per sealed session.
│       NO squash, NO shadow ref.
├─ E4  named versions = git tags  ── serves Command::NameVersion (C0)
│       tag/annotate a commit; expose is_named on history rows.
├─ E5  config + defaults (idle seconds, backstop minutes). Shipped in daemon
│       config; moved to `cairn-infra` (engine #191) once it turned out the
│       seal loop had to run on the Tauri transport too — see #175. Both
│       transports now read the same `<cairn>/cairn.toml` `[sync]`.
└─ (out of scope) Level-2 awareness channel: peer identity + cursor positions.

UI TRACK (cairn-web-ui) — becomes a CONSUMER + hint-sender          [parallel]
├─ U1  remove CommitBar + CommitDialog + frontend commit timers
│       (commitManual / autoCommit / rearmInterval). KEEP write_note autosave
│       (Layer 1 → the ● dirty dot on the tab, TabStrip.tsx:91).
├─ U2  StatusBar (new bottom strip): Saved · Synced/Offline · 🕘 Versions
│       · last-version summary. Home for the vault-wide save/sync/version axes.
├─ U3  PresenceCluster (top-right, replaces CollabPresencePill + LiveUpdatesBanner)
│       all states + typing pips; calm conflict dialog (replaces CollabReloadDialog,
│       "Keep my version / See their version"). Degrades to "Someone is editing…".
├─ U4  Versions browser upgrade (HistoryPane / HistoryList): group by note/session/
│       day, per-row change summary (from Revision fields/message), named-only
│       filter, "Name this version" → Command::NameVersion.
├─ U5  seal-now hints: on note-switch / window blur → commit with no message.
└─ U6  Settings: retire the git-flavored idle/interval auto-commit toggles.

Phase 3 — INTEGRATE
└─ swap MockClient → DaemonClient once C0 is vendored + E-track landed;
   e2e vs a live cairn-daemon (the /tmp/cairn-demo-live harness from design).
```

**Ownership:** cross-repo, each repo owns its PRs. The UI track is a single-branch
feature in this repo → claim it (`scripts/claim-plan.sh`) and plant the flag
before executing, per the parallel-workspace discipline.

## Testing

- Presence cluster: unit tests per state (connected/others/reconnecting/offline/
  conflict/typing) incl. anonymous degradation ("Someone is editing…").
- Conflict dialog: dirty+foreign-op opens the calm dialog; "Keep my version"
  leaves the buffer intact; "See their version" is non-destructive.
- Session-sealing commit logic: pure-function tests for the seal/backstop/skip
  decisions (idle gap seals; whitespace-only skipped; note-switch commits).
- Diff-summary generation: word-delta + first-changed-heading from a known diff.
- Versions browser: session grouping, named-only filter, per-row summary render.
- Status bar: saved/saving, synced/offline copy.

## Non-goals

- No CRDT / true merge — collab stays presence + last-writer, plus the calm
  conflict choice. No change to the receive-only transport.
- No engine or contract changes.
