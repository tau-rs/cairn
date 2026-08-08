# Standalone Graph-Viz App — Design (B1)

**Date:** 2026-08-08
**Branch:** `standalone-graph-viz-app` (engine repo `tau-rs/cairn`)
**Roadmap item:** Epic B / B1 — see `docs/superpowers/plans/2026-07-24-cairn-full-roadmap.md`.
**Status:** Approved design, ready for implementation plan.

## Purpose

Build a **standalone** browser graph-visualization app that renders a live link
graph from a running `cairn-daemon` and is the **first real consumer** of the
frozen temporal-graph contract:

- `get_graph` — live HEAD graph.
- `graph_at { revision }` — time-scrubbing across history.
- `graph_diff { from, to }` — change-highlighting between two revisions.

It targets **current main** via the daemon's HTTP `/query`, deliberately *not*
built inside `cairn-web-ui` (that app is pinned ~99 commits behind at engine rev
`079f9f9` and predates GraphAt/GraphDiff/enriched-GraphNode — an in-app view
would be blocked on the D1 migration). Standalone lets B1 run as a parallel
Wave-1 stream. The graph logic is kept in **portable modules** so it folds into
`cairn-web-ui` during/after D1; only the app shell is throwaway.

### Definition of Done

The standalone app renders a live graph from a running daemon; **time-scrub via
`graph_at`** and a **diff view via `graph_diff`** both work end-to-end against
current main.

### Non-goals (YAGNI for v1)

Color-groups editor, force-settings panel, local/global subgraph toggle,
click-to-open-note, search, note rendering, plugins, in-app vault switching, and
`GraphScope::Focused` neighborhoods. These re-integrate at D1 or are out of
scope. v1 ships the reusable rendering core + the three data modes.

## Context (verified against current main)

- **Contract bindings are current and complete.** `crates/cairn-contract/bindings/`
  already exports `Query.ts` (with `graph_at {revision, scope}` and
  `graph_diff {from, to, scope}`), `QueryResponse.ts` (`graph` arm =
  `nodes: Array<GraphNode>` enriched; `graph_diff` arm =
  `nodes_added/removed/changed`, `edges_added/removed`), plus `GraphNode.ts`
  (`{path, title, degree, tags, mtime_secs: number}`), `GraphEdge.ts`,
  `GraphScope.ts`, `Revision.ts`. B0 (mtime_secs → TS `number`, #134) has
  landed. **No contract gap found** — this design surfaced none.
- **B0 status:** already merged (`58a629d`, issue #110 closed). No work required.
- **Vendor bindings from THIS repo**, not cairn-ui's stale vendored copy.

## Architecture

### Placement & tooling

New top-level `graph-viz/` directory committed to this repo. Stack mirrors
cairn-ui so ports are drop-in: **Vite 6 + React 19 + TypeScript 5 +
`react-force-graph-2d` + pnpm**.

```
graph-viz/                       # committed here; folds into cairn-web-ui at D1
├── index.html                   ← THROWAWAY shell
├── vite.config.ts               ← THROWAWAY shell (build + /query proxy)
├── package.json, tsconfig.json  ← THROWAWAY shell
├── scripts/sync-contract.ts     ← copies ../crates/cairn-contract/bindings/*.ts → src/contract/
└── src/
    ├── main.tsx                 ← THROWAWAY shell
    ├── App.tsx                  ← THROWAWAY shell (mode switch + timeline chrome)
    ├── contract/                ← vendored ts-rs bindings (generated; do not edit)
    ├── client/
    │   └── daemonClient.ts      ← PORTABLE: typed HTTP /query client
    └── graph/                   ← PORTABLE (ports from cairn-ui, adapted to GraphNode)
        ├── GraphView.tsx        (react-force-graph-2d rendering surface)
        ├── graphData.ts         (build RF nodes/links; nodeRadius; labelAlpha)
        └── diffStyle.ts         (NEW: added/removed/changed classification + tint)
```

**Portable vs throwaway boundary.** `src/client/` and `src/graph/` are the
foldable modules — they operate on the contract types and a transport interface,
with no dependency on the app shell. `index.html`, `vite.config.ts`,
`package.json`, `main.tsx`, and `App.tsx` are throwaway (their responsibilities —
window, build, proxy, routing — are provided by cairn-web-ui at D1).

### Transport (token injection via Vite proxy)

The daemon requires `Authorization: Bearer <token>` where the token lives in
`<vault>/.cairn/token` (mode 0600) and is regenerated each startup. A pure
browser cannot read that file. Solution: the **Vite dev-server proxy** reads the
token file at request time and injects the header. The browser talks
**same-origin** to Vite, so there is **no CORS** and the token never enters
browser JS.

```
browser ──POST /query──▶ Vite proxy (reads <vault>/.cairn/token,
                          adds Authorization: Bearer) ──▶ daemon 127.0.0.1:7777
                                                          ──▶ QueryResponse JSON
```

- Proxy config in `vite.config.ts`: `server.proxy['/query']` → target
  `http://127.0.0.1:7777`, with a `configure` hook that reads the token file on
  each proxied request (re-read so a daemon restart / new token is picked up) and
  sets the `Authorization` header on the proxied request.
- Vault/token path resolved from an env var (e.g. `CAIRN_VAULT`) with a sensible
  default; documented in `graph-viz/README.md`.
- The daemon does not need `--cors-origin` in this setup (traffic is same-origin
  through the proxy). It is only needed if someone points the browser directly at
  `:7777`, which we do not do.

### Client module (`daemonClient.ts`)

A thin typed wrapper over `fetch('/query', {method:'POST', body: JSON})` returning
the parsed `QueryResponse`, importing the vendored `Query`/`QueryResponse` types.
Surface:

```ts
getGraph(): Promise<GraphResponse>                       // { type:"get_graph" }
graphAt(revision: string): Promise<GraphResponse>        // { type:"graph_at", revision, scope:{type:"full"} }
graphDiff(from: string, to: string): Promise<GraphDiffResponse>
vaultHistory(limit?: number): Promise<Revision[]>        // { type:"vault_history", limit }
```

Errors: non-2xx responses carry a `ContractError` body (`not_found` /
`invalid_request` / `internal`); the client throws a typed error the UI renders
as a small banner.

## Data flow & modes

Three modes over one shared revision timeline (`vaultHistory()`, newest-first).

1. **Live** — `getGraph()`, no timeline controls. Default on load.
2. **Scrub** — single slider handle over the timeline; on **release** (debounced,
   not on-drag — `graph_at` walks git history per call) fire
   `graphAt(revision.id)`. A "loading…" pill shows during the fetch. Handle label
   shows `id · date · message`.
3. **Diff** — two handles (`from` older → `to` newer) over the same timeline; on
   release fire `graphDiff(from.id, to.id)`. Renders the union graph tinted:
   **added = green**, **removed = ghosted red**, **changed = amber**, unchanged =
   default. Built from the `nodes_added/removed/changed` + `edges_added/removed`
   response arms.

### Rendering core (`GraphView.tsx` + `graphData.ts`)

Ported from cairn-ui, adapted from string-path nodes to enriched `GraphNode`:

- Data build: `GraphNode[] + GraphEdge[]` → RF `{nodes, links}`; links filtered
  to edges whose endpoints both exist.
- Node sizing: `nodeRadius(degree) = 3 + 1.6*sqrt(degree)` (sublinear, from
  cairn-ui). Now driven by the enriched `degree` field directly.
- Node tint: fixed palette keyed off `tags` (first tag → stable color); nodes
  with no tags use the default. (No user color-groups editor in v1.)
- Zoom-adaptive label opacity (`labelAlpha`, from cairn-ui) and hover-neighbor
  dimming — both cheap, high-readability; kept.
- Fixed sensible d3 forces (no settings panel).

### Layout persistence across scrub (critical)

react-force-graph mutates node objects with `x/y/vx/vy` and keeps them stable
across renders **as long as the same object identity is reused per node id**. The
data layer maintains a `Map<path, RFNode>` and, on each new graph payload
(scrub/diff frame), **reuses existing node objects by `path`**, only adding new
ones and dropping absent ones. Result: the graph **morphs** as you scrub instead
of re-exploding every frame — the single biggest readability win for time-scrub.

## Error handling

- Daemon unreachable / proxy can't read token → banner: "Can't reach daemon —
  is `cairn-daemon` running and `CAIRN_VAULT` set?" with the resolved vault path.
- `ContractError` from `/query` → banner with the error `message`/`what`.
- Empty graph (new vault) → empty-state message, not a crash.
- Invalid revspec in scrub/diff → surfaced as the `invalid_request` banner.

## Testing

- **Unit (vitest):** `graphData.ts` (node/link build, degree sizing, dangling-edge
  filtering, node-identity reuse across successive payloads), `diffStyle.ts`
  (added/removed/changed classification from a `graph_diff` payload),
  `daemonClient.ts` (request shape per mode; error mapping) against a mocked
  `fetch`.
- **Component (vitest + Testing Library):** mode switch renders the right
  controls; scrub-release calls `graphAt` once (debounce); diff-release calls
  `graphDiff` with ordered from/to.
- **End-to-end (manual, DoD gate):** run `cargo run -p cairn-daemon -- --cairn
  <vault>`; `pnpm dev` in `graph-viz/`; confirm live graph renders, scrubbing
  changes the graph via `graph_at`, and diff highlights changes via `graph_diff`.
  The react-force-graph canvas itself is not unit-tested (canvas/WebGL); it is
  covered by this manual E2E pass. Stated explicitly per "tests are part of done."

## Pre-implementation note

This branch (`82c60ed`) is 4 commits behind `origin/main` (`4f48cec`). Rebase
onto latest main before opening the B1 PR so CI runs against current main. The B0
fix is already in this checkout.

## Foldability summary (D1 handoff)

At D1, `src/graph/` and `src/client/daemonClient.ts` move into cairn-web-ui; the
transport swaps from HTTP `/query` to the existing `CairnClient` interface
(`runQuery`) — `daemonClient.ts` is written to that same method shape to make the
swap mechanical. The Vite shell, proxy, and `App.tsx` chrome are discarded;
cairn-web-ui provides window/routing/store. The enriched bindings this app
vendors are what cairn-web-ui gains when it re-syncs the contract at D1.
