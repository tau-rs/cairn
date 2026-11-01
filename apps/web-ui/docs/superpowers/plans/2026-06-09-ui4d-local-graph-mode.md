# UI‑4d Local Graph Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local-graph mode — a Local|Global toolbar switch + a depth slider that shows only the current note + its neighbors (BFS to N hops), persisted to localStorage.

**Architecture:** A pure `localGraph.ts` (`localSubgraph` BFS + settings load/save). `GraphView` keeps the **global** graph data memoized on `[nodes,edges]` (so global mode never re-simulates on note-open) and computes a **local** subgraph (depending on `activePath`/`depth`) only when enabled; `const data = useLocal ? localData : globalData`. A toolbar switch toggles the mode; a gear-panel slider sets depth; a hint shows when local is on with no open note.

**Tech Stack:** React 18 + TypeScript, `react-force-graph-2d`, Vite, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-09-ui4d-local-graph-mode-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if needed.
- e2e on port 5273. Current: 160 unit, 8 e2e, all green.
- **Relevant existing `GraphView.tsx`:**
  - `const data = useMemo(() => buildGraphData(props.nodes, props.edges), [props.nodes, props.edges])` and `const adjacency = useMemo(() => buildAdjacency(buildGraphData(props.nodes, props.edges).links), [props.nodes, props.edges])`.
  - Props: `nodes`, `edges`, `activePath`, `tagsByNote`, `onOpenNote`.
  - `fittedRef` reset on `[data]` change (zoom-to-fit re-runs when `data` identity changes).
  - Gear overlay (top-right): `{panelOpen && (<><GraphGroupsPanel …/><GraphForcesPanel …/></>)}`.
  - The `<ForceGraph2D>` is rendered under `{size.width>0 && size.height>0 && (…)}`.
  - localStorage settings pattern: `forceSettings.ts`/`colorGroups.ts`. jsdom localStorage works (vitest.setup fix in place).
- Canvas can't be unit-tested under jsdom — unit-test `localGraph.ts`; the mode/depth UI + subgraph render are verified by build + a canvas e2e + manual visual check.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/graph/localGraph.ts` | Pure `localSubgraph` + `LocalGraphSettings` + `DEPTH_RANGE` + load/save. |
| `web/src/components/graph/localGraph.test.ts` | Unit tests. |
| `web/src/components/GraphView.tsx` | Toolbar Local\|Global switch + depth slider + subgraph data + no-active hint. |
| `web/e2e/skeleton.spec.ts` | Open a note → Graph → Local → canvas renders. |

---

## Task 1: localGraph (pure subgraph + settings)

**Files:**
- Create: `web/src/components/graph/localGraph.ts`
- Create: `web/src/components/graph/localGraph.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/localGraph.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  DEFAULT_LOCAL_GRAPH,
  localSubgraph,
  loadLocalGraph,
  saveLocalGraph,
} from "./localGraph";

const nodes = ["a", "b", "c", "d", "x"];
// a-b, b-c, c-d (chain); x isolated. Edges are directional in the data but
// neighbors are undirected.
const edges = [
  { from: "a", to: "b" },
  { from: "c", to: "b" }, // incoming to b — undirected reach
  { from: "c", to: "d" },
];

describe("localSubgraph", () => {
  it("depth 0 → just the root", () => {
    expect(localSubgraph(nodes, edges, "b", 0)).toEqual({
      nodes: ["b"],
      edges: [],
    });
  });
  it("depth 1 → root + direct neighbors (both directions)", () => {
    const s = localSubgraph(nodes, edges, "b", 1);
    expect(s.nodes.sort()).toEqual(["a", "b", "c"]);
    expect(s.edges).toEqual([
      { from: "a", to: "b" },
      { from: "c", to: "b" },
    ]);
  });
  it("depth 2 → two hops", () => {
    const s = localSubgraph(nodes, edges, "a", 2);
    expect(s.nodes.sort()).toEqual(["a", "b", "c"]); // a→b→c
  });
  it("returns empty when the root is absent or null", () => {
    expect(localSubgraph(nodes, edges, "missing", 2)).toEqual({
      nodes: [],
      edges: [],
    });
    expect(localSubgraph(nodes, edges, null, 2)).toEqual({
      nodes: [],
      edges: [],
    });
  });
  it("keeps each node once even via multiple paths", () => {
    const s = localSubgraph(nodes, edges, "b", 3);
    expect(new Set(s.nodes).size).toBe(s.nodes.length);
  });
});

describe("loadLocalGraph / saveLocalGraph", () => {
  beforeEach(() => localStorage.clear());
  it("returns the default when empty or corrupt", () => {
    expect(loadLocalGraph()).toEqual(DEFAULT_LOCAL_GRAPH);
    localStorage.setItem("cairn.graph.local", "{bad");
    expect(loadLocalGraph()).toEqual(DEFAULT_LOCAL_GRAPH);
  });
  it("clamps depth to 1..3 and coerces enabled to bool", () => {
    localStorage.setItem(
      "cairn.graph.local",
      JSON.stringify({ enabled: 1, depth: 99 }),
    );
    expect(loadLocalGraph()).toEqual({ enabled: true, depth: 3 });
  });
  it("round-trips and swallows storage errors", () => {
    saveLocalGraph({ enabled: true, depth: 2 });
    expect(loadLocalGraph()).toEqual({ enabled: true, depth: 2 });
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveLocalGraph(DEFAULT_LOCAL_GRAPH)).not.toThrow();
    spy.mockRestore();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- localGraph`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `localGraph.ts`**

Create `web/src/components/graph/localGraph.ts`:

```ts
export interface LocalGraphSettings {
  enabled: boolean;
  depth: number;
}

export const DEFAULT_LOCAL_GRAPH: LocalGraphSettings = {
  enabled: false,
  depth: 1,
};
export const DEPTH_RANGE = { min: 1, max: 3, step: 1 } as const;

const STORAGE_KEY = "cairn.graph.local";

/** BFS from `root` over UNDIRECTED edges to `depth` hops. Returns the reached
 *  nodes (incl. root, in input order) and the edges whose endpoints are both
 *  reached. root null / not in `nodes` → empty. */
export function localSubgraph(
  nodes: string[],
  edges: { from: string; to: string }[],
  root: string | null,
  depth: number,
): { nodes: string[]; edges: { from: string; to: string }[] } {
  if (!root || !nodes.includes(root)) return { nodes: [], edges: [] };

  const present = new Set(nodes);
  const adj = new Map<string, string[]>();
  const addAdj = (a: string, b: string) => {
    const list = adj.get(a);
    if (list) list.push(b);
    else adj.set(a, [b]);
  };
  for (const e of edges) {
    if (present.has(e.from) && present.has(e.to)) {
      addAdj(e.from, e.to);
      addAdj(e.to, e.from);
    }
  }

  const reached = new Set<string>([root]);
  let frontier = [root];
  for (let d = 0; d < depth && frontier.length; d++) {
    const next: string[] = [];
    for (const n of frontier) {
      for (const m of adj.get(n) ?? []) {
        if (!reached.has(m)) {
          reached.add(m);
          next.push(m);
        }
      }
    }
    frontier = next;
  }

  return {
    nodes: nodes.filter((n) => reached.has(n)),
    edges: edges.filter((e) => reached.has(e.from) && reached.has(e.to)),
  };
}

const clampDepth = (d: number): number =>
  Math.min(DEPTH_RANGE.max, Math.max(DEPTH_RANGE.min, Math.round(d)));

export function loadLocalGraph(): LocalGraphSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_LOCAL_GRAPH;
    const p = JSON.parse(raw) as Partial<LocalGraphSettings>;
    return {
      enabled: !!p.enabled,
      depth: clampDepth(typeof p.depth === "number" ? p.depth : 1),
    };
  } catch {
    return DEFAULT_LOCAL_GRAPH;
  }
}

export function saveLocalGraph(s: LocalGraphSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- localGraph`
Expected: PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (160 + new localGraph tests). Fix format if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/localGraph.ts web/src/components/graph/localGraph.test.ts
git commit -m "feat(graph): localSubgraph BFS + local-graph settings persistence"
```

---

## Task 2: Wire local mode into GraphView + e2e

**Files:**
- Modify: `web/src/components/GraphView.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Imports + local state**

In `web/src/components/GraphView.tsx`, add imports:

```tsx
import {
  type LocalGraphSettings,
  DEPTH_RANGE,
  localSubgraph,
  loadLocalGraph,
  saveLocalGraph,
} from "./graph/localGraph";
```

Add state near the other state hooks (after `groups`/`changeGroups`):

```tsx
  const [local, setLocal] = useState<LocalGraphSettings>(loadLocalGraph);
  const changeLocal = (next: LocalGraphSettings) => {
    setLocal(next);
    saveLocalGraph(next);
  };
```

- [ ] **Step 2: Global vs local graph data (preserve the no-re-sim invariant)**

Replace the existing `data` + `adjacency` memos with the global pair (renamed) plus a local pair and a switch. Replace:

```tsx
  const data = useMemo(
    () => buildGraphData(props.nodes, props.edges),
    [props.nodes, props.edges],
  );
  const adjacency = useMemo(
    () => buildAdjacency(buildGraphData(props.nodes, props.edges).links),
    [props.nodes, props.edges],
  );
```

with:

```tsx
  // Global graph — memoized on [nodes, edges] ONLY, so opening a note in global
  // mode never restarts the simulation.
  const globalData = useMemo(
    () => buildGraphData(props.nodes, props.edges),
    [props.nodes, props.edges],
  );
  const globalAdj = useMemo(
    () => buildAdjacency(buildGraphData(props.nodes, props.edges).links),
    [props.nodes, props.edges],
  );

  // Local subgraph — computed ONLY when local mode is on with a note open;
  // depends on activePath/depth (the focused neighborhood genuinely changes).
  const useLocal = local.enabled && !!props.activePath;
  const localSub = useMemo(
    () =>
      useLocal
        ? localSubgraph(props.nodes, props.edges, props.activePath, local.depth)
        : null,
    [useLocal, props.nodes, props.edges, props.activePath, local.depth],
  );
  const localData = useMemo(
    () => (localSub ? buildGraphData(localSub.nodes, localSub.edges) : null),
    [localSub],
  );
  const localAdj = useMemo(
    () =>
      localSub
        ? buildAdjacency(buildGraphData(localSub.nodes, localSub.edges).links)
        : null,
    [localSub],
  );

  const data = localData ?? globalData;
  const adjacency = localAdj ?? globalAdj;
```

(The rest of the component — `paintNode`, the `fittedRef`-reset effect keyed on `[data]`, the `<ForceGraph2D graphData={data}>` — uses `data`/`adjacency` unchanged. When `data` switches global↔local or the local subgraph changes, `fittedRef` resets and `zoomToFit` re-runs on the next engine stop.)

- [ ] **Step 3: Local|Global toolbar switch (top-left)**

Inside the container `<div … className="relative h-full w-full">`, add this as the FIRST child (before the existing top-right gear overlay div):

```tsx
      <div className="absolute left-2 top-2 z-10 flex overflow-hidden rounded-md border border-border text-[11px]">
        {(["local", "global"] as const).map((m) => {
          const isLocal = m === "local";
          const selected = local.enabled === isLocal;
          return (
            <button
              key={m}
              type="button"
              aria-pressed={selected}
              className={
                "px-2.5 py-1 capitalize " +
                (selected
                  ? "bg-accent text-accent-fg"
                  : "bg-surface text-muted hover:text-text")
              }
              onClick={() => changeLocal({ ...local, enabled: isLocal })}
            >
              {m}
            </button>
          );
        })}
      </div>
```

- [ ] **Step 4: Depth slider in the gear panel**

In the `{panelOpen && (…)}` overlay fragment, add a "Local graph" section ABOVE the Groups panel (so the panel reads Local → Groups → Forces):

```tsx
        {panelOpen && (
          <>
            <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
              <div className="mb-2 text-[10px] uppercase tracking-wide text-faint">
                Local graph
              </div>
              <div className="flex items-center justify-between gap-2 text-[11px] text-text">
                <span>Depth</span>
                <span className="text-faint">{local.depth}</span>
              </div>
              <input
                type="range"
                aria-label="Local graph depth"
                className="w-full accent-accent"
                min={DEPTH_RANGE.min}
                max={DEPTH_RANGE.max}
                step={DEPTH_RANGE.step}
                value={local.depth}
                onChange={(e) =>
                  changeLocal({ ...local, depth: Number(e.target.value) })
                }
              />
            </div>
            <GraphGroupsPanel groups={groups} onChange={changeGroups} />
            <GraphForcesPanel
              settings={forces}
              onChange={changeForces}
              onReset={() => changeForces(DEFAULT_FORCE_SETTINGS)}
            />
          </>
        )}
```

- [ ] **Step 5: No-active-note hint**

Wrap the `<ForceGraph2D>` render so that, when local is enabled with no open note, a hint shows instead. Replace:

```tsx
      {size.width > 0 && size.height > 0 && (
        <ForceGraph2D
          /* …props… */
        />
      )}
```

with:

```tsx
      {local.enabled && !props.activePath ? (
        <div className="flex h-full w-full items-center justify-center text-sm text-faint">
          Open a note to see its local graph
        </div>
      ) : (
        size.width > 0 &&
        size.height > 0 && (
          <ForceGraph2D
            /* …keep ALL existing props exactly as they are… */
          />
        )
      )}
```

(Keep the existing `<ForceGraph2D>` element and every prop unchanged — only the surrounding conditional changes.)

- [ ] **Step 6: Gate (component is canvas — no new unit test here)**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS (160 unit; build confirms wiring). The data/adjacency switch and the new state must typecheck; lint stays warning-clean.

- [ ] **Step 7: Add the e2e (local mode renders the canvas)**

In `web/e2e/skeleton.spec.ts`, add a new test:

```ts
test("graph local mode: open a note, switch to Local, canvas renders", async ({
  page,
}) => {
  await page.goto("/");
  // Open a note so the graph has a root.
  await page.getByRole("button", { name: "index.md" }).click();
  // Switch to the graph view.
  await page.getByRole("button", { name: /^graph$/i }).click();
  // Toggle to Local — the canvas (now the index.md neighborhood) still renders.
  await page.getByRole("button", { name: "Local" }).click();
  await expect(page.locator("canvas").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Local" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});
```

(Keep the existing graph tests unchanged. The selector `index.md` is a note in the sidebar list; clicking it opens that note so `activePath` is set before entering the graph.)

- [ ] **Step 8: Run e2e**

Run: `pnpm e2e`
Expected: 9/9 (8 existing + the new local-mode test). If the "Local" button isn't found or the canvas doesn't render after toggling Local (e.g. the no-active hint showed because the note didn't open first), debug the test sequence; if local mode genuinely renders nothing with an open note, STOP and report.

- [ ] **Step 9: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 10: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop it. Report the app loads. (The human confirms: with a note open, Local shows only that note + neighbors; the depth slider grows/shrinks it; Global restores the full graph; with no note open, Local shows the hint; the choice + depth persist across reload; groups/forces still apply in local mode.)

- [ ] **Step 11: Commit**

```bash
git add web/src/components/GraphView.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(graph): local-graph mode (Local|Global switch + depth + subgraph)"
```

---

## Notes for the executor

- **Re-sim invariant:** `globalData`/`globalAdj` stay memoized on `[props.nodes, props.edges]` ONLY. `localData`/`localAdj` are computed (and depend on `activePath`/`depth`) but are `null` unless local mode is on with a note open. `const data = localData ?? globalData` means global mode uses the stable global data (no relayout on note-open); local mode relayouts when the focused note/depth changes — which is intended.
- **Adjacency must match the shown graph** (built from `localSub` in local mode) so hover-neighbor highlighting reflects the visible subgraph.
- **`fittedRef` reset on `[data]`** already exists — switching mode / changing the subgraph changes `data` identity, so `zoomToFit` re-runs on the next engine stop and re-frames the local view.
- **Don't change `paintNode`, `linkColor`, the forces effect, or the `<ForceGraph2D>` props** — this task only restructures the data memos and adds the toolbar switch, the depth section, and the no-active hint.
- **Persistence:** key `cairn.graph.local` (distinct from forces/groups). The mode + depth persist; the no-active hint covers reopening in local mode with no note.
- **Canvas is manual-visual:** `localSubgraph` + settings are unit-tested; the toggle/render are covered by build + the canvas e2e (open note → Graph → Local → canvas) + the manual visual check.
