# Graph Seam Upgrade — Implementation Plan v3 (rebased on PR #105)

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Steps use `- [ ]` checkboxes.

> **Lineage:** supersedes `2026-07-19-graph-seam-upgrade-reconciled.md` (v2), which supersedes `2026-07-18-graph-seam-upgrade.md`. v3 rebases the whole plan onto the **post-#105 baseline**: the engine migration + temporal graph-viz already shipped in PR #105, which re-vendored the contract and migrated the graph pipeline. v3 keeps only the *upgrade* layer (server degree/tags → cap/filter/recency/legend) and drops what #105 already did.

## Baseline: what PR #105 already shipped (verified against the branch)

- **Contract vendored** at engine `36855f5`: `GraphScope = {type:"full"} | {type:"focused",path,depth}`; `SuggestedEdge = {from,to,weight,why:string|null}`; `SuggestionScope = {type:"note",path} | {type:"vault"}`; `GraphNode = {path, title, mtime_secs: bigint}` — **no `degree`/`tags` yet** (pre-#109).
- **Store**: `graph: { nodes: GraphNode[]; edges } | null`; `temporal` slice (snapshot/diff/timeline). `loadGraph()` takes no args and **always queries `get_graph` with `scope: {type:"full"}`**.
- **Live local mode is client-side**: `localGraph.ts` BFS runs on the full graph; `graphData.ts` `buildGraphData(nodes: string[], …)` stays string-based; GraphView projects `GraphNode[]`→paths and computes **degree client-side**.
- **Temporal shipped**: `TemporalScrubber`, `useTemporalGraph`, `temporalControls`, `buildCompareGraphData` (GraphNode-based, `GraphState = appeared|disappeared|unchanged|changed`), mock `graph_at`/`graph_diff`. All temporal fetches use `scope:{type:"full"}`.
- **Force settings + color groups + tags** already present (`GraphForcesPanel`, `forceSettings`, `colorGroups`, `tags`).

## Resolved from v2's VERIFY list

| v2 VERIFY | Resolution |
|---|---|
| #1 `GraphNode.title`? | **Present** — keep `n.title \|\| stem(n.path)`. |
| #2 `SuggestionScope` shape | `{type:"note",path} \| {type:"vault"}` — **not** full/focused. (Out of scope for v3; suggestions is a separate spec.) |
| #4 `mtime_secs` bigint or number | **`bigint`** at 36855f5 (#110 not applied). Coerce `Number(...)` at the seam. |

**Remaining VERIFY (at the #109 rev-bump — Task 0):** exact TS type of the new `GraphNode.degree` (ts-rs maps `usize`→`number`, `u64`→`bigint` — confirm) and `tags` (expect `Array<string>`).

## ⚠️ DECISION TO CONFIRM — reverses v2 Fork 2

v2 (and the original brief, Fork 2) decided: **move neighborhood scoping server-side and delete `localGraph.ts`**. PR #105 shipped the opposite — full-fetch + client-side BFS — and built temporal on it.

**v3 recommendation: KEEP #105's client-side local model. Do NOT migrate to server-side focused scope in this pass, and do NOT delete `localGraph.ts`.**

Why:
- The upgrade's value — degree/tags, global cap, min-degree + tag filters, recency ring — all operate on the full graph fetched once. None of it needs server-side scope.
- `loadGraph()` and temporal both assume `scope:{type:"full"}`. Migrating live local mode to `scope:{type:"focused"}` would fork that assumption and fight freshly-shipped temporal for no user-visible gain.
- Server-side scope offload is a legitimate but *separate* optimization → its own follow-on if ever wanted.

**If you reject this** (still want server-side scope): Tasks 3 (store) and 4 (GraphView) grow to rewrite `loadGraph` to take a scope, drop client BFS, delete `localGraph.ts`, and reconcile with temporal's full-scope assumption. Everything else is unchanged.

## Scope of v3

**IN:** adopt server `degree`+`tags` (via #109 rev-bump) and build the visual/filter upgrade on top of #105's shipped graph:
- global overview **cap by degree** + banner,
- **min-degree + tag-group filters**,
- **color-groups legend** (from `matchGroup` + server tags),
- **recency ring** from `mtime_secs`.

**OUT (already done or separate):**
- ❌ Temporal (scrubber, graph_at/diff) — shipped in #105.
- ❌ Suggested edges overlay (`SuggestedEdge`/`get_suggestions`/`SuggestionScope`) — **its own follow-on spec** (per the split libreville locked); `why` is nullable when that spec runs.
- ❌ Server-side focused scope + `localGraph.ts` deletion — see the decision above (kept client-side).

## Global constraints

- **Gated on #105 merging + a #109 rev-bump.** Do not start Task 1+ until #105 is on `main` and Task 0 (rev-bump) is green. As of writing #105 is OPEN (auto-merge on, `tauri` pending).
- Contract files are vendored, generated, raw ts-rs — copy verbatim, never hand-format (`web/.prettierignore`).
- Global node cap = 1,500; recency window default = 30 days; recency default OFF. Named constants.
- TDD: red → run → green → commit; one logical change per commit; full local gate (`just web-ci` incl. `prettier --check`) before claiming green.

---

## Task 0: Rev-bump 36855f5 → engine main (add `degree`+`tags`)

**Not a from-scratch vendor** — #105 already vendored the seam. This bumps the pinned engine rev past #109 so `GraphNode` gains `degree`+`tags`.

**Files:** the 6 engine rev pins (as in #105's migration) · `web/src/contract/GraphNode.ts` (+ any file #109 regenerated) · `web/src/client/contractGuards.ts` · `web/src/client/mock.ts`.

- [ ] **Step 1:** Bump the engine rev pins from `36855f5` to current engine `main` (≥ `ef9e70a`, the #109 merge). Re-vendor `web/src/contract/*` verbatim from `crates/cairn-contract/bindings/`.
- [ ] **Step 2 (VERIFY):** Confirm the regenerated `GraphNode` = `{path, title, degree: <number?>, tags: Array<string>, mtime_secs: bigint}`. Record the exact TS types of `degree`/`tags`. If the bump drags in other contract changes (it shouldn't — #109 is graph-only), diff and reconcile.
- [ ] **Step 3:** Extend `contractGuards.ts` `"graph"` case to validate `degree`/`tags` on each node (follow existing per-variant style). Write the failing guard test first, then implement.
- [ ] **Step 4:** Update `mock.ts` `get_graph` to populate `degree` (undirected edge count) + `tags` (existing tag parse) on each `GraphNode`. Update the temporal mocks (`graph_at`/`graph_diff`) similarly so their `GraphNode`s carry the new fields. Red test → green.
- [ ] **Step 5:** `just web-ci` — typecheck will surface every downstream site that must now handle `degree`/`tags` (fixed in Tasks 3–4). Commit: `feat(graph): rev-bump engine to #109 — GraphNode degree+tags`.

## Task 1: `recency.ts` — mtime → ring visual (pure, greenfield)

Identical to **v2 Task 1** (`2026-07-19-graph-seam-upgrade-reconciled.md`) — the helper is greenfield on #105. Copy its test + implementation verbatim (`recencyRing`, `DEFAULT_RECENCY {enabled:false, windowDays:30}`, `loadRecency`/`saveRecency`). Commit: `feat(graph): recency ring pure helper from mtime_secs`.

## Task 2: `globalCap.ts` — cap global to top-N by degree (pure, greenfield)

Identical to **v2 Task 2**. Consumes the now-present `GraphNode.degree`. Copy its test + `capByDegree`/`GLOBAL_NODE_CAP=1500` verbatim. Commit: `feat(graph): global overview cap to top-N by degree`.

## Task 3: `matchGroup()` + `graphFilter.ts` (pure, greenfield)

As **v2 Task 3**: add `matchGroup()` identity to `colorGroups.ts` (delegate `matchGroupColor`), create `graphFilter.ts` (`applyFilters` over min-degree + hidden-group + hide-ungrouped, `DEFAULT_FILTER`, load/save). Consumes `GraphNode.degree`/`.tags`. Copy v2's tests + impl. Commit: `feat(graph): matchGroup identity + min-degree/tag/ungrouped filter`.

## Task 4: Feed server degree/tags into the render pipeline

**This is the integration delta vs #105** — #105 computes degree client-side and drops tags; v3 uses the server's.

**Files:** `web/src/components/graph/graphData.ts`, `forceGraphTypes.ts`, `web/src/components/GraphView.tsx`, tests.

- [ ] **Step 1:** In `graphData.ts`, add a `GraphNode[]`-aware builder (or extend `buildGraphData`) that carries `degree`, `tags`, and `mtimeSecs: Number(n.mtime_secs)` onto `GNode` — instead of GraphView projecting to `string[]` and recomputing degree. **Do not touch `buildCompareGraphData`** (temporal). Keep `buildAdjacency` explicit-links-only. Red test → green.
- [ ] **Step 2:** `RFNode` (`forceGraphTypes.ts`) gains `tags: string[]; mtimeSecs: number;`. `nodeRadius(degree)` now takes the **server** degree.
- [ ] **Step 3:** In `GraphView.tsx`, thread the display pipeline: `capByDegree` (global view only) → `applyFilters` → the `GraphNode[]`-aware `buildGraphData`. Add the **recency ring** in `paintNode` (`recencyRing(node.mtimeSecs, Date.now()/1000, windowDays)`) and the **cap banner** when truncated. Leave temporal rendering paths untouched.
- [ ] **Step 4:** `just web-ci` green. Commit: `feat(graph): consume server degree/tags; cap + recency ring in render`.

## Task 5: Store — expose recency/filter/cap state; keep full-fetch + client local

**Files:** `web/src/store/store.ts`, `store.test.ts`.

- [ ] Keep `loadGraph()` as-is (`scope:{type:"full"}`) and `localGraph.ts` client BFS (per the confirmed decision). **No `graphFocus`/`setGraphFocus`/scoped-cache work** (that was v2's server-side-scope path — dropped).
- [ ] Add UI-state the panel/GraphView need if not already local to GraphView: `filter: FilterSettings`, `recency: RecencySettings` (or keep them component-local via `useState`+load/save — match where `forceSettings`/`colorGroups` state currently lives in #105). Add store tests only if state moves into the store.
- [ ] Commit: `feat(graph): recency/filter settings wired to graph view`.

## Task 6: `GraphGroupsPanel` — legend + filter controls

As **v2 Task 7**, layered on #105's existing panel: per-group eye toggles (`hiddenGroupQueries`), "other/untagged" row (`hideUngrouped`), min-degree slider, recency toggle + window slider. **Drop** the suggestions toggle (separate spec). Commit: `feat(graph): panel legend + filters (eye toggles, min-degree, recency)`.

## Task 7: EditorPane / GraphView wiring + e2e

- [ ] Wire the panel's new controls through to GraphView (props or store selectors, matching #105's existing wiring for force settings).
- [ ] `web/e2e/graph.spec.ts`: cap banner in global view; hide a tag group → node count drops; recency toggle persists across reload. **Do not** add default-local/re-root/scope e2e (server-side scope dropped). Keep temporal e2e (owned by #105) untouched.
- [ ] Commit: `test(graph): e2e for cap banner, tag-group filter, recency persistence`.

---

## Final gate

- [ ] `just web-ci` (lint · format:check · typecheck · test · build) green — remember `prettier --check` ([[ci-local-gates]]).
- [ ] `pnpm audit --audit-level=high` clean before enqueue ([[merge-queue-live-audit-gate]]).
- [ ] Mutation-test the new pure helpers (`recency`/`globalCap`/`graphFilter`) via Stryker.
- [ ] `gh pr create --base main`; auto-merge via the queue ([[merge-queue]]). Do not merge before #105.

## Coverage map

- Server degree/tags → Task 0 (rev-bump) + Task 4 (consume). Cap → Tasks 2, 4. Filter → Tasks 3, 4, 6. Legend/color-groups → Tasks 3, 6. Recency → Tasks 1, 4, 6.
- Dropped vs v2: temporal (Fork 7 scrubber) = #105; suggested edges (Fork 7 dashed) = separate spec; server-side scope + `localGraph.ts` deletion (Fork 2) = reversed, kept client-side.
