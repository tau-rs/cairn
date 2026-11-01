# Temporal Graph-Viz Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user scrub the knowledge graph through git history — snapshot the graph at a past revision, or compare two revisions with appeared/disappeared styling.

**Architecture:** Temporal is a *data-source swap* over the existing `GraphView` pipeline. Every query uses `scope:{type:"full"}`; client-side `localSubgraph` still handles local mode. Temporal **data** lives in a store slice (`loadTimeline`/`loadSnapshot`/`loadDiff`/`clearTemporal`, like `loadGraph`); temporal **controls** (scrubber selection) live in view state (like `forceSettings`). The scrubber's ticks come from `note_history(activePath)` — so temporal is gated on an open note.

**Tech Stack:** React 19 + TypeScript, Zustand (`src/app/cairnStore`), `react-force-graph-2d`, Vitest + Testing Library. Package manager: pnpm (run all commands from `web/`).

## Global Constraints

- Engine `36855f5` contract, already vendored in `web/src/contract` (do not re-vendor).
- `GraphNode = { path: string; title: string; mtime_secs: bigint }` — **no** `degree`/`tags`; degree is computed client-side from edges.
- Every temporal query passes `scope: { type: "full" }`. Spatial (local/global) stays client-side.
- **Never** emit or fake a `"changed"` diff state — the engine reports no `nodes_changed`; `mtime_secs` is dual-basis and must not be diffed. `"changed"` is a reserved-but-dormant enum value only.
- `mtime_secs` is display-only; nothing reads it this plan. The mock seeds `0n`.
- Vendored contract files under `web/src/contract` are prettier-ignored and byte-generated — never hand-edit them.
- Full local gate before any "done" claim (run from `web/`): `npx tsc --noEmit`, `npx eslint .`, `npx prettier --check "src/**/*.{ts,tsx}"`, `npx vitest run`. All must be green.
- Conventional-commit messages; end each with the `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.

## Scope note / known limitation

**Snapshot** honors local mode (subsets the historical graph exactly like live). **Compare** renders **global-only** — the local toggle is ignored while comparing, because subsetting `graph_diff` ghosts (removed nodes absent from the base topology) into a local neighborhood is genuinely ambiguous and out of scope here. The compare UI disables the local toggle with a tooltip. This is the one intentional deviation from spec §6's "any source" line; everything else honors the spec.

**Scrubber interaction:** the spec describes "drag a range." This plan implements the range with a **two-click** model (click a tick = snapshot; click a second tick while a snapshot is active = compare range), which carries identical semantics, is far simpler to test deterministically in jsdom, and keeps the scrubber a pure presentational component. A drag affordance can be layered on later without changing `selectionToRequest` or any store/render code.

## File Structure

- **Create** `web/src/components/graph/temporalControls.ts` — `TemporalSelection`/`TemporalRequest` types, the pure `selectionToRequest` mapper, and `open`-flag persistence. One responsibility: translate scrubber intent into a query request.
- **Create** `web/src/components/graph/temporalControls.test.ts`
- **Modify** `web/src/components/graph/graphData.ts` — add `GraphState` + optional `state` on `GNode`/`GLink`; add pure `buildCompareGraphData`.
- **Modify** `web/src/components/graph/graphData.test.ts`
- **Modify** `web/src/client/mock.ts` — extract `buildGraph` helper; add `graph_at` + `graph_diff` cases backed by a synthetic vault-snapshot fixture.
- **Modify** `web/src/client/mock.test.ts`
- **Modify** `web/src/store/store.ts` — `temporal` slice (state + 4 actions), seq counters, `openCairn` reset.
- **Modify** `web/src/store/store.test.ts`
- **Create** `web/src/components/graph/TemporalScrubber.tsx` — presentational scrubber (ticks + handle(s)).
- **Create** `web/src/components/graph/TemporalScrubber.test.tsx`
- **Create** `web/src/components/graph/useTemporalGraph.ts` — hook: owns selection, runs load effects, returns `{ timeline, selection, setSelection, disabled, source }`.
- **Create** `web/src/components/graph/useTemporalGraph.test.tsx`
- **Modify** `web/src/components/GraphView.tsx` — mount scrubber + toggle, pick the data source, apply diff styling in the paint callbacks.
- **Modify** `web/src/components/GraphView.test.tsx`

---

### Task 1: `selectionToRequest` mapper + control persistence

**Files:**
- Create: `web/src/components/graph/temporalControls.ts`
- Test: `web/src/components/graph/temporalControls.test.ts`

**Interfaces:**
- Consumes: `Revision` from `../../contract`.
- Produces:
  - `type TemporalSelection = { kind: "live" } | { kind: "snapshot"; at: number } | { kind: "compare"; from: number; to: number }`
  - `type TemporalRequest = { mode: "live" } | { mode: "snapshot"; revision: string } | { mode: "compare"; from: string; to: string }`
  - `selectionToRequest(sel: TemporalSelection, timeline: Revision[] | null): TemporalRequest`
  - `loadTemporalOpen(): boolean` / `saveTemporalOpen(open: boolean): void`
  - Indices index into `timeline` **as stored (newest-first)**. In `compare`, `from` is the older (higher index), `to` the newer (lower index).

- [ ] **Step 1: Write the failing test**

```ts
// web/src/components/graph/temporalControls.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import {
  selectionToRequest,
  loadTemporalOpen,
  saveTemporalOpen,
} from "./temporalControls";
import type { Revision } from "../../contract";

const tl: Revision[] = [
  { id: "c3", message: "third", timestamp_secs: 30n, author: "a" }, // newest
  { id: "c2", message: "second", timestamp_secs: 20n, author: "a" },
  { id: "c1", message: "first", timestamp_secs: 10n, author: "a" }, // oldest
];

describe("selectionToRequest", () => {
  it("maps live to a live request", () => {
    expect(selectionToRequest({ kind: "live" }, tl)).toEqual({ mode: "live" });
  });

  it("maps snapshot to graph_at at that revision id", () => {
    expect(selectionToRequest({ kind: "snapshot", at: 1 }, tl)).toEqual({
      mode: "snapshot",
      revision: "c2",
    });
  });

  it("maps a range to compare with older=from, newer=to", () => {
    expect(
      selectionToRequest({ kind: "compare", from: 2, to: 0 }, tl),
    ).toEqual({ mode: "compare", from: "c1", to: "c3" });
  });

  it("degrades a collapsed range to snapshot", () => {
    expect(
      selectionToRequest({ kind: "compare", from: 1, to: 1 }, tl),
    ).toEqual({ mode: "snapshot", revision: "c2" });
  });

  it("falls back to live on empty timeline or out-of-range index", () => {
    expect(selectionToRequest({ kind: "snapshot", at: 0 }, null)).toEqual({
      mode: "live",
    });
    expect(selectionToRequest({ kind: "snapshot", at: 9 }, tl)).toEqual({
      mode: "live",
    });
    expect(
      selectionToRequest({ kind: "compare", from: 9, to: 0 }, tl),
    ).toEqual({ mode: "live" });
  });
});

describe("open persistence", () => {
  beforeEach(() => localStorage.clear());
  it("defaults to false and round-trips", () => {
    expect(loadTemporalOpen()).toBe(false);
    saveTemporalOpen(true);
    expect(loadTemporalOpen()).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/graph/temporalControls.test.ts`
Expected: FAIL — `Cannot find module './temporalControls'`.

- [ ] **Step 3: Write the implementation**

```ts
// web/src/components/graph/temporalControls.ts
import type { Revision } from "../../contract";

export type TemporalSelection =
  | { kind: "live" }
  | { kind: "snapshot"; at: number }
  | { kind: "compare"; from: number; to: number };

export type TemporalRequest =
  | { mode: "live" }
  | { mode: "snapshot"; revision: string }
  | { mode: "compare"; from: string; to: string };

const STORAGE_KEY = "cairn.graph.temporal";

/** Translate a scrubber selection into the temporal request to run. Indices
 *  index into `timeline` as stored (newest-first). A collapsed range degrades
 *  to snapshot; an empty timeline or any out-of-range index falls back to live
 *  (the safe default when the scrubber and data are momentarily out of sync). */
export function selectionToRequest(
  sel: TemporalSelection,
  timeline: Revision[] | null,
): TemporalRequest {
  const at = (i: number): string | null =>
    timeline && i >= 0 && i < timeline.length ? timeline[i].id : null;

  if (sel.kind === "live") return { mode: "live" };
  if (sel.kind === "snapshot") {
    const rev = at(sel.at);
    return rev ? { mode: "snapshot", revision: rev } : { mode: "live" };
  }
  const from = at(sel.from);
  const to = at(sel.to);
  if (!from || !to) return { mode: "live" };
  if (from === to) return { mode: "snapshot", revision: to };
  return { mode: "compare", from, to };
}

export function loadTemporalOpen(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveTemporalOpen(open: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, open ? "1" : "0");
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/graph/temporalControls.test.ts`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/temporalControls.ts web/src/components/graph/temporalControls.test.ts
git commit -m "feat(graph): temporal selection→request mapper + open persistence

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `buildCompareGraphData` + diff `state` on graph data

**Files:**
- Modify: `web/src/components/graph/graphData.ts`
- Test: `web/src/components/graph/graphData.test.ts`

**Interfaces:**
- Consumes: `GraphNode`, `GraphEdge` from `../../contract`; existing `stem`.
- Produces:
  - `type GraphState = "appeared" | "disappeared" | "unchanged" | "changed"`
  - `GNode` gains optional `state?: GraphState`; `GLink` gains optional `state?: GraphState`.
  - `buildCompareGraphData(base: { nodes: GraphNode[]; edges: GraphEdge[] }, diff: { nodes_added: GraphNode[]; nodes_removed: GraphNode[]; edges_added: GraphEdge[]; edges_removed: GraphEdge[] }): { nodes: GNode[]; links: GLink[] }`
  - Rules: base nodes whose path ∈ `nodes_added` → `appeared`; `nodes_removed` are injected as extra nodes with `disappeared`; all other base nodes → `unchanged`. Edges follow the same rule over the union of base edges + removed edges, keyed `from|to`. Degree is undirected over that union, counting only endpoints present in the node set. `"changed"` is never produced.

- [ ] **Step 1: Write the failing test** (append to `graphData.test.ts`)

```ts
import { buildCompareGraphData } from "./graphData";
import type { GraphNode, GraphEdge } from "../../contract";

const gn = (path: string): GraphNode => ({ path, title: path, mtime_secs: 0n });
const ge = (from: string, to: string): GraphEdge => ({ from, to });

describe("buildCompareGraphData", () => {
  // base `to` = {a,b,c}, a-b, b-c ; diff: c appeared (+ b-c), x disappeared (+ a-x)
  const base = { nodes: [gn("a"), gn("b"), gn("c")], edges: [ge("a", "b"), ge("b", "c")] };
  const diff = {
    nodes_added: [gn("c")],
    nodes_removed: [gn("x")],
    edges_added: [ge("b", "c")],
    edges_removed: [ge("a", "x")],
  };

  it("labels appeared / disappeared / unchanged and injects removed nodes", () => {
    const { nodes, links } = buildCompareGraphData(base, diff);
    const byId = Object.fromEntries(nodes.map((n) => [n.id, n.state]));
    expect(byId).toEqual({ a: "unchanged", b: "unchanged", c: "appeared", x: "disappeared" });
    const linkState = links.map((l) => [l.source, l.target, l.state]);
    expect(linkState).toContainEqual(["a", "b", "unchanged"]);
    expect(linkState).toContainEqual(["b", "c", "appeared"]);
    expect(linkState).toContainEqual(["a", "x", "disappeared"]);
  });

  it("never emits a changed state", () => {
    const { nodes, links } = buildCompareGraphData(base, diff);
    expect(nodes.every((n) => n.state !== "changed")).toBe(true);
    expect(links.every((l) => l.state !== "changed")).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/graph/graphData.test.ts`
Expected: FAIL — `buildCompareGraphData` is not exported.

- [ ] **Step 3: Write the implementation** (edit `graphData.ts`)

Change the interfaces and add the builder:

```ts
export type GraphState = "appeared" | "disappeared" | "unchanged" | "changed";

export interface GNode {
  id: string;
  label: string;
  degree: number;
  state?: GraphState;
}
export interface GLink {
  source: string;
  target: string;
  state?: GraphState;
}
```

Add the import at the top and the builder at the end of the file:

```ts
import type { GraphNode, GraphEdge } from "../../contract";

const edgeKey = (e: { from: string; to: string }) => `${e.from} ${e.to}`;

/** Force-graph data for COMPARE mode: the `to` graph (base) styled by the
 *  from→to diff. Added nodes/edges = "appeared"; removed = "disappeared" and
 *  re-injected (they are absent from the base `to` graph); everything else =
 *  "unchanged". Degree is undirected over base ∪ removed edges. Never emits
 *  "changed" — the engine reports no such delta. */
export function buildCompareGraphData(
  base: { nodes: GraphNode[]; edges: GraphEdge[] },
  diff: {
    nodes_added: GraphNode[];
    nodes_removed: GraphNode[];
    edges_added: GraphEdge[];
    edges_removed: GraphEdge[];
  },
): { nodes: GNode[]; links: GLink[] } {
  const appearedNodes = new Set(diff.nodes_added.map((n) => n.path));
  const appearedEdges = new Set(diff.edges_added.map(edgeKey));

  // Node id list: base nodes + injected removed ghosts (dedup by path).
  const ids: string[] = [];
  const stateOf = new Map<string, GraphState>();
  for (const n of base.nodes) {
    ids.push(n.path);
    stateOf.set(n.path, appearedNodes.has(n.path) ? "appeared" : "unchanged");
  }
  for (const n of diff.nodes_removed) {
    if (!stateOf.has(n.path)) {
      ids.push(n.path);
      stateOf.set(n.path, "disappeared");
    }
  }
  const present = new Set(ids);

  // Edges: base ∪ removed (removed are absent from base). Filter to present
  // endpoints so a ghost edge only shows if both endpoints render.
  const seen = new Set<string>();
  const rawLinks: GLink[] = [];
  const push = (e: { from: string; to: string }, state: GraphState) => {
    const k = edgeKey(e);
    if (seen.has(k)) return;
    if (!present.has(e.from) || !present.has(e.to)) return;
    seen.add(k);
    rawLinks.push({ source: e.from, target: e.to, state });
  };
  for (const e of base.edges)
    push(e, appearedEdges.has(edgeKey(e)) ? "appeared" : "unchanged");
  for (const e of diff.edges_removed) push(e, "disappeared");

  const degree = new Map<string, number>();
  for (const id of ids) degree.set(id, 0);
  for (const l of rawLinks) {
    degree.set(l.source, (degree.get(l.source) ?? 0) + 1);
    degree.set(l.target, (degree.get(l.target) ?? 0) + 1);
  }

  const nodes: GNode[] = ids.map((id) => ({
    id,
    label: stem(id),
    degree: degree.get(id) ?? 0,
    state: stateOf.get(id),
  }));
  return { nodes, links: rawLinks };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/graph/graphData.test.ts`
Expected: PASS (existing tests + 2 new).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/graphData.ts web/src/components/graph/graphData.test.ts
git commit -m "feat(graph): buildCompareGraphData with appeared/disappeared/unchanged state

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Mock `graph_at` + `graph_diff` over a synthetic vault timeline

**Files:**
- Modify: `web/src/client/mock.ts`
- Test: `web/src/client/mock.test.ts`

**Interfaces:**
- Consumes: existing mock internals (`this.notes`, `displayTitle`, `extractLinks`, `splitFrontmatter`, `stemIndex`), `GraphNode`/`GraphEdge`.
- Produces:
  - Exported `interface VaultSnapshot { notes: Record<string, string> }`.
  - `MockClient` constructor 3rd param: `vaultSnapshots: Record<string, VaultSnapshot> = {}` (keyed by revspec id).
  - Private `buildGraph(notes: Map<string, string>, scope: GraphScope): { nodes: GraphNode[]; edges: GraphEdge[] }` (the shared node/edge builder, extracted from `get_graph`).
  - `runQuery` handles `graph_at` (unknown revspec → `not_found`) and `graph_diff`.

- [ ] **Step 1: Write the failing test** (append to `mock.test.ts`)

```ts
it("graph_at builds the graph from the vault snapshot at that revspec", async () => {
  const c = new MockClient(freshNotes(), {}, {
    r1: { notes: { "a.md": "start" } },
    r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
  });
  expect(
    await c.runQuery({ type: "graph_at", revision: "r2", scope: { type: "full" } }),
  ).toEqual({
    type: "graph",
    nodes: [
      { path: "a.md", title: "a", mtime_secs: 0n },
      { path: "b.md", title: "b", mtime_secs: 0n },
    ],
    edges: [{ from: "a.md", to: "b.md" }],
  });
});

it("graph_at rejects an unknown revspec", async () => {
  const c = new MockClient(freshNotes());
  await expect(
    c.runQuery({ type: "graph_at", revision: "nope", scope: { type: "full" } }),
  ).rejects.toEqual({ type: "not_found", what: "nope" });
});

it("graph_diff reports added and removed nodes and edges", async () => {
  const c = new MockClient(freshNotes(), {}, {
    r1: { notes: { "a.md": "lone" } },
    r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
  });
  expect(
    await c.runQuery({
      type: "graph_diff",
      from: "r1",
      to: "r2",
      scope: { type: "full" },
    }),
  ).toEqual({
    type: "graph_diff",
    nodes_added: [{ path: "b.md", title: "b", mtime_secs: 0n }],
    nodes_removed: [],
    edges_added: [{ from: "a.md", to: "b.md" }],
    edges_removed: [],
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/client/mock.test.ts`
Expected: FAIL — mock throws `unsupported query graph_at`.

- [ ] **Step 3a: Extract `buildGraph` and store `vaultSnapshots`**

Add the interface + import near the top of `mock.ts`:

```ts
import type { GraphScope } from "../contract";

/** A full note-set at one revspec — the mock's synthetic vault history, used by
 *  graph_at / graph_diff so the temporal surface is testable offline. */
export interface VaultSnapshot {
  notes: Record<string, string>;
}
```

Add the field + constructor param:

```ts
  private vaultSnapshots: Map<string, VaultSnapshot>;

  constructor(
    seed: Record<string, string> = {},
    history: Record<string, HistoryFixture> = {},
    vaultSnapshots: Record<string, VaultSnapshot> = {},
  ) {
    this.notes = new Map(Object.entries(seed));
    this.history = new Map(Object.entries(history));
    this.vaultSnapshots = new Map(Object.entries(vaultSnapshots));
  }
```

Extract the shared builder (move the body of the current `get_graph` case into it):

```ts
  private buildGraph(
    notes: Map<string, string>,
    scope: GraphScope,
  ): { nodes: GraphNode[]; edges: GraphEdge[] } {
    const byStem = new Map<string, string>();
    for (const path of notes.keys()) byStem.set(stem(path), path);
    const nodes: GraphNode[] = [...notes.entries()]
      .map(([path, raw]) => ({
        path,
        title: displayTitle(path, raw),
        mtime_secs: 0n,
      }))
      .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
    const seen = new Set<string>();
    const edges: GraphEdge[] = [];
    for (const [from, raw] of notes.entries()) {
      for (const target of extractLinks(splitFrontmatter(raw).body)) {
        const to = byStem.get(target);
        if (to && !seen.has(`${from} ${to}`)) {
          seen.add(`${from} ${to}`);
          edges.push({ from, to });
        }
      }
    }
    edges.sort((a, b) =>
      a.from === b.from
        ? a.to < b.to
          ? -1
          : a.to > b.to
            ? 1
            : 0
        : a.from < b.from
          ? -1
          : 1,
    );
    if (scope.type === "focused") {
      const { path: root, depth } = scope;
      if (!nodes.some((n) => n.path === root)) return { nodes: [], edges: [] };
      const adj = new Map<string, string[]>();
      const addAdj = (a: string, b: string) =>
        (adj.get(a) ?? adj.set(a, []).get(a)!).push(b);
      for (const e of edges) {
        addAdj(e.from, e.to);
        addAdj(e.to, e.from);
      }
      const reached = new Set([root]);
      let frontier = [root];
      for (let d = 0; d < depth && frontier.length; d++) {
        const next: string[] = [];
        for (const n of frontier)
          for (const m of adj.get(n) ?? [])
            if (!reached.has(m)) {
              reached.add(m);
              next.push(m);
            }
        frontier = next;
      }
      return {
        nodes: nodes.filter((n) => reached.has(n.path)),
        edges: edges.filter((e) => reached.has(e.from) && reached.has(e.to)),
      };
    }
    return { nodes, edges };
  }
```

Replace the `get_graph` case body with:

```ts
      case "get_graph":
        return { type: "graph", ...this.buildGraph(this.notes, q.scope) };
```

- [ ] **Step 3b: Add `graph_at` and `graph_diff` cases** (in `runQuery`, before `default`)

```ts
      case "graph_at": {
        const snap = this.vaultSnapshots.get(q.revision);
        if (!snap) {
          const err: ContractError = { type: "not_found", what: q.revision };
          throw err;
        }
        const notes = new Map(Object.entries(snap.notes));
        return { type: "graph", ...this.buildGraph(notes, q.scope) };
      }
      case "graph_diff": {
        const load = (rev: string): Map<string, string> => {
          const snap = this.vaultSnapshots.get(rev);
          if (!snap) {
            const err: ContractError = { type: "not_found", what: rev };
            throw err;
          }
          return new Map(Object.entries(snap.notes));
        };
        const a = this.buildGraph(load(q.from), q.scope);
        const b = this.buildGraph(load(q.to), q.scope);
        const aNodes = new Set(a.nodes.map((n) => n.path));
        const bNodes = new Set(b.nodes.map((n) => n.path));
        const eKey = (e: GraphEdge) => `${e.from} ${e.to}`;
        const aEdges = new Set(a.edges.map(eKey));
        const bEdges = new Set(b.edges.map(eKey));
        return {
          type: "graph_diff",
          nodes_added: b.nodes.filter((n) => !aNodes.has(n.path)),
          nodes_removed: a.nodes.filter((n) => !bNodes.has(n.path)),
          edges_added: b.edges.filter((e) => !aEdges.has(eKey(e))),
          edges_removed: a.edges.filter((e) => !bEdges.has(eKey(e))),
        };
      }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/client/mock.test.ts`
Expected: PASS (existing get_graph tests still green + 3 new).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(mock): graph_at + graph_diff over a synthetic vault timeline

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Store temporal slice (data + actions)

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

**Interfaces:**
- Consumes: existing store closure helpers `set`, `get`, `client`, `pushError`, `unexpected`; `GraphNode`; `Revision` (add to the contract import).
- Produces on `CairnState`:
  - `type GraphDiff = { nodes_added: GraphNode[]; nodes_removed: GraphNode[]; edges_added: GraphEdge[]; edges_removed: GraphEdge[] }`
  - `temporal: { timeline: Revision[] | null; snapshot: { nodes: GraphNode[]; edges: { from: string; to: string }[] } | null; diff: GraphDiff | null }`
  - `loadTimeline(path: string): Promise<void>`, `loadSnapshot(revision: string): Promise<void>`, `loadDiff(from: string, to: string): Promise<void>`, `clearTemporal(): void`
  - Reset to `EMPTY_TEMPORAL` in `openCairn`.

- [ ] **Step 1: Write the failing test** (append to `store.test.ts`; `setup()` seeds `a.md`/`b.md` — pass `vaultSnapshots` to the mock via the existing `setup` seam; if `setup` doesn't forward a 3rd arg, extend it to do so)

```ts
describe("temporal graph", () => {
  const snaps = {
    r1: { notes: { "a.md": "lone" } },
    r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
  };

  it("loadTimeline populates the timeline from note_history", async () => {
    const history = {
      "a.md": {
        revisions: [
          { id: "r2", message: "add b", timestamp_secs: 20n, author: "x" },
          { id: "r1", message: "init", timestamp_secs: 10n, author: "x" },
        ],
        contents: { r1: "lone", r2: "links [[b]]" },
      },
    };
    const { store } = setup({ history });
    await store.getState().init();
    await store.getState().loadTimeline("a.md");
    expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual(["r2", "r1"]);
  });

  it("loadSnapshot fills snapshot from graph_at and clears diff", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadSnapshot("r2");
    const t = store.getState().temporal;
    expect(t.snapshot?.nodes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
    expect(t.diff).toBeNull();
  });

  it("loadDiff fills base snapshot + deltas", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadDiff("r1", "r2");
    const t = store.getState().temporal;
    expect(t.snapshot?.nodes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
    expect(t.diff?.nodes_added.map((n) => n.path)).toEqual(["b.md"]);
  });

  it("clearTemporal drops snapshot and diff but keeps the timeline", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadSnapshot("r2");
    store.getState().clearTemporal();
    expect(store.getState().temporal.snapshot).toBeNull();
    expect(store.getState().temporal.diff).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/store/store.test.ts -t "temporal graph"`
Expected: FAIL — `loadTimeline`/`temporal` undefined (and possibly `setup` needs the new options).

- [ ] **Step 3a: Extend the test `setup` helper**

The current helper (top of `store.test.ts`) takes no args:

```ts
function setup() {
  const client = new MockClient({
    "a.md": "links to [[b]]",
    "b.md": "target note",
  });
  const store = createCairnStore(client);
  return { client, store };
}
```

Replace it with an optional-opts version (existing no-arg call sites keep working — the defaults reproduce today's behavior). Import the fixture types from the mock:

```ts
import { MockClient, type HistoryFixture, type VaultSnapshot } from "../client/mock";

function setup(
  opts: {
    history?: Record<string, HistoryFixture>;
    vaultSnapshots?: Record<string, VaultSnapshot>;
  } = {},
) {
  const client = new MockClient(
    { "a.md": "links to [[b]]", "b.md": "target note" },
    opts.history ?? {},
    opts.vaultSnapshots ?? {},
  );
  const store = createCairnStore(client);
  return { client, store };
}
```

(`HistoryFixture` is already exported from `mock.ts`; `VaultSnapshot` is exported in Task 3.)

- [ ] **Step 3b: Add state, seq, actions, and reset (`store.ts`)**

Add `Revision` to the contract import:

```ts
import type { TagCount, Event, GraphNode, Revision } from "../contract";
```

Add the diff type + state field near the `graph:` line (~142):

```ts
  temporal: {
    timeline: Revision[] | null;
    snapshot: { nodes: GraphNode[]; edges: { from: string; to: string }[] } | null;
    diff: GraphDiff | null;
  };
```

Add above `CairnState` (module scope) the type + empty constant:

```ts
export type GraphDiff = {
  nodes_added: GraphNode[];
  nodes_removed: GraphNode[];
  edges_added: { from: string; to: string }[];
  edges_removed: { from: string; to: string }[];
};

const EMPTY_TEMPORAL = {
  timeline: null,
  snapshot: null,
  diff: null,
} as const;
```

(Export `GraphDiff` so Task 6's hook can name it if needed.)

Add the action signatures to the `CairnState` interface (near `loadGraph`):

```ts
  loadTimeline(path: string): Promise<void>;
  loadSnapshot(revision: string): Promise<void>;
  loadDiff(from: string, to: string): Promise<void>;
  clearTemporal(): void;
```

Extend the seq object (`const seq = { backlinks: 0, results: 0, graph: 0 }`):

```ts
  const seq = { backlinks: 0, results: 0, graph: 0, timeline: 0, snapshot: 0, diff: 0 };
```

Add `temporal: { ...EMPTY_TEMPORAL }` to the initial state object and to **both** `openCairn` reset points (the two `graph: null,` lines — add `temporal: { ...EMPTY_TEMPORAL },` beside each).

Add the actions (after `loadGraph`):

```ts
      async loadTimeline(path) {
        const token = ++seq.timeline;
        try {
          const res = await client.runQuery({ type: "note_history", path });
          if (token !== seq.timeline) return;
          if (res.type === "history")
            set((s) => ({ temporal: { ...s.temporal, timeline: res.revisions } }));
          else unexpected("Load timeline", res);
        } catch (err) {
          if (token === seq.timeline) pushError("Load timeline", err, { path });
        }
      },

      async loadSnapshot(revision) {
        const token = ++seq.snapshot;
        try {
          const res = await client.runQuery({
            type: "graph_at",
            revision,
            scope: { type: "full" },
          });
          if (token !== seq.snapshot) return;
          if (res.type === "graph")
            set((s) => ({
              temporal: {
                ...s.temporal,
                snapshot: { nodes: res.nodes, edges: res.edges },
                diff: null,
              },
            }));
          else unexpected("Load snapshot", res);
        } catch (err) {
          if (token === seq.snapshot) pushError("Load snapshot", err);
        }
      },

      async loadDiff(from, to) {
        const token = ++seq.diff;
        try {
          const [base, delta] = await Promise.all([
            client.runQuery({ type: "graph_at", revision: to, scope: { type: "full" } }),
            client.runQuery({ type: "graph_diff", from, to, scope: { type: "full" } }),
          ]);
          if (token !== seq.diff) return;
          if (base.type === "graph" && delta.type === "graph_diff")
            set((s) => ({
              temporal: {
                ...s.temporal,
                snapshot: { nodes: base.nodes, edges: base.edges },
                diff: {
                  nodes_added: delta.nodes_added,
                  nodes_removed: delta.nodes_removed,
                  edges_added: delta.edges_added,
                  edges_removed: delta.edges_removed,
                },
              },
            }));
          else unexpected("Load diff", base.type === "graph" ? delta : base);
        } catch (err) {
          if (token === seq.diff) pushError("Load diff", err);
        }
      },

      clearTemporal() {
        set((s) => ({ temporal: { ...s.temporal, snapshot: null, diff: null } }));
      },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/store/store.test.ts`
Expected: PASS (existing suite + 4 new temporal tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): temporal graph slice — timeline/snapshot/diff actions

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `TemporalScrubber` presentational component

**Files:**
- Create: `web/src/components/graph/TemporalScrubber.tsx`
- Test: `web/src/components/graph/TemporalScrubber.test.tsx`

**Interfaces:**
- Consumes: `Revision` from `../../contract`; `TemporalSelection` from `./temporalControls`.
- Produces: `TemporalScrubber(props: { timeline: Revision[]; selection: TemporalSelection; onSelect: (s: TemporalSelection) => void })`.
  - Ticks laid out oldest→newest, left→right (reverse of the newest-first `timeline`).
  - A "Live" button emits `{ kind: "live" }`. Each tick button emits `{ kind: "snapshot", at }` where `at` is the **timeline index** (newest-first) for that tick.
  - Range/compare interaction: a "Compare from here" affordance on a snapshot tick emits `{ kind: "compare", from, to }`. Keep the interaction minimal (two clicks): clicking a second tick while a snapshot is active forms the range (older index = `from`, newer = `to`).

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/graph/TemporalScrubber.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TemporalScrubber } from "./TemporalScrubber";
import type { Revision } from "../../contract";

const tl: Revision[] = [
  { id: "c3", message: "third", timestamp_secs: 30n, author: "a" },
  { id: "c2", message: "second", timestamp_secs: 20n, author: "a" },
  { id: "c1", message: "first", timestamp_secs: 10n, author: "a" },
];

describe("TemporalScrubber", () => {
  it("renders one tick per revision plus a Live control", () => {
    render(<TemporalScrubber timeline={tl} selection={{ kind: "live" }} onSelect={vi.fn()} />);
    expect(screen.getAllByRole("button", { name: /revision/i })).toHaveLength(3);
    expect(screen.getByRole("button", { name: /live/i })).toBeInTheDocument();
  });

  it("emits a snapshot selection with the newest-first index on tick click", async () => {
    const onSelect = vi.fn();
    render(<TemporalScrubber timeline={tl} selection={{ kind: "live" }} onSelect={onSelect} />);
    // leftmost tick is the oldest (c1) → timeline index 2
    await userEvent.click(screen.getByRole("button", { name: /revision first/i }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "snapshot", at: 2 });
  });

  it("emits live from the Live control", async () => {
    const onSelect = vi.fn();
    render(<TemporalScrubber timeline={tl} selection={{ kind: "snapshot", at: 1 }} onSelect={onSelect} />);
    await userEvent.click(screen.getByRole("button", { name: /live/i }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "live" });
  });

  it("forms a compare range (older=from, newer=to) on a second tick", async () => {
    const onSelect = vi.fn();
    // snapshot at c2 (index 1) already active; click the oldest tick c1 (index 2)
    render(<TemporalScrubber timeline={tl} selection={{ kind: "snapshot", at: 1 }} onSelect={onSelect} />);
    await userEvent.click(screen.getByRole("button", { name: /revision first/i }));
    expect(onSelect).toHaveBeenCalledWith({ kind: "compare", from: 2, to: 1 });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/graph/TemporalScrubber.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Write the implementation**

```tsx
// web/src/components/graph/TemporalScrubber.tsx
import type { Revision } from "../../contract";
import type { TemporalSelection } from "./temporalControls";

/** A horizontal timeline scrubber. `timeline` is newest-first; ticks render
 *  oldest→newest, left→right. One tick = snapshot; picking a second tick while a
 *  snapshot is active forms a compare range (older index = from, newer = to). */
export function TemporalScrubber(props: {
  timeline: Revision[];
  selection: TemporalSelection;
  onSelect: (s: TemporalSelection) => void;
}) {
  const { timeline, selection, onSelect } = props;
  // Render oldest→newest: map display position → newest-first index.
  const order = timeline.map((_, i) => timeline.length - 1 - i); // display→timeline idx

  const activeIdx =
    selection.kind === "snapshot"
      ? selection.at
      : selection.kind === "compare"
        ? selection.to
        : null;

  const onTick = (idx: number) => {
    if (selection.kind === "snapshot" && selection.at !== idx) {
      const from = Math.max(selection.at, idx); // older = higher index
      const to = Math.min(selection.at, idx); // newer = lower index
      onSelect({ kind: "compare", from, to });
    } else {
      onSelect({ kind: "snapshot", at: idx });
    }
  };

  return (
    <div className="pointer-events-auto absolute inset-x-2 bottom-2 z-10 flex items-center gap-2 rounded-md border border-border bg-surface/90 px-2 py-1.5">
      <button
        type="button"
        aria-pressed={selection.kind === "live"}
        className={
          "rounded px-2 py-0.5 text-[11px] " +
          (selection.kind === "live"
            ? "bg-accent text-accent-fg"
            : "text-muted hover:text-text")
        }
        onClick={() => onSelect({ kind: "live" })}
      >
        Live
      </button>
      <div className="flex flex-1 items-center gap-1 overflow-x-auto">
        {order.map((idx) => {
          const rev = timeline[idx];
          const on = activeIdx === idx;
          return (
            <button
              key={rev.id}
              type="button"
              aria-pressed={on}
              title={`${rev.id} — ${rev.message}`}
              aria-label={`Revision ${rev.message}`}
              className={
                "h-4 w-2 shrink-0 rounded-sm " +
                (on ? "bg-accent" : "bg-border hover:bg-muted")
              }
              onClick={() => onTick(idx)}
            />
          );
        })}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/graph/TemporalScrubber.test.tsx`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/TemporalScrubber.tsx web/src/components/graph/TemporalScrubber.test.tsx
git commit -m "feat(graph): TemporalScrubber timeline component

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `useTemporalGraph` hook — selection state + load effects + source selection

**Files:**
- Create: `web/src/components/graph/useTemporalGraph.ts`
- Test: `web/src/components/graph/useTemporalGraph.test.tsx`

**Interfaces:**
- Consumes: `useCairn`, `useActions` from `../../app/cairnStore`; `selectionToRequest`, `TemporalSelection`, `loadTemporalOpen`/`saveTemporalOpen` from `./temporalControls`.
- Produces: `useTemporalGraph(activePath: string | null): { timeline: Revision[] | null; selection: TemporalSelection; setSelection: (s: TemporalSelection) => void; open: boolean; setOpen: (o: boolean) => void; disabled: boolean; mode: "live" | "snapshot" | "compare"; source: { nodes: GraphNode[]; edges: { from: string; to: string }[] } | null; diff: GraphDiff | null }`
  - `disabled = !activePath` (no timeline source).
  - Effect A: when `activePath` changes → if present, `actions.loadTimeline(activePath)` and reset `selection` to `{ kind: "live" }`; if null, reset to live.
  - Effect B: when the request derived from `(selection, timeline)` changes → dispatch `loadSnapshot`/`loadDiff`/`clearTemporal`.
  - `mode`/`source`/`diff`: `live` → source `null` (caller uses live props); `snapshot` → `temporal.snapshot`, diff `null`; `compare` → `temporal.snapshot` (base) + `temporal.diff`.

- [ ] **Step 1: Write the failing test**

This repo's store is a **module singleton** (`cairnStore`). Store-connected tests
seed it with `cairnStore.setState({...})` and spy on its actions — they do **not**
rebuild the store (that pattern is used only in `store.test.ts` for action-level
tests, which already cover real data loading in Task 4). So here we spy the
actions (so the load effects don't touch the singleton's fixture mock) and seed
`temporal` directly, asserting the hook's derivation + effect dispatch.

```tsx
// web/src/components/graph/useTemporalGraph.test.tsx
import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTemporalGraph } from "./useTemporalGraph";
import { cairnStore } from "../../app/cairnStore";
import type { Revision } from "../../contract";

const TL: Revision[] = [
  { id: "r2", message: "add b", timestamp_secs: 20n, author: "x" },
  { id: "r1", message: "init", timestamp_secs: 10n, author: "x" },
];

describe("useTemporalGraph", () => {
  beforeEach(() => {
    // Neutralize the real load actions so effects don't hit the fixture mock or
    // clobber the seeded timeline; Task 4 already tests the real actions.
    vi.spyOn(cairnStore.getState(), "loadTimeline").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadSnapshot").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadDiff").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(() => {});
    cairnStore.setState({ temporal: { timeline: TL, snapshot: null, diff: null } });
  });

  it("is disabled with no active note; loads the timeline when one opens", () => {
    const { result, rerender } = renderHook(({ p }) => useTemporalGraph(p), {
      initialProps: { p: null as string | null },
    });
    expect(result.current.disabled).toBe(true);

    rerender({ p: "a.md" });
    expect(result.current.disabled).toBe(false);
    expect(cairnStore.getState().loadTimeline).toHaveBeenCalledWith("a.md");
    expect(result.current.mode).toBe("live");
    expect(result.current.source).toBeNull();
  });

  it("dispatches loadSnapshot with the mapped revision on a tick selection", () => {
    const { result } = renderHook(() => useTemporalGraph("a.md"));
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 })); // r2
    expect(cairnStore.getState().loadSnapshot).toHaveBeenCalledWith("r2");
    expect(result.current.mode).toBe("snapshot");
  });

  it("reflects a seeded snapshot as the source in snapshot mode", () => {
    cairnStore.setState({
      temporal: {
        timeline: TL,
        snapshot: { nodes: [{ path: "a.md", title: "a", mtime_secs: 0n }], edges: [] },
        diff: null,
      },
    });
    const { result } = renderHook(() => useTemporalGraph("a.md"));
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 }));
    expect(result.current.source?.nodes.map((n) => n.path)).toEqual(["a.md"]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/graph/useTemporalGraph.test.tsx`
Expected: FAIL — `Cannot find module './useTemporalGraph'`.

- [ ] **Step 3: Write the implementation**

```tsx
// web/src/components/graph/useTemporalGraph.ts
import { useEffect, useMemo, useState } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import {
  selectionToRequest,
  loadTemporalOpen,
  saveTemporalOpen,
  type TemporalSelection,
} from "./temporalControls";

/** Wires the temporal controls (view state) to the store's temporal data. Owns
 *  the scrubber selection; runs the timeline load on note change and the
 *  snapshot/diff load on selection change; returns the effective data source
 *  (null in live mode → caller uses the live graph). */
export function useTemporalGraph(activePath: string | null) {
  const temporal = useCairn((s) => s.temporal);
  const actions = useActions();
  const [selection, setSelection] = useState<TemporalSelection>({ kind: "live" });
  const [open, setOpenState] = useState(loadTemporalOpen);
  const setOpen = (o: boolean) => {
    setOpenState(o);
    saveTemporalOpen(o);
  };

  // Effect A: note change → (re)load its timeline and reset to live.
  useEffect(() => {
    setSelection({ kind: "live" });
    if (activePath) void actions.loadTimeline(activePath);
  }, [activePath, actions]);

  const request = useMemo(
    () => selectionToRequest(selection, temporal.timeline),
    [selection, temporal.timeline],
  );

  // Effect B: request change → fetch the matching temporal data.
  useEffect(() => {
    if (request.mode === "live") actions.clearTemporal();
    else if (request.mode === "snapshot") void actions.loadSnapshot(request.revision);
    else void actions.loadDiff(request.from, request.to);
  }, [request, actions]);

  const mode = request.mode;
  const source = mode === "live" ? null : temporal.snapshot;
  const diff = mode === "compare" ? temporal.diff : null;

  return {
    timeline: temporal.timeline,
    selection,
    setSelection,
    open,
    setOpen,
    disabled: !activePath,
    mode,
    source,
    diff,
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/graph/useTemporalGraph.test.tsx`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/useTemporalGraph.ts web/src/components/graph/useTemporalGraph.test.tsx
git commit -m "feat(graph): useTemporalGraph hook — selection state + load effects

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Wire temporal into `GraphView` (source swap + diff styling + scrubber)

**Files:**
- Modify: `web/src/components/GraphView.tsx`
- Test: `web/src/components/GraphView.test.tsx`

**Interfaces:**
- Consumes: `useTemporalGraph` (Task 6), `TemporalScrubber` (Task 5), `buildCompareGraphData` + `GraphState` (Task 2).
- Behavior:
  - Add a **temporal toggle** button (clock icon) beside the forces button; toggles `open`. When `disabled` (no active note), the toggle is `disabled` with a tooltip "Open a note to scrub its history".
  - When `open && !disabled`, render `<TemporalScrubber>` with `timeline`/`selection`/`setSelection`.
  - **Data source:** `mode==="live"` → today's `props.nodes/props.edges` path (unchanged). `mode==="snapshot"` → build from `source.nodes` (projected to paths) + `source.edges`, still honoring local mode. `mode==="compare"` → `buildCompareGraphData(source, diff)`, **global only** (skip local subsetting; disable the local toggle with a tooltip while comparing).
  - **Diff styling** in `paintNode`: `state==="appeared"` → green `#22c55e`; `"disappeared"` → ghost `#6b7280` at `globalAlpha` 0.4; else current behavior. In `linkColor`: `"appeared"` → green, `"disappeared"` → faint gray; add a `linkLineDash` accessor returning `[4, 3]` for `"disappeared"`, else `[]`.

- [ ] **Step 1: Write the failing test** (append to `GraphView.test.tsx`; these assert chrome, not canvas)

`GraphView` becomes store-connected via `useTemporalGraph`, but the store is a
module singleton, so the existing bare `render(<GraphView .../>)` still works —
no provider needed. Seed temporal state with `cairnStore.setState` and spy the
load actions (so Effect A doesn't clobber the seeded timeline). Add these imports
at the top of the file if absent:

```tsx
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { cairnStore } from "../app/cairnStore";
import type { GraphNode, Revision } from "../contract";

const gnode = (p: string): GraphNode => ({ path: p, title: p, mtime_secs: 0n });
const TL: Revision[] = [
  { id: "r2", message: "add b", timestamp_secs: 20n, author: "x" },
  { id: "r1", message: "init", timestamp_secs: 10n, author: "x" },
];
```

```tsx
it("disables the temporal toggle when no note is open", () => {
  vi.spyOn(cairnStore.getState(), "loadTimeline").mockResolvedValue();
  setup({ nodes: [gnode("a.md")], activePath: null });
  expect(
    screen.getByRole("button", { name: /graph history/i }),
  ).toBeDisabled();
});

it("shows the scrubber when temporal is opened with a note active", async () => {
  vi.spyOn(cairnStore.getState(), "loadTimeline").mockResolvedValue();
  vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(() => {});
  cairnStore.setState({ temporal: { timeline: TL, snapshot: null, diff: null } });
  setup({ nodes: [gnode("a.md")], activePath: "a.md" });
  await userEvent.click(screen.getByRole("button", { name: /graph history/i }));
  expect(screen.getByRole("button", { name: /live/i })).toBeInTheDocument();
});
```

> Keep the toggle's accessible name **stable**: `aria-label="Graph history"` in both states (so the tests find it whether enabled or disabled). Put the "Open a note to scrub its history" hint in the `title` attribute only when disabled.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/GraphView.test.tsx`
Expected: FAIL — no temporal toggle button.

- [ ] **Step 3: Implement the integration** (edit `GraphView.tsx`)

Add imports:

```tsx
import { TemporalScrubber } from "./graph/TemporalScrubber";
import { useTemporalGraph } from "./graph/useTemporalGraph";
import { buildCompareGraphData } from "./graph/graphData";
```

Near the other hooks, call the temporal hook:

```tsx
  const temporal = useTemporalGraph(props.activePath);
```

Replace the data-derivation block so the source depends on temporal mode. Keep `nodePaths`/`globalData`/`localData` for live+snapshot; add a compare branch:

```tsx
  // Effective source: live uses props; snapshot uses the historical graph
  // (still honoring local mode); compare builds a diff-styled global graph.
  const srcNodes = temporal.source
    ? temporal.source.nodes.map((n) => n.path)
    : nodePaths;
  const srcEdges = temporal.source ? temporal.source.edges : props.edges;

  const compareData = useMemo(
    () =>
      temporal.mode === "compare" && temporal.source && temporal.diff
        ? buildCompareGraphData(temporal.source, temporal.diff)
        : null,
    [temporal.mode, temporal.source, temporal.diff],
  );
```

Change `globalData`/`globalAdj`/`localSub` to build from `srcNodes`/`srcEdges` instead of `nodePaths`/`props.edges` (so snapshot flows through the same local machinery). Then:

```tsx
  const forcedGlobal = temporal.mode === "compare"; // compare is global-only
  const data = compareData ?? (forcedGlobal ? globalData : (localData ?? globalData));
  const adjacency = compareData
    ? buildAdjacency(compareData.links)
    : forcedGlobal
      ? globalAdj
      : (localAdj ?? globalAdj);
```

In `paintNode`, before computing `base`, branch on `node.state`:

```tsx
      const stateColor =
        node.state === "appeared"
          ? "#22c55e"
          : node.state === "disappeared"
            ? "#6b7280"
            : null;
```

Use `stateColor ?? base` for the fill, and when `node.state === "disappeared"` set `ctx.globalAlpha = 0.4` for the arc fill (restore to 1 after, as the code already does).

In `linkColor`, honor link state first:

```tsx
      const st = (link as { state?: string }).state;
      if (st === "appeared") return "#22c55e";
      if (st === "disappeared") return "#6b728066";
      // ...existing hover behavior...
```

Add a `linkLineDash` accessor and pass it to `<ForceGraph2D linkLineDash={...} />`:

```tsx
  const linkLineDash = useCallback(
    (link: { state?: string }) => (link.state === "disappeared" ? [4, 3] : []),
    [],
  );
```

Add the temporal toggle in the top-right control stack (beside the forces `IconButton`). `IconButton` maps its `label` prop to `aria-label` and spreads the rest onto the `<button>`, so:

```tsx
<IconButton
  label="Graph history"            // stable aria-label in both states
  className="border border-border bg-surface"
  disabled={temporal.disabled}
  title={temporal.disabled ? "Open a note to scrub its history" : "Graph history"}
  onClick={() => temporal.setOpen(!temporal.open)}
>
  {/* clock SVG, same 16x16 stroke style as the forces icon */}
</IconButton>
``` Disable the local toggle buttons when `temporal.mode === "compare"` (add `disabled` + a `title` like "Compare shows the whole graph"). Render the scrubber:

```tsx
      {temporal.open && !temporal.disabled && temporal.timeline && (
        <TemporalScrubber
          timeline={temporal.timeline}
          selection={temporal.selection}
          onSelect={temporal.setSelection}
        />
      )}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/components/GraphView.test.tsx`
Expected: PASS (existing + 2 new).

- [ ] **Step 5: Full gate + commit**

```bash
cd web
npx tsc --noEmit && npx eslint . && npx prettier --check "src/**/*.{ts,tsx}" && npx vitest run
cd ..
git add web/src/components/GraphView.tsx web/src/components/GraphView.test.tsx
git commit -m "feat(graph): temporal scrubber + snapshot/compare rendering in GraphView

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification

- [ ] From `web/`: `npx tsc --noEmit` → 0 errors.
- [ ] From `web/`: `npx eslint .` → 0 problems.
- [ ] From `web/`: `npx prettier --check "src/**/*.{ts,tsx}"` → all matched files use Prettier code style.
- [ ] From `web/`: `npx vitest run` → all green.
- [ ] Manual smoke (optional, via the app): open a note → temporal toggle enables → scrub to a past revision shows the historical graph → select a second tick shows appeared/disappeared styling → Live returns to the working-tree graph.

## Follow-ons (out of scope here)

- Semantic-suggestions overlay (`get_suggestions` → dashed weighted edges) — separate spec.
- `vault_history` global timeline — file the low-priority engine ask; when it lands, swap Effect A's `loadTimeline(activePath)` source and drop the note-open gating (render path unchanged).
- Compare + local mode (currently global-only).
- Amber `"changed"` state — dormant until the engine reports `nodes_changed`.
