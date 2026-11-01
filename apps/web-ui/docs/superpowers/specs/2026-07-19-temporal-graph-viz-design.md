# Temporal Graph-Viz — Design

**Status:** APPROVED DESIGN · 2026-07-19 · branch `engine-migration-temporal-graph`
**Depends on:** engine `36855f5` (contract vendored; see the 7a migration commit
`a73c2ab`). Related: [[graph-seam-upgrade]] history, [[graph-rework]] (UI-4a–d).

## 1. Goal

Let the user view the knowledge graph **through time**: scrub to a past
revision to see the graph as it existed then (snapshot), or select a range to
see what changed between two revisions (compare). This is orthogonal to the
existing spatial local/global view.

Non-goal (this spec): the AI semantic-suggestions overlay
(`get_suggestions` → dashed weighted edges). That is a separate follow-on spec;
it shares no machinery with the temporal feature and is live-only.

## 2. Contract surface (engine `36855f5`, already vendored)

```ts
// Query
{ type: "graph_at",   revision: string, scope: GraphScope }              // whole graph @ revspec
{ type: "graph_diff", from: string, to: string, scope: GraphScope }      // deltas from→to
{ type: "note_history", path: string }                                   // Revision[] newest-first

// QueryResponse
{ type: "graph",      nodes: GraphNode[], edges: GraphEdge[] }
{ type: "graph_diff", nodes_added: GraphNode[], nodes_removed: GraphNode[],
                      edges_added: GraphEdge[], edges_removed: GraphEdge[] }
{ type: "history",    revisions: Revision[] }

GraphNode = { path: string, title: string, mtime_secs: bigint }          // NO degree/tags here
Revision  = { id: string, message: string, timestamp_secs: bigint, author: string }
GraphScope = { type: "full" } | { type: "focused", path: string, depth: number }
```

**Reality notes that shape this design:**

- **No `vault_history`.** There is no whole-repo timeline query at `36855f5`
  and no engine PR for one. The scrubber's revision list therefore comes from
  `note_history(activePath)` — so temporal scrubbing is **gated on an open
  note**. A low-priority engine ask for `vault_history` will be filed; when it
  lands it is a pure *source swap* for the timeline (the render path via
  `graph_at`/`graph_diff` is unaffected).
- **No `nodes_changed`.** `graph_diff` reports added/removed only. The amber
  "Changed" state is reserved but **dormant**. Do **not** synthesize it from
  `mtime_secs` — mtime is dual-basis (HEAD = filesystem mtime, historical =
  commit time), so a diff on it is all false positives.
- **`mtime_secs` is `bigint`** and is **display-only** for now (no feature
  reads it). The mock seeds a placeholder.

## 3. Core idea: temporal is a data-source swap

The spatial axis (local/global) is unchanged: `get_graph`/`graph_at` are always
called with `scope: {type:"full"}`, and `GraphView` subsets client-side via the
existing `localSubgraph` for local mode. **The temporal feature only changes
which full-graph data feeds the existing pipeline.** It never touches spatial
machinery. This keeps `buildGraphData`/`localGraph` (string-path based) intact.

```
             ┌── live ──────  get_graph{full}          → store.graph
data source ─┼── snapshot ──  graph_at{rev, full}       → store.temporal.snapshot
             └── compare ───  graph_at{to,full}  (base)
                             + graph_diff{from,to,full} (deltas)  → base ⊕ ghosts
                                                       │
                                          same GraphView + client-side local subset
```

## 4. Axes and state model

Two orthogonal axes. **Time is a separate toggle, not a third spatial value.**

- **Spatial** — `local | global` — existing view state (`localGraph.ts`),
  unchanged.
- **Temporal** — `live | snapshot | compare` — new.

Per the locked split: temporal **data** lives in the store (actions, like
`loadGraph`); temporal **controls** live in view state (like `forceSettings`).

### 4.1 Store — temporal data slice

A new slice on `CairnState`, reset on `openCairn` like `graph`:

```ts
temporal: {
  timeline: Revision[] | null;        // note_history(activePath), newest-first
  snapshot: GraphData | null;         // graph_at result (also the compare base)
  diff: GraphDiff | null;             // graph_diff deltas (compare only)
} | null;

type GraphData = { nodes: GraphNode[]; edges: GraphEdge[] };
type GraphDiff = {
  nodes_added: GraphNode[]; nodes_removed: GraphNode[];
  edges_added: GraphEdge[]; edges_removed: GraphEdge[];
};
```

Actions (each token-guarded against stale reloads, mirroring `loadGraph`):

- `loadTimeline(path)` → `note_history{path}` → `temporal.timeline`.
- `loadSnapshot(revision)` → `graph_at{revision, {type:"full"}}` →
  `temporal.snapshot`.
- `loadDiff(from, to)` → parallel `graph_at{to}` (base) + `graph_diff{from,to}`
  → `temporal.snapshot` + `temporal.diff`.
- `clearTemporal()` → back to live (nulls snapshot/diff; keeps/refreshes
  timeline while a note is open).

### 4.2 View state — temporal controls

A `TemporalSettings` module mirroring `localGraph.ts` (localStorage optional;
selection is ephemeral, persistence limited to "panel open"):

```ts
interface TemporalControls {
  open: boolean;                  // scrubber visible
  selection:
    | { kind: "live" }
    | { kind: "snapshot"; at: number }               // index into timeline
    | { kind: "compare"; from: number; to: number };  // index range
}
```

A small pure mapper turns a `selection` + `timeline` into the store action to
run (and its args) — unit-testable without React:

```ts
selectionToRequest(sel, timeline)
  → { mode: "live" }
  | { mode: "snapshot"; revision: string }
  | { mode: "compare"; from: string; to: string }
```

## 5. Scrubber (unified handle)

`TemporalScrubber` — a bottom-bar overlay in `GraphView`, plus a temporal
toggle in the graph controls.

- Ticks = `timeline` revisions laid out **oldest → newest, left → right**
  (`timeline` is newest-first; reverse for layout). Each tick shows the short
  `id`/`message`/relative time on hover.
- **One handle** → `snapshot`. **Drag to a range (two handles)** → `compare`.
  **Collapse the range** (from == to) → back to `snapshot`.
- **Live** is a distinct rightmost position *past* the newest commit tick — it
  represents the working tree (`get_graph`), which can differ from a snapshot at
  HEAD (`graph_at{HEAD}` = the committed state, uncommitted edits excluded).
  Dragging the handle onto that rightmost position → `live`; the "Live" toggle
  also jumps there directly.
- **Gating:** no active note → no timeline → controls **disabled** with a hint
  ("Open a note to scrub its history"); the graph stays **live**. Switching the
  active note reloads the timeline and resets `selection` to `live`.

## 6. Rendering & diff styling

`GraphView` chooses the data source from the temporal `selection`:

- **live** → `store.graph` (today's path, unchanged).
- **snapshot** → `store.temporal.snapshot`; rendered normally (no diff styling).
- **compare** → base = `store.temporal.snapshot` (the `to` graph), styled with
  `store.temporal.diff`.

Compare styling (base `to` ⊕ deltas):

| Delta | Source | Style |
|-------|--------|-------|
| Appeared | `nodes_added` / `edges_added` (∈ base `to`) | **green** |
| Disappeared | `nodes_removed` / `edges_removed` (∉ base `to`) | **dashed ghost**, re-injected onto the base |
| Unchanged | rest of base | **dim** |
| Changed | *(none — no `nodes_changed`)* | **reserved, dormant (amber)** |

Seam: `GNode`/`GLink` gain an optional `state?: "appeared" | "disappeared" |
"unchanged" | "changed"`, consumed by the existing node/link paint callbacks
in `GraphView`/`forceGraphTypes`. A pure `buildCompareGraphData(base, diff)`
produces the stated node/link arrays (parallels `buildGraphData`); live/snapshot
keep using `buildGraphData` (state undefined → today's styling).

Client-side local mode still applies on top of any source (it subsets by path),
so "local + snapshot" and "local + compare" work with no extra code.

## 7. Mock — synthetic vault timeline

To make the whole surface testable offline, `MockClient` gains an **additive**
vault-level timeline used only by the temporal queries; the existing per-note
`HistoryFixture` (which drives `note_history` / `note_at` / `restore_note`) is
untouched.

```ts
interface VaultSnapshot { notes: Record<string, string>; } // full note-set @ a revspec
// constructor gains: vaultSnapshots?: Record<string /*revspec id*/, VaultSnapshot>
```

- `graph_at{revision, scope}` → build the graph from
  `vaultSnapshots[revision].notes` using the **same** node/edge logic as
  `get_graph` (and honor `focused` scope via the same BFS as 7a). Unknown
  revspec → `not_found`.
- `graph_diff{from, to, scope}` → build both graphs, compute
  added/removed nodes & edges by set difference on `path` / `from|to`.
- Revspec ids are shared with the seeded `HistoryFixture.revisions[].id`, so a
  scrubber tick (from `note_history`) always resolves to a `vaultSnapshots`
  entry — the offline surface is internally consistent.

## 8. Component boundaries

- **store temporal slice** (`store.ts` or a `temporalSlice.ts`): data + actions.
  What it does: fetch/hold timeline, snapshot, diff. Depends on: `CairnClient`.
- **`temporalControls.ts`**: `TemporalControls` type, `selectionToRequest`
  mapper, persistence of `open`. Pure; depends on nothing but the contract types.
- **`TemporalScrubber.tsx`**: renders ticks + handle(s) from `timeline`, emits
  `selection` changes. View-only; depends on `Revision[]` + callbacks.
- **`buildCompareGraphData`** (in `graphData.ts`): `(base, diff) → stated
  {nodes, links}`. Pure; unit-tested.
- **`GraphView.tsx`**: wires the temporal toggle + scrubber, picks the data
  source, applies diff styling. The one integration point.

## 9. Testing

- **Pure units:** `selectionToRequest` (each selection → correct query/args,
  incl. collapse-range → snapshot); `buildCompareGraphData` (added=green,
  removed=ghost-injected, unchanged=dim, no `changed` ever emitted).
- **Store actions vs mock:** `loadTimeline`/`loadSnapshot`/`loadDiff` populate
  the slice from the synthetic vault timeline; token-guard drops stale results;
  `clearTemporal` returns to live.
- **Mock:** `graph_at` builds the historical graph; unknown revspec →
  `not_found`; `graph_diff` computes correct added/removed sets;
  focused scope narrows.
- **GraphView (RTL, like existing graph tests):** scrubber **disabled** with no
  active note; enabling temporal + selecting a snapshot swaps the rendered data;
  the canvas itself isn't asserted (jsdom has no measured size — existing
  pattern).

## 10. Out of scope

- Semantic-suggestions overlay (`get_suggestions`) — separate follow-on spec.
- Server-side scope/focusing — spatial stays client-side.
- `vault_history` note-independent global timeline — engine ask to be filed;
  a later source-swap for §5 ticks, no render change.
- Amber "Changed" diff state — reserved until the engine reports
  `nodes_changed`; never faked from `mtime_secs`.
