# Note history — Phase 2: diff-vs-current view

**Date:** 2026-06-14
**Builds on:** Phase 1 (PR #61, `note-history-track`) — read-only revision view.
**Branch:** `note-history-diff`.
**Phase-1 spec:** `2026-06-14-note-history-ui-design.md` (§ Scope/phasing names this as the fast-follow).

## Goal

Upgrade "view at revision" from a read-only full-content `<pre>` to a **diff vs
the current working copy**, defaulting to diff with a Diff/Full toggle. This is the
pattern Notion, VS Code Timeline, and Obsidian-Git all converge on. It is purely a
render upgrade — no store, contract, mock, or engine change.

## Why it's low-risk

The seam already exists. `RevisionView` was written in Phase 1 with a documented
`mode` seam. Both sides of the diff are already in memory:

- **old** = `viewingRevision.contents` (fetched via `note_at` in Phase 1).
- **new** = the active note's working buffer (`openNotes[activePath].contents`,
  surfaced in `EditorPane` as `buffer`).

So the only wiring is one new prop (`current`) passed from `EditorPane`, plus one
pure util and the render changes inside `RevisionView`.

## Components

### `lineDiff.ts` (new, pure, zero deps)

```ts
export type DiffLineType = "add" | "del" | "ctx";

export interface DiffLine {
  type: DiffLineType;
  text: string;            // line content, no trailing newline
  oldLine: number | null;  // 1-based index in OLD; null for "add"
  newLine: number | null;  // 1-based index in NEW; null for "del"
}

export function lineDiff(oldText: string, newText: string): DiffLine[];
```

- **What it does:** splits both texts on `\n`, computes an LCS over the line
  arrays, backtracks to emit an ordered row list with per-side line numbers.
- **How you use it:** `lineDiff(revision.contents, currentBuffer)`.
- **Depends on:** nothing — deterministic, string in / array out.
- **Algorithm:** classic LCS dynamic-programming table (`dp[i][j]`), then a
  backtrack that emits `ctx` on equal lines, `del` when advancing old, `add` when
  advancing new, draining any remaining old→del / new→add at the end.
  O(n·m) time/space, acceptable for note-sized documents.

### `RevisionView.tsx` (modified)

New props / state:

- `current: string` — the working-copy buffer (new side of the diff).
- local `useState<"diff" | "full">("diff")` — toggle, **default diff**. No store
  state, no persistence.

Render:

- `mode === "diff"` → `lineDiff(contents, current)` rendered as a scrollable
  `font-mono` block; each row = `[newLine#][oldLine#][+/−/ ][text]`, whitespace
  preserved, background tint per type.
- `mode === "full"` → the Phase-1 `<pre>{contents}</pre>` unchanged.
- Banner keeps the literal "read-only" wording (still true — no editing in either
  mode; also keeps the Phase-1 test assertion valid) and gains a two-button
  Diff/Full segmented toggle.
- `← Back to current` and `Restore` buttons unchanged in both modes.

### `EditorPane.tsx` (one line)

Add `current={buffer}` to the existing `<RevisionView/>`. No other change.

### `tailwind.config.ts` (two tokens)

The theme has `danger`/`danger-bg` but no green/success pair. Add, mirroring them:

```
success:    "#4ade80"
success-bg: "#0f2417"
```

Green-add / red-del is the universal diff convention; reusing `accent` (indigo)
for additions would read as non-standard.

## Data flow

```
store.viewingRevision.contents ─(old)─┐
                                       ├─► RevisionView ─► lineDiff ─► DiffLine[] ─► rows
store.openNotes[active].contents ─(new)┘        (current prop)
```

## Diff semantics

`lineDiff(old = revision, new = current)` answers "what changed **since** this
revision":

- line only in current → `add` (`+`, green) — added since the revision.
- line only in revision → `del` (`−`, red) — removed/changed since the revision.
- line in both → `ctx` (muted).

The new side is the **live working buffer**, so unsaved/uncommitted edits are
included in the diff. This is intentional and matches "what changed since".

## Error handling / edge cases

`lineDiff` must handle: identical texts (all `ctx`); empty old (all `add`); empty
new (all `del`); both empty (`[]`); mid-document insertion; trailing-newline
consistency (a trailing `\n` should not produce a spurious empty diff line in one
side only). No exceptions thrown — pure total function.

## Testing

- `lineDiff.test.ts` — add / del / ctx, plus all edge cases above.
- `RevisionView.test.tsx` — extends Phase-1 tests: diff renders by default with
  +/− markers; Full toggle shows raw contents; Diff toggle returns; Restore and
  Back still fire from the diff view; existing read-only assertion still passes.
- Full local gate: `pnpm typecheck && pnpm lint && pnpm run format:check &&
  pnpm test` (run from `web/`).

## Out of scope (YAGNI)

- Word/character-level intra-line diff (would justify a `diff` dep — not now).
- Persisting the Diff/Full preference.
- Diffing against anything other than the current working buffer (e.g.
  revision-to-revision) — separate feature if ever wanted.

## Done

Viewing a revision shows a diff vs the current note by default, with a Diff/Full
toggle; restore still works from the diff view; `lineDiff` + `RevisionView` tests
cover add/del/context + the toggle; full local gate green.
