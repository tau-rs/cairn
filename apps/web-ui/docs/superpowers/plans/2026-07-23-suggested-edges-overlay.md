# Suggested-edges Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render engine-suggested (non-explicit) note links as a distinct, toggleable dashed overlay on the force graph, default OFF, scope auto-following the graph's full/local mode.

**Architecture:** Thin overlay adopting raw contract types (no ACL). The store owns the transport call (`loadSuggestions` behind `CairnClient`, modeled on `loadGraph`). `GraphView` owns the on/off toggle state (persisted), derives `SuggestionScope` from its own full/local mode, fires a load callback, and merges suggested links into the live build. A pure `buildSuggestedLinks` helper does the visibility-filter + dedup. `GraphGroupsPanel` stays stateless with a new controlled toggle. `EditorPane` is the thin adapter wiring store ↔ view.

**Tech Stack:** React + TypeScript, zustand store, react-force-graph-2d (canvas), vitest + @testing-library/react, pnpm, `just` task runner.

## Global Constraints

- Contract files under `web/src/contract/` are vendored raw ts-rs — **never edit or hand-format** them (they're in `web/.prettierignore`). Import types from `../contract` / `../../contract`.
- `SuggestedEdge.weight` is a 0..1 cosine-similarity **ranking only** (not a plottable distance). Use it for relative opacity/width, not absolute positioning.
- Suggestions never introduce nodes: a suggested edge renders only if **both** endpoints are already visible nodes.
- Default OFF. No `get_suggestions` query fires until the user enables the overlay.
- Full gate before PR: `just web-ci` — **includes `prettier --check`** (eslint won't catch formatting; easy to miss).
- PR `--base main`; merge via the **merge queue** ("Merge when ready"), never direct push.
- Overlay UI state lives in `GraphView`, persisted alongside groups/filter/recency (localStorage). Suggested-edge **data** lives in the store.

---

## File Structure

- `web/src/components/graph/graphData.ts` — MODIFY: add `kind`/`weight`/`why` to `GLink`; add pure `buildSuggestedLinks`.
- `web/src/components/graph/suggestionsOverlay.ts` — CREATE: `SuggestionsSettings` type, defaults, `load/saveSuggestionsSettings`, pure `suggestionScopeFor`.
- `web/src/store/store.ts` — MODIFY: `suggestions` state field + `loading.suggestions` flag + `seq.suggestions` token + `loadSuggestions(scope)` action + interface decl.
- `web/src/components/graph/GraphGroupsPanel.tsx` — MODIFY: new `suggestions`/`onSuggestionsChange` prop pair + toggle section.
- `web/src/components/GraphView.tsx` — MODIFY: new props, scope effect, link merge, link accessors (color/dash/width/label), pass panel props.
- `web/src/components/EditorPane.tsx` — MODIFY: read `s.suggestions`, pass `suggestions` + `onLoadSuggestions` to `GraphView`.

Test files (co-located, matching repo convention `*.test.ts(x)` next to source):
- `web/src/components/graph/graphData.test.ts` (may already exist — append cases)
- `web/src/components/graph/suggestionsOverlay.test.ts` (create)
- `web/src/store/store.test.ts` or existing store test file — add `loadSuggestions` cases (see Task 3 step 1 for locating the file)
- `web/src/components/graph/GraphGroupsPanel.test.tsx` (create or append)

---

## Task 1: `GLink.kind` seam + `buildSuggestedLinks` helper

**Files:**
- Modify: `web/src/components/graph/graphData.ts`
- Test: `web/src/components/graph/graphData.test.ts`

**Interfaces:**
- Produces:
  - `GLink` gains optional `kind?: "real" | "suggested"`, `weight?: number`, `why?: string | null`.
  - `buildSuggestedLinks(suggestions: SuggestedEdge[], visibleNodeIds: Set<string>, realLinks: GLink[]): GLink[]` — maps to `kind:"suggested"` links, dropping edges with a missing endpoint and deduping (undirected) against real links and against each other.

- [ ] **Step 1: Check whether the test file exists**

Run: `ls web/src/components/graph/graphData.test.ts`
If it does not exist, create it with this import header:

```ts
import { describe, it, expect } from "vitest";
import { buildSuggestedLinks } from "./graphData";
import type { GLink } from "./graphData";
import type { SuggestedEdge } from "../../contract";
```

If it exists, add the same imports (only the ones missing) and append the `describe` block from Step 2.

- [ ] **Step 2: Write the failing tests**

Append to `web/src/components/graph/graphData.test.ts`:

```ts
describe("buildSuggestedLinks", () => {
  const sug = (from: string, to: string, weight = 0.5, why: string | null = null): SuggestedEdge => ({
    from,
    to,
    weight,
    why,
  });
  const visible = new Set(["a.md", "b.md", "c.md"]);

  it("maps a suggestion to a suggested GLink carrying weight and why", () => {
    const out = buildSuggestedLinks([sug("a.md", "b.md", 0.8, "shared: x")], visible, []);
    expect(out).toEqual([
      { source: "a.md", target: "b.md", kind: "suggested", weight: 0.8, why: "shared: x" },
    ]);
  });

  it("drops an edge whose endpoint is not a visible node (one missing)", () => {
    const out = buildSuggestedLinks([sug("a.md", "z.md")], visible, []);
    expect(out).toEqual([]);
  });

  it("drops an edge when both endpoints are missing", () => {
    const out = buildSuggestedLinks([sug("y.md", "z.md")], visible, []);
    expect(out).toEqual([]);
  });

  it("suppresses a suggestion that duplicates a real link (undirected)", () => {
    const real: GLink[] = [{ source: "b.md", target: "a.md" }];
    const out = buildSuggestedLinks([sug("a.md", "b.md")], visible, real);
    expect(out).toEqual([]);
  });

  it("dedupes duplicate suggestions among themselves (undirected)", () => {
    const out = buildSuggestedLinks([sug("a.md", "b.md"), sug("b.md", "a.md")], visible, []);
    expect(out).toHaveLength(1);
  });

  it("passes null why through untouched", () => {
    const out = buildSuggestedLinks([sug("a.md", "c.md")], visible, []);
    expect(out[0].why).toBeNull();
  });

  it("returns [] for empty suggestions or empty visible set", () => {
    expect(buildSuggestedLinks([], visible, [])).toEqual([]);
    expect(buildSuggestedLinks([sug("a.md", "b.md")], new Set(), [])).toEqual([]);
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `pnpm --dir web test -- graphData`
Expected: FAIL — `buildSuggestedLinks is not a function` (or import error).

- [ ] **Step 4: Add the `kind`/`weight`/`why` fields to `GLink`**

In `web/src/components/graph/graphData.ts`, replace the `GLink` interface (currently lines 16–20):

```ts
export interface GLink {
  source: string;
  target: string;
  state?: GraphState;
  // Overlay seam: undefined ≡ a real (explicit) edge — the three real-edge
  // builders leave it unset. "suggested" links carry engine similarity data.
  kind?: "real" | "suggested";
  weight?: number; // suggested-only: 0..1 similarity ranking → opacity/width
  why?: string | null; // suggested-only: provenance, shown via linkLabel tooltip
}
```

- [ ] **Step 5: Add the `SuggestedEdge` import**

At the top of `graphData.ts`, extend the contract import (currently `import type { GraphNode, GraphEdge } from "../../contract";`):

```ts
import type { GraphNode, GraphEdge, SuggestedEdge } from "../../contract";
```

- [ ] **Step 6: Implement `buildSuggestedLinks`**

Add to `graphData.ts` (place it after `buildAdjacency`, near the other builders):

```ts
/** Undirected pair key so a↔b compares equal regardless of direction. */
const pairKey = (a: string, b: string) => (a < b ? `${a}|${b}` : `${b}|${a}`);

/** Map engine SuggestedEdge[] → suggested GLink[] for overlay rendering.
 *  Drops any edge whose endpoint is not a visible node (suggestions never
 *  introduce nodes), and dedupes (undirected) against real links and against
 *  earlier suggestions — so a suggestion duplicating an explicit link, or a
 *  reciprocal duplicate, is suppressed. */
export function buildSuggestedLinks(
  suggestions: SuggestedEdge[],
  visibleNodeIds: Set<string>,
  realLinks: GLink[],
): GLink[] {
  const seen = new Set<string>();
  for (const l of realLinks) seen.add(pairKey(l.source, l.target));
  const out: GLink[] = [];
  for (const s of suggestions) {
    if (!visibleNodeIds.has(s.from) || !visibleNodeIds.has(s.to)) continue;
    const k = pairKey(s.from, s.to);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push({
      source: s.from,
      target: s.to,
      kind: "suggested",
      weight: s.weight,
      why: s.why,
    });
  }
  return out;
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `pnpm --dir web test -- graphData`
Expected: PASS (all `buildSuggestedLinks` cases green; existing graphData tests still green).

- [ ] **Step 8: Commit**

```bash
git add web/src/components/graph/graphData.ts web/src/components/graph/graphData.test.ts
git commit -m "feat(graph): add GLink.kind seam + buildSuggestedLinks helper"
```

---

## Task 2: Overlay settings persistence + scope helper

**Files:**
- Create: `web/src/components/graph/suggestionsOverlay.ts`
- Test: `web/src/components/graph/suggestionsOverlay.test.ts`

**Interfaces:**
- Produces:
  - `interface SuggestionsSettings { enabled: boolean }`
  - `const DEFAULT_SUGGESTIONS_SETTINGS: SuggestionsSettings` (`{ enabled: false }`)
  - `loadSuggestionsSettings(): SuggestionsSettings`, `saveSuggestionsSettings(s: SuggestionsSettings): void` (localStorage key `cairn.graph.suggestions`)
  - `suggestionScopeFor(enabled: boolean, localEnabled: boolean, activePath: string | null): SuggestionScope | null`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/suggestionsOverlay.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import {
  DEFAULT_SUGGESTIONS_SETTINGS,
  loadSuggestionsSettings,
  saveSuggestionsSettings,
  suggestionScopeFor,
} from "./suggestionsOverlay";

describe("suggestionScopeFor", () => {
  it("returns null when the overlay is disabled", () => {
    expect(suggestionScopeFor(false, true, "a.md")).toBeNull();
  });

  it("returns a note scope in local mode with an active note", () => {
    expect(suggestionScopeFor(true, true, "a.md")).toEqual({ type: "note", path: "a.md" });
  });

  it("returns vault scope in global mode", () => {
    expect(suggestionScopeFor(true, false, "a.md")).toEqual({ type: "vault" });
  });

  it("returns vault scope in local mode when no note is active", () => {
    expect(suggestionScopeFor(true, true, null)).toEqual({ type: "vault" });
  });
});

describe("suggestions settings persistence", () => {
  beforeEach(() => localStorage.clear());

  it("defaults to disabled when nothing is stored", () => {
    expect(loadSuggestionsSettings()).toEqual(DEFAULT_SUGGESTIONS_SETTINGS);
  });

  it("round-trips a saved setting", () => {
    saveSuggestionsSettings({ enabled: true });
    expect(loadSuggestionsSettings()).toEqual({ enabled: true });
  });

  it("falls back to default on malformed storage", () => {
    localStorage.setItem("cairn.graph.suggestions", "not json");
    expect(loadSuggestionsSettings()).toEqual(DEFAULT_SUGGESTIONS_SETTINGS);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm --dir web test -- suggestionsOverlay`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the module**

Create `web/src/components/graph/suggestionsOverlay.ts` (mirrors `recency.ts` load/save shape):

```ts
import type { SuggestionScope } from "../../contract";

export interface SuggestionsSettings {
  enabled: boolean;
}
export const DEFAULT_SUGGESTIONS_SETTINGS: SuggestionsSettings = {
  enabled: false,
};

const STORAGE_KEY = "cairn.graph.suggestions";

/** Suggestion scope follows the graph's own full/local mode: local mode with a
 *  note open → that note's suggestions; otherwise the whole vault. null when the
 *  overlay is off (no query should fire). */
export function suggestionScopeFor(
  enabled: boolean,
  localEnabled: boolean,
  activePath: string | null,
): SuggestionScope | null {
  if (!enabled) return null;
  if (localEnabled && activePath) return { type: "note", path: activePath };
  return { type: "vault" };
}

export function loadSuggestionsSettings(): SuggestionsSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_SUGGESTIONS_SETTINGS;
    const p = JSON.parse(raw) as Partial<SuggestionsSettings>;
    return { enabled: !!p.enabled };
  } catch {
    return DEFAULT_SUGGESTIONS_SETTINGS;
  }
}

export function saveSuggestionsSettings(s: SuggestionsSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pnpm --dir web test -- suggestionsOverlay`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/suggestionsOverlay.ts web/src/components/graph/suggestionsOverlay.test.ts
git commit -m "feat(graph): suggestions overlay settings + scope helper"
```

---

## Task 3: Store `loadSuggestions` action

**Files:**
- Modify: `web/src/store/store.ts`
- Test: existing store test file (locate in Step 1)

**Interfaces:**
- Consumes: `SuggestedEdge`, `SuggestionScope` from `../contract`; existing `client.runQuery`, `seq`, `setLoading`, `pushError`, `unexpected`.
- Produces:
  - state: `suggestions: SuggestedEdge[] | null`
  - loading flag: `loading.suggestions: boolean`
  - action: `loadSuggestions(scope: SuggestionScope): Promise<void>`

- [ ] **Step 1: Locate the store test file and confirm the mock-client pattern**

Run: `ls web/src/store/*.test.ts && grep -rln "loadGraph\|runQuery" web/src/store/*.test.ts`
Use the file that already tests store actions against a mock `CairnClient`. Read its top to copy the exact store-construction / mock-client helper (e.g. `createCairnStore(mockClient, ...)`). If no store test file exists, create `web/src/store/store.suggestions.test.ts` and build a minimal mock client exposing `runQuery` returning a resolved `QueryResponse`, mirroring how `loadGraph` is exercised elsewhere.

- [ ] **Step 2: Write the failing tests**

Add (to the located file, or the new one) — adapt `makeStore()` to the file's existing helper name:

```ts
import { describe, it, expect } from "vitest";
import type { Query, QueryResponse, SuggestedEdge } from "../contract";
// import { <existing store-with-mock-client helper> } from "./<...>";

const EDGE: SuggestedEdge = { from: "a.md", to: "b.md", weight: 0.7, why: "shared: x" };

describe("loadSuggestions", () => {
  it("populates suggestions on a suggestions response", async () => {
    const store = makeStore({
      runQuery: async (_q: Query): Promise<QueryResponse> => ({
        type: "suggestions",
        suggestions: [EDGE],
      }),
    });
    await store.getState().loadSuggestions({ type: "vault" });
    expect(store.getState().suggestions).toEqual([EDGE]);
  });

  it("passes the given scope through to runQuery", async () => {
    let seenQuery: Query | null = null;
    const store = makeStore({
      runQuery: async (q: Query): Promise<QueryResponse> => {
        seenQuery = q;
        return { type: "suggestions", suggestions: [] };
      },
    });
    await store.getState().loadSuggestions({ type: "note", path: "a.md" });
    expect(seenQuery).toEqual({ type: "get_suggestions", scope: { type: "note", path: "a.md" } });
  });

  it("leaves state untouched and reports an error on an unexpected variant", async () => {
    const store = makeStore({
      runQuery: async (): Promise<QueryResponse> => ({ type: "paths", paths: [] }),
    });
    await store.getState().loadSuggestions({ type: "vault" });
    expect(store.getState().suggestions).toBeNull();
    expect(store.getState().errors.length).toBeGreaterThan(0);
  });
});
```

> Note: `makeStore` here stands for the file's existing helper that builds a store over a partial mock client. Reuse it; do not invent a new construction path if one exists.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `pnpm --dir web test -- <store test file basename>`
Expected: FAIL — `loadSuggestions is not a function` / `suggestions` undefined.

- [ ] **Step 4: Add the contract import**

In `web/src/store/store.ts`, extend the existing contract import that already brings in `GraphNode` to also import `SuggestedEdge` and `SuggestionScope`. (Search for `GraphNode` in the import list at the top and add both names.)

- [ ] **Step 5: Add the state field to the `CairnState` interface**

In the `CairnState` interface, directly after the `graph: ...` line (currently line 155):

```ts
  suggestions: SuggestedEdge[] | null;
```

- [ ] **Step 6: Add the loading flag to the interface**

In the `loading: { ... }` block of the interface (currently lines 183–188), add `suggestions: boolean;`:

```ts
  loading: {
    search: boolean;
    graph: boolean;
    backlinks: boolean;
    note: boolean;
    suggestions: boolean;
  };
```

- [ ] **Step 7: Declare the action on the interface**

Immediately after `loadGraph(): Promise<void>;` (currently line 228):

```ts
  loadSuggestions(scope: SuggestionScope): Promise<void>;
```

- [ ] **Step 8: Add the `seq` token**

In the `seq` object (currently lines 264–270), add:

```ts
    suggestions: 0,
```

- [ ] **Step 9: Initialize state in BOTH initial-state blocks**

There are two spots that set `graph: null` and the `loading` object — the `openCairn` reset (~lines 486/497) and the store's returned initial state (~lines 548/563). In **each**:

- After `graph: null,` add:

```ts
        suggestions: null,
```

- In the `loading: { search: false, graph: false, backlinks: false, note: false },` literal, add `suggestions: false`:

```ts
        loading: { search: false, graph: false, backlinks: false, note: false, suggestions: false },
```

(Apply to both occurrences — the reset block and the returned-state block.)

- [ ] **Step 10: Implement the action**

Add the `loadSuggestions` method next to `loadGraph` in the returned action object (after the `loadGraph` closing `},` at line 1067), modeled on `refreshBacklinks`/`loadGraph`:

```ts
      async loadSuggestions(scope: SuggestionScope) {
        const token = ++seq.suggestions;
        setLoading("suggestions", true);
        try {
          const res = await client.runQuery({ type: "get_suggestions", scope });
          if (token !== seq.suggestions) return; // superseded by a newer request
          if (res.type === "suggestions") set({ suggestions: res.suggestions });
          else unexpected("Load suggestions", res, { scope: scope.type });
        } catch (err) {
          if (token !== seq.suggestions) return;
          pushError("Load suggestions", err, { scope: scope.type });
        } finally {
          if (token === seq.suggestions) setLoading("suggestions", false);
        }
      },
```

- [ ] **Step 11: Run the tests to verify they pass**

Run: `pnpm --dir web test -- <store test file basename>`
Expected: PASS.

- [ ] **Step 12: Typecheck (the interface has new required members)**

Run: `pnpm --dir web exec tsc --noEmit`
Expected: no errors. (If any other object literal constructs a full `CairnState`/`loading` and now complains, add the new fields there too.)

- [ ] **Step 13: Commit**

```bash
git add web/src/store/store.ts web/src/store/<store test file>
git commit -m "feat(store): loadSuggestions action + suggestions state/loading"
```

---

## Task 4: `GraphGroupsPanel` overlay toggle

**Files:**
- Modify: `web/src/components/graph/GraphGroupsPanel.tsx`
- Test: `web/src/components/graph/GraphGroupsPanel.test.tsx`

**Interfaces:**
- Consumes: `SuggestionsSettings` from `./suggestionsOverlay`.
- Produces: two new props on `GraphGroupsPanel` — `suggestions: SuggestionsSettings`, `onSuggestionsChange: (next: SuggestionsSettings) => void`. New checkbox labeled "Suggested links".

- [ ] **Step 1: Write the failing test**

Create (or append to) `web/src/components/graph/GraphGroupsPanel.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GraphGroupsPanel } from "./GraphGroupsPanel";

const baseProps = {
  groups: [],
  onChange: () => {},
  filter: { minDegree: 0, hiddenGroupQueries: [], hideUngrouped: false },
  onFilterChange: () => {},
  recency: { enabled: false, windowDays: 30 },
  onRecencyChange: () => {},
};

describe("GraphGroupsPanel suggested-links toggle", () => {
  it("reflects the enabled state and fires onSuggestionsChange when toggled", () => {
    const onSuggestionsChange = vi.fn();
    render(
      <GraphGroupsPanel
        {...baseProps}
        suggestions={{ enabled: false }}
        onSuggestionsChange={onSuggestionsChange}
      />,
    );
    const box = screen.getByLabelText("Suggested links") as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(box);
    expect(onSuggestionsChange).toHaveBeenCalledWith({ enabled: true });
  });
});
```

> If `FilterSettings` has fields beyond those in `baseProps.filter`, copy the exact shape from `web/src/components/graph/graphFilter.ts` so the render typechecks.

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm --dir web test -- GraphGroupsPanel`
Expected: FAIL — no element labeled "Suggested links" (and a TS error on the missing props).

- [ ] **Step 3: Add the import and props**

In `web/src/components/graph/GraphGroupsPanel.tsx`, add the import:

```ts
import type { SuggestionsSettings } from "./suggestionsOverlay";
```

Extend the props type (the object passed to `GraphGroupsPanel(props: { ... })`, currently ending at `onRecencyChange` on line 35) with:

```ts
  suggestions: SuggestionsSettings;
  onSuggestionsChange: (next: SuggestionsSettings) => void;
```

And extend the destructure (currently lines 37–38):

```ts
  const {
    groups,
    onChange,
    filter,
    onFilterChange,
    recency,
    onRecencyChange,
    suggestions,
    onSuggestionsChange,
  } = props;
```

- [ ] **Step 4: Add the toggle section**

Insert a new bordered section immediately **after** the recency block's closing `</div>` (currently line 176), before the panel's outer closing `</div>` (line 177):

```tsx
      {/* Suggested links overlay */}
      <div className="mt-3 border-t border-border pt-3">
        <label className="flex items-center gap-2 text-[11px] text-text">
          <input
            type="checkbox"
            aria-label="Suggested links"
            className="accent-accent"
            checked={suggestions.enabled}
            onChange={(e) => onSuggestionsChange({ enabled: e.target.checked })}
          />
          Suggested links
        </label>
      </div>
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `pnpm --dir web test -- GraphGroupsPanel`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/GraphGroupsPanel.tsx web/src/components/graph/GraphGroupsPanel.test.tsx
git commit -m "feat(graph): suggested-links toggle in GraphGroupsPanel"
```

---

## Task 5: `GraphView` wiring — merge, scope effect, link styling

**Files:**
- Modify: `web/src/components/GraphView.tsx`

**Interfaces:**
- Consumes: `buildSuggestedLinks` (Task 1), `SuggestionsSettings`/`loadSuggestionsSettings`/`saveSuggestionsSettings`/`suggestionScopeFor` (Task 2), `SuggestedEdge`/`SuggestionScope` (contract), `GraphGroupsPanel`'s new props (Task 4).
- Produces: two new props on `GraphView` — `suggestions: SuggestedEdge[] | null`, `onLoadSuggestions: (scope: SuggestionScope) => void`.

This task is mostly canvas/render wiring — **no unit test** (canvas paint output has no meaningful assertion; the pure logic it relies on is already covered in Tasks 1–2). Verified manually via `/run` in Task 7.

- [ ] **Step 1: Add imports**

In `web/src/components/GraphView.tsx`:

- Extend the contract import (currently `import type { GraphNode } from "../contract";`):

```ts
import type { GraphNode, SuggestedEdge, SuggestionScope } from "../contract";
```

- Add `buildSuggestedLinks` to the existing `./graph/graphData` import (currently imports `buildGraphData, buildGraphDataFromNodes, buildAdjacency, nodeRadius, labelAlpha`):

```ts
  buildSuggestedLinks,
```

- Add a new import for the overlay settings:

```ts
import {
  type SuggestionsSettings,
  loadSuggestionsSettings,
  saveSuggestionsSettings,
  suggestionScopeFor,
} from "./graph/suggestionsOverlay";
```

- [ ] **Step 2: Add the two new props**

Extend the `GraphView` props object (currently lines 57–64) with:

```ts
  suggestions: SuggestedEdge[] | null;
  onLoadSuggestions: (scope: SuggestionScope) => void;
```

- [ ] **Step 3: Add persisted toggle state**

Alongside the other panel-state trios (after the `recency` state, ~line 165), add:

```ts
  const [suggestOverlay, setSuggestOverlay] = useState<SuggestionsSettings>(
    loadSuggestionsSettings,
  );
  const changeSuggestOverlay = (next: SuggestionsSettings) => {
    setSuggestOverlay(next);
    saveSuggestionsSettings(next);
  };
```

- [ ] **Step 4: Derive the scope and fire the load effect**

After `liveData`/`liveAdj` are defined (after line 200), add:

```ts
  // Scope follows the graph's own full/local mode (see suggestionScopeFor).
  const suggestionScope = suggestionScopeFor(
    suggestOverlay.enabled,
    useLocal,
    props.activePath,
  );
  // Stable string key so vault scope doesn't refetch on every note switch.
  const scopeKey = suggestionScope
    ? suggestionScope.type === "note"
      ? `note:${suggestionScope.path}`
      : "vault"
    : null;
  const onLoadSuggestions = props.onLoadSuggestions;
  useEffect(() => {
    if (suggestionScope) onLoadSuggestions(suggestionScope);
    // scopeKey encodes enabled + scope + path; refetch only when it changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKey, onLoadSuggestions]);
```

> The eslint-disable is required because `suggestionScope` is a fresh object each render; `scopeKey` is the intentional primitive dependency. If the repo's eslint config does not flag this, drop the disable comment (verify in Step 8 — `just web-lint` output).

- [ ] **Step 5: Merge suggested links into the live build**

Immediately after the `liveData` memo (line 193), add a memo that appends suggested links only when the overlay is on:

```ts
  const suggestedLinks = useMemo(() => {
    if (!suggestOverlay.enabled || !props.suggestions) return [];
    const visible = new Set(liveData.nodes.map((n) => n.id));
    return buildSuggestedLinks(props.suggestions, visible, liveData.links);
  }, [suggestOverlay.enabled, props.suggestions, liveData]);
  const liveDataWithSuggestions = useMemo(
    () =>
      suggestedLinks.length
        ? { nodes: liveData.nodes, links: [...liveData.links, ...suggestedLinks] }
        : liveData,
    [liveData, suggestedLinks],
  );
```

Then change the `data` selector (currently line 205–211) to use the merged live data on the live path — replace `isLive ? liveData` with `isLive ? liveDataWithSuggestions`:

```ts
  const data = compareData
    ? compareData
    : isLive
      ? liveDataWithSuggestions
      : forcedGlobal
        ? globalData
        : (localData ?? globalData);
```

(Leave `adjacency` using `liveAdj` — adjacency stays real-links-only so hover/degree are unaffected by suggestions. The cap banner keeps reading `liveData.nodes.length`.)

- [ ] **Step 6: Extend the link style accessors**

- `linkColor` (currently lines 361–380): add a suggested branch at the top of the callback, before the `state` checks. Insert after the destructured `link` param body begins:

```ts
      if ((link as { kind?: string }).kind === "suggested") return "#8b8fa3aa";
```

  (Muted grey-violet; distinct from the real-link hover accent. Place it as the first `return` in the callback body.)

- `linkLineDash` (currently lines 383–386): replace with:

```ts
  const linkLineDash = useCallback(
    (link: { state?: string; kind?: string }) =>
      link.kind === "suggested"
        ? [4, 4]
        : link.state === "disappeared"
          ? [4, 3]
          : [],
    [],
  );
```

- Add a `linkWidth` accessor (currently the JSX passes the constant `linkWidth={1}`). Add this callback near the other link callbacks:

```ts
  // Suggested links get width from their similarity weight (0..1); real links
  // stay at the constant 1.
  const linkWidth = useCallback(
    (link: { kind?: string; weight?: number }) =>
      link.kind === "suggested" ? 0.5 + (link.weight ?? 0) * 2 : 1,
    [],
  );
```

- Add a `linkLabel` accessor (native hover tooltip for `why`):

```ts
  const linkLabel = useCallback(
    (link: { kind?: string; why?: string | null }) =>
      link.kind === "suggested" ? (link.why ?? "") : "",
    [],
  );
```

- [ ] **Step 7: Wire the accessors into `<ForceGraph2D>` and pass panel props**

- In the `<ForceGraph2D ... />` props (currently line 534 `linkWidth={1}`), replace `linkWidth={1}` with `linkWidth={linkWidth}` and add `linkLabel={linkLabel}`:

```tsx
            linkColor={linkColor}
            linkLineDash={linkLineDash}
            linkWidth={linkWidth}
            linkLabel={linkLabel}
```

- Pass the new props to `<GraphGroupsPanel>` (currently lines 501–508):

```tsx
            <GraphGroupsPanel
              groups={groups}
              onChange={changeGroups}
              filter={filter}
              onFilterChange={changeFilter}
              recency={recency}
              onRecencyChange={changeRecency}
              suggestions={suggestOverlay}
              onSuggestionsChange={changeSuggestOverlay}
            />
```

- [ ] **Step 8: Typecheck + lint**

Run: `pnpm --dir web exec tsc --noEmit && just web-lint`
Expected: no type errors; lint clean (adjust/remove the eslint-disable from Step 4 per actual lint output).

- [ ] **Step 9: Commit**

```bash
git add web/src/components/GraphView.tsx
git commit -m "feat(graph): render suggested-edge overlay in GraphView"
```

---

## Task 6: `EditorPane` adapter wiring

**Files:**
- Modify: `web/src/components/EditorPane.tsx`

**Interfaces:**
- Consumes: store `s.suggestions` and `actions.loadSuggestions` (auto-exposed by `useActions()`).
- Produces: passes `suggestions` + `onLoadSuggestions` into `<GraphView>`.

- [ ] **Step 1: Read `suggestions` from the store**

In `EditorPane.tsx`, after `const noteTags = useCairn((s) => s.noteTags);` (line 96):

```ts
  const suggestions = useCairn((s) => s.suggestions);
```

- [ ] **Step 2: Pass the new props to `<GraphView>`**

In the `<GraphView ... />` block (currently lines 158–165), add:

```tsx
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                tagsByNote={noteTags}
                activePath={activePath}
                loading={loading.graph}
                onOpenNote={(p) => navigate(noteUrl(p))}
                suggestions={suggestions}
                onLoadSuggestions={actions.loadSuggestions}
              />
```

- [ ] **Step 3: Typecheck**

Run: `pnpm --dir web exec tsc --noEmit`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/EditorPane.tsx
git commit -m "feat(graph): wire suggestions store slice into GraphView"
```

---

## Task 7: Full gate + manual verification + PR

**Files:** none (verification + integration).

- [ ] **Step 1: Run the full web CI gate**

Run: `just web-ci`
Expected: all green — typecheck, lint, **`prettier --check`**, and the full test suite. Fix any prettier formatting on the files you touched (NOT contract files). Re-run until clean.

- [ ] **Step 2: Manual verification via the real app**

Use the `/run` skill (or `just` dev target) to launch the app. Verify, per verification-before-completion:
- Open the graph view. Toggle **Suggested links** on in the panel → dashed overlay links appear between existing nodes; stronger suggestions render wider/less faint.
- Hover a suggested link → native tooltip shows the `why` string (or nothing when `why` is null).
- Switch to **local** mode with a note open → overlay updates to that note's suggestions; switching notes refetches. In **global** mode, switching notes does not refetch (vault scope stable).
- Toggle off → overlay disappears; real edges and hover-highlight behave exactly as before.
- Confirm suggested links never appear to a node that isn't visible, and never duplicate an existing solid edge.

Record what you observed (a sentence per check) in the PR description.

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin HEAD
gh pr create --base main --title "feat(graph): suggested-edges overlay" --body "$(cat <<'EOF'
Renders engine-suggested (non-explicit) note links as a toggleable dashed overlay on the force graph. Default OFF; scope auto-follows the graph's full/local mode (vault vs note).

Spec: docs/superpowers/specs/2026-07-23-suggested-edges-overlay-design.md
Plan: docs/superpowers/plans/2026-07-23-suggested-edges-overlay.md

## What
- `GLink.kind` seam + `buildSuggestedLinks` (visibility filter + undirected dedup vs real links)
- `loadSuggestions(scope)` store action over `get_suggestions` (raw contract types, no ACL)
- Panel toggle in GraphGroupsPanel; weight→width/opacity, `why` via linkLabel tooltip

## Verification
<paste the per-check observations from Step 2>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Merge via the merge queue**

Do NOT merge directly. Once CI is green and the PR is approved, use GitHub's **"Merge when ready"** (merge queue). Never push to `main` directly.

---

## Self-Review Notes

- **Spec coverage:** Q1 scope→`suggestionScopeFor` + GraphView effect (Task 2/5); Q2 drop-missing-endpoint→`buildSuggestedLinks` (Task 1); Q3 toggle-only→panel checkbox (Task 4); Q4 `why` tooltip→`linkLabel` (Task 5). Data model `GLink.kind` seam (Task 1). Store transport (Task 3). Lifecycle default-OFF/persist/token-guard/vault-stable (Tasks 2,3,5). Testing split: pure logic covered (Tasks 1–4), canvas explicitly manual (Task 5/7). Gate + merge-queue (Task 7).
- **Type consistency:** `buildSuggestedLinks(suggestions, visibleNodeIds, realLinks)` used identically in Task 1 and Task 5. `SuggestionsSettings`/`suggestionScopeFor` signatures match across Tasks 2/4/5. `loadSuggestions(scope: SuggestionScope)` matches interface decl (Task 3) and EditorPane call (Task 6). `suggestions: SuggestedEdge[] | null` consistent store→EditorPane→GraphView.
- **Adjacency isolation:** suggested links merged only into render `data`, never into `liveAdj` — hover-highlight and degree unaffected.
