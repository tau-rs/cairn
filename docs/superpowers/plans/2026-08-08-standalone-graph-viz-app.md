# Standalone Graph-Viz App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone browser app (`graph-viz/` in this repo) that renders a live cairn link graph from a running daemon, with time-scrubbing (`graph_at`) and change-highlighting diff (`graph_diff`).

**Architecture:** Vite/React SPA talking to the daemon's HTTP `POST /query` through a Vite dev-server proxy that injects the bearer token (same-origin, no CORS, token never in browser JS). Graph logic lives in portable modules (`src/client/`, `src/graph/`) that fold into `cairn-web-ui` at D1; the Vite shell is throwaway. Three modes ride one shared `vault_history` timeline: Live, Scrub, Diff.

**Tech Stack:** Vite 6, React 19, TypeScript 5, `react-force-graph-2d`, vitest + @testing-library/react, pnpm. Design spec: `docs/superpowers/specs/2026-08-08-standalone-graph-viz-design.md`.

## Global Constraints

- App lives at repo-root `graph-viz/`; committed to this repo (branch `standalone-graph-viz-app`).
- Package manager: **pnpm**. Node ≥ 20.
- Portable modules (`src/client/`, `src/graph/`) must NOT import from the app shell (`App.tsx`, `main.tsx`, `vite.config.ts`). They depend only on vendored contract types + a transport function.
- Contract types are **vendored from THIS repo's** `crates/cairn-contract/bindings/*.ts` — never hand-rolled, never from cairn-ui's stale copy. Files under `src/contract/` are generated; do not edit.
- Daemon wire shapes (verified): request = `Query` enum (`{"type": ...}`); `get_graph`/`graph_at` → `QueryResponse::Graph {nodes: GraphNode[], edges: GraphEdge[]}`; `graph_diff` → `{nodes_added, nodes_removed, nodes_changed: GraphNode[], edges_added, edges_removed: GraphEdge[]}`; `vault_history` → `{type:"history", revisions: Revision[]}` (newest first). `GraphScope` for full graph = `{"type":"full"}`.
- `graph_at`/`graph_diff` are git-history walks — fire on slider **release** (debounced), never on drag.
- Node layout persistence: reuse RF node objects by `path` across payloads so the graph morphs, not re-explodes.
- YAGNI (not in v1): color-groups editor, force-settings panel, focused/neighborhood scope, click-to-open-note, search, plugins, in-app vault switching.

---

## File Structure

```
graph-viz/
├── .gitignore                   Task 1  (node_modules, dist)
├── package.json                 Task 1  (deps, scripts)
├── tsconfig.json                Task 1
├── vite.config.ts               Task 1  THROWAWAY: build + /query token-injecting proxy
├── index.html                   Task 1  THROWAWAY shell
├── vitest.setup.ts              Task 1  (jsdom + testing-library)
├── scripts/sync-contract.mjs    Task 1  copies ../crates/cairn-contract/bindings/*.ts → src/contract/
├── README.md                    Task 7
└── src/
    ├── main.tsx                 Task 6  THROWAWAY shell
    ├── App.tsx                  Task 6  THROWAWAY: mode switch + timeline + wiring
    ├── contract/                Task 1  vendored ts-rs bindings (generated)
    ├── client/
    │   ├── daemonClient.ts      Task 2  PORTABLE: typed /query client
    │   └── daemonClient.test.ts Task 2
    └── graph/
        ├── graphData.ts         Task 3  PORTABLE: RF data build, nodeRadius, labelAlpha, node reuse
        ├── graphData.test.ts    Task 3
        ├── diffStyle.ts         Task 4  PORTABLE: diff classification + palette
        ├── diffStyle.test.ts    Task 4
        ├── GraphView.tsx        Task 5  PORTABLE: react-force-graph-2d rendering surface
        └── GraphView.test.tsx   Task 5
```

---

## Task 1: Scaffold app, build config, token-injecting proxy, contract vendoring

**Files:**
- Create: `graph-viz/package.json`, `graph-viz/tsconfig.json`, `graph-viz/vite.config.ts`, `graph-viz/index.html`, `graph-viz/vitest.setup.ts`, `graph-viz/.gitignore`, `graph-viz/scripts/sync-contract.mjs`
- Create (generated): `graph-viz/src/contract/*.ts` (via the sync script)

**Interfaces:**
- Produces: a booting Vite app; `pnpm --dir graph-viz test` runs vitest; `src/contract/` populated with the vendored bindings (imported by all later tasks); `/query` proxied to the daemon with `Authorization: Bearer <token>` injected.

- [ ] **Step 1: Create `graph-viz/package.json`**

```json
{
  "name": "cairn-graph-viz",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "test:watch": "vitest",
    "sync-contract": "node scripts/sync-contract.mjs"
  },
  "dependencies": {
    "react": "^19.2.7",
    "react-dom": "^19.2.7",
    "react-force-graph-2d": "^1.29.1"
  },
  "devDependencies": {
    "@testing-library/jest-dom": "^6.4.0",
    "@testing-library/react": "^16.1.0",
    "@types/node": "^22.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "jsdom": "^25.0.0",
    "typescript": "^5.4.0",
    "vite": "^6.4.3",
    "vitest": "^4.1.8"
  }
}
```

- [ ] **Step 2: Create `graph-viz/.gitignore`**

```
node_modules
dist
*.tsbuildinfo
```

- [ ] **Step 3: Create `graph-viz/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": ["node", "vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src", "vite.config.ts", "vitest.setup.ts", "scripts"]
}
```

- [ ] **Step 4: Create `graph-viz/scripts/sync-contract.mjs`**

```js
// Vendors ts-rs bindings from the engine contract into src/contract/.
// Run whenever the contract changes: `pnpm sync-contract`.
import { readdirSync, mkdirSync, copyFileSync, cpSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = resolve(here, "../../crates/cairn-contract/bindings");
const dst = resolve(here, "../src/contract");

if (!existsSync(src)) {
  console.error(`contract bindings not found at ${src}`);
  process.exit(1);
}
mkdirSync(dst, { recursive: true });
for (const entry of readdirSync(src, { withFileTypes: true })) {
  const from = resolve(src, entry.name);
  const to = resolve(dst, entry.name);
  if (entry.isDirectory()) cpSync(from, to, { recursive: true });
  else if (entry.name.endsWith(".ts")) copyFileSync(from, to);
}
console.log(`synced contract bindings → ${dst}`);
```

- [ ] **Step 5: Run the sync script to vendor bindings**

Run: `cd graph-viz && pnpm install && node scripts/sync-contract.mjs`
Expected: `synced contract bindings → .../src/contract`; `src/contract/Query.ts`, `QueryResponse.ts`, `GraphNode.ts`, `GraphEdge.ts`, `GraphScope.ts`, `Revision.ts` exist.

- [ ] **Step 6: Create `graph-viz/vite.config.ts` (throwaway shell + token-injecting proxy)**

```ts
// defineConfig from "vitest/config" (not "vite") so the `test` key typechecks.
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { homedir } from "node:os";

// Vault whose daemon we talk to. Override with CAIRN_VAULT; daemon addr with CAIRN_DAEMON.
const VAULT = process.env.CAIRN_VAULT ?? resolve(homedir(), "notes");
const DAEMON = process.env.CAIRN_DAEMON ?? "http://127.0.0.1:7777";

function readToken(): string | null {
  try {
    return readFileSync(resolve(VAULT, ".cairn/token"), "utf8").trim();
  } catch {
    return null; // daemon not started / no token yet
  }
}

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/query": {
        target: DAEMON,
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on("proxyReq", (proxyReq) => {
            const token = readToken(); // re-read per request: survives daemon restart
            if (token) proxyReq.setHeader("authorization", `Bearer ${token}`);
          });
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./vitest.setup.ts"],
  },
});
```

- [ ] **Step 7: Create `graph-viz/vitest.setup.ts`**

```ts
import "@testing-library/jest-dom/vitest";
```

- [ ] **Step 8: Create `graph-viz/index.html` (throwaway shell)**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>cairn graph-viz</title>
  </head>
  <body style="margin:0;background:#0f1017">
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 9: Verify the toolchain builds**

Run: `cd graph-viz && pnpm exec tsc -b`
Expected: zero errors. `vite.config.ts` and the vendored `src/contract/*` typecheck. `index.html`'s reference to `/src/main.tsx` is not part of the `tsc` program (Vite resolves it at bundle time; `main.tsx` arrives in Task 6), so it does not cause a type error here.

- [ ] **Step 10: Commit**

```bash
git add graph-viz/package.json graph-viz/pnpm-lock.yaml graph-viz/.gitignore graph-viz/tsconfig.json \
  graph-viz/vite.config.ts graph-viz/vitest.setup.ts graph-viz/index.html \
  graph-viz/scripts/sync-contract.mjs graph-viz/src/contract
git commit -m "feat(graph-viz): scaffold app shell, token-injecting proxy, vendored contract"
```

---

## Task 2: Typed daemon `/query` client

**Files:**
- Create: `graph-viz/src/client/daemonClient.ts`
- Test: `graph-viz/src/client/daemonClient.test.ts`

**Interfaces:**
- Consumes: vendored `Query`, `QueryResponse`, `GraphNode`, `GraphEdge`, `Revision` from `../contract/`.
- Produces:
  - `interface GraphPayload { nodes: GraphNode[]; edges: GraphEdge[] }`
  - `interface GraphDiffPayload { nodesAdded, nodesRemoved, nodesChanged: GraphNode[]; edgesAdded, edgesRemoved: GraphEdge[] }`
  - `class DaemonError extends Error { kind: string }`
  - `getGraph(): Promise<GraphPayload>`
  - `graphAt(revision: string): Promise<GraphPayload>`
  - `graphDiff(from: string, to: string): Promise<GraphDiffPayload>`
  - `vaultHistory(limit?: number): Promise<Revision[]>`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { getGraph, graphAt, graphDiff, vaultHistory, DaemonError } from "./daemonClient";

function mockFetch(status: number, body: unknown) {
  return vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  });
}

describe("daemonClient", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("getGraph posts get_graph with full scope and returns nodes/edges", async () => {
    const f = mockFetch(200, { type: "graph", nodes: [{ path: "a.md", title: "A", degree: 1, tags: [], mtime_secs: 1 }], edges: [{ from: "a.md", to: "b.md" }] });
    vi.stubGlobal("fetch", f);
    const g = await getGraph();
    expect(f).toHaveBeenCalledWith("/query", expect.objectContaining({ method: "POST" }));
    const sent = JSON.parse(f.mock.calls[0][1].body);
    expect(sent).toEqual({ type: "get_graph", scope: { type: "full" } });
    expect(g.nodes).toHaveLength(1);
    expect(g.edges[0]).toEqual({ from: "a.md", to: "b.md" });
  });

  it("graphAt posts graph_at with revision", async () => {
    const f = mockFetch(200, { type: "graph", nodes: [], edges: [] });
    vi.stubGlobal("fetch", f);
    await graphAt("HEAD~2");
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "graph_at", revision: "HEAD~2", scope: { type: "full" } });
  });

  it("graphDiff posts graph_diff with from/to and maps snake_case arms", async () => {
    const f = mockFetch(200, { type: "graph_diff", nodes_added: [1], nodes_removed: [2], nodes_changed: [3], edges_added: [4], edges_removed: [5] });
    vi.stubGlobal("fetch", f);
    const d = await graphDiff("old", "new");
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "graph_diff", from: "old", to: "new", scope: { type: "full" } });
    expect(d).toEqual({ nodesAdded: [1], nodesRemoved: [2], nodesChanged: [3], edgesAdded: [4], edgesRemoved: [5] });
  });

  it("vaultHistory posts vault_history and returns revisions", async () => {
    const f = mockFetch(200, { type: "history", revisions: [{ id: "a", message: "m", timestamp_secs: 1, author: "x" }] });
    vi.stubGlobal("fetch", f);
    const revs = await vaultHistory(50);
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({ type: "vault_history", limit: 50 });
    expect(revs).toHaveLength(1);
  });

  it("maps a non-2xx ContractError body to DaemonError", async () => {
    const f = mockFetch(400, { type: "invalid_request", message: "bad revspec" });
    vi.stubGlobal("fetch", f);
    await expect(graphAt("nope")).rejects.toMatchObject({ name: "DaemonError", kind: "invalid_request", message: "bad revspec" });
  });

  it("throws when the response arm is unexpected", async () => {
    const f = mockFetch(200, { type: "tags", tags: [] });
    vi.stubGlobal("fetch", f);
    await expect(getGraph()).rejects.toBeInstanceOf(DaemonError);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd graph-viz && pnpm exec vitest run src/client/daemonClient.test.ts`
Expected: FAIL — cannot find module `./daemonClient`.

- [ ] **Step 3: Write minimal implementation**

```ts
import type { Query } from "../contract/Query";
import type { QueryResponse } from "../contract/QueryResponse";
import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";
import type { Revision } from "../contract/Revision";

export interface GraphPayload {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface GraphDiffPayload {
  nodesAdded: GraphNode[];
  nodesRemoved: GraphNode[];
  nodesChanged: GraphNode[];
  edgesAdded: GraphEdge[];
  edgesRemoved: GraphEdge[];
}

export class DaemonError extends Error {
  constructor(
    public kind: string,
    message: string,
  ) {
    super(message);
    this.name = "DaemonError";
  }
}

async function runQuery(q: Query): Promise<QueryResponse> {
  const res = await fetch("/query", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(q),
  });
  const body = (await res.json()) as Record<string, unknown>;
  if (!res.ok) {
    const message = (body.message ?? body.what ?? "request failed") as string;
    throw new DaemonError((body.type as string) ?? "internal", message);
  }
  return body as QueryResponse;
}

function expectArm<T extends QueryResponse["type"]>(
  r: QueryResponse,
  arm: T,
): Extract<QueryResponse, { type: T }> {
  if (r.type !== arm) {
    throw new DaemonError("internal", `expected "${arm}" response, got "${r.type}"`);
  }
  return r as Extract<QueryResponse, { type: T }>;
}

export async function getGraph(): Promise<GraphPayload> {
  const r = expectArm(await runQuery({ type: "get_graph", scope: { type: "full" } }), "graph");
  return { nodes: r.nodes, edges: r.edges };
}

export async function graphAt(revision: string): Promise<GraphPayload> {
  const r = expectArm(await runQuery({ type: "graph_at", revision, scope: { type: "full" } }), "graph");
  return { nodes: r.nodes, edges: r.edges };
}

export async function graphDiff(from: string, to: string): Promise<GraphDiffPayload> {
  const r = expectArm(await runQuery({ type: "graph_diff", from, to, scope: { type: "full" } }), "graph_diff");
  return {
    nodesAdded: r.nodes_added,
    nodesRemoved: r.nodes_removed,
    nodesChanged: r.nodes_changed,
    edgesAdded: r.edges_added,
    edgesRemoved: r.edges_removed,
  };
}

export async function vaultHistory(limit?: number): Promise<Revision[]> {
  const r = expectArm(await runQuery({ type: "vault_history", limit: limit ?? null }), "history");
  return r.revisions;
}
```

> Note: if the vendored `Query` type declares `vault_history`'s `limit` as `number` (not `number | null`), pass `limit ?? undefined` and omit the field when undefined. Check `src/contract/Query.ts` and match its exact type — do not fight the generated binding.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd graph-viz && pnpm exec vitest run src/client/daemonClient.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add graph-viz/src/client/daemonClient.ts graph-viz/src/client/daemonClient.test.ts
git commit -m "feat(graph-viz): typed daemon /query client for graph modes"
```

---

## Task 3: Graph data build + layout persistence

**Files:**
- Create: `graph-viz/src/graph/graphData.ts`
- Test: `graph-viz/src/graph/graphData.test.ts`

**Interfaces:**
- Consumes: `GraphNode`, `GraphEdge` from `../contract/`.
- Produces:
  - `interface RFNode { id: string; label: string; degree: number; tags: string[]; mtime: number; x?: number; y?: number; fx?: number; fy?: number }`
  - `interface RFLink { source: string; target: string }`
  - `interface RFGraph { nodes: RFNode[]; links: RFLink[] }`
  - `nodeRadius(degree: number): number`
  - `labelAlpha(zoom: number): number`
  - `buildGraphData(nodes: GraphNode[], edges: GraphEdge[], prev?: Map<string, RFNode>): { graph: RFGraph; index: Map<string, RFNode> }`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { buildGraphData, nodeRadius, labelAlpha, type RFNode } from "./graphData";
import type { GraphNode } from "../contract/GraphNode";

const node = (path: string, over: Partial<GraphNode> = {}): GraphNode => ({
  path, title: "", degree: 0, tags: [], mtime_secs: 0, ...over,
});

describe("nodeRadius", () => {
  it("is sublinear in degree", () => {
    expect(nodeRadius(0)).toBeCloseTo(3);
    expect(nodeRadius(4)).toBeCloseTo(3 + 1.6 * 2);
  });
});

describe("labelAlpha", () => {
  it("ramps 0->1 between zoom 1.2 and 2.5", () => {
    expect(labelAlpha(1.0)).toBe(0);
    expect(labelAlpha(2.5)).toBe(1);
    expect(labelAlpha(1.85)).toBeCloseTo(0.5, 1);
  });
});

describe("buildGraphData", () => {
  it("derives label from title, falls back to stem, carries enriched fields", () => {
    const { graph } = buildGraphData(
      [node("dir/a.md", { title: "Alpha", degree: 3, tags: ["x"], mtime_secs: 99 }), node("dir/b.md")],
      [],
    );
    expect(graph.nodes[0]).toMatchObject({ id: "dir/a.md", label: "Alpha", degree: 3, tags: ["x"], mtime: 99 });
    expect(graph.nodes[1].label).toBe("b"); // stem of dir/b.md
  });

  it("drops edges whose endpoints are missing", () => {
    const { graph } = buildGraphData([node("a.md"), node("b.md")], [
      { from: "a.md", to: "b.md" },
      { from: "a.md", to: "ghost.md" },
    ]);
    expect(graph.links).toEqual([{ source: "a.md", target: "b.md" }]);
  });

  it("reuses node object identity across payloads so x/y persist", () => {
    const first = buildGraphData([node("a.md")], []);
    first.index.get("a.md")!.x = 42; // simulate react-force-graph placing the node
    const second = buildGraphData([node("a.md", { degree: 5 })], [], first.index);
    const reused = second.graph.nodes[0];
    expect(reused).toBe(first.index.get("a.md")); // same object identity
    expect(reused.x).toBe(42); // position preserved
    expect(reused.degree).toBe(5); // fields refreshed
  });

  it("drops nodes absent from the new payload", () => {
    const first = buildGraphData([node("a.md"), node("b.md")], []);
    const second = buildGraphData([node("a.md")], [], first.index);
    expect(second.graph.nodes.map((n) => n.id)).toEqual(["a.md"]);
    expect(second.index.has("b.md")).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd graph-viz && pnpm exec vitest run src/graph/graphData.test.ts`
Expected: FAIL — cannot find module `./graphData`.

- [ ] **Step 3: Write minimal implementation**

```ts
import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";

export interface RFNode {
  id: string;
  label: string;
  degree: number;
  tags: string[];
  mtime: number;
  // Mutated in place by react-force-graph-2d; preserved via object reuse.
  x?: number;
  y?: number;
  fx?: number;
  fy?: number;
}

export interface RFLink {
  source: string;
  target: string;
}

export interface RFGraph {
  nodes: RFNode[];
  links: RFLink[];
}

// Sublinear node sizing (ported from cairn-ui graphData.ts).
export function nodeRadius(degree: number): number {
  return 3 + 1.6 * Math.sqrt(degree);
}

// Zoom-adaptive label opacity (ported from cairn-ui graphData.ts).
export function labelAlpha(zoom: number): number {
  const lo = 1.2;
  const hi = 2.5;
  if (zoom <= lo) return 0;
  if (zoom >= hi) return 1;
  return (zoom - lo) / (hi - lo);
}

function stem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.[^.]+$/, "");
}

// Build react-force-graph data. When `prev` is supplied, reuse node objects by
// path so react-force-graph keeps x/y/vx/vy — the graph morphs instead of
// re-exploding when scrubbing/diffing.
export function buildGraphData(
  nodes: GraphNode[],
  edges: GraphEdge[],
  prev?: Map<string, RFNode>,
): { graph: RFGraph; index: Map<string, RFNode> } {
  const index = new Map<string, RFNode>();
  const out: RFNode[] = [];
  for (const n of nodes) {
    const node: RFNode = prev?.get(n.path) ?? { id: n.path, label: "", degree: 0, tags: [], mtime: 0 };
    node.label = n.title || stem(n.path);
    node.degree = n.degree;
    node.tags = n.tags;
    node.mtime = n.mtime_secs;
    index.set(n.path, node);
    out.push(node);
  }
  const links: RFLink[] = edges
    .filter((e) => index.has(e.from) && index.has(e.to))
    .map((e) => ({ source: e.from, target: e.to }));
  return { graph: { nodes: out, links }, index };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd graph-viz && pnpm exec vitest run src/graph/graphData.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add graph-viz/src/graph/graphData.ts graph-viz/src/graph/graphData.test.ts
git commit -m "feat(graph-viz): graph data build with layout-preserving node reuse"
```

---

## Task 4: Diff classification + palette

**Files:**
- Create: `graph-viz/src/graph/diffStyle.ts`
- Test: `graph-viz/src/graph/diffStyle.test.ts`

**Interfaces:**
- Consumes: `GraphPayload`, `GraphDiffPayload` from `../client/daemonClient`; `GraphNode`, `GraphEdge` from `../contract/`.
- Produces:
  - `type DiffClass = "added" | "removed" | "changed" | "unchanged"`
  - `const DIFF_COLORS: Record<DiffClass, string>`
  - `interface DiffGraph { nodes: (GraphNode & { diff: DiffClass })[]; edges: (GraphEdge & { diff: DiffClass })[] }`
  - `buildDiffGraph(base: GraphPayload, diff: GraphDiffPayload): DiffGraph`

Diff mode fetches `graphAt(to)` as the **base** (the graph as it exists at the newer revision) and `graphDiff(from, to)` for the change classification, then overlays. Removed nodes/edges (absent from `base`) are re-added as ghosts so the viewer sees what left.

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { buildDiffGraph, DIFF_COLORS } from "./diffStyle";
import type { GraphNode } from "../contract/GraphNode";

const n = (path: string): GraphNode => ({ path, title: path, degree: 0, tags: [], mtime_secs: 0 });

describe("buildDiffGraph", () => {
  it("classifies added, changed, unchanged from the base, and re-adds removed as ghosts", () => {
    const base = {
      nodes: [n("keep.md"), n("added.md"), n("changed.md")],
      edges: [{ from: "keep.md", to: "changed.md" }],
    };
    const diff = {
      nodesAdded: [n("added.md")],
      nodesChanged: [n("changed.md")],
      nodesRemoved: [n("gone.md")],
      edgesAdded: [{ from: "keep.md", to: "added.md" }],
      edgesRemoved: [{ from: "keep.md", to: "gone.md" }],
    };
    const g = buildDiffGraph(base, diff);
    const cls = Object.fromEntries(g.nodes.map((x) => [x.path, x.diff]));
    expect(cls).toEqual({ "keep.md": "unchanged", "added.md": "added", "changed.md": "changed", "gone.md": "removed" });

    const ecls = g.edges.map((e) => `${e.from}->${e.to}:${e.diff}`);
    expect(ecls).toContain("keep.md->changed.md:unchanged");
    expect(ecls).toContain("keep.md->added.md:added");
    expect(ecls).toContain("keep.md->gone.md:removed");
  });

  it("exposes a color per class", () => {
    expect(DIFF_COLORS.added).toBeTruthy();
    expect(DIFF_COLORS.removed).toBeTruthy();
    expect(DIFF_COLORS.changed).toBeTruthy();
    expect(DIFF_COLORS.unchanged).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd graph-viz && pnpm exec vitest run src/graph/diffStyle.test.ts`
Expected: FAIL — cannot find module `./diffStyle`.

- [ ] **Step 3: Write minimal implementation**

```ts
import type { GraphNode } from "../contract/GraphNode";
import type { GraphEdge } from "../contract/GraphEdge";
import type { GraphPayload, GraphDiffPayload } from "../client/daemonClient";

export type DiffClass = "added" | "removed" | "changed" | "unchanged";

export const DIFF_COLORS: Record<DiffClass, string> = {
  added: "#22c55e", // green
  removed: "#ef4444", // red (painted ghosted via alpha in GraphView)
  changed: "#f59e0b", // amber
  unchanged: "#cdd0e0",
};

export interface DiffGraph {
  nodes: (GraphNode & { diff: DiffClass })[];
  edges: (GraphEdge & { diff: DiffClass })[];
}

const edgeKey = (e: GraphEdge) => `${e.from} ${e.to}`;

// Overlay diff classification on the base graph (base = graph at `to`).
// Removed nodes/edges are absent from base, so re-add them as ghosts.
export function buildDiffGraph(base: GraphPayload, diff: GraphDiffPayload): DiffGraph {
  const nodeClass = new Map<string, DiffClass>();
  for (const x of diff.nodesChanged) nodeClass.set(x.path, "changed");
  for (const x of diff.nodesAdded) nodeClass.set(x.path, "added");

  const nodes: (GraphNode & { diff: DiffClass })[] = base.nodes.map((x) => ({
    ...x,
    diff: nodeClass.get(x.path) ?? "unchanged",
  }));
  for (const x of diff.nodesRemoved) nodes.push({ ...x, diff: "removed" });

  const edgeClass = new Map<string, DiffClass>();
  for (const e of diff.edgesAdded) edgeClass.set(edgeKey(e), "added");

  const removedKeys = new Set(diff.edgesRemoved.map(edgeKey));
  const edges: (GraphEdge & { diff: DiffClass })[] = base.edges.map((e) => ({
    ...e,
    diff: edgeClass.get(edgeKey(e)) ?? "unchanged",
  }));
  for (const e of diff.edgesRemoved) {
    if (!removedKeys.has(edgeKey(e))) continue; // defensive; always true here
    edges.push({ ...e, diff: "removed" });
  }
  return { nodes, edges };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd graph-viz && pnpm exec vitest run src/graph/diffStyle.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add graph-viz/src/graph/diffStyle.ts graph-viz/src/graph/diffStyle.test.ts
git commit -m "feat(graph-viz): diff classification and palette over a base graph"
```

---

## Task 5: GraphView rendering surface

**Files:**
- Create: `graph-viz/src/graph/GraphView.tsx`
- Test: `graph-viz/src/graph/GraphView.test.tsx`

**Interfaces:**
- Consumes: `RFGraph`, `nodeRadius`, `labelAlpha` from `./graphData`; `DiffClass`, `DIFF_COLORS` from `./diffStyle`.
- Produces:
  - `interface GraphViewProps { graph: RFGraph; diffByPath?: Map<string, DiffClass> }`
  - `default export function GraphView(props: GraphViewProps)`

`react-force-graph-2d` renders to canvas (not unit-testable for pixels), so tests **mock the module** and assert `GraphView` forwards the right `graphData` and computes node color from diff class. Node radius/label painting are covered by Task 3 unit tests + the Task 7 manual E2E.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import type { RFGraph } from "./graphData";

// Mock react-force-graph-2d: capture props, expose the color fn for assertions.
const captured: { props?: any } = {};
vi.mock("react-force-graph-2d", () => ({
  default: (props: any) => {
    captured.props = props;
    return null;
  },
}));

import GraphView from "./GraphView";
import { DIFF_COLORS } from "./diffStyle";

const graph: RFGraph = {
  nodes: [
    { id: "a.md", label: "A", degree: 2, tags: [], mtime: 0 },
    { id: "b.md", label: "B", degree: 0, tags: [], mtime: 0 },
  ],
  links: [{ source: "a.md", target: "b.md" }],
};

describe("GraphView", () => {
  it("forwards graphData to the force graph and defaults node color without a diff", () => {
    render(<GraphView graph={graph} />);
    expect(captured.props.graphData.nodes).toHaveLength(2);
    expect(captured.props.graphData.links).toHaveLength(1);
    // No diffByPath → default (unchanged) color for every node.
    expect(captured.props.nodeColor(graph.nodes[0])).toBe(DIFF_COLORS.unchanged);
  });

  it("colors nodes by diff class when provided", () => {
    const diffByPath = new Map([["a.md", "added" as const]]);
    render(<GraphView graph={graph} diffByPath={diffByPath} />);
    const color = captured.props.nodeColor;
    expect(color(graph.nodes[0])).toBe(DIFF_COLORS.added);
    expect(color(graph.nodes[1])).toBe(DIFF_COLORS.unchanged);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd graph-viz && pnpm exec vitest run src/graph/GraphView.test.tsx`
Expected: FAIL — cannot find module `./GraphView`.

- [ ] **Step 3: Write minimal implementation**

```tsx
import { useMemo } from "react";
import ForceGraph2D from "react-force-graph-2d";
import { nodeRadius, labelAlpha, type RFGraph, type RFNode } from "./graphData";
import { DIFF_COLORS, type DiffClass } from "./diffStyle";

export interface GraphViewProps {
  graph: RFGraph;
  // When present, nodes are tinted by diff class (diff mode); absent = live/scrub.
  diffByPath?: Map<string, DiffClass>;
}

const DEFAULT_COLOR = DIFF_COLORS.unchanged; // single source of truth for the base tint

export default function GraphView({ graph, diffByPath }: GraphViewProps) {
  const nodeColor = useMemo(
    () => (node: RFNode) => {
      if (diffByPath) return DIFF_COLORS[diffByPath.get(node.id) ?? "unchanged"];
      return DEFAULT_COLOR;
    },
    [diffByPath],
  );

  return (
    <ForceGraph2D
      graphData={graph}
      backgroundColor="#0f1017"
      nodeColor={nodeColor}
      nodeRelSize={1}
      nodeVal={(n: RFNode) => Math.max(1, nodeRadius(n.degree))}
      linkColor={() => "rgba(205,208,224,0.25)"}
      nodeCanvasObjectMode={() => "after"}
      nodeCanvasObject={(node: RFNode, ctx, globalScale) => {
        const alpha = labelAlpha(globalScale);
        if (alpha <= 0) return;
        ctx.globalAlpha = alpha;
        ctx.fillStyle = "#e6e8f0";
        ctx.font = `${12 / globalScale}px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const r = nodeRadius(node.degree);
        ctx.fillText(node.label, node.x ?? 0, (node.y ?? 0) + r + 1);
        ctx.globalAlpha = 1;
      }}
    />
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd graph-viz && pnpm exec vitest run src/graph/GraphView.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add graph-viz/src/graph/GraphView.tsx graph-viz/src/graph/GraphView.test.tsx
git commit -m "feat(graph-viz): react-force-graph rendering surface with diff tinting"
```

---

## Task 6: App shell — mode switch, timeline, wiring

**Files:**
- Create: `graph-viz/src/main.tsx`, `graph-viz/src/App.tsx`
- Test: `graph-viz/src/App.test.tsx`

**Interfaces:**
- Consumes: `getGraph`, `graphAt`, `graphDiff`, `vaultHistory` from `./client/daemonClient`; `buildGraphData` from `./graph/graphData`; `buildDiffGraph` from `./graph/diffStyle`; `GraphView` from `./graph/GraphView`.
- Produces: the running app (throwaway shell). Three modes; scrub/diff fire on slider release.

`App.test.tsx` mocks `./client/daemonClient` and `./graph/GraphView` to assert wiring: mode switch renders the right controls, and slider release calls the right client fn exactly once with the right args.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("./graph/GraphView", () => ({ default: () => <div data-testid="graph" /> }));
vi.mock("./client/daemonClient", () => ({
  getGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  graphAt: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  graphDiff: vi.fn().mockResolvedValue({ nodesAdded: [], nodesRemoved: [], nodesChanged: [], edgesAdded: [], edgesRemoved: [] }),
  vaultHistory: vi.fn().mockResolvedValue([
    { id: "newest", message: "c3", timestamp_secs: 3, author: "x" },
    { id: "mid", message: "c2", timestamp_secs: 2, author: "x" },
    { id: "oldest", message: "c1", timestamp_secs: 1, author: "x" },
  ]),
  DaemonError: class extends Error {},
}));

import App from "./App";
import * as client from "./client/daemonClient";

describe("App", () => {
  beforeEach(() => vi.clearAllMocks());

  it("loads the live graph on mount", async () => {
    render(<App />);
    await waitFor(() => expect(client.getGraph).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("graph")).toBeInTheDocument();
  });

  it("scrub mode fires graphAt with the selected revision on release", async () => {
    render(<App />);
    await waitFor(() => expect(client.vaultHistory).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /scrub/i }));
    const slider = await screen.findByRole("slider");
    // index 0 = oldest ... last = newest (HEAD). Pick middle revision.
    fireEvent.change(slider, { target: { value: "1" } });
    fireEvent.mouseUp(slider);
    await waitFor(() => expect(client.graphAt).toHaveBeenCalledWith("mid"));
    expect(client.graphAt).toHaveBeenCalledTimes(1);
  });

  it("diff mode fires graphDiff(from,to) ordered oldest→newest on release", async () => {
    render(<App />);
    await waitFor(() => expect(client.vaultHistory).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /diff/i }));
    const [fromSlider, toSlider] = await screen.findAllByRole("slider");
    fireEvent.change(fromSlider, { target: { value: "0" } }); // oldest
    fireEvent.change(toSlider, { target: { value: "2" } }); // newest
    fireEvent.mouseUp(toSlider);
    await waitFor(() => expect(client.graphDiff).toHaveBeenCalledWith("oldest", "newest"));
  });

  it("switching back to Live re-fetches getGraph", async () => {
    render(<App />);
    await waitFor(() => expect(client.getGraph).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: /diff/i }));
    fireEvent.click(screen.getByRole("button", { name: /live/i }));
    // Live re-fetches the HEAD graph (and clears any diff overlay).
    await waitFor(() => expect(client.getGraph).toHaveBeenCalledTimes(2));
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd graph-viz && pnpm exec vitest run src/App.test.tsx`
Expected: FAIL — cannot find module `./App`.

- [ ] **Step 3: Write `src/main.tsx`**

```tsx
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
```

- [ ] **Step 4: Write `src/App.tsx`**

Revisions come back newest-first; the timeline is indexed oldest→newest (left→right), so reverse into `timeline` where `timeline[timeline.length-1]` is HEAD.

```tsx
import { useCallback, useEffect, useRef, useState } from "react";
import type { Revision } from "./contract/Revision";
import type { GraphNode } from "./contract/GraphNode";
import type { GraphEdge } from "./contract/GraphEdge";
import { getGraph, graphAt, graphDiff, vaultHistory, DaemonError } from "./client/daemonClient";
import { buildGraphData, type RFGraph, type RFNode } from "./graph/graphData";
import { buildDiffGraph, type DiffClass } from "./graph/diffStyle";
import GraphView from "./graph/GraphView";

type Mode = "live" | "scrub" | "diff";

export default function App() {
  const [mode, setMode] = useState<Mode>("live");
  const [timeline, setTimeline] = useState<Revision[]>([]); // oldest → newest (HEAD last)
  const [graph, setGraph] = useState<RFGraph>({ nodes: [], links: [] });
  const [diffByPath, setDiffByPath] = useState<Map<string, DiffClass> | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const indexRef = useRef<Map<string, RFNode>>(new Map());
  const genRef = useRef(0); // request generation — guards against stale async overwrites

  const [scrubIdx, setScrubIdx] = useState(0);
  const [fromIdx, setFromIdx] = useState(0);
  const [toIdx, setToIdx] = useState(0);

  const apply = useCallback((nodes: GraphNode[], edges: GraphEdge[]) => {
    const { graph: g, index } = buildGraphData(nodes, edges, indexRef.current);
    indexRef.current = index;
    setGraph(g);
  }, []);

  // Wraps an async op with the loading/error lifecycle and a staleness guard:
  // each run bumps a generation counter; only the most recently started run may
  // mutate graph/error/loading state, so a slow response can't clobber a newer one.
  const run = useCallback(async (fn: (isStale: () => boolean) => Promise<void>) => {
    const myGen = ++genRef.current;
    const isStale = () => genRef.current !== myGen;
    setLoading(true);
    setError(null);
    try {
      await fn(isStale);
    } catch (e) {
      if (isStale()) return;
      setError(e instanceof DaemonError ? e.message : "Can't reach daemon — is cairn-daemon running and CAIRN_VAULT set?");
    } finally {
      if (!isStale()) setLoading(false);
    }
  }, []);

  // Initial: live graph + timeline.
  useEffect(() => {
    void run(async (isStale) => {
      const g = await getGraph();
      if (isStale()) return;
      apply(g.nodes, g.edges);
      const revs = await vaultHistory();
      if (isStale()) return;
      const tl = [...revs].reverse(); // oldest → newest
      setTimeline(tl);
      const last = Math.max(0, tl.length - 1);
      setScrubIdx(last);
      setFromIdx(0);
      setToIdx(last);
    });
  }, [run, apply]);

  const onScrubRelease = () =>
    run(async (isStale) => {
      const rev = timeline[scrubIdx];
      if (!rev) return;
      const g = await graphAt(rev.id);
      if (isStale()) return;
      setDiffByPath(undefined);
      apply(g.nodes, g.edges);
    });

  const onDiffRelease = () =>
    run(async (isStale) => {
      // Order the handles so `from` is always older than `to` (oldest → newest),
      // even if the user dragged `from` past `to`.
      const lo = Math.min(fromIdx, toIdx);
      const hi = Math.max(fromIdx, toIdx);
      const from = timeline[lo];
      const to = timeline[hi];
      if (!from || !to) return;
      const [base, diff] = await Promise.all([graphAt(to.id), graphDiff(from.id, to.id)]);
      if (isStale()) return;
      const dg = buildDiffGraph(base, diff);
      setDiffByPath(new Map(dg.nodes.map((n) => [n.path, n.diff])));
      apply(dg.nodes, dg.edges);
    });

  const switchMode = (m: Mode) => {
    setMode(m);
    if (m === "live") {
      setDiffByPath(undefined);
      void run(async (isStale) => {
        const g = await getGraph();
        if (isStale()) return;
        apply(g.nodes, g.edges);
      });
    }
  };

  const label = (r?: Revision) => (r ? `${r.id.slice(0, 7)} · ${new Date(r.timestamp_secs * 1000).toISOString().slice(0, 10)} · ${r.message}` : "");
  const maxIdx = Math.max(0, timeline.length - 1);

  return (
    <div style={{ position: "fixed", inset: 0, color: "#e6e8f0", fontFamily: "sans-serif" }}>
      <div style={{ position: "absolute", zIndex: 1, top: 12, left: 12, right: 12, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <button onClick={() => switchMode("live")} aria-pressed={mode === "live"}>Live</button>
        <button onClick={() => switchMode("scrub")} aria-pressed={mode === "scrub"}>Scrub</button>
        <button onClick={() => switchMode("diff")} aria-pressed={mode === "diff"}>Diff</button>
        {loading && <span>loading…</span>}
        {error && <span style={{ color: "#ef4444" }}>{error}</span>}

        {mode === "scrub" && timeline.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
            <input type="range" min={0} max={maxIdx} value={scrubIdx} onChange={(e) => setScrubIdx(Number(e.target.value))} onMouseUp={onScrubRelease} onTouchEnd={onScrubRelease} />
            <small>{label(timeline[scrubIdx])}</small>
          </div>
        )}

        {mode === "diff" && timeline.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
            <label>from <input type="range" min={0} max={maxIdx} value={fromIdx} onChange={(e) => setFromIdx(Number(e.target.value))} onMouseUp={onDiffRelease} onTouchEnd={onDiffRelease} /></label>
            <label>to <input type="range" min={0} max={maxIdx} value={toIdx} onChange={(e) => setToIdx(Number(e.target.value))} onMouseUp={onDiffRelease} onTouchEnd={onDiffRelease} /></label>
            <small>{label(timeline[fromIdx])} → {label(timeline[toIdx])}</small>
          </div>
        )}
      </div>
      <GraphView graph={graph} diffByPath={diffByPath} />
    </div>
  );
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd graph-viz && pnpm exec vitest run src/App.test.tsx`
Expected: PASS (4 tests). If the diff test flakes because both sliders share a release handler, confirm the test sets `fromIdx` then `toIdx` before `mouseUp`; state updates are batched and both are read at release.

- [ ] **Step 6: Run the whole suite + typecheck**

Run: `cd graph-viz && pnpm exec vitest run && pnpm exec tsc -b`
Expected: all tests PASS; no type errors.

- [ ] **Step 7: Commit**

```bash
git add graph-viz/src/main.tsx graph-viz/src/App.tsx graph-viz/src/App.test.tsx
git commit -m "feat(graph-viz): app shell with live/scrub/diff modes over shared timeline"
```

---

## Task 7: README, manual end-to-end DoD verification, rebase

**Files:**
- Create: `graph-viz/README.md`
- Modify: (none — verification + rebase)

**Interfaces:**
- Consumes: everything above.
- Produces: documented run instructions; confirmed DoD.

- [ ] **Step 1: Write `graph-viz/README.md`**

````markdown
# cairn graph-viz (standalone, B1)

Standalone browser graph-viz targeting a running `cairn-daemon` on current-main.
Folds into `cairn-web-ui` at roadmap item D1 — see
`docs/superpowers/specs/2026-08-08-standalone-graph-viz-design.md`.

## Run

1. Start the daemon against a real vault:

   ```bash
   cargo run -p cairn-daemon -- --cairn /path/to/vault --port 7777
   ```

2. Point the app at that vault (so the Vite proxy can read `.cairn/token`) and start it:

   ```bash
   cd graph-viz
   pnpm install
   CAIRN_VAULT=/path/to/vault pnpm dev
   ```

   Optional: `CAIRN_DAEMON=http://127.0.0.1:7777` (default). Open the printed URL.

The Vite proxy reads `<vault>/.cairn/token` per request and injects the bearer
token, so the browser talks same-origin — no CORS, token never in browser JS.

## Modes
- **Live** — full HEAD graph.
- **Scrub** — slide over vault history; releases fire `graph_at`.
- **Diff** — pick from→to; releases fire `graph_diff`; added=green, removed=ghost-red, changed=amber.

## Contract bindings
`pnpm sync-contract` re-vendors `../crates/cairn-contract/bindings/*.ts` into `src/contract/`.
````

- [ ] **Step 2: Rebase the branch onto latest main**

Per the spec's pre-implementation note, the branch was behind main. Rebase so CI runs against current main.

Run:
```bash
git fetch origin
git rebase origin/main
```
Expected: clean rebase (graph-viz/ is new, no conflicts). If conflicts appear in unrelated files, stop and resolve per the conflict.

- [ ] **Step 3: Manual end-to-end (DoD gate)**

Start the daemon and app per the README against a vault with git history and links. Confirm, in the browser:
1. **Live** — a graph renders with nodes sized by degree and labels appearing on zoom-in.
2. **Scrub** — moving the slider and releasing changes the graph; nodes present in both revisions keep their position (morph, not re-explode).
3. **Diff** — choosing two revisions and releasing highlights added (green), removed (ghost-red), changed (amber) nodes/edges.

Record the result (pass/fail per item) in the PR description. This is the DoD; the react-force-graph canvas has no unit coverage by design.

- [ ] **Step 4: Commit README**

```bash
git add graph-viz/README.md
git commit -m "docs(graph-viz): run instructions and DoD notes"
```

- [ ] **Step 5: Open the PR**

```bash
gh pr create --base main --title "feat(graph-viz): standalone temporal graph-viz app (B1)" \
  --body "Standalone graph-viz consuming get_graph / graph_at / graph_diff via the daemon /query proxy. First consumer of the temporal-graph contract; no contract gaps found. DoD (live/scrub/diff) verified manually — see checklist. Folds into cairn-web-ui at D1. Spec: docs/superpowers/specs/2026-08-08-standalone-graph-viz-design.md"
```

Per repo convention (merge queue enabled), enable auto-merge rather than manually updating the branch.

---

## Self-Review

**Spec coverage:**
- Purpose / 3 modes → Tasks 2 (client), 6 (modes). ✓
- Placement `graph-viz/`, Vite/React/RFG/pnpm → Task 1. ✓
- Portable-vs-throwaway split → File Structure + Task interfaces (client/graph portable; shell in 1/6). ✓
- Vite-proxy token injection, no CORS → Task 1 Step 6. ✓
- Vendor bindings from this repo → Task 1 Steps 4–5. ✓
- `daemonClient` surface (getGraph/graphAt/graphDiff/vaultHistory) → Task 2. ✓
- Node sizing, labelAlpha, node-identity reuse (morph not explode) → Task 3. ✓
- Diff union-with-tint (base=graph_at(to)) → Task 4 + Task 6 onDiffRelease. ✓
- Fire-on-release debounce → Task 6 (onMouseUp/onTouchEnd, no onChange fetch). ✓
- Error banners → Task 6 `run()` + error state. ✓
- Testing: unit (client/graphData/diffStyle) + component (GraphView/App) + manual E2E → Tasks 2–7. ✓
- B0 already done / rebase-before-PR → Task 7 Step 2. ✓
- Contract-gap surfacing → done during design (none found); noted in PR body Task 7 Step 5. ✓

**Placeholder scan:** No TBD/TODO; all code steps carry real code. The one conditional (Task 2 Step 3 note on `limit` nullability) instructs matching the generated binding exactly — actionable, not a placeholder.

**Type consistency:** `GraphPayload`/`GraphDiffPayload` defined in Task 2 and consumed by Task 4/6; `RFNode`/`RFGraph`/`buildGraphData` defined Task 3, consumed Task 5/6; `DiffClass`/`DIFF_COLORS`/`buildDiffGraph` defined Task 4, consumed Task 5/6; `GraphView` props defined Task 5, consumed Task 6. Names align across tasks. ✓
