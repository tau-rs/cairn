# Graph Seam Upgrade Implementation Plan

> ⛔ **SUPERSEDED — DO NOT EXECUTE.** This plan targets the *frozen brief* shapes. The engine shipped the graph seam diverged from that brief (PRs #106/#107). The live plan is **`2026-07-19-graph-seam-upgrade-reconciled.md`**, which retargets every task to the engine's actual shapes (tagged-enum `GraphScope`, `SuggestedEdge {weight,why}`, `mtime_secs` coercion, etc.). See `docs/superpowers/specs/2026-07-19-graph-seam-contract-reconcile-design.md` for the decision. This file is retained only for lineage/fork-decision context.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Evolve the existing `react-force-graph-2d` graph view onto the new engine seam contract (`GetGraph { scope }` → `Graph { GraphNode[], GraphEdge[] }`), moving neighborhood scoping server-side, adopting server degree/title/tags, adding recency encoding, defaulting to a local/focus view, and scaffolding inert seams for suggested edges + temporal.

**Architecture:** Server-side scope replaces the client BFS: `loadGraph` issues a scoped `GetGraph` and the engine returns node metadata. Pure helpers (`recency.ts`, `globalCap.ts`, `graphFilter.ts`, `graphData.ts`) do the transforms and are unit-tested in isolation; `store.ts` owns the fetch/cache/focus state; `GraphView.tsx` + `GraphGroupsPanel.tsx` render. Suggested edges and temporal are designed as one merged `GLink { kind }` model + flag-gated queries so they slot in without a rewrite.

**Tech Stack:** TypeScript, React 19, zustand, react-force-graph-2d ^1.29, Vitest, Playwright, Stryker. Package manager: `pnpm` (run from `web/`). Repo gate: `just` (see `justfile`).

## Global Constraints

- **BLOCKED ON ENGINE TRACK 1.** Do not start Task 1+ until the Track-1 seam is merged in `tau-rs/cairn` `main` and Task 0 (re-vendor) is complete. As of writing, `get_graph` still returns `nodes: string[]` and there are no `GraphNode`/`GraphScope` bindings.
- **In-place upgrade, not a rewrite.** Modify the named files; do not create a parallel graph component.
- **Contract files are vendored, generated, raw ts-rs.** `web/src/contract/*` is in `web/.prettierignore`; never hand-format them. Copy verbatim from `crates/cairn-contract/bindings/`.
- **Global node cap = 1,500.** Recency window default = 30 days. Both are named constants.
- **`showSuggestions` and recency ring both default OFF.** Suggested edges never affect node degree/size.
- **TDD:** failing test → run → implement → run → commit. One logical change per commit. Run the full local gate (`just web-check` or the repo's documented gate incl. `prettier --check`) before claiming green.
- **Contract drift:** after re-vendoring, `contractGuards.ts` must cover the new `graph` response shape and the drift-check CI must pass.

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `web/src/contract/*` | Re-vendored bindings (`GraphNode`, `GraphScope`, `Graph`, updated `Query`/`QueryResponse`, `SuggestedEdge`, `Suggestions`, `GetSuggestions`, `GraphAt`) | 0 |
| `web/src/client/contractGuards.ts` | Extend response guard for the new `graph` shape | 0 |
| `web/src/client/mock.ts` | Mock scoped `get_graph` (metadata + BFS), empty `get_suggestions`, unsupported `graph_at` | 0 |
| `web/src/components/graph/recency.ts` (+ `.test.ts`) | Pure: `mtime_secs` → ring alpha/width; settings load/save | 1 |
| `web/src/components/graph/globalCap.ts` (+ `.test.ts`) | Pure: cap global to top-N by degree + truncation info | 2 |
| `web/src/components/graph/colorGroups.ts` | Add `matchGroup()` (group identity, not just color) | 3 |
| `web/src/components/graph/graphFilter.ts` (+ `.test.ts`) | Pure: apply min-degree + hidden-group + hide-ungrouped filters | 3 |
| `web/src/components/graph/graphData.ts` (+ `.test.ts`) | Consume `GraphNode[]`; `GLink.kind`/`weight`; explicit+suggested merge | 4 |
| `web/src/components/graph/forceGraphTypes.ts` | `RFNode` gains `tags`/`mtimeSecs` | 4 |
| `web/src/store/store.ts` (+ `store.test.ts`) | New graph state; scoped/cached `loadGraph`; `setGraphFocus`; sync rule; cascade fix; drop `noteTags` | 5 |
| `web/src/components/GraphView.tsx` | New props; local-default; modifier-split click; recency ring; link dash; cap banner; gated scrubber stub | 6 |
| `web/src/components/graph/GraphGroupsPanel.tsx` | Legend + filter controls (eye toggles, min-degree, recency, ungrouped, suggestions toggle) | 7 |
| `web/src/components/EditorPane.tsx` | Wire new store props + `setGraphFocus`/open callbacks | 8 |
| `web/src/components/graph/localGraph.ts` (+ `.test.ts`) | **Deleted** | 9 |
| `web/src/client/{types,daemon,tauri,mock}.ts` | Remove now-dead `noteTags()` | 9 |
| `web/e2e/graph.spec.ts` | E2E: default-local, re-root vs open, cap banner, recency, filter | 10 |

---

## Task 0: Re-vendor Track-1 bindings + mock scope (THE UNBLOCK GATE)

**Do not begin until Track 1 is merged upstream.** This task has no TDD red/green for the copy step (generated files); its deliverable is verified by the guard test + a green typecheck.

**Files:**
- Modify (copy): `web/src/contract/{Query.ts,QueryResponse.ts,GraphEdge.ts}` + Create: `web/src/contract/{GraphNode.ts,GraphScope.ts,Graph.ts,SuggestedEdge.ts,Suggestions.ts}` (exact set per generated output)
- Modify: `web/src/client/contractGuards.ts`
- Modify: `web/src/client/mock.ts`
- Test: `web/src/client/contractGuards.test.ts`, `web/src/client/mock.test.ts`

**Interfaces:**
- Produces (consumed everywhere downstream):
  - `GraphScope = { focus: string | null; depth: number }` (confirm optionality vs the generated `?`)
  - `GraphNode = { path: string; title: string; degree: number; tags: string[]; mtime_secs: number }`
  - `GraphEdge = { from: string; to: string }`
  - `QueryResponse` gains `{ type: "graph"; nodes: GraphNode[]; edges: GraphEdge[] }` (replacing `nodes: string[]`)
  - `Query` gains `{ type: "get_graph"; scope: GraphScope }`, `{ type: "get_suggestions" }`, `{ type: "graph_at"; revision: string; scope: GraphScope }`
  - `SuggestedEdge` — **verify exact fields at copy time** (§7 open item; design assumes `{ from, to, score? }`)

- [ ] **Step 1: Copy generated bindings verbatim**

From the engine checkout: copy every changed/new file under `crates/cairn-contract/bindings/` into `web/src/contract/`. Do not reformat (they're in `.prettierignore`). Update `web/src/contract/index.ts` exports to include the new types.

- [ ] **Step 2: Extend the response guard — write the failing test**

In `web/src/client/contractGuards.test.ts`:

```ts
it("accepts a graph response with GraphNode metadata", () => {
  const q = {
    type: "graph",
    nodes: [{ path: "a.md", title: "A", degree: 1, tags: ["x"], mtime_secs: 100 }],
    edges: [{ from: "a.md", to: "b.md" }],
  };
  expect(assertQueryResponse(q)).toBe(q);
});
```

- [ ] **Step 3: Run it — expect FAIL**

Run: `cd web && pnpm vitest run src/client/contractGuards.test.ts`
Expected: FAIL (guard rejects object `nodes` or has no metadata branch).

- [ ] **Step 4: Update the guard**

In `web/src/client/contractGuards.ts`, the `"graph"` case must validate `nodes` as an array of `{ path, title, degree, tags, mtime_secs }` and `edges` as `{ from, to }`. Follow the existing per-variant validation style in that file (match the pattern already used for `search_results`/`notes`).

- [ ] **Step 5: Mock scoped get_graph — write the failing test**

In `web/src/client/mock.test.ts`:

```ts
it("get_graph returns GraphNode metadata and honors scope focus+depth", async () => {
  const c = mockClient({
    "a.md": "# A\n[[b]]",
    "b.md": "# B\n#topic\n[[c]]",
    "c.md": "# C",
  });
  const global = await c.runQuery({ type: "get_graph", scope: { focus: null, depth: 1 } });
  expect(global.type).toBe("graph");
  expect(global.nodes.map((n) => n.path).sort()).toEqual(["a.md", "b.md", "c.md"]);
  expect(global.nodes.find((n) => n.path === "b.md")).toMatchObject({
    title: "B", degree: 2, tags: ["topic"],
  });
  const local = await c.runQuery({ type: "get_graph", scope: { focus: "a.md", depth: 1 } });
  expect(local.nodes.map((n) => n.path).sort()).toEqual(["a.md", "b.md"]); // depth-1 from a
});

it("get_suggestions is empty; graph_at is unsupported", async () => {
  const c = mockClient({ "a.md": "# A" });
  expect(await c.runQuery({ type: "get_suggestions" })).toEqual({ type: "suggestions", suggestions: [] });
  await expect(c.runQuery({ type: "graph_at", revision: "HEAD", scope: { focus: null, depth: 1 } }))
    .rejects.toThrow(/unsupported/i);
});
```

- [ ] **Step 6: Run it — expect FAIL**, then implement the mock

Run: `cd web && pnpm vitest run src/client/mock.test.ts`
Expected: FAIL. Then in `web/src/client/mock.ts`, replace the `case "get_graph"` to: build `GraphNode[]` (degree from undirected edge count, `title` from first `# heading` else stem, `tags` from the existing tag parse, `mtime_secs` deterministic — 0 or a per-note counter), then if `scope.focus` is non-null BFS `scope.depth` hops (undirected) and slice nodes+edges to the reached set. Add `case "get_suggestions"` → `{ type: "suggestions", suggestions: [] }` and `case "graph_at"` → throw an `unsupported` error mirroring the engine's `Unsupported`.

- [ ] **Step 7: Run guard + mock tests — expect PASS**

Run: `cd web && pnpm vitest run src/client/contractGuards.test.ts src/client/mock.test.ts`
Expected: PASS.

- [ ] **Step 8: Typecheck + contract drift**

Run: `cd web && pnpm tsc --noEmit` and the repo's contract-drift check. Expected: PASS (downstream files still reference the OLD shape and will fail to typecheck — that is expected and fixed in Tasks 4–9; if you want a clean gate here, do Task 0 and Tasks 1–9 as one branch and run the gate at the end).

- [ ] **Step 9: Commit**

```bash
git add web/src/contract web/src/client/contractGuards.ts web/src/client/contractGuards.test.ts web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(graph): re-vendor Track-1 seam bindings + mock scoped get_graph"
```

---

## Task 1: `recency.ts` — mtime → ring visual (pure)

**Files:**
- Create: `web/src/components/graph/recency.ts`
- Test: `web/src/components/graph/recency.test.ts`

**Interfaces:**
- Produces:
  - `interface RecencySettings { enabled: boolean; windowDays: number }`
  - `const DEFAULT_RECENCY: RecencySettings` (`{ enabled: false, windowDays: 30 }`)
  - `function recencyRing(mtimeSecs: number, nowSecs: number, windowDays: number): { alpha: number; width: number } | null`
  - `function loadRecency(): RecencySettings` / `function saveRecency(s: RecencySettings): void`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { recencyRing, DEFAULT_RECENCY } from "./recency";

describe("recencyRing", () => {
  const now = 1_000_000; // secs
  const DAY = 86_400;
  it("full ring when edited now", () => {
    expect(recencyRing(now, now, 30)).toEqual({ alpha: 1, width: 4 });
  });
  it("null when older than the window", () => {
    expect(recencyRing(now - 31 * DAY, now, 30)).toBeNull();
  });
  it("ramps down linearly across the window", () => {
    const mid = recencyRing(now - 15 * DAY, now, 30)!;
    expect(mid.alpha).toBeCloseTo(0.5, 1);
    expect(mid.width).toBeGreaterThan(1);
    expect(mid.width).toBeLessThan(4);
  });
  it("future/negative age clamps to full, not >1", () => {
    expect(recencyRing(now + DAY, now, 30)!.alpha).toBe(1);
  });
  it("DEFAULT_RECENCY is off, 30 days", () => {
    expect(DEFAULT_RECENCY).toEqual({ enabled: false, windowDays: 30 });
  });
});
```

- [ ] **Step 2: Run it — expect FAIL**

Run: `cd web && pnpm vitest run src/components/graph/recency.test.ts`
Expected: FAIL ("Cannot find module './recency'").

- [ ] **Step 3: Implement**

```ts
export interface RecencySettings {
  enabled: boolean;
  windowDays: number;
}
export const DEFAULT_RECENCY: RecencySettings = { enabled: false, windowDays: 30 };
export const RECENCY_WINDOW_RANGE = { min: 1, max: 365, step: 1 } as const;

const RING_MAX_WIDTH = 4;
const RING_MIN_WIDTH = 1.5;
const STORAGE_KEY = "cairn.graph.recency";

/** Ring alpha+width from a note's mtime. `t` = fraction of newness in [0,1]
 *  (1 = edited now, 0 = at the window edge). null when older than the window. */
export function recencyRing(
  mtimeSecs: number,
  nowSecs: number,
  windowDays: number,
): { alpha: number; width: number } | null {
  const ageDays = (nowSecs - mtimeSecs) / 86_400;
  if (ageDays > windowDays) return null;
  const t = Math.min(1, Math.max(0, 1 - ageDays / windowDays));
  return { alpha: t, width: RING_MIN_WIDTH + (RING_MAX_WIDTH - RING_MIN_WIDTH) * t };
}

export function loadRecency(): RecencySettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_RECENCY;
    const p = JSON.parse(raw) as Partial<RecencySettings>;
    return {
      enabled: !!p.enabled,
      windowDays:
        typeof p.windowDays === "number"
          ? Math.min(RECENCY_WINDOW_RANGE.max, Math.max(RECENCY_WINDOW_RANGE.min, Math.round(p.windowDays)))
          : DEFAULT_RECENCY.windowDays,
    };
  } catch {
    return DEFAULT_RECENCY;
  }
}

export function saveRecency(s: RecencySettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
```

Note the mid-window test: at 15/30 days, `t = 0.5`, `alpha = 0.5`, `width = 1.5 + 2.5*0.5 = 2.75`. The `width: 4` full case: `1.5 + 2.5*1 = 4`. ✓

- [ ] **Step 4: Run it — expect PASS**

Run: `cd web && pnpm vitest run src/components/graph/recency.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/recency.ts web/src/components/graph/recency.test.ts
git commit -m "feat(graph): recency ring pure helper from mtime_secs"
```

---

## Task 2: `globalCap.ts` — cap global to top-N by degree (pure)

**Files:**
- Create: `web/src/components/graph/globalCap.ts`
- Test: `web/src/components/graph/globalCap.test.ts`

**Interfaces:**
- Consumes: `GraphNode`, `GraphEdge` from `../../contract`.
- Produces:
  - `const GLOBAL_NODE_CAP = 1500`
  - `interface CapResult { nodes: GraphNode[]; edges: GraphEdge[]; total: number; truncated: boolean }`
  - `function capByDegree(nodes: GraphNode[], edges: GraphEdge[], limit?: number): CapResult`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { capByDegree } from "./globalCap";

const n = (path: string, degree: number) => ({ path, title: path, degree, tags: [], mtime_secs: 0 });

describe("capByDegree", () => {
  it("returns everything untruncated under the limit", () => {
    const nodes = [n("a", 1), n("b", 0)];
    const edges = [{ from: "a", to: "b" }];
    const r = capByDegree(nodes, edges, 10);
    expect(r.truncated).toBe(false);
    expect(r.total).toBe(2);
    expect(r.nodes).toHaveLength(2);
  });
  it("keeps the top-N by degree and drops edges to dropped nodes", () => {
    const nodes = [n("hub", 5), n("mid", 2), n("leaf", 0)];
    const edges = [{ from: "hub", to: "mid" }, { from: "mid", to: "leaf" }];
    const r = capByDegree(nodes, edges, 2);
    expect(r.truncated).toBe(true);
    expect(r.total).toBe(3);
    expect(r.nodes.map((x) => x.path).sort()).toEqual(["hub", "mid"]);
    expect(r.edges).toEqual([{ from: "hub", to: "mid" }]); // mid→leaf dropped
  });
});
```

- [ ] **Step 2: Run it — expect FAIL**

Run: `cd web && pnpm vitest run src/components/graph/globalCap.test.ts`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement**

```ts
import type { GraphNode } from "../../contract/GraphNode";
import type { GraphEdge } from "../../contract/GraphEdge";

export const GLOBAL_NODE_CAP = 1500;

export interface CapResult {
  nodes: GraphNode[];
  edges: GraphEdge[];
  total: number;
  truncated: boolean;
}

/** Cap a global graph to the `limit` highest-degree nodes. Ties broken by path
 *  for determinism. Edges with a dropped endpoint are removed. */
export function capByDegree(
  nodes: GraphNode[],
  edges: GraphEdge[],
  limit: number = GLOBAL_NODE_CAP,
): CapResult {
  const total = nodes.length;
  if (total <= limit) return { nodes, edges, total, truncated: false };

  const kept = [...nodes]
    .sort((a, b) => b.degree - a.degree || a.path.localeCompare(b.path))
    .slice(0, limit);
  const keptIds = new Set(kept.map((x) => x.path));
  const keptEdges = edges.filter((e) => keptIds.has(e.from) && keptIds.has(e.to));
  return { nodes: kept, edges: keptEdges, total, truncated: true };
}
```

- [ ] **Step 4: Run it — expect PASS**

Run: `cd web && pnpm vitest run src/components/graph/globalCap.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/globalCap.ts web/src/components/graph/globalCap.test.ts
git commit -m "feat(graph): global overview cap to top-N by degree"
```

---

## Task 3: `matchGroup()` + `graphFilter.ts` (pure)

**Files:**
- Modify: `web/src/components/graph/colorGroups.ts`
- Create: `web/src/components/graph/graphFilter.ts`
- Test: `web/src/components/graph/graphFilter.test.ts` (+ extend `colorGroups.test.ts` if present)

**Interfaces:**
- Produces:
  - `colorGroups.ts`: `function matchGroup(path, tags, groups): ColorGroup | null` (identity, not just color); `matchGroupColor` delegates to it.
  - `graphFilter.ts`:
    - `interface FilterSettings { minDegree: number; hiddenGroupQueries: string[]; hideUngrouped: boolean }`
    - `const DEFAULT_FILTER: FilterSettings` (`{ minDegree: 0, hiddenGroupQueries: [], hideUngrouped: false }`)
    - `function applyFilters(nodes: GraphNode[], edges: GraphEdge[], f: FilterSettings, groups: ColorGroup[]): { nodes: GraphNode[]; edges: GraphEdge[] }`
    - `function loadFilter(): FilterSettings` / `saveFilter(f): void`

- [ ] **Step 1: Add `matchGroup` to colorGroups.ts**

Refactor `matchGroupColor` to delegate:

```ts
export function matchGroup(
  path: string,
  tags: string[],
  groups: ColorGroup[],
): ColorGroup | null {
  const lowerPath = path.toLowerCase();
  const lowerTags = tags.map((t) => t.toLowerCase());
  for (const g of groups) {
    const q = g.query.trim().toLowerCase();
    if (!q) continue;
    if (g.kind === "path" ? lowerPath.includes(q) : lowerTags.includes(q)) return g;
  }
  return null;
}

export function matchGroupColor(path: string, tags: string[], groups: ColorGroup[]): string | null {
  return matchGroup(path, tags, groups)?.color ?? null;
}
```

- [ ] **Step 2: Write the failing filter test**

```ts
import { describe, it, expect } from "vitest";
import { applyFilters, DEFAULT_FILTER } from "./graphFilter";
import type { ColorGroup } from "./colorGroups";

const n = (path: string, degree: number, tags: string[] = []) => ({ path, title: path, degree, tags, mtime_secs: 0 });
const groups: ColorGroup[] = [{ kind: "tag", query: "topic", color: "#6366f1" }];

describe("applyFilters", () => {
  it("passthrough on defaults", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 0)];
    const edges = [{ from: "a", to: "b" }];
    expect(applyFilters(nodes, edges, DEFAULT_FILTER, groups)).toEqual({ nodes, edges });
  });
  it("min-degree hides low-degree nodes and their edges", () => {
    const nodes = [n("a", 2, ["topic"]), n("b", 1)];
    const edges = [{ from: "a", to: "b" }];
    const r = applyFilters(nodes, edges, { ...DEFAULT_FILTER, minDegree: 2 }, groups);
    expect(r.nodes.map((x) => x.path)).toEqual(["a"]);
    expect(r.edges).toEqual([]);
  });
  it("hidden group query hides matching nodes", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 1, ["other"])];
    const r = applyFilters(nodes, [], { ...DEFAULT_FILTER, hiddenGroupQueries: ["topic"] }, groups);
    expect(r.nodes.map((x) => x.path)).toEqual(["b"]);
  });
  it("hideUngrouped hides nodes matching no group", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 1, [])];
    const r = applyFilters(nodes, [], { ...DEFAULT_FILTER, hideUngrouped: true }, groups);
    expect(r.nodes.map((x) => x.path)).toEqual(["a"]);
  });
});
```

- [ ] **Step 3: Run — expect FAIL**

Run: `cd web && pnpm vitest run src/components/graph/graphFilter.test.ts`
Expected: FAIL (module missing).

- [ ] **Step 4: Implement `graphFilter.ts`**

```ts
import type { GraphNode } from "../../contract/GraphNode";
import type { GraphEdge } from "../../contract/GraphEdge";
import { matchGroup, type ColorGroup } from "./colorGroups";

export interface FilterSettings {
  minDegree: number;
  hiddenGroupQueries: string[]; // group.query values whose nodes are hidden
  hideUngrouped: boolean;       // hide nodes that match no group ("other" row)
}
export const DEFAULT_FILTER: FilterSettings = {
  minDegree: 0,
  hiddenGroupQueries: [],
  hideUngrouped: false,
};
const STORAGE_KEY = "cairn.graph.filter";

export function applyFilters(
  nodes: GraphNode[],
  edges: GraphEdge[],
  f: FilterSettings,
  groups: ColorGroup[],
): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const hidden = new Set(f.hiddenGroupQueries.map((q) => q.trim().toLowerCase()));
  const visible = nodes.filter((node) => {
    if (node.degree < f.minDegree) return false;
    const g = matchGroup(node.path, node.tags, groups);
    if (g) return !hidden.has(g.query.trim().toLowerCase());
    return !f.hideUngrouped;
  });
  const ids = new Set(visible.map((n) => n.path));
  return { nodes: visible, edges: edges.filter((e) => ids.has(e.from) && ids.has(e.to)) };
}

export function loadFilter(): FilterSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_FILTER;
    const p = JSON.parse(raw) as Partial<FilterSettings>;
    return {
      minDegree: typeof p.minDegree === "number" ? Math.max(0, Math.round(p.minDegree)) : 0,
      hiddenGroupQueries: Array.isArray(p.hiddenGroupQueries) ? p.hiddenGroupQueries.filter((x) => typeof x === "string") : [],
      hideUngrouped: !!p.hideUngrouped,
    };
  } catch {
    return DEFAULT_FILTER;
  }
}
export function saveFilter(f: FilterSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(f));
  } catch {
    // ignore
  }
}
```

- [ ] **Step 5: Run filter + colorGroups tests — expect PASS**, then commit

Run: `cd web && pnpm vitest run src/components/graph/graphFilter.test.ts src/components/graph/colorGroups.test.ts`
Expected: PASS.

```bash
git add web/src/components/graph/colorGroups.ts web/src/components/graph/graphFilter.ts web/src/components/graph/graphFilter.test.ts
git commit -m "feat(graph): matchGroup identity + min-degree/tag/ungrouped filter"
```

---

## Task 4: `graphData.ts` consumes `GraphNode[]` + `GLink.kind` merge

**Files:**
- Modify: `web/src/components/graph/graphData.ts`
- Modify: `web/src/components/graph/forceGraphTypes.ts`
- Test: `web/src/components/graph/graphData.test.ts`

**Interfaces:**
- Consumes: `GraphNode`, `GraphEdge`, `SuggestedEdge` from contract.
- Produces:
  - `interface GNode { id: string; label: string; degree: number; tags: string[]; mtimeSecs: number }`
  - `interface GLink { source: string; target: string; kind: "explicit" | "suggested"; weight?: number }`
  - `function buildGraphData(nodes: GraphNode[], edges: GraphEdge[], suggested?: SuggestedEdge[]): { nodes: GNode[]; links: GLink[] }`
  - `nodeRadius`, `labelAlpha`, `buildAdjacency` unchanged (adjacency built from explicit links only).
- `RFNode` (forceGraphTypes.ts) gains `tags: string[]; mtimeSecs: number`.

- [ ] **Step 1: Write the failing test** (rewrite `graphData.test.ts`)

```ts
import { describe, it, expect } from "vitest";
import { buildGraphData, buildAdjacency } from "./graphData";

const gn = (path: string, degree: number, title = path, tags: string[] = [], mtime = 0) =>
  ({ path, title, degree, tags, mtime_secs: mtime });

describe("buildGraphData", () => {
  it("uses server degree/title/tags/mtime and marks explicit links", () => {
    const nodes = [gn("a.md", 1, "Alpha", ["x"], 50), gn("b.md", 1, "Beta")];
    const { nodes: gnodes, links } = buildGraphData(nodes, [{ from: "a.md", to: "b.md" }]);
    expect(gnodes[0]).toMatchObject({ id: "a.md", label: "Alpha", degree: 1, tags: ["x"], mtimeSecs: 50 });
    expect(links).toEqual([{ source: "a.md", target: "b.md", kind: "explicit" }]);
  });
  it("merges suggested edges as kind:suggested with weight, dropping out-of-scope endpoints", () => {
    const nodes = [gn("a.md", 1), gn("b.md", 1)];
    const suggested = [
      { from: "a.md", to: "b.md", score: 0.8 },
      { from: "a.md", to: "zzz.md", score: 0.9 }, // endpoint not in node set → dropped
    ];
    const { links } = buildGraphData(nodes, [], suggested);
    expect(links).toEqual([{ source: "a.md", target: "b.md", kind: "suggested", weight: 0.8 }]);
  });
  it("drops explicit edges with a missing endpoint", () => {
    const { links } = buildGraphData([gn("a.md", 0)], [{ from: "a.md", to: "gone.md" }]);
    expect(links).toEqual([]);
  });
  it("adjacency ignores suggested edges", () => {
    const nodes = [gn("a.md", 1), gn("b.md", 1)];
    const { links } = buildGraphData(nodes, [{ from: "a.md", to: "b.md" }], [{ from: "a.md", to: "b.md", score: 1 }]);
    const adj = buildAdjacency(links);
    expect(adj.get("a.md")).toEqual(new Set(["b.md"])); // one, not doubled by the suggestion
  });
});
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cd web && pnpm vitest run src/components/graph/graphData.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement** (`graphData.ts`)

```ts
import type { GraphNode } from "../../contract/GraphNode";
import type { GraphEdge } from "../../contract/GraphEdge";
import type { SuggestedEdge } from "../../contract/SuggestedEdge";
import { stem } from "../../client/wikilink";

export interface GNode {
  id: string;
  label: string;
  degree: number;
  tags: string[];
  mtimeSecs: number;
}
export interface GLink {
  source: string;
  target: string;
  kind: "explicit" | "suggested";
  weight?: number;
}

/** Build force-graph data from server GraphNodes. Explicit links come from
 *  `edges`; `suggested` (optional, off by default upstream) are merged as
 *  kind:"suggested". Both link sets are filtered to endpoints present in `nodes`. */
export function buildGraphData(
  nodes: GraphNode[],
  edges: GraphEdge[],
  suggested: SuggestedEdge[] = [],
): { nodes: GNode[]; links: GLink[] } {
  const ids = new Set(nodes.map((n) => n.path));
  const explicit: GLink[] = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => ({ source: e.from, target: e.to, kind: "explicit" as const }));
  const sugg: GLink[] = suggested
    .filter((s) => ids.has(s.from) && ids.has(s.to))
    .map((s) => ({ source: s.from, target: s.to, kind: "suggested" as const, weight: s.score }));

  const gnodes: GNode[] = nodes.map((n) => ({
    id: n.path,
    label: n.title || stem(n.path),
    degree: n.degree,
    tags: n.tags,
    mtimeSecs: n.mtime_secs,
  }));
  return { nodes: gnodes, links: [...explicit, ...sugg] };
}

/** Symmetric adjacency for hover-neighbor highlighting — EXPLICIT links only
 *  (suggested edges must not change the structural neighborhood). Pass the
 *  original string-keyed links (react-force-graph mutates source/target). */
export function buildAdjacency(links: GLink[]): Map<string, Set<string>> {
  const adj = new Map<string, Set<string>>();
  const add = (a: string, b: string) => {
    if (!adj.has(a)) adj.set(a, new Set());
    adj.get(a)!.add(b);
  };
  for (const l of links) {
    if (l.kind !== "explicit") continue;
    add(l.source, l.target);
    add(l.target, l.source);
  }
  return adj;
}

/** Node radius from degree — sublinear so hubs are bigger but not huge. */
export function nodeRadius(degree: number): number {
  return 3 + 1.6 * Math.sqrt(degree);
}

/** Label opacity from the current zoom scale. */
export function labelAlpha(zoom: number): number {
  const lo = 1.2;
  const hi = 2.5;
  if (zoom <= lo) return 0;
  if (zoom >= hi) return 1;
  return (zoom - lo) / (hi - lo);
}
```

Note: `weight` is `s.score` — if the vendored `SuggestedEdge` has no `score` field (§7 open item), TypeScript will flag this line; adjust the field name to match the generated type and update the test's `score` accordingly. Do not invent a field the contract lacks.

- [ ] **Step 4: Update `RFNode`** in `forceGraphTypes.ts` — add `tags: string[]; mtimeSecs: number;` after `degree`.

- [ ] **Step 5: Run — expect PASS**

Run: `cd web && pnpm vitest run src/components/graph/graphData.test.ts && cd web && pnpm tsc --noEmit`
Expected: `graphData` tests PASS (tsc will still show downstream errors until Tasks 5–9).

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/graphData.ts web/src/components/graph/graphData.test.ts web/src/components/graph/forceGraphTypes.ts
git commit -m "feat(graph): graphData consumes GraphNode + explicit/suggested GLink merge"
```

---

## Task 5: Store — scoped/cached `loadGraph`, `graphFocus`, sync rule

**Files:**
- Modify: `web/src/store/store.ts` (state ~L142-143, ~L451-452, ~L512-513; `loadGraph` L1001-1025; cascade L411; the open-note action that sets `activePath`)
- Test: `web/src/store/store.test.ts`

**Interfaces:**
- Consumes: `Graph`, `GraphScope`, `GraphNode` (contract); `buildGraphData` not used here (store holds raw `Graph`).
- Produces (state + actions used by EditorPane/GraphView):
  - state: `graph: Graph | null`, `graphScope: GraphScope | null`, `graphFocus: string | null`, `graphRevision: string | null`, `showSuggestions: boolean`, plus a non-persisted `graphCache: Map<string, Graph>`
  - `loadGraph(opts?: { focus?: string | null; depth?: number; revision?: string | null }): Promise<void>`
  - `setGraphFocus(path: string | null): void` — plain-click re-root (Fork 5); reloads local scope
  - removes `noteTags` state
- **Sync rule:** the open-note action sets `graphFocus = path` alongside `activePath`.

- [ ] **Step 1: Write failing store tests**

```ts
// scope routing: local default uses activePath; explicit null → global
it("loadGraph local scope queries GetGraph with focus=activePath, depth 1", async () => {
  const calls: any[] = [];
  const client = makeMockClient({ onQuery: (q) => calls.push(q) /* returns graph */ });
  const store = makeStore(client);
  await store.getState().openNote("a.md");           // sets activePath + graphFocus
  await store.getState().loadGraph();                 // no opts → local
  expect(calls).toContainEqual({ type: "get_graph", scope: { focus: "a.md", depth: 1 } });
});

it("loadGraph({focus:null}) queries the global overview", async () => {
  // ...expect scope.focus === null
});

it("caches by focus@depth and does not refetch on revisit", async () => {
  // load focus a, load focus b, load focus a again → only 2 get_graph calls
});

it("setGraphFocus re-roots without changing activePath", async () => {
  const store = makeStore(client);
  await store.getState().openNote("a.md");
  await store.getState().setGraphFocus("b.md");
  expect(store.getState().activePath).toBe("a.md");
  expect(store.getState().graphFocus).toBe("b.md");
});

it("does not call get_suggestions unless showSuggestions is true", async () => {
  // default false → no get_suggestions in calls
});

it("keystroke cascade reloads the CURRENT scope, not global", async () => {
  // open a note (local scope), emit note_changed (external), assert reload used focus=a not null
});
```

Use the existing `store.test.ts` mock-client harness (see how it stubs `runQuery`) — mirror its style rather than inventing a new one.

- [ ] **Step 2: Run — expect FAIL**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement state changes**

- Replace state decl (`store.ts:142-143`):
  ```ts
  graph: Graph | null;
  graphScope: GraphScope | null;
  graphFocus: string | null;
  graphRevision: string | null;
  showSuggestions: boolean;
  // NOTE: graphCache is a Map kept OUTSIDE the persisted/serialized state
  ```
  Remove the `noteTags` line. Add a module-scoped `const graphCache = new Map<string, Graph>();` near `seq` (L235), reset in `loadCairn`.
- Update the two reset blocks (`~L451-452`, `~L512-513`): drop `noteTags: {}`, add `graph: null, graphScope: null, graphFocus: null, graphRevision: null, showSuggestions: false`, and `graphCache.clear()`.

- [ ] **Step 4: Rewrite `loadGraph`** (replace L1001-1025):

```ts
async loadGraph(opts) {
  const s = get();
  // Resolve scope: explicit opts win; else local view keyed on graphFocus ?? activePath.
  const focus =
    opts && "focus" in opts ? opts.focus ?? null : s.graphFocus ?? s.activePath;
  const depth = opts?.depth ?? s.graphScope?.depth ?? 1;
  const revision = opts && "revision" in opts ? opts.revision ?? null : s.graphRevision;
  const scope: GraphScope = { focus, depth };
  const key = `${focus ?? "*"}@${depth}@${revision ?? "live"}`;

  const token = ++seq.graph;
  setLoading("graph", true);
  try {
    let graph = graphCache.get(key) ?? null;
    if (!graph) {
      const res = revision
        ? await client.runQuery({ type: "graph_at", revision, scope })
        : await client.runQuery({ type: "get_graph", scope });
      if (token !== seq.graph) return;
      if (res.type !== "graph") { unexpected("Load graph", res); return; }
      graph = { nodes: res.nodes, edges: res.edges };
      graphCache.set(key, graph);
    }
    // Suggested edges: opt-in, merged into store.graph so GraphView renders one set.
    let suggestions: SuggestedEdge[] = [];
    if (s.showSuggestions) {
      try {
        const sres = await client.runQuery({ type: "get_suggestions" });
        if (token !== seq.graph) return;
        if (sres.type === "suggestions") suggestions = sres.suggestions;
      } catch { /* suggestions are best-effort */ }
    }
    if (token !== seq.graph) return;
    set({
      graph,
      graphScope: scope,
      graphRevision: revision,
      // stash merged suggestions on state for GraphView (or pass separately — see Task 6)
      graphSuggestions: suggestions,
    });
  } catch (err) {
    if (token !== seq.graph) return;
    pushError("Load graph", err);
  } finally {
    if (token === seq.graph) setLoading("graph", false);
  }
},

setGraphFocus(path) {
  set({ graphFocus: path });
  void get().loadGraph({ focus: path });
},
```

Add `graphSuggestions: SuggestedEdge[]` to state (init `[]`) OR pass suggestions via a separate selector — pick one and keep Task 6 consistent. (Recommended: a `graphSuggestions` state field, reset with the graph.)

- [ ] **Step 5: Apply the sync rule + cascade fix**

- In the open-note action (the one at `store.ts` ~L736 that sets `activePath` and calls `refreshBacklinks`), also set `graphFocus: path`; and if `get().graph !== null` call `void get().loadGraph({ focus: path })`.
- Cascade (`L411`): `if (get().graph !== null) fire("loadGraph", () => get().loadGraph());` — with the new resolver, no-arg `loadGraph()` already reuses the current scope. Confirm it does NOT force global. Add a store test asserting the reload scope equals the pre-change scope (the cascade test in Step 1).

- [ ] **Step 6: Run — expect PASS**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(graph): scoped/cached loadGraph, graphFocus + sync rule, drop noteTags"
```

---

## Task 6: GraphView — local default, modifier-split, recency ring, link dash, cap banner

**Files:**
- Modify: `web/src/components/GraphView.tsx`
- Test: covered by e2e (Task 10); add a focused component render test only if the existing suite has a GraphView test harness.

**Interfaces:**
- New props (from EditorPane, Task 8):
  ```ts
  {
    graph: Graph | null;
    suggestions: SuggestedEdge[];      // already gated by store.showSuggestions
    scope: GraphScope | null;          // { focus, depth }
    activePath: string | null;
    recency: RecencySettings;
    filter: FilterSettings;
    groups: ColorGroup[];
    loading?: boolean;
    onOpenNote: (path: string) => void;      // ⌘/double-click
    onRefocus: (path: string) => void;       // plain click (setGraphFocus)
    onSetScope: (focus: string | null) => void; // local/global toggle → loadGraph
  }
  ```

- [ ] **Step 1: Replace data derivation.** Drop `localSubgraph`/`loadLocalGraph` imports and the `props.nodes/edges/tagsByNote` inputs. Build render data from `props.graph`:

```ts
const capped = useMemo(() => {
  if (!props.graph) return { nodes: [], edges: [], truncated: false, total: 0 };
  // Global view (scope.focus == null) gets the perf cap; local views are already bounded.
  return props.scope?.focus == null
    ? capByDegree(props.graph.nodes, props.graph.edges)
    : { nodes: props.graph.nodes, edges: props.graph.edges, truncated: false, total: props.graph.nodes.length };
}, [props.graph, props.scope]);

const filtered = useMemo(
  () => applyFilters(capped.nodes, capped.edges, props.filter, props.groups),
  [capped, props.filter, props.groups],
);
const data = useMemo(
  () => buildGraphData(filtered.nodes, filtered.edges, props.suggestions),
  [filtered, props.suggestions],
);
const adjacency = useMemo(() => buildAdjacency(data.links), [data]);
```

- [ ] **Step 2: Local/global toggle drives scope.** The segmented control's selected state comes from `props.scope?.focus != null` (local) vs `== null` (global). `onClick` calls `props.onSetScope(local ? props.activePath : null)`. Default-local + no-focus→global is enforced by the store/EditorPane (Task 8) passing `scope.focus = activePath` initially and falling back to global when `activePath == null`.

- [ ] **Step 3: Modifier-split click (Fork 5).**

```ts
onNodeClick={(n: RFNode, ev: MouseEvent) => {
  if (ev.metaKey || ev.ctrlKey) props.onOpenNote(n.id);
  else props.onRefocus(n.id);
}}
onNodeRightClick={undefined}
// double-click also opens:
onNodeDoubleClick={(n: RFNode) => props.onOpenNote(n.id)}
```

- [ ] **Step 4: Recency ring in `paintNode`.** After filling the node, when `props.recency.enabled`, compute the ring and stroke it (uses `Date.now()/1000` for `nowSecs`):

```ts
if (props.recency.enabled) {
  const ring = recencyRing(node.mtimeSecs, Date.now() / 1000, props.recency.windowDays);
  if (ring) {
    ctx.beginPath();
    ctx.arc(node.x ?? 0, node.y ?? 0, r + 2, 0, 2 * Math.PI);
    ctx.strokeStyle = `rgba(129,140,248,${ring.alpha})`; // indigo accent
    ctx.lineWidth = ring.width / scale;
    ctx.stroke();
  }
}
```

- [ ] **Step 5: Link dash by kind (Fork 7).** Add `linkLineDash` and taper suggested links:

```ts
linkLineDash={(l: GLink) => (l.kind === "suggested" ? [5, 4] : null)}
linkColor={(l: GLink) => (l.kind === "suggested" ? "#818cf8aa" : hoverAwareExplicitColor(l))}
linkWidth={(l: GLink) => (l.kind === "suggested" ? 1 + (l.weight ?? 0) : 1)}
```

(Keep the existing hover-highlight logic for explicit links.)

- [ ] **Step 6: Cap banner (Fork 3).** When `capped.truncated`, render an absolutely-positioned banner:

```tsx
{capped.truncated && (
  <div className="absolute bottom-2 left-1/2 z-10 -translate-x-1/2 rounded-md border border-border bg-surface px-3 py-1 text-[11px] text-muted">
    Showing {capped.nodes.length.toLocaleString()} most-connected of {capped.total.toLocaleString()} notes — open a note for its full local graph
  </div>
)}
```

- [ ] **Step 7: Temporal scrubber stub (Fork 7, gated).** Add a hidden-by-default bottom bar rendered only when a `graphAtSupported` flag is true (default false — no engine support yet). Leave a `// TODO(track-3): enable when GraphAt lands` marker. Do not wire it to any query.

- [ ] **Step 8: Typecheck + run the suite**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run`
Expected: PASS (all unit tests; GraphView is exercised via e2e in Task 10).

- [ ] **Step 9: Commit**

```bash
git add web/src/components/GraphView.tsx
git commit -m "feat(graph): local-default view, modifier-split, recency ring, dashed suggestions, cap banner"
```

---

## Task 7: GraphGroupsPanel — legend + filter controls

**Files:**
- Modify: `web/src/components/graph/GraphGroupsPanel.tsx`
- Test: extend/add `web/src/components/graph/GraphGroupsPanel.test.tsx` if a component-test setup exists; otherwise cover via e2e (Task 10).

**Interfaces:**
- New props:
  ```ts
  {
    groups: ColorGroup[]; onChange: (g: ColorGroup[]) => void;
    filter: FilterSettings; onFilterChange: (f: FilterSettings) => void;
    recency: RecencySettings; onRecencyChange: (r: RecencySettings) => void;
    showSuggestions: boolean; onToggleSuggestions: (b: boolean) => void;
    suggestionsAvailable: boolean; // false until Track 2 → toggle shows "none yet"
  }
  ```

- [ ] **Step 1: Per-group eye toggle.** For each group row, add a button toggling membership of `group.query` in `filter.hiddenGroupQueries`. Show 👁 (visible) / 🚫 (hidden); apply `off` styling to hidden rows.

- [ ] **Step 2: "Other / untagged" row.** A fixed row with a neutral swatch + eye toggle bound to `filter.hideUngrouped`.

- [ ] **Step 3: Min-degree slider.** `min=0 max=10 step=1`, bound to `filter.minDegree`, labeled "Min degree" with the current value; `onFilterChange`.

- [ ] **Step 4: Recency toggle + window slider.** A checkbox bound to `recency.enabled`; when on, show a window slider (`RECENCY_WINDOW_RANGE`) bound to `recency.windowDays`.

- [ ] **Step 5: Suggestions toggle.** A checkbox bound to `showSuggestions`; when `!suggestionsAvailable`, disable it and show "none yet" helper text. `onToggleSuggestions`.

- [ ] **Step 6: Typecheck**

Run: `cd web && pnpm tsc --noEmit`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/graph/GraphGroupsPanel.tsx
git commit -m "feat(graph): panel becomes legend+filter (eye toggles, min-degree, recency, suggestions)"
```

---

## Task 8: EditorPane wiring

**Files:**
- Modify: `web/src/components/EditorPane.tsx` (L95-96 selectors; L157-163 `<GraphView>` props)

**Interfaces:**
- Consumes store: `graph`, `graphSuggestions`, `graphScope`, `activePath`, `showSuggestions`, actions `loadGraph`, `setGraphFocus`, plus panel state (`groups`, `filter`, `recency` — these live in GraphView today via `useState`; keep them there unless the store already owns them). Uses `actions.openNote` for ⌘-click.

- [ ] **Step 1: Swap selectors.** Remove `const noteTags = useCairn(s => s.noteTags)`. Add `const graph = useCairn(s => s.graph)`, `const suggestions = useCairn(s => s.graphSuggestions)`, `const scope = useCairn(s => s.graphScope)`.

- [ ] **Step 2: Default-local + no-focus→global on entering graph view.** In the effect that runs when `view === "graph"`, call `actions.loadGraph({ focus: activePath })` (activePath null → store resolves focus null → global). This enforces Fork 1.

- [ ] **Step 3: New `<GraphView>` props.**

```tsx
<GraphView
  graph={graph}
  suggestions={suggestions}
  scope={scope}
  activePath={activePath}
  loading={loading.graph}
  onOpenNote={(path) => actions.openNote(path)}
  onRefocus={(path) => actions.setGraphFocus(path)}
  onSetScope={(focus) => actions.loadGraph({ focus })}
/>
```

- [ ] **Step 4: Typecheck + full unit suite**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/EditorPane.tsx
git commit -m "feat(graph): wire EditorPane to scoped graph store + focus callbacks"
```

---

## Task 9: Delete client BFS + dead `noteTags`

**Files:**
- Delete: `web/src/components/graph/localGraph.ts`, `web/src/components/graph/localGraph.test.ts`
- Modify: `web/src/client/types.ts` (remove `noteTags` from the `Client` interface), `web/src/client/daemon.ts`, `web/src/client/tauri.ts`, `web/src/client/mock.ts` (remove `noteTags()` impls), and their tests (`daemon.test.ts:138-149`, `mock.test.ts:137-142`).

- [ ] **Step 1: Confirm no remaining importers**

Run: `cd web && grep -rn "localGraph\|\.noteTags(" src/` — expect only the files listed above (post-Task-5 the store no longer uses either).

- [ ] **Step 2: Delete files + remove `noteTags` from the Client surface.** Remove the interface method and all three implementations + the two client tests that assert `noteTags`.

- [ ] **Step 3: Typecheck + full suite**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run`
Expected: PASS (no dangling references).

- [ ] **Step 4: Commit**

```bash
git rm web/src/components/graph/localGraph.ts web/src/components/graph/localGraph.test.ts
git add -A web/src/client
git commit -m "refactor(graph): remove client BFS and orphaned noteTags client method"
```

---

## Task 10: Playwright e2e

**Files:**
- Modify/Create: `web/e2e/graph.spec.ts`

**Interfaces:** Uses the existing e2e harness (mock client seeded via the app's test setup — mirror an existing `web/e2e/*.spec.ts`). Prefer stable selectors: `aria-pressed` on the local/global buttons, `getByText` for the banner, `aria-label` on panel controls.

- [ ] **Step 1: Default-local test.** Open the app, open a note, navigate to `?view=graph`; assert the `local` toggle is `aria-pressed="true"` by default.

- [ ] **Step 2: No-focus→global.** From a clean state with no active note, open graph view; assert `global` is selected and the canvas renders (no "open a note" dead-end).

- [ ] **Step 3: Plain-click re-root vs ⌘-click open.** Seed a small graph; plain-click a neighbor node → assert the editor's active note is unchanged (URL/active tab) and the graph re-rooted (focused node marker moved). ⌘-click a node → assert the note opened.

- [ ] **Step 4: Cap banner.** Seed a vault above the cap (or set a low test cap via a query param/env if the harness supports it) → assert the "Showing … most-connected of …" banner text is visible in global view.

- [ ] **Step 5: Recency + filter.** Toggle the recency checkbox → assert it persists (reload). Hide a tag group via its eye toggle → assert those nodes disappear (node count drops).

- [ ] **Step 6: Run e2e**

Run: `cd web && pnpm playwright test e2e/graph.spec.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add web/e2e/graph.spec.ts
git commit -m "test(graph): e2e for default-local, re-root/open, cap banner, recency, filter"
```

---

## Task 11: Fork-7 seam verification (inert)

**Files:**
- Test only: `web/src/store/store.test.ts` (extend)

- [ ] **Step 1: Assert inert-by-default.** Add store tests: with `showSuggestions` false (default), `loadGraph` issues NO `get_suggestions` call and `graphSuggestions` stays `[]`. Flip `showSuggestions` true and assert `get_suggestions` is called and the (empty, mocked) result merges cleanly.

- [ ] **Step 2: Assert scrubber gated.** In GraphView (or via e2e), assert the temporal scrubber is not rendered while `graphAtSupported` is false.

- [ ] **Step 3: Run + commit**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: PASS.

```bash
git add web/src/store/store.test.ts
git commit -m "test(graph): Fork-7 seams inert until Tracks 2/3 land"
```

---

## Final gate

- [ ] Run the full repo gate before opening a PR: `just web-check` (or the documented equivalent — must include `tsc --noEmit`, `vitest run`, `eslint`, `prettier --check`, and the contract-drift check). See the `ci-local-gates` memory: `prettier --check` is easy to miss and eslint won't catch it.
- [ ] Run mutation testing on the new pure helpers: `cd web && pnpm stryker run` (scoped to `graph/*.ts` if the config supports it) — the recency/cap/filter/graphData logic should score high.
- [ ] Open the PR with `gh pr create --base main`. Do not merge directly (branch protection + merge queue; see memory `merge-queue`).

## Self-review notes (coverage map)

- Fork 2 → Tasks 4, 5, 9 (server scope, cache, BFS deletion). Fork 1 → Tasks 5, 8 (default local, no-focus→global). Fork 5 → Tasks 5, 6, 8 (graphFocus, modifier-split, sync rule). Fork 3 → Tasks 2, 6 (cap + banner). Fork 6 → Tasks 1, 6, 7 (ring helper, paint, toggle). Fork 4 → Tasks 3, 7 (filter logic + panel). Fork 7 → Tasks 4, 5, 6, 7, 11 (merge shape, gated queries, dashed render, toggle, inert tests). Contract → Task 0.
- Open items carried from spec §7: exact `SuggestedEdge` fields (Task 0/4 — adjust `weight: s.score`), out-of-scope endpoint drop (Task 4 test covers it).
