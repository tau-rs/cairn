# Vault-history timeline — design

**Date:** 2026-08-09
**Branch:** `vault-history-timeline`
**Status:** design, pending user review
**Supersedes premise of:** the "Feature B is engine-first" handoff (premise drifted — see below)

## Premise correction (verified against source, not docs)

The handoff assumed `vault_history` did not exist at the engine and this was
engine-first. **That is stale.** Verified on 2026-08-09:

- `web/src/contract/Query.ts` (origin/main) has a `vault_history` variant; it
  landed via **PR #116** (commit `dd05a3e`, engine rev-bump to `057bf5e`), not
  the still-open PR #112.
- The pinned engine (`~/.cargo/git/checkouts/cairn-197285ac2398f21e/057bf5e`)
  implements `vault_history(limit) -> Vec<Revision>` — a whole-vault commit walk,
  newest-first (`crates/cairn-infra/src/git.rs:134`, dispatched at
  `crates/cairn-service/src/lib.rs:277`).
- `graph_at(rev)` and `graph_diff(from,to)` already exist, are full/vault-scope,
  and are already wired in the UI (`store.loadSnapshot`/`loadDiff`).

**Therefore the core feature is pure UI. No engine work, no rev-bump.**

## Goal

Make the graph's temporal view a **vault-wide** "jump to a point in time & see
what changed" tool (Job 2), and make the scrubber **legible** — today's control
(`TemporalScrubber.tsx`) is a row of unlabeled ticks with a hidden compare
gesture and no "what am I looking at" feedback. Going vault-wide is also the
first time the timeline has a subject big enough to be worth exploring.

Non-goals (Job 1 "watch it grow" retrospective; Job 3 forensic recovery) are
out of scope. No play/animation. No per-commit structural analysis in Phase 1
(see Phase 2).

## Core decision: vault-primary timeline (drop note-scoping in Phase 1)

Today the timeline source is note-scoped (`loadTimeline(path)` →
`note_history`), and the scrubber is `disabled` without an active note. But the
**snapshots and diffs it drives are already full/vault-scope** (`graph_at`/
`graph_diff` with `scope: {type:"full"}`) — compare is even `forcedGlobal`
(`GraphView.tsx:254`). So the current UX shows a *whole-vault graph* over a
*single-note tick list* — incoherent.

**Decision:** the temporal timeline becomes **vault-scoped**, matching the graph
it drives. The `activePath` coupling is removed; the scrubber is always
available (the key new capability: works with no note open).

- **Alternative considered — keep both (note/vault toggle):** preserves PR #105's
  per-note timeline but keeps the note-list-over-vault-graph incoherence and
  doubles the mode surface (source × interaction). Rejected for Phase 1 on YAGNI;
  a note *filter* can be re-added later if a real need appears.
- ⚠️ **This is the one decision to confirm at spec review** — it removes the
  per-note temporal timeline shipped in PR #105.

## Phase split

| Phase | Scope | Engine? |
|---|---|---|
| **Phase 1** (this PR) | Vault timeline source + legibility redesign: state banner, explicit **Browse**/**Compare** modes, activity-density histogram, snapping playhead, range-compare, debounced snapshot loads. | **None — pure UI** |
| **Phase 2** (deferred) | Thin markers to *structural* (graph-changing) commits only. | **Engine-first** |

**Why Phase 2 is engine-first (verified):** `vault_history` carries no
structural signal (just `{id, message, timestamp_secs, author}`), and detecting
structural commits client-side is infeasible — `built_at(rev)`
(`crates/cairn-app/src/lib.rs`) reads the **entire vault tree at a commit and
re-parses every note** to rebuild the graph (LRU-cached, 16 entries). One
full-vault reparse *per commit* is O(commits × vault size). Phase 2 needs a new
engine query (e.g. `structural_revisions`) → rev-bump → contract sync, filed as
a separate tau-rs/cairn issue. **Scale does not require it** — the density
histogram already renders in fixed width for any N; the filter only improves
*signal*.

## Architecture

Reuse the existing temporal machinery; change the source and the scrubber UI.

```
 TemporalScrubber (redesigned)
   ├ mode: Browse | Compare        (explicit; replaces hidden click-twice gesture)
   ├ activity histogram            (buckets over the whole span; fixed width)
   ├ playhead (snaps to a Revision) → Browse: snapshot
   └ range brush (A→B Revisions)   → Compare: diff
        │ selection: TemporalSelection (existing type, unchanged)
        ▼
 useTemporalGraph()  (no activePath param)
   ├ Effect A: on mount → actions.loadVaultTimeline()        ★NEW action
   └ Effect B: selectionToRequest(sel, timeline) → loadSnapshot / loadDiff / clearTemporal
        │ (debounced so dragging the playhead doesn't fire graph_at per pixel) ★NEW
        ▼
 store: temporal { timeline, snapshot, diff }  (shape unchanged)
   loadVaultTimeline() → runQuery({type:"vault_history", limit:null})  ★NEW
   loadSnapshot / loadDiff / clearTemporal                              (unchanged)
        ▼
 graph_at / graph_diff → buildCompareGraphData (graphData.ts)          (unchanged)
```

### Units and responsibilities

1. **`store.loadVaultTimeline()`** — new async action, mirrors `loadTimeline`
   exactly (token-guard `seq.timeline`, write `temporal.timeline`, `unexpected`
   on wrong variant, `pushError` on throw). The *only* new store code.
   - Signature: `loadVaultTimeline(): Promise<void>`.
   - Query: `client.runQuery({ type: "vault_history", limit: null })` →
     expects `{ type: "history", revisions: Revision[] }`.

2. **`timelineBuckets(revs, span)`** — new pure module (e.g.
   `graph/timelineDensity.ts`): map `Revision[]` → histogram buckets
   (auto day/week/month by span) + per-Revision x-position (proportional to
   `timestamp_secs`). Pure, unit-tested.

3. **`nearestRevision(revs, fraction)`** — new pure helper: playhead position →
   index of nearest Revision by timestamp. Pure, unit-tested. (Lives with #2.)

4. **`TemporalScrubber.tsx`** — redesigned: banner (state + counts), Browse/Compare
   segmented control, histogram, playhead, range brush. Consumes `timeline`,
   `selection`, `onSelect`; adds internal Browse/Compare view state. Node/link
   counts in the banner come from the loaded `temporal.snapshot`/live graph
   (already in store) — no new query.

5. **`temporalControls.ts`** — `TemporalSelection`/`TemporalRequest`/
   `selectionToRequest` reused **unchanged** (indices already index into the
   timeline; vault revisions are just a longer list). The localStorage "open"
   key is reused.

6. **`useTemporalGraph.ts`** — drop the `activePath` param; Effect A calls
   `loadVaultTimeline()` once on mount instead of per-note; `disabled` is
   removed (always enabled). Add debounce on Effect B's snapshot/diff dispatch.

7. **`GraphView.tsx`** — `useTemporalGraph()` (no arg); the "Graph history"
   `IconButton` is never `disabled` (drop the no-note tooltip branch); scrubber
   mount condition drops `!temporal.disabled`.

## Data flow — Browse vs Compare

- **Browse:** playhead snaps to a Revision → `selection = {kind:"snapshot", at}`
  → existing `loadSnapshot(rev)` → `temporal.snapshot` → graph rewinds. Banner:
  "Viewing vault as of \<date\> — \<message\> · N notes · N links".
- **Compare:** brush a range (or pick two markers) → `selection =
  {kind:"compare", from, to}` → existing `loadDiff(from,to)` → `temporal.diff` →
  `buildCompareGraphData`. Banner: "Comparing \<A\> → \<B\> · +N notes/links /
  −N".
- **Live:** `{kind:"live"}` → `clearTemporal()` → live graph.

## Error handling

- Reuse existing patterns: `seq.timeline`/`seq.temporalData` token guards prevent
  stale writes; `pushError`/`unexpected` for failures and wrong-variant responses.
- Empty vault / empty history → `vault_history` returns `[]` → scrubber renders
  no markers, stays in Live (mirrors `selectionToRequest`'s empty-timeline
  fallback to live).
- Rapid playhead drag → debounce (150 ms) so only the settled position fires
  `graph_at`; the token guard drops any in-flight superseded snapshot.

## Testing (TDD)

- `store.test.ts`: `loadVaultTimeline` — mock-client `runQuery` spy asserts the
  `{type:"vault_history", limit:null}` query, `temporal.timeline` write on
  `history` response, `unexpected` on wrong variant, `pushError` on throw,
  stale-token no-write. (Mirror the existing `loadTimeline`/`loadSuggestions`
  tests.)
- `timelineDensity.test.ts`: bucketing (day/week/month selection by span, empty
  input, single revision) and `nearestRevision` (endpoints, ties, out-of-range
  clamps).
- `TemporalScrubber.test.tsx`: Browse click → snapshot selection; Compare
  brush/two-pick → compare selection; Live resets; banner text for each state;
  renders with a long (100+) timeline without per-commit DOM blowup.
- Full `just web-ci` including `prettier --check` (easy to miss).

## Files touched

- `web/src/store/store.ts` — add `loadVaultTimeline`; type in the store
  interface (near line 239).
- `web/src/components/graph/timelineDensity.ts` (+ test) — **new** pure module.
- `web/src/components/graph/TemporalScrubber.tsx` (+ test) — redesign.
- `web/src/components/graph/useTemporalGraph.ts` — vault source, drop
  `activePath`, debounce.
- `web/src/components/GraphView.tsx` — call site, un-disable the history button
  and scrubber mount.
- `web/src/components/graph/temporalControls.ts` — unchanged (reused).

## Open questions / risks

1. **Confirm the core decision** (drop per-note temporal timeline). If the user
   wants to keep it, fall back to the note/vault toggle (Option A) — larger.
2. **Snapshot cost on scrub:** each Browse landing triggers a full-vault
   `graph_at` build (cached per-oid, LRU 16). Debounce mitigates; acceptable for
   Phase 1. If janky on large vaults, that's a Phase-2 engine concern too.
3. **Merge:** PR `--base main`, merge queue only; watch the live-advisory
   `web-deny` gotcha.
