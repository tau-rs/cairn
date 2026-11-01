# UI‑4a Obsidian-Style Graph (Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the React Flow graph with a canvas force-graph (`react-force-graph-2d`) that matches Obsidian: circular dots sized by link count, faint arrowless links, live physics, drag, pan/zoom, hover-highlights-neighbors, zoom-fading labels, active-note tint, click-to-open — on the graphite palette.

**Architecture:** A pure `graphData.ts` (build force-graph data + degree + adjacency + node-radius + label-alpha — all unit-tested) feeds a rewritten `GraphView` that mounts `<ForceGraph2D>` with a custom `nodeCanvasObject` painter. Physics/positions are owned by react-force-graph; `activePath`/hover are read in the paint callbacks (never in `graphData` deps) so they don't restart the simulation.

**Tech Stack:** React 18 + TypeScript, `react-force-graph-2d` (canvas + d3-force), Vite, Vitest, Playwright. Removes `@xyflow/react` + `d3-force`.

**Spec:** `docs/superpowers/specs/2026-06-05-ui4a-obsidian-graph-core-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if format fails.
- e2e runs on port 5273 (configured; 5173 is tau-web-ui). Dev convention: `pnpm dev --port 5273 --strictPort`.
- Current state: 130 unit tests, 8 e2e, all green.
- **Relevant existing code:**
  - `GraphView.tsx` props: `{ nodes: string[]; edges: { from: string; to: string }[]; activePath: string | null; onOpenNote: (path: string) => void }`. Used in `App.tsx:117`.
  - `computeLayout.ts` (d3-force one-shot) + `buildFlowElements.ts` (React Flow elements) + `computeLayout.test.ts` — all REPLACED by `graphData.ts` (delete the three).
  - `main.tsx:7` imports `@xyflow/react/dist/style.css` — REMOVE.
  - `web/src/client/wikilink.ts` exports `stem(path)` (note path → display label), already used by the old `buildFlowElements`.
- CodeMirror/canvas don't render under jsdom — unit-test only the pure helpers; the component is verified by build + the canvas e2e + manual visual check.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/graph/graphData.ts` | Pure: `buildGraphData`, `buildAdjacency`, `nodeRadius`, `labelAlpha`, types `GNode`/`GLink`. |
| `web/src/components/graph/graphData.test.ts` | Unit tests for the four pure functions. |
| `web/src/components/GraphView.tsx` | `<ForceGraph2D>` + custom canvas paint + hover/drag/zoom/click. |
| `web/src/main.tsx` | Remove the React Flow CSS import. |
| `web/package.json` | − `@xyflow/react`, − `d3-force`, − `@types/d3-force`; + `react-force-graph-2d`. |
| `web/e2e/skeleton.spec.ts` | Canvas-based graph e2e. |
| (delete) `computeLayout.ts`, `computeLayout.test.ts`, `buildFlowElements.ts` | Removed. |

---

## Task 1: Pure graph-data helpers

**Files:**
- Create: `web/src/components/graph/graphData.ts`
- Create: `web/src/components/graph/graphData.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/graphData.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import {
  buildGraphData,
  buildAdjacency,
  nodeRadius,
  labelAlpha,
} from "./graphData";

describe("buildGraphData", () => {
  const nodes = ["a.md", "b.md", "c.md"];
  const edges = [
    { from: "a.md", to: "b.md" },
    { from: "a.md", to: "c.md" },
    { from: "a.md", to: "missing.md" }, // dropped: endpoint not in nodes
  ];
  it("labels nodes by stem and counts undirected degree", () => {
    const { nodes: gn } = buildGraphData(nodes, edges);
    const byId = Object.fromEntries(gn.map((n) => [n.id, n]));
    expect(byId["a.md"].label).toBe("a");
    expect(byId["a.md"].degree).toBe(2); // a–b, a–c (missing dropped)
    expect(byId["b.md"].degree).toBe(1);
    expect(byId["c.md"].degree).toBe(1);
  });
  it("drops links whose endpoint is not a known node", () => {
    const { links } = buildGraphData(nodes, edges);
    expect(links).toEqual([
      { source: "a.md", target: "b.md" },
      { source: "a.md", target: "c.md" },
    ]);
  });
});

describe("buildAdjacency", () => {
  it("builds symmetric neighbor sets", () => {
    const adj = buildAdjacency([
      { source: "a", target: "b" },
      { source: "a", target: "c" },
    ]);
    expect([...(adj.get("a") ?? [])].sort()).toEqual(["b", "c"]);
    expect([...(adj.get("b") ?? [])]).toEqual(["a"]);
    expect(adj.get("z")).toBeUndefined(); // isolated / unknown
  });
});

describe("nodeRadius", () => {
  it("is base at degree 0 and monotonic non-decreasing", () => {
    expect(nodeRadius(0)).toBe(3);
    expect(nodeRadius(1)).toBeGreaterThan(nodeRadius(0));
    expect(nodeRadius(9)).toBeGreaterThan(nodeRadius(4));
  });
});

describe("labelAlpha", () => {
  it("is 0 when zoomed out, 1 when zoomed in, clamped and monotonic", () => {
    expect(labelAlpha(1.0)).toBe(0);
    expect(labelAlpha(3.0)).toBe(1);
    const mid = labelAlpha(1.85);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- graphData`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `graphData.ts`**

Create `web/src/components/graph/graphData.ts`:

```ts
import { stem } from "../../client/wikilink";

export interface GNode {
  id: string;
  label: string;
  degree: number;
}
export interface GLink {
  source: string;
  target: string;
}

/** Build force-graph data: degree = count of links touching the node
 *  (undirected); label = stem(path); links filtered to edges whose endpoints
 *  both exist. */
export function buildGraphData(
  nodes: string[],
  edges: { from: string; to: string }[],
): { nodes: GNode[]; links: GLink[] } {
  const ids = new Set(nodes);
  const links: GLink[] = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));

  const degree = new Map<string, number>();
  for (const id of nodes) degree.set(id, 0);
  for (const l of links) {
    degree.set(l.source, (degree.get(l.source) ?? 0) + 1);
    degree.set(l.target, (degree.get(l.target) ?? 0) + 1);
  }

  const gnodes: GNode[] = nodes.map((id) => ({
    id,
    label: stem(id),
    degree: degree.get(id) ?? 0,
  }));
  return { nodes: gnodes, links };
}

/** Symmetric adjacency for hover-neighbor highlighting. Pass STRING-keyed links
 *  (react-force-graph mutates link.source/target into node objects, so build
 *  this from the original string links, not the mutated array). */
export function buildAdjacency(links: GLink[]): Map<string, Set<string>> {
  const adj = new Map<string, Set<string>>();
  const add = (a: string, b: string) => {
    if (!adj.has(a)) adj.set(a, new Set());
    adj.get(a)!.add(b);
  };
  for (const l of links) {
    add(l.source, l.target);
    add(l.target, l.source);
  }
  return adj;
}

/** Node radius from degree — sublinear so hubs are bigger but not huge. */
export function nodeRadius(degree: number): number {
  return 3 + 1.6 * Math.sqrt(degree);
}

/** Label opacity from the current zoom scale — hidden when zoomed out, ramps to
 *  1 as you zoom in. */
export function labelAlpha(zoom: number): number {
  const lo = 1.2;
  const hi = 2.5;
  if (zoom <= lo) return 0;
  if (zoom >= hi) return 1;
  return (zoom - lo) / (hi - lo);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- graphData`
Expected: PASS (all describe blocks green).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (130 existing + new graphData tests). The old `computeLayout.test.ts` still exists and passes here (deleted in Task 2). Fix format if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/graphData.ts web/src/components/graph/graphData.test.ts
git commit -m "feat(graph): pure force-graph data helpers (degree, adjacency, radius, label-alpha)"
```

---

## Task 2: Swap dependency + rewrite GraphView

**Files:**
- Modify: `web/package.json` (via pnpm)
- Modify: `web/src/main.tsx`
- Rewrite: `web/src/components/GraphView.tsx`
- Delete: `web/src/components/graph/computeLayout.ts`, `computeLayout.test.ts`, `buildFlowElements.ts`

- [ ] **Step 1: Swap dependencies**

Run (from `web/`):
```bash
pnpm remove @xyflow/react d3-force @types/d3-force
pnpm add react-force-graph-2d
```
Expected: `@xyflow/react`, `d3-force`, `@types/d3-force` gone from `package.json`; `react-force-graph-2d` added.

- [ ] **Step 2: Remove the React Flow CSS import**

In `web/src/main.tsx`, delete the line:
```ts
import "@xyflow/react/dist/style.css";
```

- [ ] **Step 3: Delete the obsolete layout files**

```bash
git rm web/src/components/graph/computeLayout.ts web/src/components/graph/computeLayout.test.ts web/src/components/graph/buildFlowElements.ts
```

- [ ] **Step 4: Rewrite `GraphView.tsx`**

Replace `web/src/components/GraphView.tsx` entirely with:

```tsx
import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import ForceGraph2D, { type ForceGraphMethods } from "react-force-graph-2d";
import {
  buildGraphData,
  buildAdjacency,
  nodeRadius,
  labelAlpha,
  type GLink,
} from "./graph/graphData";

// react-force-graph mutates node objects (adds x/y/vx/vy) and rewrites
// link.source/target from id strings into node references at runtime.
interface RFNode {
  id: string;
  label: string;
  degree: number;
  x?: number;
  y?: number;
}

export function GraphView(props: {
  nodes: string[];
  edges: { from: string; to: string }[];
  activePath: string | null;
  onOpenNote: (path: string) => void;
}) {
  // Graph shape only — NOT activePath/hover, so the simulation never restarts
  // when you open a note or hover.
  const data = useMemo(
    () => buildGraphData(props.nodes, props.edges),
    [props.nodes, props.edges],
  );
  // Adjacency from a fresh string-keyed build (the `data.links` array gets
  // mutated by react-force-graph, so don't read neighbor ids from it).
  const adjacency = useMemo(
    () => buildAdjacency(buildGraphData(props.nodes, props.edges).links),
    [props.nodes, props.edges],
  );

  const fgRef = useRef<ForceGraphMethods>();
  const containerRef = useRef<HTMLDivElement>(null);
  const hoverRef = useRef<string | null>(null);
  const fittedRef = useRef(false);
  const [size, setSize] = useState({ width: 0, height: 0 });

  // Size the canvas to the container.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => setSize({ width: el.clientWidth, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // New graph data → allow one zoom-to-fit.
  useEffect(() => {
    fittedRef.current = false;
  }, [data]);

  const highlight = (): Set<string> | null => {
    const h = hoverRef.current;
    if (!h) return null;
    const set = new Set<string>([h]);
    for (const n of adjacency.get(h) ?? []) set.add(n);
    return set;
  };

  const paintNode = useCallback(
    (node: RFNode, ctx: CanvasRenderingContext2D, scale: number) => {
      const hl = highlight();
      const active = node.id === props.activePath;
      const lit = hl ? hl.has(node.id) : true;
      const r = nodeRadius(node.degree);

      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, r, 0, 2 * Math.PI);
      ctx.fillStyle = active ? "#6366f1" : lit ? "#cdd0e0" : "#6b6c7755";
      ctx.fill();

      let alpha = labelAlpha(scale);
      if (active || node.id === hoverRef.current) alpha = 1;
      if (alpha > 0) {
        const font = 12 / scale; // constant on-screen size
        ctx.font = `${font}px Inter, system-ui, sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.globalAlpha = alpha;
        ctx.fillStyle = active ? "#cdd0e0" : "#9a9ba6";
        ctx.fillText(node.label, node.x ?? 0, (node.y ?? 0) + r + 1);
        ctx.globalAlpha = 1;
      }
    },
    [props.activePath, adjacency],
  );

  const paintPointer = useCallback(
    (node: RFNode, color: string, ctx: CanvasRenderingContext2D) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, nodeRadius(node.degree), 0, 2 * Math.PI);
      ctx.fill();
    },
    [],
  );

  // Link colors react to hover (links touching the hovered node light up).
  const linkColor = useCallback(
    (link: { source: RFNode | string; target: RFNode | string }) => {
      const h = hoverRef.current;
      if (!h) return "#3a3a44";
      const sid = typeof link.source === "string" ? link.source : link.source.id;
      const tid = typeof link.target === "string" ? link.target : link.target.id;
      return sid === h || tid === h ? "#6366f1aa" : "#26262e66";
    },
    [],
  );

  // Single stable container (the ref/ResizeObserver always track THIS div);
  // ForceGraph2D mounts once the container has a measured size.
  return (
    <div ref={containerRef} className="h-full w-full">
      {size.width > 0 && size.height > 0 && (
        <ForceGraph2D
          ref={fgRef}
          width={size.width}
          height={size.height}
          graphData={data as { nodes: RFNode[]; links: GLink[] }}
          backgroundColor="rgba(0,0,0,0)"
          nodeCanvasObject={paintNode}
          nodePointerAreaPaint={paintPointer}
          linkColor={linkColor}
          linkWidth={1}
          autoPauseRedraw={false}
          enableNodeDrag
          onNodeClick={(n: RFNode) => props.onOpenNote(n.id)}
          onNodeHover={(n: RFNode | null) => {
            hoverRef.current = n?.id ?? null;
          }}
          onEngineStop={() => {
            if (!fittedRef.current) {
              fittedRef.current = true;
              fgRef.current?.zoomToFit(400, 40);
            }
          }}
        />
      )}
    </div>
  );
}
```

Notes for the implementer:
- `autoPauseRedraw={false}` keeps the canvas repainting so the hover highlight (stored in a ref) updates live without a React re-render.
- `activePath` is read inside `paintNode` (and is a `useCallback` dep), so opening a note repaints but does NOT touch `graphData` → no re-simulation.
- The `as` casts bridge our `GNode` shape to react-force-graph's loose node type; if the installed types differ, adjust the casts minimally — do not change the data shape.

- [ ] **Step 5: Gate (no unit test for the component — it's canvas)**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. Unit count DROPS by the `computeLayout.test.ts` cases (deleted) and the graph tests are now `graphData.test.ts` (from Task 1). `grep -rn "xyflow\|d3-force\|computeLayout\|buildFlowElements" src` → no matches. `pnpm build` must succeed (confirms react-force-graph bundles and nothing imports the removed packages).

- [ ] **Step 6: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml web/src/main.tsx web/src/components/GraphView.tsx
git commit -m "feat(graph): Obsidian-style canvas force-graph (react-force-graph-2d)"
```

---

## Task 3: Canvas e2e + final gate + visual check

**Files:**
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Rewrite the graph e2e for canvas**

In `web/e2e/skeleton.spec.ts`, replace the existing `test("graph view: toggle, see nodes, click to open a note", …)` with:

```ts
test("graph view: toggle shows the force-graph canvas", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("ideas.md")).toBeVisible(); // app loaded (mock fixture)

  await page.getByRole("button", { name: /^graph$/i }).click();

  // The force-graph renders a <canvas>; the toggle flips to "Editor".
  await expect(page.locator("canvas").first()).toBeVisible();
  await expect(
    page.getByRole("button", { name: /^editor$/i }),
  ).toBeVisible();
});
```

(Canvas nodes have no DOM text and physics-driven positions, so we don't click a specific node here — that interaction is verified in the manual visual check. Keep the other e2e tests unchanged.)

- [ ] **Step 2: Run e2e**

Run: `pnpm e2e`
Expected: all 8 tests pass (the rewritten graph test + 7 others). If the canvas isn't visible, check that the GraphView container has a non-zero height (it's `h-full w-full` inside the graph pane) so the ResizeObserver yields a size and `<ForceGraph2D>` mounts. If a real failure (canvas never mounts), STOP and report.

- [ ] **Step 3: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 4: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop the server. Report the app loads. (The human confirms: circular degree-sized dots, faint links, label zoom-fade, hover-highlights-neighbors, drag, pan/zoom, click-opens-note, active-note tint.)

- [ ] **Step 5: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): canvas-based graph view assertion"
```

---

## Notes for the executor

- **Don't put `activePath`/hover in `graphData` deps.** The whole point of §4 in the spec: graph data is memoized on `[nodes, edges]` only; active-note + hover are read in the paint callbacks (active via `useCallback` dep → repaint; hover via a ref + `autoPauseRedraw={false}`). Otherwise the graph visibly re-lays-out on every note open/hover.
- **Adjacency from string links.** react-force-graph mutates `link.source`/`target` into node objects in place. Build `buildAdjacency` from a fresh `buildGraphData(...).links` (strings), not from the array handed to `<ForceGraph2D>`.
- **Canvas can't be unit-tested** under jsdom — only `graphData.ts` is unit-tested; the component is covered by build + the canvas e2e + the manual visual check (a real coverage reduction vs the old DOM-label test, per the spec).
- **Tuning is expected.** Node radius (`3 + 1.6√degree`), label-fade thresholds (`labelAlpha`), and colors are sensible defaults; refine at the visual check if asked, but they're not blocking.
- **react-force-graph type quirks:** the library's exported types are loose; keep the `RFNode` interface + minimal `as` casts rather than fighting the generics. Do not weaken the data shape from `graphData.ts`.
