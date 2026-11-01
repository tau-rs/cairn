# UI‑4c Graph Color Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Color graph nodes by user-defined groups (`{kind: "path"|"tag", query, color}`) via a Groups section in the graph settings panel; tags come from a new `noteTags()` client capability (mock parses real fixture tags; Tauri stubs `{}`).

**Architecture:** Pure `tags.ts` (`extractTags`) + `colorGroups.ts` (model + localStorage + `matchGroupColor`); a `noteTags()` method on `CairnClient` (Mock parses, Tauri stubs); the store loads `noteTags` with the graph; a presentational `GraphGroupsPanel`; `GraphView` holds the groups, renders the panel above Forces, and resolves each node's fill via `matchGroupColor` (first-match-wins), preserving the UI‑4a active/hover logic. No re-simulation on group/tag change.

**Tech Stack:** React 18 + TypeScript, `react-force-graph-2d`, Vite, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-09-ui4c-graph-color-groups-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if format fails.
- e2e on port 5273 (configured). Current: 141 unit, 8 e2e, all green.
- **Relevant existing code:**
  - `CairnClient` (`web/src/client/types.ts`): `{ sendCommand; runQuery; subscribe }`. You ADD `noteTags(): Promise<Record<string, string[]>>`.
  - `MockClient` (`web/src/client/mock.ts`): holds `this.notes: Map<string,string>` (path→content); already has frontmatter helpers. Implement `noteTags` by `extractTags` over `this.notes`.
  - `TauriClient` (`web/src/client/tauri.ts`): implements CairnClient via `invoke`. Stub `noteTags` → `Promise.resolve({})`.
  - Store (`web/src/store/store.ts`): `client` is captured in `createCairnStore`; `loadGraph()` (line ~248) does `client.runQuery({type:"get_graph"})` → `set({ graph: {nodes, edges} })`. State has `graph: {nodes;edges} | null`. App reads `graph` via `useCairn` and passes `nodes`/`edges` to `<GraphView>` (App.tsx ~117).
  - `GraphView` (UI-4a/4b): `<ForceGraph2D>` + `paintNode` (`useCallback` deps `[props.activePath, adjacency]`, fill `active ? "#6366f1" : lit ? "#cdd0e0" : "#6b6c7755"`); gear overlay already renders `<GraphForcesPanel>`; `RFNode = {id;label;degree;x?;y?;fx?;fy?}`.
  - localStorage persistence pattern: see `web/src/components/graph/forceSettings.ts` (UI-4b).
  - jsdom provides `localStorage` (the UI-4b `vitest.setup.ts` fix is in place).
- Canvas can't be unit-tested under jsdom — unit-test the pure modules + the DOM panel; recoloring is manual-visual (UI-4a/4b precedent).

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/graph/tags.ts` | Pure `extractTags(markdown)`. |
| `web/src/components/graph/colorGroups.ts` | `ColorGroup`, load/save (localStorage), `matchGroupColor`. |
| `web/src/components/graph/GraphGroupsPanel.tsx` | Presentational group editor. |
| `web/src/client/types.ts` / `mock.ts` / `tauri.ts` | `noteTags()` seam. |
| `web/src/store/store.ts` | Load `noteTags` in `loadGraph`; new state field. |
| `web/src/app/App.tsx` | Pass `tagsByNote` to GraphView. |
| `web/src/components/GraphView.tsx` | Groups state + panel + `paintNode` coloring. |
| `web/e2e/skeleton.spec.ts` | Groups section + add-group e2e. |

---

## Task 1: extractTags (pure)

**Files:**
- Create: `web/src/components/graph/tags.ts`
- Create: `web/src/components/graph/tags.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/tags.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { extractTags } from "./tags";

describe("extractTags", () => {
  it("reads a frontmatter inline list", () => {
    expect(extractTags("---\ntags: [Alpha, beta]\n---\nbody")).toEqual([
      "alpha",
      "beta",
    ]);
  });
  it("reads a frontmatter comma list", () => {
    expect(extractTags("---\ntags: a, b\n---\n")).toEqual(["a", "b"]);
  });
  it("reads a frontmatter block list", () => {
    expect(
      extractTags("---\ntags:\n  - one\n  - two\ntitle: x\n---\nbody"),
    ).toEqual(["one", "two"]);
  });
  it("reads inline #tags from the body, lowercased and deduped", () => {
    expect(extractTags("see #Idea and #idea and #graph-view here")).toEqual([
      "idea",
      "graph-view",
    ]);
  });
  it("combines frontmatter + inline and returns [] when none", () => {
    expect(extractTags("---\ntags: [x]\n---\nbody #y")).toEqual(["x", "y"]);
    expect(extractTags("plain note, no tags")).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- tags`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `tags.ts`**

Create `web/src/components/graph/tags.ts`:

```ts
/** Tags from a note's markdown: a frontmatter `tags:` key (inline `[a, b]` /
 *  `a, b`, or a `- item` block list) plus inline `#tag` tokens in the body.
 *  Lowercased, deduped, order preserved. (A simple scan — does not exclude `#`
 *  inside code spans/fences; acceptable for v1.) */
export function extractTags(markdown: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const add = (raw: string) => {
    const t = raw.trim().replace(/^['"]|['"]$/g, "").toLowerCase();
    if (t && !seen.has(t)) {
      seen.add(t);
      out.push(t);
    }
  };

  let body = markdown;
  if (markdown.startsWith("---\n")) {
    const end = markdown.indexOf("\n---", 4);
    if (end !== -1) {
      const fm = markdown.slice(4, end);
      body = markdown.slice(end + 4);
      const lines = fm.split("\n");
      const i = lines.findIndex((l) => /^tags:/.test(l));
      if (i !== -1) {
        const inline = lines[i].replace(/^tags:\s*/, "").trim();
        if (inline.startsWith("[")) {
          inline
            .replace(/^\[|\]$/g, "")
            .split(",")
            .forEach(add);
        } else if (inline) {
          inline.split(",").forEach(add);
        } else {
          // block list: subsequent `- item` lines until a non-list line
          for (let j = i + 1; j < lines.length; j++) {
            const m = /^\s*-\s*(.+)$/.exec(lines[j]);
            if (m) add(m[1]);
            else break;
          }
        }
      }
    }
  }

  const re = /(?:^|\s)#([A-Za-z0-9_/-]+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(body)) !== null) add(m[1]);
  return out;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- tags`
Expected: PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (141 + new tags tests).

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/tags.ts web/src/components/graph/tags.test.ts
git commit -m "feat(graph): extractTags (frontmatter + inline #tags)"
```

---

## Task 2: colorGroups (model + persistence + match)

**Files:**
- Create: `web/src/components/graph/colorGroups.ts`
- Create: `web/src/components/graph/colorGroups.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/colorGroups.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  type ColorGroup,
  loadColorGroups,
  saveColorGroups,
  matchGroupColor,
} from "./colorGroups";

beforeEach(() => localStorage.clear());

describe("loadColorGroups / saveColorGroups", () => {
  it("returns [] when empty or corrupt", () => {
    expect(loadColorGroups()).toEqual([]);
    localStorage.setItem("cairn.graph.groups", "{not json");
    expect(loadColorGroups()).toEqual([]);
  });
  it("drops malformed entries, keeps valid ones", () => {
    localStorage.setItem(
      "cairn.graph.groups",
      JSON.stringify([
        { kind: "path", query: "projects", color: "#6366f1" },
        { kind: "bogus", query: "x", color: "#fff" },
        { query: "no-kind" },
      ]),
    );
    expect(loadColorGroups()).toEqual([
      { kind: "path", query: "projects", color: "#6366f1" },
    ]);
  });
  it("round-trips", () => {
    const g: ColorGroup[] = [{ kind: "tag", query: "idea", color: "#f59e0b" }];
    saveColorGroups(g);
    expect(loadColorGroups()).toEqual(g);
  });
  it("swallows storage errors", () => {
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveColorGroups([])).not.toThrow();
    spy.mockRestore();
  });
});

describe("matchGroupColor", () => {
  const groups: ColorGroup[] = [
    { kind: "path", query: "Projects", color: "#6366f1" },
    { kind: "tag", query: "idea", color: "#f59e0b" },
  ];
  it("matches a path query case-insensitively (substring)", () => {
    expect(matchGroupColor("projects/app.md", [], groups)).toBe("#6366f1");
  });
  it("matches a tag query (exact, case-insensitive)", () => {
    expect(matchGroupColor("notes/x.md", ["idea"], groups)).toBe("#f59e0b");
  });
  it("returns the first matching group's color", () => {
    // path 'Projects' matches first even though the tag would also match
    expect(matchGroupColor("projects/x.md", ["idea"], groups)).toBe("#6366f1");
  });
  it("returns null when nothing matches and ignores empty queries", () => {
    expect(matchGroupColor("notes/x.md", ["other"], groups)).toBeNull();
    expect(
      matchGroupColor("anything", [], [{ kind: "path", query: "", color: "#fff" }]),
    ).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- colorGroups`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `colorGroups.ts`**

Create `web/src/components/graph/colorGroups.ts`:

```ts
export interface ColorGroup {
  kind: "path" | "tag";
  query: string;
  color: string;
}

export const DEFAULT_COLOR_GROUPS: ColorGroup[] = [];

const STORAGE_KEY = "cairn.graph.groups";

const isValid = (g: unknown): g is ColorGroup =>
  typeof g === "object" &&
  g !== null &&
  ((g as ColorGroup).kind === "path" || (g as ColorGroup).kind === "tag") &&
  typeof (g as ColorGroup).query === "string" &&
  typeof (g as ColorGroup).color === "string";

export function loadColorGroups(): ColorGroup[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isValid);
  } catch {
    return [];
  }
}

export function saveColorGroups(groups: ColorGroup[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(groups));
  } catch {
    // ignore (private mode / quota)
  }
}

/** First group that matches the node → its color; else null.
 *  path: case-insensitive substring of the path. tag: exact (case-insensitive)
 *  membership in the note's tags (tags are already lowercased by extractTags). */
export function matchGroupColor(
  path: string,
  tags: string[],
  groups: ColorGroup[],
): string | null {
  const lowerPath = path.toLowerCase();
  for (const g of groups) {
    const q = g.query.trim().toLowerCase();
    if (!q) continue;
    if (g.kind === "path" ? lowerPath.includes(q) : tags.includes(q)) {
      return g.color;
    }
  }
  return null;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- colorGroups`
Expected: PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/colorGroups.ts web/src/components/graph/colorGroups.test.ts
git commit -m "feat(graph): color-group model + localStorage + matchGroupColor"
```

---

## Task 3: noteTags() client seam + store wiring

**Files:**
- Modify: `web/src/client/types.ts`, `web/src/client/mock.ts`, `web/src/client/tauri.ts`
- Modify: `web/src/store/store.ts`, `web/src/app/App.tsx`
- Modify: `web/src/client/mock.test.ts` (add a noteTags test) + any client mocks in tests

- [ ] **Step 1: Add `noteTags` to the `CairnClient` interface**

In `web/src/client/types.ts`, inside `interface CairnClient`, add:

```ts
  /** All notes' tags (path → tags). Client-level capability (not a contract
   *  Query): the mock parses note content; Tauri stubs {} until the engine
   *  exposes tags. */
  noteTags(): Promise<Record<string, string[]>>;
```

- [ ] **Step 2: Implement in `MockClient`**

In `web/src/client/mock.ts`, import `extractTags`:

```ts
import { extractTags } from "../components/graph/tags";
```

Add the method to the class (it has `this.notes: Map<string,string>`):

```ts
  noteTags(): Promise<Record<string, string[]>> {
    const out: Record<string, string[]> = {};
    for (const [path, content] of this.notes) out[path] = extractTags(content);
    return Promise.resolve(out);
  }
```

- [ ] **Step 3: Stub in `TauriClient`**

In `web/src/client/tauri.ts`, add to the class:

```ts
  noteTags(): Promise<Record<string, string[]>> {
    // Stub: the engine does not expose tags yet. Swap for a query when it does.
    return Promise.resolve({});
  }
```

- [ ] **Step 4: Load `noteTags` into the store with the graph**

In `web/src/store/store.ts`:

Add to the state interface (near `graph`):

```ts
  noteTags: Record<string, string[]>;
```

Add to the initial state (near `graph: null`):

```ts
    noteTags: {},
```

Extend `loadGraph` to also load tags (graph failure path unchanged; tags failure is non-fatal):

```ts
    async loadGraph() {
      try {
        const res = await client.runQuery({ type: "get_graph" });
        if (res.type === "graph")
          set({ graph: { nodes: res.nodes, edges: res.edges } });
      } catch (err) {
        set({ error: errMsg(err) });
      }
      try {
        set({ noteTags: await client.noteTags() });
      } catch {
        set({ noteTags: {} });
      }
    },
```

- [ ] **Step 5: Pass `tagsByNote` to `GraphView` in `App.tsx`**

In `web/src/app/App.tsx`, add a selector (near the `graph` selector):

```tsx
  const noteTags = useCairn((s) => s.noteTags);
```

And pass it to `<GraphView>`:

```tsx
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                tagsByNote={noteTags}
                activePath={activePath}
                onOpenNote={(p) => {
                  void actions.openNote(p);
                  setView("editor");
                }}
              />
```

(GraphView gains the `tagsByNote` prop in Task 5; until then typecheck will flag the unused prop on the call — that's expected mid-task. If you prefer a green intermediate, do Step 5's prop-pass together with Task 5. Either is fine, but the per-task gate below must end green, so if Task 5 isn't done yet, add the `tagsByNote` prop to `GraphView`'s props type now as an accepted-but-unused prop.)

- [ ] **Step 6: Update test client mocks + add a noteTags test**

Any test that constructs a `CairnClient` mock now needs `noteTags`. Search: `grep -rn "implements CairnClient\|runQuery:" web/src --include=*.test.ts` and add `noteTags: () => Promise.resolve({})` to those mocks (and to `makeBackend`/store test doubles if present). In `web/src/client/mock.test.ts`, add:

```ts
it("noteTags parses tags from note content", async () => {
  const c = new MockClient({
    "a.md": "---\ntags: [x, y]\n---\nbody #z",
    "b.md": "plain",
  });
  expect(await c.noteTags()).toEqual({ "a.md": ["x", "y", "z"], "b.md": [] });
});
```

To keep the GraphView typecheck green this task, add the prop to `GraphView`'s props type now (used in Task 5):

```tsx
  tagsByNote: Record<string, string[]>;
```

- [ ] **Step 7: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS. `noteTags` is required on `CairnClient`, so typecheck enforces every implementation/mock provides it. (`GraphView` accepts `tagsByNote` but may not yet use it — fine; Task 5 wires the paint.)

- [ ] **Step 8: Commit**

```bash
git add web/src/client/types.ts web/src/client/mock.ts web/src/client/tauri.ts web/src/client/mock.test.ts web/src/store/store.ts web/src/app/App.tsx web/src/components/GraphView.tsx
git commit -m "feat(graph): noteTags() client seam (mock parses, tauri stubs) + store/App wiring"
```

---

## Task 4: GraphGroupsPanel (presentational)

**Files:**
- Create: `web/src/components/graph/GraphGroupsPanel.tsx`
- Create: `web/src/components/graph/GraphGroupsPanel.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/GraphGroupsPanel.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GraphGroupsPanel } from "./GraphGroupsPanel";
import type { ColorGroup } from "./colorGroups";

const groups: ColorGroup[] = [
  { kind: "path", query: "projects", color: "#6366f1" },
];

describe("GraphGroupsPanel", () => {
  it("renders a row per group (kind, query, color)", () => {
    render(<GraphGroupsPanel groups={groups} onChange={vi.fn()} />);
    expect(screen.getByDisplayValue("projects")).toBeInTheDocument();
    expect(screen.getByLabelText("Group kind")).toHaveValue("path");
  });
  it("Add group appends a default group", () => {
    const onChange = vi.fn();
    render(<GraphGroupsPanel groups={[]} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: /add group/i }));
    expect(onChange).toHaveBeenCalledWith([
      { kind: "path", query: "", color: "#6366f1" },
    ]);
  });
  it("editing the query fires onChange for that row", () => {
    const onChange = vi.fn();
    render(<GraphGroupsPanel groups={groups} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Group query"), {
      target: { value: "journal" },
    });
    expect(onChange).toHaveBeenCalledWith([
      { kind: "path", query: "journal", color: "#6366f1" },
    ]);
  });
  it("changing the kind fires onChange", () => {
    const onChange = vi.fn();
    render(<GraphGroupsPanel groups={groups} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Group kind"), {
      target: { value: "tag" },
    });
    expect(onChange).toHaveBeenCalledWith([
      { kind: "tag", query: "projects", color: "#6366f1" },
    ]);
  });
  it("remove drops the row", () => {
    const onChange = vi.fn();
    render(<GraphGroupsPanel groups={groups} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: /remove group/i }));
    expect(onChange).toHaveBeenCalledWith([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- GraphGroupsPanel`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `GraphGroupsPanel.tsx`**

Create `web/src/components/graph/GraphGroupsPanel.tsx`:

```tsx
import type { ColorGroup } from "./colorGroups";

export function GraphGroupsPanel(props: {
  groups: ColorGroup[];
  onChange: (next: ColorGroup[]) => void;
}) {
  const { groups, onChange } = props;
  const update = (i: number, patch: Partial<ColorGroup>) =>
    onChange(groups.map((g, j) => (j === i ? { ...g, ...patch } : g)));
  const remove = (i: number) => onChange(groups.filter((_, j) => j !== i));
  const add = () =>
    onChange([...groups, { kind: "path", query: "", color: "#6366f1" }]);

  return (
    <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
      <div className="mb-2 text-[10px] uppercase tracking-wide text-faint">
        Groups
      </div>
      {groups.map((g, i) => (
        <div key={i} className="mb-2 flex items-center gap-1.5">
          <select
            aria-label="Group kind"
            className="rounded border border-border bg-bg px-1 py-0.5 text-[11px] text-text"
            value={g.kind}
            onChange={(e) =>
              update(i, { kind: e.target.value as ColorGroup["kind"] })
            }
          >
            <option value="path">Path</option>
            <option value="tag">Tag</option>
          </select>
          <input
            type="text"
            aria-label="Group query"
            className="min-w-0 flex-1 rounded border border-border bg-bg px-1.5 py-0.5 text-[11px] text-text"
            value={g.query}
            onChange={(e) => update(i, { query: e.target.value })}
          />
          <input
            type="color"
            aria-label="Group color"
            className="h-5 w-5 flex-none rounded border border-border bg-bg"
            value={g.color}
            onChange={(e) => update(i, { color: e.target.value })}
          />
          <button
            type="button"
            aria-label="Remove group"
            className="flex-none text-faint hover:text-text"
            onClick={() => remove(i)}
          >
            ×
          </button>
        </div>
      ))}
      <button
        type="button"
        className="text-[11px] text-accent hover:underline"
        onClick={add}
      >
        + Add group
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- GraphGroupsPanel`
Expected: PASS (5 tests).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/GraphGroupsPanel.tsx web/src/components/graph/GraphGroupsPanel.test.tsx
git commit -m "feat(graph): presentational color-groups panel"
```

---

## Task 5: Wire groups into GraphView + e2e

**Files:**
- Modify: `web/src/components/GraphView.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Wire groups state + panel + node coloring into `GraphView`**

In `web/src/components/GraphView.tsx`:

Add imports:

```tsx
import { GraphGroupsPanel } from "./graph/GraphGroupsPanel";
import {
  type ColorGroup,
  loadColorGroups,
  saveColorGroups,
  matchGroupColor,
} from "./graph/colorGroups";
```

Add state (near the `forces`/`panelOpen` state):

```tsx
  const [groups, setGroups] = useState<ColorGroup[]>(loadColorGroups);
  const changeGroups = (next: ColorGroup[]) => {
    setGroups(next);
    saveColorGroups(next);
  };
```

Update `paintNode`'s fill to use the group color, preserving active/hover. Replace the fill computation (the `const active = …; const lit = …; ctx.fillStyle = active ? … : lit ? … : …` portion) with:

```tsx
      const active = node.id === props.activePath;
      const inHL = hl ? hl.has(node.id) : true;
      const r = nodeRadius(node.degree);
      const base = active
        ? "#6366f1"
        : (matchGroupColor(node.id, props.tagsByNote[node.id] ?? [], groups) ??
          "#cdd0e0");

      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, r, 0, 2 * Math.PI);
      // Hover focus: dim non-neighbors (keep their group hue at low alpha).
      ctx.globalAlpha = hl && !inHL && !active ? 0.25 : 1;
      ctx.fillStyle = base;
      ctx.fill();
      ctx.globalAlpha = 1;
```

(Keep the existing label-drawing block below it exactly as-is. The `hl` set is computed earlier in `paintNode` — leave that block in place. This replacement re-declares `const active` and `const r`, replacing the originals — make sure you remove the old `const active`/`const lit`/`const r` lines you're replacing so there are no duplicate declarations; `const r` is still used by the label block below.) Add `groups` and `props.tagsByNote` to `paintNode`'s `useCallback` dependency array (so it repaints on group/tag change) — they are NOT added to the `graphData` memo (no re-simulation).

Render the Groups panel in the gear overlay, ABOVE `<GraphForcesPanel>`:

```tsx
        {panelOpen && (
          <>
            <GraphGroupsPanel groups={groups} onChange={changeGroups} />
            <GraphForcesPanel
              settings={forces}
              onChange={changeForces}
              onReset={() => changeForces(DEFAULT_FORCE_SETTINGS)}
            />
          </>
        )}
```

(Replace the existing single `{panelOpen && <GraphForcesPanel … />}` with the fragment above — the overlay container already stacks its children vertically with `gap-2`.)

- [ ] **Step 2: Per-task gate (component is canvas — no new unit test here)**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. `props.tagsByNote` is now used (added in Task 3). Build confirms wiring.

- [ ] **Step 3: Add the e2e**

In `web/e2e/skeleton.spec.ts`, extend the graph test (after the gear-opens-Forces assertions from UI-4b):

```ts
  // Color groups: the Groups section is present and Add group adds a row.
  await expect(page.getByText("Groups", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /add group/i }).click();
  await expect(page.getByLabelText("Group query")).toBeVisible();
```

(Place inside the existing graph test, after the gear has been clicked open. Keep all prior assertions.)

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e`
Expected: 8/8. If "Groups"/"Add group"/"Group query" aren't found, check the panel renders both sections when open and the labels match. If a real failure, STOP and report.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 6: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop it. Report the app loads. (The human confirms: adding a Path/Tag group recolors matching nodes live; first-match-wins; remove reverts; colors persist across reload; active note stays accent. Note: the mock fixtures may have no tags — to see a Tag group color something, a tagged fixture note helps; Path groups work regardless.)

- [ ] **Step 7: Commit**

```bash
git add web/src/components/GraphView.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(graph): color nodes by groups (panel + paint integration)"
```

---

## Notes for the executor

- **No re-simulation on group/tag change.** `groups` + `props.tagsByNote` feed `paintNode` (a `useCallback` dep → repaint) but are NOT in the `graphData` memo deps — editing a group must not re-lay-out the graph (UI-4a invariant, same as UI-4b's forces effect).
- **Active overrides group color**; hover dims non-neighbors by lowering alpha while keeping their group hue (don't revert to flat grey). Keep the existing label block in `paintNode` unchanged.
- **`noteTags` is a client capability, not a contract Query** — mirror how `host.assetUrl` was added. Tauri returns `{}` (stub); the mock parses fixtures. Every `CairnClient` test double must gain `noteTags` (typecheck enforces).
- **Tags are lowercased by `extractTags`**, and `matchGroupColor` lowercases the query — so tag matching is exact-case-insensitive; path matching is substring-case-insensitive.
- **Canvas recoloring is manual-visual** — pure modules (`tags`, `colorGroups`) + the DOM panel are unit-tested; the e2e only asserts the panel/section. Consistent with UI-4a/4b.
- **localStorage** read/write is try/catch-guarded (as UI-4b); jsdom localStorage works via the existing `vitest.setup.ts` fix.
