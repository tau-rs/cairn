# Cairn Web UI — UI‑4a: Obsidian-Style Graph (Core) Design Spec

**Date:** 2026-06-05
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑4a — the first of the UI‑4 graph cycles. Replaces the current
React Flow graph with a canvas force-graph matching Obsidian's look/feel.
**Builds on:** the graphite design system and the existing graph data flow
(`store.graph` → `GraphView`). The editor rework (UI‑3/CE‑A/CE‑B) is done.

---

## 1. Purpose

The current graph (React Flow) renders boxy rectangular nodes with arrowed edges
and a dotted grid — a flowchart, not Obsidian. Replace it with a **canvas
force-graph** that matches Obsidian: circular dots **sized by link count**, faint
arrowless links, **live physics**, drag, pan/zoom, **hover-highlights-neighbors**,
**labels that fade in with zoom**, the active note tinted accent, click-to-open —
on the graphite palette.

### Decomposition (UI‑4 is four cycles; this is the first)

UI‑4 was scoped to the full Obsidian graph experience. That is four independent
subsystems, so it is split; **this spec is UI‑4a only.** The rest are separate
later cycles (each its own spec → plan → build), all building on UI‑4a:

- **UI‑4a (this):** core Obsidian-style global graph (look + drag + hover-neighbors + zoom-labels).
- UI‑4b: force-settings sliders (live-tune center/repel/link forces).
- UI‑4c: color groups (color nodes by folder/tag).
- UI‑4d: local graph mode (current-note neighborhood, depth N).

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Renderer | **`react-force-graph-2d`** (canvas + d3-force, vasturiano) — replaces React Flow. |
| Nodes | Circular dots, radius **sublinear in degree** (link count); active = accent. |
| Links | Faint, thin, **no arrowheads**. |
| Interactions | Continuous physics, drag nodes, pan/zoom, click-to-open, **hover highlights neighbors + dims the rest**, **labels fade in with zoom**. |
| Chrome | No grid background, no controls widget. Graphite palette. |
| Out of scope (UI‑4b/c/d) | Force-settings sliders, color groups, local-graph mode. |

### Non-goals (deferred)

- Everything in UI‑4b/c/d (sliders, color groups, local graph).
- Node context menus, multi-select, search/filter, saved layouts, 3D.
- Per-link direction styling (links are undirected for layout + sizing).

---

## 2. Architecture

Contained to the graph view. **No store/host/contract changes** — `GraphView`
still receives `nodes: string[]`, `edges: {from,to}[]`, `activePath`, `onOpenNote`.

```
web/package.json                         REMOVE @xyflow/react, d3-force, @types/d3-force; ADD react-force-graph-2d.
web/src/components/GraphView.tsx         REWRITE — mount <ForceGraph2D>; custom canvas paint + interactions.
web/src/components/graph/graphData.ts    NEW (replaces computeLayout.ts + buildFlowElements.ts) — pure helpers.
web/src/components/graph/graphData.test.ts  NEW — unit tests for the pure helpers (replaces computeLayout.test.ts).
web/src/components/graph/computeLayout.ts       DELETE.
web/src/components/graph/buildFlowElements.ts   DELETE.
web/e2e/skeleton.spec.ts                 MODIFY — rewrite the graph e2e for canvas (no DOM labels).
```

### Pure data model (`graphData.ts`, fully unit-testable)

```ts
export interface GNode { id: string; label: string; degree: number }
export interface GLink { source: string; target: string }

// Build force-graph data: degree = count of links touching the node (undirected);
// label = stem(path); links filtered to edges whose endpoints both exist.
buildGraphData(nodes: string[], edges: { from: string; to: string }[]): { nodes: GNode[]; links: GLink[] }

// Adjacency for hover-neighbor highlighting (symmetric).
buildAdjacency(links: GLink[]): Map<string, Set<string>>

// Node radius from degree — sublinear so hubs are bigger but not huge.
nodeRadius(degree: number): number          // e.g. BASE + K * Math.sqrt(degree)

// Label opacity from the current zoom scale — hidden when zoomed out, fades in.
labelAlpha(zoom: number): number            // 0 below a threshold → clamped to 1
```

These (and only these) are unit-tested. The renderer/physics are owned by
`react-force-graph` and verified visually + by a thin e2e (canvas presence).

### `GraphView` (rewrite)

`<ForceGraph2D>` configured for the Obsidian look:
- `graphData` memoized on `[nodes, edges]` only (NOT on `activePath`/hover — those
  feed the paint callback so changing them never restarts the simulation).
- `backgroundColor="transparent"`, no grid, no controls.
- `nodeCanvasObject(node, ctx, globalScale)` — draws the circle (`nodeRadius(node.degree)`)
  and, with `labelAlpha(globalScale)`, the label below it; colors per state
  (active / hovered-or-neighbor / dimmed / default).
- `nodePointerAreaPaint` — paints the circle as the hit region.
- `linkColor`/`linkWidth` — faint, thin; no directional arrows; hovered node's
  links highlighted, others dimmed.
- `onNodeClick(node)` → `props.onOpenNote(node.id)`.
- `onNodeHover(node|null)` → store the hovered id in a ref and nudge a repaint
  (see §4); the paint callback derives the highlight set from `buildAdjacency`.
- `enableNodeDrag` on (default); pan/zoom built in; `zoomToFit` on first render.

---

## 3. Rendering & interaction details

- **Node radius:** `nodeRadius(degree) = 3 + 1.6 * Math.sqrt(degree)` (px in graph
  units); degree 0 → 3, degree 1 → ~4.6, degree 9 → ~7.8. Sublinear so hubs read
  bigger without dwarfing leaves. (Constants tunable at the visual check.)
- **Node color (graphite):** default `#6b6c77`; active note `#6366f1`; on hover,
  the hovered node + its neighbors → `#cdd0e0`, all others → `#6b6c7755` (dimmed).
- **Links:** color `#3a3a44`, width 1, no arrows. On hover: links incident to the
  hovered node → `#6366f1aa`; others → `#26262e66` (dimmed).
- **Labels:** drawn centered under the dot in Inter; alpha = `labelAlpha(zoom)`
  where `labelAlpha` is 0 below a zoom threshold and ramps to 1 over a small range
  (e.g. 0 at zoom ≤ 1.2, 1 at zoom ≥ 2.5, linear between). The **hovered and
  active** nodes are always labeled (alpha forced to 1) so they're identifiable
  when zoomed out. Label color `#cdd0e0` (active/hover) / `#9a9ba6` (default).
- **Physics:** react-force-graph's internal d3-force; default warmup/cooldown so
  it settles then idles; dragging re-heats. `zoomToFit` (with padding) after the
  first cooldown so the whole vault is framed on open.
- **Hover highlight:** `onNodeHover` sets a `hoverIdRef`; the highlight set =
  `{hoverId} ∪ adjacency.get(hoverId)`; the paint callbacks read the ref so nodes
  and links repaint highlighted/dimmed; clearing hover restores the default look.

---

## 4. Key implementation notes

- **No re-simulation on active/hover change.** `graphData` is `useMemo`'d on
  `[nodes, edges]`. `activePath` is read inside `nodeCanvasObject` (compare
  `node.id === activePath`). Hover lives in a ref. So neither triggers a graphData
  identity change → the simulation is not rebuilt/restarted.
- **Repaint after settle.** Once physics cools, react-force-graph stops painting;
  hovering must nudge a repaint. Call the graph ref's repaint hook on
  `onNodeHover` (e.g. a no-op `d3ReheatSimulation` alternative is too heavy — use
  the ref's `.refresh()`/a tiny state toggle that forces a re-render, or set
  `autoPauseRedraw={false}` so it keeps painting). Pick the lightest that yields a
  smooth hover; decide at implementation, verify in the visual check.
- **graphData object identity.** react-force-graph mutates node objects (adds
  `x/y/vx/vy`). Rebuilding `graphData` (notes added/removed) yields fresh objects
  and a fresh layout — acceptable. Don't share mutated arrays across rebuilds.
- **jsdom.** Never import/render `ForceGraph2D` in unit tests (needs canvas). Unit
  tests cover only `graphData.ts`.

---

## 5. Testing

- **Unit (Vitest):** `graphData.ts` —
  - `buildGraphData`: degree = touching-link count (undirected); `label = stem`;
    links with a missing endpoint dropped; self-consistency (node ids unique).
  - `buildAdjacency`: symmetric neighbor sets; isolated node → empty set.
  - `nodeRadius`: monotonic non-decreasing in degree; `nodeRadius(0)` = base.
  - `labelAlpha`: 0 at/below the low threshold, 1 at/above the high threshold,
    within `[0,1]` and monotonic between.
- **e2e (Playwright):** REWRITE the existing "graph view" test for canvas — toggle
  to Graph, assert a `<canvas>` is visible inside the graph container and the
  toggle button flips to "Editor" (view switched). Do NOT assert DOM node labels
  (none exist) or click a specific node (positions are physics-driven). Keep the
  other e2e tests unchanged.
- **Manual/visual check:** the human confirms circular degree-sized nodes, faint
  links, label zoom-fade, hover-highlights-neighbors, drag, pan/zoom, click-opens-
  note, and the active-note tint.
- All existing unit tests stay green (minus the deleted `computeLayout.test.ts`,
  replaced by `graphData.test.ts`); Tauri/desktop unaffected.

---

## 6. Files & dependencies

| File | Change |
|---|---|
| `web/package.json` | **Modify.** − `@xyflow/react`, − `d3-force`, − `@types/d3-force`; + `react-force-graph-2d`. |
| `web/src/components/graph/graphData.ts` (+ test) | **New.** Pure helpers (replaces computeLayout + buildFlowElements). |
| `web/src/components/graph/computeLayout.ts` / `.test.ts` | **Delete.** |
| `web/src/components/graph/buildFlowElements.ts` | **Delete.** |
| `web/src/components/GraphView.tsx` | **Rewrite.** `<ForceGraph2D>` + canvas paint + interactions. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Canvas-based graph e2e. |

No store/host/contract changes.

---

## 7. Risks

- **Dependency weight / offline:** `react-force-graph-2d` pulls `force-graph` + d3
  modules; acceptable for an app and bundles offline (Tauri-safe). Removing
  `@xyflow/react` + `d3-force` offsets some. Verify nothing else imports the
  removed packages before deleting them.
- **Canvas e2e limitation:** can't click a node by text/position reliably →
  click-opens-note + hover/drag/zoom are manual-visual, not e2e. Pure logic is
  unit-tested; flagged as a coverage reduction vs the old DOM-label test.
- **Re-simulation churn:** keep `activePath`/hover out of `graphData` deps (§4) or
  the graph visibly re-lays-out on every note open/hover.
- **Repaint-after-settle for hover:** must nudge a repaint or hover highlight won't
  show once physics idles (§4).
- **jsdom can't render canvas:** unit tests cover only the pure helpers; the
  component is verified visually + the thin canvas e2e.
- **Theme constants:** node radius / label-fade thresholds / colors are tuned at
  the visual check; the spec's values are sensible defaults, not final.
