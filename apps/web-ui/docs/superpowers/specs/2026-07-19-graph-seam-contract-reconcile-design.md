# Graph Seam Contract Reconciliation — Design

**Date:** 2026-07-19
**Branch:** `graph-seam-contract-reconcile`
**Status:** Approved (direction confirmed 2026-07-19)
**Supersedes brief target for:** the gated 11-task graph-seam-upgrade plan (Task 0 in particular)

## Context

The graph-seam-upgrade design was approved against a *frozen brief*. The cairn
engine then implemented the graph seam **before** the contract was frozen
(engine PRs #106/#107) and shipped shapes that diverge from the brief. Engine
PR #109 (`feat(graph): enrich GraphNode with degree and tags`) is still **OPEN**
as of this writing; it adds `degree` + `tags` to `GraphNode`.

The vendored contract here currently exposes only `GraphEdge` (`{from, to}`).
The richer seam types (`GraphNode`, `GraphScope`, `SuggestedEdge`,
`GetSuggestions`) are **not yet consumed** by the UI — the graph view computes
degree client-side in `web/src/components/graph/graphData.ts` and fetches tags
via a separate `noteTags()` seam. So for scope and suggestions, the UI side is
effectively greenfield.

## Decision

**The UI adapts to the engine's shapes.** The contract is ts-rs-generated in the
engine repo and vendored here; the engine owns the domain, the UI is the
adapter. The frozen brief was a planning artifact, not a binding contract, and
on the merits the engine's shapes are the *better* domain models. Reconciling
the engine back toward the brief would invert the dependency to buy UI
convenience — rejected.

**One targeted engine ask** is the sole exception (see `mtime_secs` below).

## Per-delta reconciliation

| Contract element | Brief shape | Engine shape (target) | Direction |
|---|---|---|---|
| `GraphScope` | struct `{focus?, depth?}` | tagged enum `{type:"full"} \| {type:"focused", path, depth}` | **UI adopts engine** |
| `SuggestedEdge` | `{from, to, score}` | `{from, to, weight, why}` | **UI adopts engine** |
| `GetSuggestions` | stub | takes `SuggestionScope`, returns real edges | **UI adopts engine** |
| `GraphNode.degree` / `.tags` | absent | present (#109) | **UI adopts** |
| `GraphNode.mtime_secs` | `u64` | `i64` → `bigint` in TS bindings | **Engine ask** (annotate `number`); UI coerces if declined |
| `GraphAt`, `GraphDiff` / `GraphDelta` | absent | present | **Additive — ignore this pass** |

### Rationale per delta

- **`GraphScope` tagged enum** — makes illegal states unrepresentable (no
  `depth` without a focus). The optional-struct brief was weaker modeling. No
  existing UI consumer, so adoption is free.
- **`SuggestedEdge {from,to,weight,why}`** — strictly richer than `{score}`.
  `why` is a free UX win (suggestion tooltip/explanation). Rename `score` →
  `weight` in the (unwritten) UI consumer. Greenfield.
- **`GetSuggestions` real** — a real capability replaces a stub. Pure upside.
- **`degree` + `tags` on `GraphNode`** — lets `graphData.ts` stop recomputing
  degree client-side and lets the store drop the separate `noteTags()` round
  trip for graph coloring. Net simplification of the store's graph slice
  (`graph: {nodes: string[], edges}` + `noteTags` map → `GraphNode[]`).
- **`GraphAt` / `GraphDiff` / `GraphDelta`** — not in the brief; time-travel /
  live-delta capability. Purely additive. UI wires them in a later pass; no
  action now.

### The `mtime_secs` carve-out

ts-rs maps Rust `i64`/`u64` → TS `bigint` by default. `bigint` at the JSON/JS
boundary is pure friction: it does not `JSON.stringify` cleanly, cannot mix with
`number` in arithmetic, and complicates recency sort/coloring. A Unix-seconds
timestamp fits the JS safe-integer range for millions of years, so `bigint`
buys zero domain benefit.

**Engine ask (one line):**

```rust
#[ts(type = "number")]
pub mtime_secs: i64,
```

If the engine team declines, the UI coerces `Number(node.mtime_secs)` at the
seam boundary in the client adapters — survivable, but ask first.

## Sequencing

1. **Hold the re-vendor** — #109 is still open. Do not re-vendor until it merges.
2. When #109 merges, **re-vendor bindings from engine `main`** (pulls
   `GraphNode.degree`/`.tags` + the seam types).
3. Rewrite the 11-task graph-seam-upgrade plan against these target shapes.
   Task 0 currently targets the brief and needs rewriting to the engine's
   `GraphScope` enum and `SuggestedEdge` shape.
4. Open the `mtime_secs` `#[ts(type = "number")]` request against the engine in
   parallel; it is not a merge blocker for the UI work (coercion fallback
   exists).

## Out of scope

- Wiring `GraphAt` / `GraphDiff` / `GraphDelta` into the UI.
- Any change to the engine beyond the `mtime_secs` annotation request.
- The graph *rendering* layer (react-force-graph) — unchanged; only the data
  seam feeding it changes.
