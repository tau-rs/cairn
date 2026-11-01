# Suggested-edges overlay — design

**Date:** 2026-07-23
**Status:** Approved (design), pending implementation plan
**Feature:** Feature A of the graph-seam-upgrade v3 follow-ons (Feature B, vault_history
timeline, is a separate engine-first track and out of scope here).

## Summary

Render engine-suggested (non-explicit) note links as a distinct, toggleable overlay on
the existing force graph. Suggestions are rendered as dashed links, visually separate from
real `GraphEdge`s, with suggestion strength (`weight`) driving visual prominence and the
provenance string (`why`) surfaced as a hover tooltip. Default OFF.

The engine suggestion contract is already vendored (engine rev `ef9e70a`), so this is a
greenfield UI overlay — no engine change is required.

## Contract (already vendored — do not edit)

```ts
// SuggestedEdge.ts
type SuggestedEdge = {
  from: string,        // source note path
  to: string,          // target note path
  weight: number,      // cosine similarity 0..1 — RELATIVE ORDERING ONLY, not a distance
  why: string | null,  // human-readable provenance, e.g. "shared: ownership, borrow"
};

// SuggestionScope.ts
type SuggestionScope =
  | { type: "note", path: string }   // relative note path
  | { type: "vault" };

// Query.ts (variant)      | { type: "get_suggestions", scope: SuggestionScope }
// QueryResponse.ts (variant) | { type: "suggestions", suggestions: Array<SuggestedEdge> }
```

Boundary decision: **adopt the raw contract types directly, no anti-corruption layer.**
Consistent with the thin-boundary decision carried through the graph-seam work.

## Approved decisions

| # | Question | Decision |
|---|----------|----------|
| Q1 | How is suggestion scope chosen? | **Auto-follow graph mode.** Full graph → `{type:"vault"}`; local graph (focused on active note) → `{type:"note", path: activePath}`. Single toggle; scope implied by context. |
| Q2 | Edge points to a non-visible node? | **Drop it.** Render a suggested edge only if *both* endpoints are currently visible nodes. Suggestions never introduce nodes; they stay consistent with all existing filters (min-degree, tag-group, local mode). |
| Q3 | Weight threshold control? | **On/off toggle only.** Weight drives opacity/width; the engine's own similarity cutoff stands. A top-N cap is the easy follow-up if vault scope proves noisy. |
| Q4 | How is `why` surfaced? | **`linkLabel` native tooltip.** react-force-graph's `linkLabel` accessor returns `why ?? ""` for suggested links. Near-zero build, degrades gracefully. |

## Architecture

Thin overlay; dependencies point inward (transport in the store behind `CairnClient`,
components consume state).

- **`store.ts`** — owns the transport call. New `suggestions: SuggestedEdge[] | null`
  state field + `loadSuggestions(scope: SuggestionScope)` action, modeled structurally on
  `loadGraph` / `refreshBacklinks`:
  - token-guarded via a new `seq.suggestions`
  - `setLoading("suggestions", …)`
  - `client.runQuery({ type: "get_suggestions", scope })`
  - narrow on `res.type === "suggestions"`; unknown variants → `unexpected(...)`; thrown
    errors → `pushError("Load suggestions", …)`
  - stores raw `SuggestedEdge[]` (no mapping layer)
- **`EditorPane.tsx`** — reads `s.suggestions` from the store and passes it to `GraphView`
  alongside `graph`/`noteTags`; triggers `loadSuggestions` per the lifecycle rules.
- **`GraphView.tsx`** — owns overlay on/off UI state (persisted), decides scope from the
  current full/local graph mode, merges suggested edges into the render links, renders the
  panel toggle.
- **`GraphGroupsPanel.tsx`** — stays stateless/controlled. New `suggestions` /
  `onSuggestionsChange` prop pair; new bordered section after the recency block
  (`mt-3 border-t border-border pt-3`), mirroring the min-degree/recency pattern.

## Data model — the `GLink.kind` seam

Note: the graph-seam brief claimed an inert `GLink.kind` seam was reserved. Verified against
current `main` — it does **not** exist. `GLink` today has only `source`, `target`, `state?`.
This design adds the seam.

```ts
// graphData.ts
export interface GLink {
  source: string;
  target: string;
  state?: GraphState;
  kind?: "real" | "suggested";  // undefined ≡ "real" — real edges stay untagged
  weight?: number;              // suggested-only: 0..1, drives opacity/width
  why?: string | null;          // suggested-only: linkLabel tooltip text
}
```

- The three existing builders (`buildGraphData`, `buildGraphDataFromNodes`,
  `buildCompareGraphData`) are unchanged — untagged links are treated as real. Zero churn
  to the compare/temporal path.
- New pure helper `buildSuggestedLinks(suggestions, visibleNodeIds, realLinks)` →
  `GLink[]`:
  - maps `SuggestedEdge[]` → `GLink[]` with `kind:"suggested"`, carrying `weight`/`why`
  - **drops** any edge whose `from` or `to` is not in `visibleNodeIds` (Q2)
  - **dedupes** against `realLinks`, undirected (`a→b` suppressed if a real `b→a` exists) —
    defensive; the engine claims non-explicit suggestions but this guarantees it
  - `why: null` passes through untouched
- **Adjacency** (`buildAdjacency`) is built from **real links only** — hover-highlight and
  degree must not be polluted by suggestions.
- No bigint coercion needed here — `weight` is already `number`. (The `mtime_secs`
  bigint→`Number` seam is unrelated and stays in `buildGraphDataFromNodes`.)

Merge point: `GraphView`'s live build (~lines 187–197). `buildGraphDataFromNodes(...)`
produces real links; when the overlay flag is ON, append `buildSuggestedLinks(...)`.

## Rendering

react-force-graph-2d link styling, branched on `link.kind === "suggested"`:

- **Real links:** unchanged (existing solid color/width).
- **Suggested links:**
  - `linkLineDash` → dash pattern (e.g. `[4, 4]`); real → `null` (solid)
  - `linkColor` → distinct muted, theme-aware accent (CSS var); real → current
  - `linkWidth` → scaled by `weight` (e.g. `0.5 + weight * 2`); real → current
  - `linkLabel` → `why ?? ""` (native hover tooltip); real → existing behavior

Opacity is expressed via width + a muted base color rather than a separate prop
(react-force-graph has no first-class per-link opacity); weight→width + muted color reads as
"secondary/fainter" cleanly.

Implementation caveat to verify: if the graph uses a custom `linkCanvasObject` painter
rather than default link rendering, the dash/color/width branch goes in that painter
(`ctx.setLineDash(...)`) instead of the accessor props. The discriminator is the same
either way.

## Loading, scope & lifecycle

- **Default OFF.** No query fires until enabled. Toggle state persisted alongside
  `groups`/`filter`/`recency` (same `GraphView` state + persistence trio).
- **On enable** → fire `loadSuggestions(scope)` for the current mode.
- **While ON:** re-fire on the trigger that changes what's shown. In note scope that is
  `activePath` changing. In vault scope the result is stable across note switches, so the
  effect is gated to not re-query on every note change.
- **On disable** → stop rendering. Keep the last `suggestions` in the store (cheap; avoids
  refetch on quick re-toggle). The render merge is gated purely by the on/off flag, so
  stale data is never shown while off.
- **Token guard** (`seq.suggestions`) drops out-of-order responses on rapid toggle/switch.
- **Errors** via `pushError("Load suggestions", …)`; a failing overlay never breaks the
  base graph.

Open point resolved in the plan (not here): exactly how `GraphView` reads "current graph
mode" (full vs local). Wire the scope decision off whatever flag already drives local-graph
mode rather than adding new mode state.

## Testing (TDD)

Unit tests carry the logic; the canvas render layer is the untestable seam, stated
explicitly rather than faked.

**Pure logic — `buildSuggestedLinks` (primary coverage):**
- maps `SuggestedEdge[]` → `GLink[]` with `kind:"suggested"`, carrying `weight`/`why`
- drops edges whose `from` or `to` is not in `visibleNodeIds` (both-missing, one-missing,
  both-present)
- dedupes against real links, undirected (`a→b` suppressed if real `b→a` exists)
- `why: null` passes through
- empty suggestions / empty visible-set → `[]`

**Store — `loadSuggestions`:**
- success: `res.type === "suggestions"` → state populated
- scope plumbing: vault vs note passes the right `SuggestionScope` to `runQuery`
  (mock client asserts the query arg)
- token guard: superseded response dropped
- unknown response variant → `unexpected(...)`; thrown error → `pushError(...)`, base graph
  state untouched

**Panel — `GraphGroupsPanel`:**
- new toggle renders, reflects on/off state, fires `onSuggestionsChange`

**Untestable (stated explicitly):** the react-force-graph accessor / canvas-painter wiring
(dash/color/width/linkLabel) — no meaningful unit assertion for canvas drawing. Verified
manually via `/run` (drive the real app, toggle the overlay) before the PR.

## Gate & process

- Full `just web-ci`, including `prettier --check` (easy to miss; eslint won't catch it).
- Contract files stay untouched (raw ts-rs, `web/.prettierignore`).
- PR `--base main`; merge via the **merge queue** ("Merge when ready"), never direct.

## Out of scope

- Feature B (vault_history global timeline) — engine-first; a `vault_history` source did not
  exist at `ef9e70a`. File/track the engine issue before any UI work.
- Injecting suggested nodes not already on the graph (Q2-B/C) — deferred.
- Weight threshold / top-N cap controls (Q3-B/C) — deferred follow-up.
- Custom styled DOM tooltip for `why` (Q4-C) — deferred.
