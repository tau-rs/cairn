# Graph view → new seam contract: upgrade design

**STATUS: APPROVED DESIGN.** Brainstorming complete; all 7 forks decided (§3). This
document is the spec for an *in-place* upgrade of the existing graph view onto the
new engine seam contract. Implementation is gated on engine Track 1 landing (§6) —
design is not.

**Date:** 2026-06-29
**History:** began as a pre-design brief (handoff from a session in the wrong repo,
`tau-rs/cairn`); the brainstorm that produced §3's decisions ran 2026-06-29 with the
visual companion. The original "STATUS: pre-design" header has been replaced by this
approved design per the task.

---

## 1. What this task actually is

The original framing was "build the knowledge-graph visualization UI from scratch."
That is **wrong for this repo** — a mature graph view already ships here. The real
task is to **evolve the existing graph view onto the new engine seam contract** and
apply the research-driven UX changes, working *in place* (not a rewrite, not a
parallel component).

### Existing data flow (verified)

```
store.ts  loadGraph() → runQuery({ type: "get_graph" })          // OLD contract
          state.graph: { nodes: string[], edges: {from,to}[] }   // paths only, no metadata
          state.noteTags via list_notes → Record<path, tags[]>   // tags fetched separately
   │
   ▼  EditorPane.tsx   (route ?view=graph)
   └─ <GraphView nodes edges tagsByNote activePath loading onOpenNote/>
        └─ react-force-graph-2d canvas — ALREADY BUILT:
           • local/global toggle               (DEFAULT = global)
           • local mode = CLIENT-side BFS       (graph/localGraph.ts), depth 1–3 slider
           • degree computed CLIENT-side        (graph/graphData.ts) → node radius
           • color-groups-by-tag panel          (graph/GraphGroupsPanel.tsx, colorGroups.ts)
           • force-settings panel + freeze/pin  (graph/GraphForcesPanel.tsx, forceSettings.ts)
           • hover-neighbor highlight, zoom-dependent labels
```

So ~80% of the task's "table stakes" (focus+context local mode, degree-sizing, tag
color/filter, togglable forces, force-directed layout) **already exists.**

### Files in play

| File | Role |
|---|---|
| `web/src/store/store.ts` | `graph` state + `loadGraph()` + `noteTags`; the query call site |
| `web/src/components/EditorPane.tsx` | renders `<GraphView>` for `?view=graph`, wires store → props |
| `web/src/components/GraphView.tsx` | 340-line canvas component; all rendering + panels |
| `web/src/components/graph/graphData.ts` | builds force-graph data, **computes degree**, node radius, label alpha |
| `web/src/components/graph/localGraph.ts` | **client-side** depth-bounded BFS subgraph |
| `web/src/components/graph/colorGroups.ts` / `GraphGroupsPanel.tsx` | tag→color groups |
| `web/src/components/graph/forceSettings.ts` / `GraphForcesPanel.tsx` | d3-force tuning |
| `web/src/components/graph/forceGraphTypes.ts` | typed seam over react-force-graph mutation |
| `web/src/client/daemon.ts` | `runQuery`; `noteTags()` (list_notes) |
| `web/src/contract/{Query,QueryResponse,GraphEdge}.ts` | **vendored OLD bindings** |

### Layout engine is already decided

`react-force-graph-2d` ^1.29 (canvas, wraps d3-force). The perf ceiling is designed
*around* this engine (§3, Fork 3) — the engine choice is not re-litigated.

---

## 2. The contract delta

### New contract to consume (frozen by the engine Track-1 seam PR)

```ts
Query  GetGraph { scope: GraphScope }
       GraphScope { focus?: string; depth?: number }   // focus=null ⇒ global; depth default 1
QueryResponse Graph { nodes: GraphNode[]; edges: GraphEdge[] }
       GraphNode { path: string; title: string; degree: number; tags: string[]; mtime_secs: number }
       GraphEdge { from: string; to: string }
// also available, design seams for but don't build yet:
       GetSuggestions → Suggestions { suggestions: SuggestedEdge[] }  // empty until engine Track 2
       GraphAt { revision: string; scope: GraphScope }                // errors Unsupported until Track 3
```

### What changes when we adopt it

| New field / query | Replaces / adds in this repo |
|---|---|
| `GetGraph { scope:{focus,depth} }` | **Server-side** neighborhood → replaces client BFS in `localGraph.ts`. Don't ship the whole graph to the browser to slice depth-1 — this *is* the perf-ceiling strategy. |
| `GraphNode.degree` | Removes client degree computation in `graphData.ts`. |
| `GraphNode.tags[]` | Folds in the separate `noteTags`/`list_notes` fetch; feeds existing `colorGroups`. |
| `GraphNode.title` | Better node labels than `stem(path)`. |
| `GraphNode.mtime_secs` | **New capability** — recency encoding (Fork 6); nothing uses it today. |
| `GetSuggestions` / `GraphAt` | New seams (Fork 7): dashed/distinct suggested edges merged client-side; temporal scrubber. |

### Decisions locked by the engine team (do NOT re-litigate)

- Suggested edges and explicit edges are **separate engine queries**, merged
  **client-side** into ONE visual graph.
- Temporal is a **separate query** (`GraphAt`).

### Research — table stakes / traps (from the task brief)

- **DO** local-first / focus+context (Degree-of-Interest; Perer & van Ham 2009):
  render the focused note's neighborhood via `GetGraph` focus+depth, expandable.
- The **global** graph is the documented "hairball" failure mode, unvalidated above
  ~100 nodes / 200 edges across 152 studies → keep it a **secondary "overview,"
  never the headline.**
- Force-directed with **togglable forces** (already present) + a **hard perf
  ceiling**: org-roam-ui warns >2k nodes degrades; don't naively simulate thousands.
- Use node metadata: size by degree, color/filter by tags, encode recency via
  `mtime_secs`.

---

## 3. Approved design decisions (the 7 forks)

Decided in the 2026-06-29 brainstorm. Order of decision was 2 → 1 → 5 → 3 → 6 → 4 → 7
(architecture foundation first).

### Fork 2 — Server-side scope (foundation) ✅
Adopt **full server-side scope**. Every view is a scoped `GetGraph` query; the engine
returns the neighborhood with metadata. **Delete** the client BFS (`localGraph.ts`)
and its tests — no hybrid/fallback BFS (keeping a dead path is the YAGNI trap).
`noteTags`/`list_notes` graph fetch is removed; tags ride on `GraphNode.tags`. A small
client cache keyed by `${focus}@${depth}` avoids refetch on revisit.

### Fork 1 — Default local, keep the toggle ✅
Keep the existing local/global segmented control; **flip the default to local**
(focus = active note). When **no note is focused**, fall back to the global overview
(never the dead-end "open a note" placeholder). Global remains the secondary,
perf-capped overview. (Chose T1+A1 over a toggle-less unified click-to-refocus model.)

### Fork 5 — Modifier-split focus ✅
- **Plain node-click → re-root the local graph only** (sets `graphFocus`; editor
  untouched — lets you wander without losing your place).
- **⌘-click / double-click → open the note** (and re-root).
- **Sync rule:** opening a note *anywhere* (tree, ⌘-click, search, palette) sets
  **both** `activePath` and `graphFocus`; a plain graph click sets **only**
  `graphFocus`. So normal navigation keeps the graph tracking the editor, and a peek
  re-syncs on the next real open.

### Fork 3 — Global perf ceiling ✅
Hard cap the global overview at **1,500 nodes** (under the ~2k degrade line). When a
vault exceeds it, keep the **top-N by `GraphNode.degree`** (hubs are the useful
skeleton) and show a banner: *"Showing 1,500 most-connected of N notes — open a note
for its full local graph."* Existing LOD (label/link fade when zoomed out) is
retained. The cap is a tunable constant. (Chose hard-cap-by-degree over degree-
threshold cull, which has no real ceiling, and over folder clustering, a future seam.)

### Fork 6 — Recency ring ✅
Encode `mtime_secs` as a **recency ring** — a single accent-hue halo around the node
whose **opacity + width** ramp from full (edited today) → nothing (older than the
window). Orthogonal to the tag-color fill channel (rejected brightness/saturation
modulation, which muddies tag identity — the brief's exact trap). Ships as an **opt-in
toggle, default OFF**, with a tunable window (default 30 days). Lives in the panel
(Fork 4).

### Fork 4 — Legend + filtering in the existing panel ✅
The color-groups panel becomes the **legend AND filter** (no new always-on canvas
chrome):
- each tag-group row gets an **eye toggle** (hide/show those nodes),
- a **min-degree** slider (cull low-connectivity noise),
- a **recency-window** slider, shown only when the recency ring is on,
- an **"other / untagged"** catch-all row so filtering covers notes that match no group.

### Fork 7 — Seams for suggestions + temporal (design now, build later) ✅
**Merge shape (the lock-in that prevents a rewrite):**
```ts
// graphData.ts — every rendered link carries its origin
interface GLink { source: string; target: string;
                  kind: "explicit" | "suggested"; weight?: number }

// store.ts — one unified fetch path, flags choose which queries fire + merge
graphScope:      { focus, depth }
graphRevision:   string | null    // null ⇒ live (GetGraph); set ⇒ GraphAt
showSuggestions: boolean           // default FALSE — opt-in overlay

loadGraph():
  base = graphRevision ? GraphAt({revision, scope}) : GetGraph({scope})   // Track 1 / Track 3
  sugg = showSuggestions ? GetSuggestions() : { suggestions: [] }          // Track 2
  graph = merge(base.edges → kind:"explicit",
                sugg.suggestions → kind:"suggested", weight:s.score?)
```
- **Visual:** explicit = solid; suggested = **dashed indigo**, dimmer, optional
  weight→width. Node **degree/size stays explicit-only** — suggestions are a pure
  visual overlay, never structural.
- **Controls:** "✦ Suggestions" is a **panel toggle, default off** (shows "none yet"
  until Track 2). The **temporal scrubber** is a slim bottom bar, **hidden/gated**
  until `GraphAt` is supported (Track 3) — no visible chrome for unbuilt features.
- **Out-of-scope endpoints:** drop suggested edges whose endpoints aren't both in the
  rendered node set (same rule `buildGraphData` already applies to explicit edges).
  Revisit if Track 2's real output makes that wrong (see §7).

---

## 4. Architecture & component changes

### Store (`store.ts`)
```ts
// REMOVE:
graph: { nodes: string[]; edges: {from,to}[] } | null;
noteTags: Record<string, string[]>;
// ADD:
graph: Graph | null;                 // Graph = { nodes: GraphNode[]; edges: GraphEdge[] }
graphScope: GraphScope | null;       // currently-loaded { focus, depth }
graphFocus: string | null;           // Fork 5 — graph re-root, distinct from activePath
graphRevision: string | null;        // Fork 7 — null = live
showSuggestions: boolean;            // Fork 7 — default false
graphCache: Map<string, Graph>;      // key `${focus ?? "*"}@${depth}` (Fork 2)
```
- `loadGraph(opts?)` becomes scope-aware (focus/depth/revision) + cache-checked, and
  performs the suggestion merge when `showSuggestions`. Keeps the existing `seq.graph`
  staleness-token guard.
- The keystroke-cascade refresh (store.ts ~L411 `if (get().graph !== null) loadGraph`)
  must reload the **current scope**, not force global.
- Sync rule (Fork 5): the note-open paths that set `activePath` also set `graphFocus`.

### `graphData.ts`
- `buildGraphData` consumes `GraphNode[]` (use server `degree`, `title`, `tags`)
  instead of `string[]`; stops computing degree.
- `GLink` gains `kind` + optional `weight`; add a merge helper for explicit+suggested.
- `nodeRadius`, `labelAlpha`, `buildAdjacency` unchanged in spirit.
- New: recency-ring alpha/width from `mtime_secs` + window (pure helper, unit-tested).

### `GraphView.tsx`
- Reads `graph`/`graphScope`/`graphFocus` from props instead of `nodes/edges/tagsByNote`.
- Local/global toggle drives `graphScope.focus` (Fork 1 default local + no-focus→global).
- `onNodeClick` → modifier-split (Fork 5). `paintNode` draws the recency ring (Fork 6).
- `linkColor`/link dash keyed off `GLink.kind` (Fork 7).
- Global cap banner (Fork 3).

### Panel (`GraphGroupsPanel.tsx` / `colorGroups.ts`)
- Add eye toggles (per-group visibility), min-degree slider, recency toggle + window,
  "other/untagged" row (Fork 4). Filtering state persists like existing panel state.

### `localGraph.ts`
- **Deleted** (BFS replaced by server scope). Remove `localSubgraph` + its tests.

### Contract (`web/src/contract/`)
- Re-vendor on Track-1 merge (§6): `GraphNode`, `GraphScope`, updated `Query`/
  `QueryResponse`, plus `SuggestedEdge`/`Suggestions`. Extend `contractGuards.ts`.

---

## 5. Testing (part of done)

- **Vitest unit:** `graphData.test.ts` — degree/title/tags from `GraphNode`; GLink
  merge (explicit+suggested, kind/weight); out-of-scope suggestion drop; recency
  ring alpha/width ramp; min-degree + tag-visibility filtering. Delete
  `localGraph.test.ts`.
- **Store tests:** `loadGraph` scope routing (local/global/revision), cache hit/miss,
  staleness token, suggestion merge gated by `showSuggestions`, Fork-5 sync rule.
- **Playwright (`web/e2e`):** default-local on graph open, plain-click re-root vs
  ⌘-click open, global cap banner, recency toggle, tag filter hide/show.
- **Stryker** mutation testing is configured; keep the new pure helpers well-covered.

---

## 6. Blockers & sequencing

- **Implementation is blocked on the engine Track-1 seam landing in `tau-rs/cairn`
  `main`.** As of this writing it is **not merged**: `get_graph` still returns
  `nodes: string[]` and there are no `GraphNode`/`GraphScope` bindings. Sibling tracks:
  semantic-edges-suggestions = Track 2, temporal-graph-at = Track 3.
- **Design is NOT blocked** — this spec is complete against the frozen contract.
- **Re-vendor step (when Track 1 merges):** copy the generated TS bindings from
  `crates/cairn-contract/bindings/` (GraphNode, GraphScope, updated Query/QueryResponse,
  plus SuggestedEdge/Suggestions) into `web/src/contract/`; confirm `contractGuards.ts`
  covers the new `graph` response shape.
- **Build order once unblocked:** re-vendor → store/`graphData` core (Forks 2/1) →
  `GraphView` interactions (Forks 5/3/6) → panel filters (Fork 4) → seam scaffolding
  (Fork 7, inert until Tracks 2/3).

## 7. Open items (confirm later — do not block design)

- **Exact `SuggestedEdge` fields** are NOT frozen (Track 2 unmerged); the brief only
  names the type. We design against `{ from, to, score? }`. Verify field names at
  re-vendor; the merge adapter is a one-line `.map`, so any shape is cheap to absorb.
- **Out-of-scope suggestion endpoints:** default = drop (§3 Fork 7). Reconsider only if
  Track 2's output references nodes outside the scope by design.

## 8. Next steps

1. ~~Brainstorm forks §3~~ ✅. ~~Approve design~~ ✅. ~~Replace STATUS header~~ ✅.
2. `writing-plans` → implementation plan (after user reviews this spec).
3. When Track 1 merges: re-vendor bindings, then implement in place per §4 build order.
4. Tests per §5 are part of done.
