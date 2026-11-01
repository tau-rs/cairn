# Cairn Web UI — Graph View Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 4 of [`docs/roadmap.md`](../../roadmap.md) (Graph view)
**Builds on:** the skeleton + Tauri desktop + editor rework (Phases 1–3). Runs on
both the mock (browser/tests) and the real Tauri backend unchanged.

---

## 1. Purpose

Add a **whole-cairn link graph** — a force-directed map of notes and the links
between them — shown in the center pane via a top-bar toggle. Clicking a node
opens that note. This makes the cairn's structure visible and navigable. The
engine already exposes the data (`get_graph`); this is a presentation-layer
feature with no contract changes.

### Non-goals (deferred)

- Local/neighborhood graph (active note ± 1 hop). Global only this cycle.
- Full-window/immersive graph mode; a separate `/graph` route.
- Graph filtering, search-in-graph, tag/folder coloring, orphan toggles,
  link-strength weighting, saved layouts.
- Animated/live force simulation in-canvas (we compute a static layout once).

---

## 2. Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Placement | **Center-pane toggle.** A top-bar "Graph" button swaps the editor area for the graph; notes list + backlinks panels stay. |
| Scope | **Global whole-cairn** graph (all notes + resolved links). |
| Layout | **Force-directed** via `d3-force` (organic web), computed once to static positions. |
| Rendering | **`@xyflow/react`** (React Flow) canvas: pan/zoom/fit built in. |
| Interaction | Click node → open note + return to editor. Active note highlighted. Directed edges (arrowheads). |

---

## 3. Architecture

```
App.tsx
  view: "editor" | "graph"  (local useState; transient center-pane state)
  top bar: a "Graph"/"Editor" toggle button
  on entering graph view → actions.loadGraph()
  center pane: view === "graph" ? <GraphView …/> : <Editor …/>

GraphView.tsx  (thin React Flow wrapper)
  props: nodes: string[], edges: {from,to}[], activePath: string|null, onOpenNote(path)
  positions = useMemo(() => computeGraphLayout(nodes, edges), [nodes, edges])
  elements  = useMemo(() => buildFlowElements(nodes, edges, positions, activePath), …)
  <ReactFlow nodes={…} edges={…} onNodeClick={(_, n) => onOpenNote(n.id)} fitView />

components/graph/computeLayout.ts   computeGraphLayout(nodes, edges) -> Map<path,{x,y}>   (d3-force, pure)
components/graph/buildFlowElements.ts buildFlowElements(nodes, edges, positions, activePath)
                                      -> { nodes: FlowNode[], edges: FlowEdge[] }            (pure)

store.ts
  graph: { nodes: string[]; edges: GraphEdge[] } | null
  loadGraph(): runQuery({type:"get_graph"}) -> set graph
  event reaction: on note_changed/note_deleted, if graph !== null -> void loadGraph()
```

- The two pure helpers hold all the logic and are independently testable; the
  React Flow wrapper is thin (rendering only) and covered by e2e.
- `node.id` in React Flow = the note path, so `onNodeClick` yields the path directly.

---

## 4. Data flow & store

- `loadGraph()`:
  ```ts
  async loadGraph() {
    try {
      const res = await client.runQuery({ type: "get_graph" });
      if (res.type === "graph") set({ graph: { nodes: res.nodes, edges: res.edges } });
    } catch (err) { set({ error: errMsg(err) }); }
  }
  ```
- The existing `init` event-subscription handler gains: on `note_changed` /
  `note_deleted`, `if (get().graph !== null) void get().loadGraph();` (mirrors the
  search/backlinks refresh pattern), so an open graph stays current as notes change.
- `graph` starts `null`; `App` calls `loadGraph()` when the user switches to the
  graph view (and the data then refreshes via events while the view stays open).

`GraphEdge` is the vendored contract type (`{ from: string; to: string }`).

---

## 5. Layout & rendering

- **`computeGraphLayout(nodes: string[], edges: {from,to}[]): Map<string,{x,y}>`** —
  builds d3-force nodes/links and runs the simulation synchronously for a fixed
  number of ticks (e.g. `simulation.stop()` + a `for` loop of ~300 `tick()`s, or
  `simulation.tick(300)`), using `forceManyBody`, `forceLink(edges).id(d=>d.id)`,
  and `forceCenter`. Returns a finite `{x,y}` for every node (including isolated
  ones). No animation; positions are computed once per `(nodes,edges)` change.
- **`buildFlowElements(nodes, edges, positions, activePath)`** — maps to React
  Flow shapes: each node `{ id: path, position: positions.get(path)!, data: {
  label: stem(path) }, …}` with the active note flagged (e.g. a `selected`/class
  marker or distinct style); each edge `{ id: \`${from}->${to}\`, source: from,
  target: to, markerEnd: arrow }`. Pure; no React Flow imports needed for the
  shapes (plain objects).
- **`GraphView`** renders `<ReactFlow>` with those elements, `fitView`, a
  `<Background>`/`<Controls>` (React Flow extras) for pan/zoom, and
  `onNodeClick={(_, node) => props.onOpenNote(node.id)}`. Node labels use `stem()`
  (reuse `web/src/client/wikilink.ts`). Styling: dark theme consistent with the app.
- Import `@xyflow/react/dist/style.css` once (in `main.tsx`).

---

## 6. App wiring

- `App` adds `const [view, setView] = useState<"editor" | "graph">("editor");`.
- Top bar: a button toggling `view`; when switching to `"graph"`, call
  `actions.loadGraph()`.
- Center pane (inside the existing `editor={…}` Shell region, alongside the
  `SearchResults` overlay): render `view === "graph" ? <GraphView … /> : <Editor … />`.
- `GraphView` props: `nodes`/`edges` from the store `graph` (empty arrays when
  `graph` is null), `activePath`, and `onOpenNote={(p) => { void actions.openNote(p); setView("editor"); }}`.
- The "Graph"/"Editor" toggle label reflects the current view.

---

## 7. Testing

- **Unit (Vitest):**
  - `computeGraphLayout`: returns a position with finite `x`/`y` for **every**
    node (connected and isolated); given N nodes, the map has N entries. (Don't
    assert exact coordinates — d3-force is not coordinate-stable.)
  - `buildFlowElements`: N nodes → N React Flow nodes with `id === path` and
    `label === stem(path)`; M edges → M React Flow edges with correct
    `source`/`target`; the `activePath` node carries the active flag; others don't.
  - Store: `loadGraph()` populates `graph` from the mock's `get_graph`; a
    `note_changed` event reloads the graph when it's open. (`MockClient.get_graph`
    already returns sorted nodes + resolved edges.)
- **React Flow rendering needs real DOM (ResizeObserver/sizing), so it is NOT
  unit-tested**; the canvas + node-click is covered by **e2e**.
- **e2e (Playwright):** in the existing flow, after notes exist, click the top-bar
  **Graph** toggle; assert node labels are visible (e.g. `index`, `ideas`); click a
  node and assert it opens that note (the editor shows its path/heading) and the
  view returned to the editor. Keep all prior assertions.
- All green on the **mock**; desktop/Tauri unaffected (presentation-only).

---

## 8. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/graph/computeLayout.ts` | **New.** `computeGraphLayout` (d3-force → positions). |
| `web/src/components/graph/computeLayout.test.ts` | **New.** |
| `web/src/components/graph/buildFlowElements.ts` | **New.** Pure mapper to React Flow nodes/edges. |
| `web/src/components/graph/buildFlowElements.test.ts` | **New.** |
| `web/src/components/GraphView.tsx` | **New.** React Flow wrapper (uses the two helpers; `onNodeClick`). |
| `web/src/store/store.ts` | **Modify.** `graph` state + `loadGraph()` + event refresh. |
| `web/src/store/store.test.ts` | **Modify.** `loadGraph` + refresh tests. |
| `web/src/app/App.tsx` | **Modify.** `view` toggle, top-bar Graph button, center swap, GraphView wiring. |
| `web/src/main.tsx` | **Modify.** Import `@xyflow/react/dist/style.css`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Add the graph toggle + node-click navigation steps. |
| `web/package.json` | **Modify.** Add `@xyflow/react`, `d3-force`, `@types/d3-force` (dev). |

---

## 9. Risks

- **React Flow + jsdom:** React Flow won't render meaningfully under jsdom
  (needs ResizeObserver + element sizing). Mitigation: keep all logic in the two
  pure helpers (unit-tested) and cover the canvas/interaction in e2e only. Do not
  attempt to unit-test `<ReactFlow>`.
- **d3-force determinism:** simulation output isn't coordinate-stable; tests
  assert *structure* (a finite position per node), never exact coordinates.
- **Performance:** running the simulation for many ticks on large cairns could be
  slow; fixed tick count + `useMemo` keyed on `(nodes,edges)` bounds it. Fine for
  current scale; revisit for very large cairns later.
- **e2e node-click hit target:** React Flow nodes are positioned absolutely; in
  Playwright, click by visible node label text (or a node test id) rather than
  coordinates.
- **`@xyflow/react` style import:** must be imported or the canvas renders
  unstyled/zero-size; import it once in `main.tsx`.

---

## 10. Build order (for the plan)

1. Add deps (`@xyflow/react`, `d3-force`, `@types/d3-force`) + import the React Flow CSS; verify build.
2. `computeGraphLayout` (TDD: finite position per node, isolated nodes included).
3. `buildFlowElements` (TDD: node/edge mapping, labels via stem, active flag).
4. Store: `graph` + `loadGraph()` + event refresh (TDD with the mock).
5. `GraphView` React Flow wrapper (wired to the helpers; verified via build + e2e, not unit).
6. `App`: `view` toggle + top-bar Graph button + center swap + GraphView wiring.
7. e2e: toggle to Graph, see nodes, click a node → opens the note.
8. Full gate: `pnpm test`/`typecheck`/`lint`/`format:check`/`build` + `pnpm e2e`.
```
