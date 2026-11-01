# Cairn Web UI — UI‑4d: Local Graph Mode Design Spec

**Date:** 2026-06-09
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑4d — the last of the UI‑4 graph cycles. Adds a local-graph
mode (current note + neighbors) to the Obsidian-style graph.
**Builds on:** UI‑4a (`react-force-graph-2d` canvas, `buildGraphData`), UI‑4b
(gear settings overlay + localStorage pattern), UI‑4c (groups). Graphite design.

---

## 1. Purpose

Let the user focus the graph on the **current note and its neighbors** (Obsidian's
"local graph") instead of the whole vault: a **Local | Global** toolbar switch
plus a **depth** slider. Local mode is rooted at the open note and grows by N
link-hops (both directions).

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Control | **Local \| Global segmented switch** overlaid on the graph toolbar (top-left); **depth** slider in the gear panel ("Local graph" section). |
| Root | The **currently-open note** (`activePath`) — already accent-tinted (UI‑4a). |
| Neighbors | **Undirected** BFS over links, out to `depth` hops. |
| Depth | Default **1**; range **1–3** (step 1). |
| Persistence | The mode (enabled) + depth persist to localStorage (`cairn.graph.local`), reopening in the last choice. Default `{enabled:false, depth:1}`. |
| No active note | When local is enabled but no note is open → a centered **"Open a note to see its local graph"** hint instead of the canvas. |
| Implementation | A pure `localSubgraph(nodes, edges, root, depth)`; `GraphView` feeds the subgraph to the renderer when enabled. |
| Out of scope | A separate local-graph pane (Obsidian-style second view); animating depth; per-note depth. |

### Non-goals (deferred)

- A distinct second graph instance/pane (the single graph view switches modes).
- Direction-aware (outgoing-only / incoming-only) neighbor modes.
- Depth > 3, or a "show whole connected component" option.

---

## 2. Architecture

Contained to the graph view. **No store/host/contract changes.**

```
web/src/components/graph/localGraph.ts        NEW (pure + localStorage) — localSubgraph + settings load/save.
web/src/components/graph/localGraph.test.ts   NEW — unit tests.
web/src/components/GraphView.tsx              MODIFY — Local|Global toolbar switch + depth slider + subgraph data + no-active hint.
web/e2e/skeleton.spec.ts                      MODIFY — toggle Local, assert canvas still renders.
```

### `localGraph.ts` (pure + localStorage)

```ts
export interface LocalGraphSettings { enabled: boolean; depth: number }
export const DEFAULT_LOCAL_GRAPH: LocalGraphSettings = { enabled: false, depth: 1 };
export const DEPTH_RANGE = { min: 1, max: 3, step: 1 } as const;

// BFS from `root` over UNDIRECTED edges to `depth` hops. Returns the reached
// nodes (incl. root) and the edges whose endpoints are both reached. root not in
// `nodes` (or empty/null) → { nodes: [], edges: [] }.
localSubgraph(
  nodes: string[],
  edges: { from: string; to: string }[],
  root: string | null,
  depth: number,
): { nodes: string[]; edges: { from: string; to: string }[] }

loadLocalGraph(): LocalGraphSettings   // localStorage["cairn.graph.local"] → validated+clamped, else default
saveLocalGraph(s: LocalGraphSettings): void   // JSON write, swallow errors
```

- `localSubgraph`: build an undirected adjacency from `edges` (only edges whose
  both endpoints are in `nodes`), BFS from `root` collecting nodes within `depth`
  hops (depth 0 = just the root), then keep the edges whose endpoints are both in
  the reached set. No duplicate nodes/edges; preserves input order where simple.
- `loadLocalGraph`: missing/corrupt → default; `depth` clamped to `DEPTH_RANGE`;
  `enabled` coerced to bool. `saveLocalGraph` try/catch (private mode / quota).

### `GraphView.tsx` (wiring)

- `const [local, setLocal] = useState(loadLocalGraph)`; `changeLocal(next) = setLocal + saveLocalGraph`.
- **Global data stays memoized on `[nodes, edges]` only** (so opening a note in
  global mode does NOT re-simulate — UI‑4a invariant). Add:
  - `localData = useMemo(() => buildGraphData(localSubgraph(props.nodes, props.edges, props.activePath, local.depth)), [props.nodes, props.edges, props.activePath, local.depth])`.
  - `const active = local.enabled && props.activePath; const data = active ? localData : globalData;`
  - `adjacency` derived from whichever is shown (build from the displayed subgraph's
    string links, same fresh-string-build approach UI‑4c uses, to match hover-neighbor
    highlighting to the visible graph).
  - In local mode, `data` changes with `activePath`/`depth` → the layout re-fits —
    **intended** (the focused subgraph genuinely changed); `zoomToFit` should run
    again on that change (reset the `fitted` flag when `data` changes, as it already
    does on data change).
- **Toolbar switch:** a small segmented `Local | Global` control absolutely
  positioned top-left of the graph container (z above the canvas); clicking toggles
  `local.enabled`.
- **Depth slider:** a "Local graph" section in the gear panel (rendered with the
  Groups/Forces sections), an `<input type="range">` (min/max/step from
  `DEPTH_RANGE`, aria-label "Local graph depth") bound to `local.depth`.
- **No-active hint:** when `local.enabled && !props.activePath`, render a centered
  muted message ("Open a note to see its local graph") in place of `<ForceGraph2D>`
  (keep the toolbar + gear so the user can switch back to Global).
- Groups/forces/colors/hover all operate on the displayed `data` unchanged.

---

## 3. Testing

- **Unit (Vitest):**
  - `localSubgraph`: depth 0 → just root; depth 1 → root + direct neighbors (both
    directions); depth 2 → two hops; edges limited to reached endpoints; a root not
    in `nodes` → `{nodes:[], edges:[]}`; no duplicate nodes when reachable via
    multiple paths.
  - `loadLocalGraph`/`saveLocalGraph`: default on empty/corrupt; depth clamped to
    1–3; `enabled` bool; round-trip; save swallows a throwing `setItem`.
- **e2e (Playwright):** in the graph view, click **Local**, assert the `<canvas>`
  still renders and the **Local** control reads active (e.g. an aria-pressed/active
  class or that the **Global** option is now the toggle target). (The subgraph
  *content* is physics/canvas → manual-visual.)
- **Manual/visual check:** with a note open, switching to Local shows only that
  note + neighbors; the depth slider grows/shrinks the neighborhood; switching
  back to Global restores the full graph; with no note open, Local shows the hint;
  the Local/Global choice + depth survive a reload; groups/forces still apply.
- All existing tests stay green; Tauri unaffected.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/graph/localGraph.ts` (+ test) | **New.** `localSubgraph` + settings load/save. |
| `web/src/components/GraphView.tsx` | **Modify.** Toolbar switch + depth slider + subgraph data + no-active hint. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Toggle Local, assert canvas. |

No new npm dependencies. No store/host/contract changes. (Distinct localStorage
key `cairn.graph.local` — no collision with `cairn.graph.forces`/`cairn.graph.groups`.)

---

## 5. Risks

- **Re-simulation discipline:** global `data` must remain memoized on `[nodes,
  edges]` only (no `activePath`) so global mode doesn't relayout on note-open;
  `localData` (which DOES depend on `activePath`/`depth`) is only *used* when local
  is enabled. The `const data = enabled ? localData : globalData` switch is what
  preserves the invariant.
- **Adjacency must match the displayed graph** (build it from the shown subgraph),
  or hover-neighbor highlighting in local mode would reference the global edges.
- **zoomToFit on subgraph change:** reset the `fitted` flag when `data` changes so
  the local view re-frames when you change the focused note/depth (the existing
  `fitted`-reset-on-`data`-change already does this if `data` identity changes).
- **No active note:** guard the hint so local mode with no open note doesn't render
  an empty/confusing canvas; keep the controls visible to switch back.
- **Persistence default:** persisting `enabled` means reopening in local mode shows
  only the last note's neighborhood — acceptable per the user's choice; the hint
  covers the no-active case.
- **localStorage in Tauri/private mode:** try/catch-guarded (as UI‑4b/4c). jsdom
  localStorage works via the existing `vitest.setup.ts` fix.
- **Canvas testability:** `localSubgraph` + settings are unit-tested; the rendered
  subgraph + the toggle behavior are manual-visual + a thin canvas e2e.
