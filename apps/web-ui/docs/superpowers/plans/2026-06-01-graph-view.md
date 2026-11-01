# Cairn Graph View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a force-directed whole-cairn link graph shown in the center pane via a top-bar "Graph" toggle; clicking a node opens that note.

**Architecture:** All logic lives in two pure, unit-tested helpers — `computeGraphLayout` (d3-force → static `{x,y}` per note) and `buildFlowElements` (→ React Flow node/edge objects). A thin `GraphView` wraps `@xyflow/react` (pan/zoom/fit + `onNodeClick`). A store `graph` slice + `loadGraph()` pull `get_graph` and refresh on note events. `App` toggles the center pane between `Editor` and `GraphView`.

**Tech Stack:** React 18 + TS + Tailwind (existing); add `@xyflow/react`, `d3-force`, `@types/d3-force`. Vitest for the pure helpers + store; Playwright e2e for the React Flow canvas (it needs real DOM).

**Reference:** Spec `docs/superpowers/specs/2026-06-01-graph-view-design.md`. The engine `get_graph` query and the `MockClient` implementation already exist; `get_graph` returns `{ type: "graph", nodes: string[], edges: { from, to }[] }` (paths). Reuse `stem()` from `web/src/client/wikilink.ts` for node labels. All work under `web/`; run commands from `web/`.

---

## File Structure

```
web/src/components/graph/computeLayout.ts        NEW  d3-force layout (pure)
web/src/components/graph/computeLayout.test.ts   NEW
web/src/components/graph/buildFlowElements.ts    NEW  → React Flow nodes/edges (pure)
web/src/components/graph/buildFlowElements.test.ts NEW
web/src/components/GraphView.tsx                 NEW  React Flow wrapper (e2e-covered)
web/src/store/store.ts                           MOD  graph slice + loadGraph + event refresh
web/src/store/store.test.ts                      MOD  loadGraph + refresh tests
web/src/app/App.tsx                              MOD  view toggle + top-bar Graph button + center swap
web/src/main.tsx                                 MOD  import @xyflow/react CSS
web/e2e/skeleton.spec.ts                         MOD  add a graph-view test
web/package.json                                 MOD  deps
```

---

## Task 1: Add dependencies + React Flow styles

**Files:** Modify `web/package.json`, `web/src/main.tsx`.

- [ ] **Step 1: Install dependencies**

From `web/`:
```bash
pnpm add @xyflow/react d3-force
pnpm add -D @types/d3-force
```

- [ ] **Step 2: Import the React Flow stylesheet**

In `web/src/main.tsx`, add (with the other CSS imports, after `import "./index.css";`):
```ts
import "@xyflow/react/dist/style.css";
```

- [ ] **Step 3: Verify build + typecheck**

Run (from `web/`): `pnpm typecheck && pnpm build`
Expected: PASS (deps resolve, CSS bundles). Chunk-size advisory is expected.

- [ ] **Step 4: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml web/src/main.tsx
git commit -m "build: add @xyflow/react + d3-force for the graph view"
```

---

## Task 2: computeGraphLayout (d3-force, pure)

**Files:** Create `web/src/components/graph/computeLayout.ts`, `web/src/components/graph/computeLayout.test.ts`.

- [ ] **Step 1: Write the failing test**

`web/src/components/graph/computeLayout.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { computeGraphLayout } from "./computeLayout";

describe("computeGraphLayout", () => {
  it("returns a finite position for every node, including isolated ones", () => {
    const nodes = ["a.md", "b.md", "c.md"]; // c.md has no links
    const edges = [{ from: "a.md", to: "b.md" }];
    const pos = computeGraphLayout(nodes, edges);
    expect(pos.size).toBe(3);
    for (const id of nodes) {
      const p = pos.get(id);
      expect(p).toBeDefined();
      expect(Number.isFinite(p!.x)).toBe(true);
      expect(Number.isFinite(p!.y)).toBe(true);
    }
  });

  it("ignores edges whose endpoints are not in the node set", () => {
    const pos = computeGraphLayout(["a.md"], [{ from: "a.md", to: "ghost.md" }]);
    expect(pos.size).toBe(1);
    expect(Number.isFinite(pos.get("a.md")!.x)).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- computeLayout`
Expected: FAIL — cannot find module `./computeLayout`.

- [ ] **Step 3: Implement**

`web/src/components/graph/computeLayout.ts`:
```ts
import {
  forceSimulation,
  forceManyBody,
  forceLink,
  forceCenter,
  forceCollide,
} from "d3-force";

interface SimNode {
  id: string;
  x?: number;
  y?: number;
}
interface SimLink {
  source: string;
  target: string;
}

/** Run a d3-force simulation to static positions, one per node. Pure: returns
 *  a Map keyed by note path. No animation; fixed tick count. */
export function computeGraphLayout(
  nodes: string[],
  edges: { from: string; to: string }[],
): Map<string, { x: number; y: number }> {
  const ids = new Set(nodes);
  const simNodes: SimNode[] = nodes.map((id) => ({ id }));
  const simLinks: SimLink[] = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));

  const sim = forceSimulation<SimNode>(simNodes)
    .force("charge", forceManyBody().strength(-200))
    .force(
      "link",
      forceLink<SimNode, SimLink>(simLinks)
        .id((d) => d.id)
        .distance(80),
    )
    .force("center", forceCenter(0, 0))
    .force("collide", forceCollide(36))
    .stop();
  sim.tick(300);

  const out = new Map<string, { x: number; y: number }>();
  for (const n of simNodes) out.set(n.id, { x: n.x ?? 0, y: n.y ?? 0 });
  return out;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- computeLayout`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/computeLayout.ts web/src/components/graph/computeLayout.test.ts
git commit -m "feat: d3-force graph layout (pure)"
```

---

## Task 3: buildFlowElements (pure mapper)

**Files:** Create `web/src/components/graph/buildFlowElements.ts`, `web/src/components/graph/buildFlowElements.test.ts`.

- [ ] **Step 1: Write the failing test**

`web/src/components/graph/buildFlowElements.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { buildFlowElements } from "./buildFlowElements";

const positions = new Map([
  ["a.md", { x: 0, y: 0 }],
  ["dir/b.md", { x: 10, y: 20 }],
]);

describe("buildFlowElements", () => {
  it("maps notes to nodes with stem labels and an active flag", () => {
    const { nodes } = buildFlowElements(
      ["a.md", "dir/b.md"],
      [],
      positions,
      "a.md",
    );
    expect(nodes).toHaveLength(2);
    const a = nodes.find((n) => n.id === "a.md")!;
    const b = nodes.find((n) => n.id === "dir/b.md")!;
    expect(a.data.label).toBe("a");
    expect(b.data.label).toBe("b"); // stem strips dir + .md
    expect(a.position).toEqual({ x: 0, y: 0 });
    expect(a.data.active).toBe(true);
    expect(b.data.active).toBe(false);
  });

  it("maps edges to React Flow edges with source/target/id", () => {
    const { edges } = buildFlowElements(
      ["a.md", "dir/b.md"],
      [{ from: "a.md", to: "dir/b.md" }],
      positions,
      null,
    );
    expect(edges).toHaveLength(1);
    expect(edges[0]).toMatchObject({
      id: "a.md->dir/b.md",
      source: "a.md",
      target: "dir/b.md",
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- buildFlowElements`
Expected: FAIL — cannot find module `./buildFlowElements`.

- [ ] **Step 3: Implement**

`web/src/components/graph/buildFlowElements.ts` (no `@xyflow/react` import — keep this helper dependency-free so its unit test never loads React Flow's runtime; `"arrowclosed"` is the string value of `MarkerType.ArrowClosed`):
```ts
import { stem } from "../../client/wikilink";

export interface FlowNode {
  id: string;
  position: { x: number; y: number };
  data: { label: string; active: boolean };
  style: Record<string, string | number>;
}
export interface FlowEdge {
  id: string;
  source: string;
  target: string;
  markerEnd: { type: "arrowclosed" };
}

const ACTIVE_STYLE: Record<string, string | number> = {
  background: "#5b7cff",
  color: "#fff",
  border: "1px solid #9ab0ff",
  borderRadius: 6,
  fontSize: 11,
  padding: 4,
};
const NODE_STYLE: Record<string, string | number> = {
  background: "#23232e",
  color: "#cfd2dc",
  border: "1px solid #3a3a4a",
  borderRadius: 6,
  fontSize: 11,
  padding: 4,
};

/** Map cairn graph data + positions to React Flow node/edge objects. Pure. */
export function buildFlowElements(
  nodes: string[],
  edges: { from: string; to: string }[],
  positions: Map<string, { x: number; y: number }>,
  activePath: string | null,
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const flowNodes: FlowNode[] = nodes.map((path) => {
    const active = path === activePath;
    return {
      id: path,
      position: positions.get(path) ?? { x: 0, y: 0 },
      data: { label: stem(path), active },
      style: active ? ACTIVE_STYLE : NODE_STYLE,
    };
  });
  const flowEdges: FlowEdge[] = edges.map((e) => ({
    id: `${e.from}->${e.to}`,
    source: e.from,
    target: e.to,
    markerEnd: { type: "arrowclosed" },
  }));
  return { nodes: flowNodes, edges: flowEdges };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- buildFlowElements`
Expected: PASS (2 tests). The helper imports nothing from `@xyflow/react`, so the unit test never loads React Flow's runtime.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/buildFlowElements.ts web/src/components/graph/buildFlowElements.test.ts
git commit -m "feat: map cairn graph to React Flow elements (pure)"
```

---

## Task 4: Store — graph slice + loadGraph + event refresh

**Files:** Modify `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Write the failing tests**

Add to `web/src/store/store.test.ts` (inside `describe("cairn store", ...)`):
```ts
  it("loadGraph populates the graph from get_graph", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().loadGraph();
    const g = store.getState().graph;
    expect(g).not.toBeNull();
    expect([...g!.nodes].sort()).toEqual(["a.md", "b.md"]);
    expect(g!.edges).toEqual([{ from: "a.md", to: "b.md" }]);
  });

  it("refreshes the graph on a note event when it is loaded", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().loadGraph();
    await client.sendCommand({ type: "write_note", path: "c.md", contents: "x" });
    await vi.waitFor(() => expect(store.getState().graph!.nodes).toContain("c.md"));
  });
```
(The `setup()` helper seeds `{ "a.md": "links to [[b]]", "b.md": "target note" }`, so `get_graph` yields nodes `["a.md","b.md"]` and edge `a.md->b.md`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- store`
Expected: FAIL — `graph`/`loadGraph` undefined.

- [ ] **Step 3: Implement**

In `web/src/store/store.ts`:

(a) Add to the `CairnState` interface:
```ts
  graph: { nodes: string[]; edges: { from: string; to: string }[] } | null;
  loadGraph(): Promise<void>;
```

(b) Add `graph: null,` to the initial state object.

(c) Add the `loadGraph` action (place near `refreshBacklinks`):
```ts
    async loadGraph() {
      try {
        const res = await client.runQuery({ type: "get_graph" });
        if (res.type === "graph") set({ graph: { nodes: res.nodes, edges: res.edges } });
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },
```

(d) In `init`, inside the event handler's `if (e.type === "note_changed" || e.type === "note_deleted") { … }` block, add a graph refresh alongside the existing refreshes:
```ts
          if (get().graph !== null) void get().loadGraph();
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test -- store` then `pnpm test`
Expected: PASS (all store tests, including the two new ones; the rest unaffected).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat: store graph slice + loadGraph with event refresh"
```

---

## Task 5: GraphView (React Flow wrapper)

**Files:** Create `web/src/components/GraphView.tsx`. (No unit test — React Flow needs real DOM; covered by e2e in Task 7.)

- [ ] **Step 1: Implement the component**

`web/src/components/GraphView.tsx`:
```tsx
import { useMemo } from "react";
import { ReactFlow, Background, Controls } from "@xyflow/react";
import { computeGraphLayout } from "./graph/computeLayout";
import { buildFlowElements } from "./graph/buildFlowElements";

export function GraphView(props: {
  nodes: string[];
  edges: { from: string; to: string }[];
  activePath: string | null;
  onOpenNote: (path: string) => void;
}) {
  const elements = useMemo(() => {
    const positions = computeGraphLayout(props.nodes, props.edges);
    return buildFlowElements(props.nodes, props.edges, positions, props.activePath);
  }, [props.nodes, props.edges, props.activePath]);

  return (
    <div className="h-full w-full">
      <ReactFlow
        nodes={elements.nodes}
        edges={elements.edges}
        fitView
        proOptions={{ hideAttribution: true }}
        onNodeClick={(_, node) => props.onOpenNote(node.id)}
      >
        <Background />
        <Controls />
      </ReactFlow>
    </div>
  );
}
```
Note: `elements.nodes`/`elements.edges` are structurally React Flow `Node[]`/`Edge[]`. If TS rejects the assignment (strict `Node`/`Edge` generics), import `type { Node, Edge } from "@xyflow/react"` and annotate `buildFlowElements`' return, or cast at the call site: `nodes={elements.nodes as Node[]}` / `edges={elements.edges as Edge[]}`. Do whatever makes `pnpm typecheck` clean without changing runtime shape.

- [ ] **Step 2: Verify typecheck + build (no unit test for this component)**

Run (from `web/`): `pnpm typecheck && pnpm build`
Expected: PASS. (React Flow renders only in a real browser; do NOT write a jsdom unit test for it — Task 7's e2e exercises it.)

- [ ] **Step 3: Commit**

```bash
git add web/src/components/GraphView.tsx
git commit -m "feat: GraphView React Flow wrapper"
```

---

## Task 6: Wire the graph toggle in App + full gate

**Files:** Modify `web/src/app/App.tsx`.

- [ ] **Step 1: Add view state, the toggle button, and the center swap**

In `web/src/app/App.tsx`:

(a) Add to the imports:
```tsx
import { useEffect, useState } from "react";
import { GraphView } from "../components/GraphView";
```
(replace the existing `import { useEffect } from "react";`).

(b) Add state + a graph selector near the other `useCairn` selectors:
```tsx
  const graph = useCairn((s) => s.graph);
  const [view, setView] = useState<"editor" | "graph">("editor");
```

(c) In the top bar's left group (after the `<SearchBar … />`), add the toggle button:
```tsx
              <button
                className="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-300 hover:bg-neutral-800"
                onClick={() => {
                  const next = view === "graph" ? "editor" : "graph";
                  setView(next);
                  if (next === "graph") void actions.loadGraph();
                }}
              >
                {view === "graph" ? "Editor" : "Graph"}
              </button>
```

(d) In the `editor={ <div className="relative h-full"> … </div> }` Shell prop, wrap the existing `<Editor … />` so the graph shows when `view === "graph"`. Replace the `<Editor … />` element with:
```tsx
            {view === "graph" ? (
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                activePath={activePath}
                onOpenNote={(p) => {
                  void actions.openNote(p);
                  setView("editor");
                }}
              />
            ) : (
              <Editor
                path={activePath}
                value={activeContents}
                mode={editorMode}
                notePaths={notePaths}
                onChange={actions.editBuffer}
                onOpenNote={actions.openNote}
                onToggleMode={() =>
                  actions.setSettings({
                    editorMode: editorMode === "rendered" ? "source" : "rendered",
                  })
                }
              />
            )}
```
(Keep the `<SearchResults … />` sibling above it unchanged.)

- [ ] **Step 2: Full gate**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. If `format:check` fails, run `pnpm format` and include the changes.

- [ ] **Step 3: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat: top-bar Graph toggle swaps the center pane to the graph view"
```

---

## Task 7: e2e — graph toggle + node-click navigation

**Files:** Modify `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: Add a graph-view test**

Append a new test to `web/e2e/skeleton.spec.ts` (it is independent — it uses the fixture notes `index.md`/`ideas.md`/`todo.md`, not the first test's mutations):
```ts
test("graph view: toggle, see nodes, click to open a note", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("ideas.md")).toBeVisible(); // app loaded (mock fixture)

  await page.getByRole("button", { name: /^graph$/i }).click();

  // React Flow renders node labels (stems) inside .react-flow.
  const flow = page.locator(".react-flow");
  await expect(flow.getByText("ideas", { exact: true })).toBeVisible();

  // Clicking the "index" node opens index.md and returns to the editor (rendered).
  await flow.getByText("index", { exact: true }).click();
  await expect(page.getByRole("heading", { name: "Index" })).toBeVisible();
});
```

- [ ] **Step 2: Run the e2e**

Run (from `web/`): `pnpm e2e`
Expected: PASS (both the original loop test and the new graph test). If port 5173 is held by a stale dev server, `lsof -ti:5173 | xargs kill` and retry. If a node label isn't clickable because React Flow centers/zooms oddly, the `fitView` should make all three visible; if `index` is occluded, click `todo` instead and assert its rendered heading "Todo" — but try `index` first.

- [ ] **Step 3: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): graph toggle shows nodes and node-click opens the note"
```

---

## Done criteria

- A top-bar **Graph** toggle swaps the center pane to a force-directed whole-cairn graph (notes list + backlinks stay); toggling back shows the editor.
- Nodes are labeled by note stem, the active note is highlighted, edges are directed; **clicking a node opens that note** and returns to the editor.
- The graph loads via `get_graph` and refreshes live on note changes while open.
- Pure helpers (`computeGraphLayout`, `buildFlowElements`) + store `loadGraph` are unit-tested; the React Flow canvas + node-click is covered by e2e. `pnpm test`/`typecheck`/`lint`/`format:check`/`build` clean; both e2e tests pass. Desktop/Tauri unaffected.
```
