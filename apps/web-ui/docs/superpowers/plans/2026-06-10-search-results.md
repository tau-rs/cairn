# Rich Search Results Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make search work against the real engine (`search_results`, not `paths`) and show each hit's snippet with the matched text highlighted in the results overlay.

**Architecture:** A pure `splitSnippet` highlight-splitter; the mock's `search` returns the rich shape and the store consumes it (one task, since they're interdependent); the store keeps `searchResults: string[]` and adds a parallel `searchSnippets` map; `SearchResults` renders an optional highlighted snippet line. The contract is already synced.

**Tech Stack:** React 18 + TypeScript, Zustand, Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-10-search-results-design.md`

**Working conventions (read before starting):**
- Run `pnpm` from `web/`. Git from repo root.
- Per-task gate: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. `pnpm format` + re-stage if needed. Ignore stale LSP noise — trust `pnpm typecheck`.
- e2e on port 5273. Baseline (post-real-tags merge on `main`): 255 unit, 14 e2e green.
- **Relevant existing code:**
  - Contract (synced): `QueryResponse` has `{type:"search_results", results: Array<SearchResult>}`; `SearchResult = { path; score; snippet; highlights: Array<[number,number]> }` (`src/contract/SearchResult.ts`). `src/contract/index.ts` is a hand-written barrel.
  - `MockClient.runQuery` `case "search"` currently returns `{type:"paths", paths}` using `splitFrontmatter(raw).body`. `get_backlinks`/`notes_by_tag` return `{type:"paths"}` and DON'T change.
  - Store: `searchResults: string[] | null`, `activeTag`, `runSearch` (sets `{query, searchResults: res.paths, activeTag: null}` on `res.type==="paths"`), `filterByTag` (sets `{searchResults: res.paths, activeTag: tag}`), `closeSearch` (`{searchResults: null, activeTag: null}`), `openCairn` reset, and an activeTag-aware note-event refresh.
  - `SearchResults.tsx` (post-real-tags): `{results, onOpen, onClose, title?}`; `max-h-[60vh]` scroll; renders path buttons. Existing tests cover default/custom title + click.
  - App renders `<SearchResults results={searchResults} title={activeTag ? ...} onOpen onClose />` in the editor slot.
  - Tailwind tokens: `surface`, `surface-2`, `border`, `text`, `muted`, `faint`, `accent`.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/searchHighlight.ts` | Pure `SearchSnippet`/`SnippetSegment` types + `splitSnippet`. |
| `web/src/client/mock.ts` + `web/src/contract/index.ts` | `search` → `search_results`; barrel-export `SearchResult`. |
| `web/src/store/store.ts` | `searchSnippets` state; `runSearch` consumes `search_results`; clear it elsewhere. |
| `web/src/components/SearchResults.tsx` | Optional `snippets` prop → highlighted snippet line. |
| `web/src/app/App.tsx` | Select + pass `searchSnippets`. |
| `web/e2e/skeleton.spec.ts` | Search-snippet e2e. |

---

## Task 1: searchHighlight (pure)

**Files:**
- Create: `web/src/components/searchHighlight.ts`
- Create: `web/src/components/searchHighlight.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/searchHighlight.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { splitSnippet } from "./searchHighlight";

describe("splitSnippet", () => {
  it("no highlights → one plain segment", () => {
    expect(splitSnippet("alpha note", [])).toEqual([
      { text: "alpha note", match: false },
    ]);
  });
  it("empty snippet → no segments", () => {
    expect(splitSnippet("", [])).toEqual([]);
  });
  it("a single leading range splits into match + plain", () => {
    expect(splitSnippet("alpha note", [[0, 5]])).toEqual([
      { text: "alpha", match: true },
      { text: " note", match: false },
    ]);
  });
  it("a mid range yields plain/match/plain", () => {
    expect(splitSnippet("abcdef", [[2, 4]])).toEqual([
      { text: "ab", match: false },
      { text: "cd", match: true },
      { text: "ef", match: false },
    ]);
  });
  it("multiple ranges, in order", () => {
    expect(splitSnippet("a b a", [[0, 1], [4, 5]])).toEqual([
      { text: "a", match: true },
      { text: " b ", match: false },
      { text: "a", match: true },
    ]);
  });
  it("merges overlapping/adjacent ranges", () => {
    expect(splitSnippet("abcdef", [[0, 3], [2, 5]])).toEqual([
      { text: "abcde", match: true },
      { text: "f", match: false },
    ]);
  });
  it("clamps out-of-range ends", () => {
    expect(splitSnippet("abc", [[1, 99]])).toEqual([
      { text: "a", match: false },
      { text: "bc", match: true },
    ]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- searchHighlight` — expect FAIL (module not found).

- [ ] **Step 3: Implement `searchHighlight.ts`**

Create `web/src/components/searchHighlight.ts`:
```ts
export interface SearchSnippet {
  snippet: string;
  highlights: [number, number][]; // [start, end) offsets within `snippet`
}

export interface SnippetSegment {
  text: string;
  match: boolean;
}

/** Slice `snippet` into alternating plain/matched segments. Ranges are clamped to
 *  [0, len], invalid/empty dropped, sorted, and overlapping/adjacent merged. No
 *  highlights → a single plain segment (or [] for an empty snippet). */
export function splitSnippet(
  snippet: string,
  highlights: [number, number][],
): SnippetSegment[] {
  const len = snippet.length;
  const norm = highlights
    .map(
      ([s, e]) =>
        [Math.max(0, Math.min(s, len)), Math.max(0, Math.min(e, len))] as [
          number,
          number,
        ],
    )
    .filter(([s, e]) => e > s)
    .sort((a, b) => a[0] - b[0]);

  const merged: [number, number][] = [];
  for (const [s, e] of norm) {
    const last = merged[merged.length - 1];
    if (last && s <= last[1]) last[1] = Math.max(last[1], e);
    else merged.push([s, e]);
  }

  const segments: SnippetSegment[] = [];
  let cursor = 0;
  for (const [s, e] of merged) {
    if (s > cursor) segments.push({ text: snippet.slice(cursor, s), match: false });
    segments.push({ text: snippet.slice(s, e), match: true });
    cursor = e;
  }
  if (cursor < len) segments.push({ text: snippet.slice(cursor), match: false });
  return segments;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- searchHighlight` — expect PASS (7 tests).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/searchHighlight.ts web/src/components/searchHighlight.test.ts
git commit -m "feat(search): pure splitSnippet highlight segmenter"
```

---

## Task 2: Mock `search_results` + store consumption

These are interdependent (the mock returning `search_results` and the store reading it must land together for the suite to be green), so they're one task with one commit.

**Files:** modify `web/src/client/mock.ts`, `web/src/contract/index.ts`, `web/src/client/mock.test.ts`, `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Update the failing tests**

(a) In `web/src/client/mock.test.ts`, replace the "search matches body and path" test's expectation:
```ts
  it("search matches body and path, case-insensitive, sorted by path", async () => {
    const c = new MockClient({
      "zeta.md": "alpha note",
      "alpha.md": "zeta body",
    });
    expect(await c.runQuery({ type: "search", query: "ALPHA" })).toEqual({
      type: "search_results",
      results: [
        { path: "alpha.md", score: 0, snippet: "zeta body", highlights: [] },
        { path: "zeta.md", score: 1, snippet: "alpha note", highlights: [[0, 5]] },
      ],
    });
  });
```
(b) Append to `describe("cairn store", …)` in `web/src/store/store.test.ts`:
```ts
  it("runSearch stores ranked paths + snippets keyed by path", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "the quick brown fox" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().runSearch("quick");
    expect(store.getState().searchResults).toEqual(["a.md"]);
    expect(store.getState().searchSnippets?.["a.md"].snippet).toContain("quick");
    expect(store.getState().searchSnippets?.["a.md"].highlights.length).toBe(1);
  });
  it("filterByTag leaves searchSnippets null; closeSearch clears it", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "---\ntags: [rust]\n---\nx" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().filterByTag("rust");
    expect(store.getState().searchSnippets).toBeNull();
    await store.getState().runSearch("x");
    store.getState().closeSearch();
    expect(store.getState().searchSnippets).toBeNull();
    expect(store.getState().searchResults).toBeNull();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- mock store` — expect FAIL (mock returns `paths`; store has no `searchSnippets` and `runSearch` reads `paths`).

- [ ] **Step 3: Mock — export `SearchResult` + return `search_results`**

(a) In `web/src/contract/index.ts`, add (if not already present):
```ts
export type { SearchResult } from "./SearchResult";
```
(b) In `web/src/client/mock.ts`, add `SearchResult` to the `import type { … } from "../contract";` list, then replace the `case "search"` block with:
```ts
      case "search": {
        const needle = q.query.toLowerCase();
        const results: SearchResult[] = [];
        for (const [path, raw] of this.notes) {
          const body = splitFrontmatter(raw).body;
          const lowerBody = body.toLowerCase();
          const idx = lowerBody.indexOf(needle);
          const pathMatch = path.toLowerCase().includes(needle);
          if (idx === -1 && !pathMatch) continue;
          let snippet: string;
          let highlights: [number, number][];
          if (idx !== -1) {
            const start = Math.max(0, idx - 20);
            const end = Math.min(body.length, idx + needle.length + 20);
            snippet = body.slice(start, end);
            highlights = [[idx - start, idx - start + needle.length]];
          } else {
            snippet = body.slice(0, 40);
            highlights = [];
          }
          const score = lowerBody.split(needle).length - 1;
          results.push({ path, score, snippet, highlights });
        }
        results.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
        return { type: "search_results", results };
      }
```

- [ ] **Step 4: Store — `searchSnippets` + consume `search_results`**

In `web/src/store/store.ts`:

(a) Import the type near the top:
```ts
import type { SearchSnippet } from "../components/searchHighlight";
```
(b) In `CairnState`, after `searchResults: string[] | null;` add:
```ts
  searchSnippets: Record<string, SearchSnippet> | null;
```
(c) In the initial state, after `searchResults: null,` add:
```ts
      searchSnippets: null,
```
(d) In `openCairn`'s reset `set({ … })`, add:
```ts
            searchSnippets: null,
```
(e) Replace `runSearch`:
```ts
      async runSearch(query) {
        try {
          const res = await client.runQuery({ type: "search", query });
          if (res.type === "search_results") {
            set({
              query,
              searchResults: res.results.map((r) => r.path),
              searchSnippets: Object.fromEntries(
                res.results.map((r) => [
                  r.path,
                  { snippet: r.snippet, highlights: r.highlights },
                ]),
              ),
              activeTag: null,
            });
          }
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },
```
(f) In `filterByTag`, add `searchSnippets: null`:
```ts
          if (res.type === "paths")
            set({ searchResults: res.paths, searchSnippets: null, activeTag: tag });
```
(g) Replace `closeSearch`:
```ts
      closeSearch() {
        set({ searchResults: null, searchSnippets: null, activeTag: null });
      },
```

- [ ] **Step 5: Run to verify pass**

Run: `pnpm test -- mock store` — expect PASS. The existing "runSearch populates results; closeSearch clears them" test still passes (mock now returns `search_results`; store maps to the same paths).

- [ ] **Step 6: Full gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts web/src/contract/index.ts web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(search): consume search_results (rich) + store searchSnippets"
```

---

## Task 3: SearchResults — highlighted snippet line

**Files:** modify `web/src/components/SearchResults.tsx`, `web/src/components/SearchResults.test.tsx`.

- [ ] **Step 1: Add failing tests**

Append inside `describe("SearchResults", …)` in `web/src/components/SearchResults.test.tsx`:
```ts
  it("renders a highlighted snippet when provided", () => {
    render(
      <SearchResults
        results={["a.md"]}
        snippets={{ "a.md": { snippet: "the quick fox", highlights: [[4, 9]] } }}
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("quick")).toBeInTheDocument(); // the matched segment
    expect(screen.getByRole("button", { name: "a.md" })).toHaveTextContent(
      "the quick fox",
    );
  });
  it("renders path-only when no snippet is provided", () => {
    render(
      <SearchResults results={["a.md"]} onOpen={vi.fn()} onClose={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: "a.md" })).toHaveTextContent("a.md");
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- SearchResults` — expect FAIL (`snippets` prop doesn't exist; matched segment not rendered).

- [ ] **Step 3: Implement**

Replace `web/src/components/SearchResults.tsx` with:
```tsx
import { IconButton } from "./ui/IconButton";
import { SectionLabel } from "./ui/SectionLabel";
import { splitSnippet, type SearchSnippet } from "./searchHighlight";

export function SearchResults(props: {
  results: string[] | null;
  onOpen: (path: string) => void;
  onClose: () => void;
  title?: string;
  snippets?: Record<string, SearchSnippet>;
}) {
  if (props.results === null) return null;
  return (
    <div
      data-testid="search-results"
      className="absolute left-2 top-12 z-10 flex max-h-[60vh] w-72 flex-col rounded border border-border bg-surface p-2 shadow-lg"
    >
      <div className="mb-1 flex items-center justify-between">
        <SectionLabel>
          {props.title ?? "Results"} ({props.results.length})
        </SectionLabel>
        <IconButton label="close" onClick={props.onClose}>
          ✕
        </IconButton>
      </div>
      {props.results.length === 0 ? (
        <span className="text-sm text-faint">No matches</span>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          {props.results.map((path) => {
            const snip = props.snippets?.[path];
            return (
              <button
                key={path}
                aria-label={path}
                className="block w-full rounded px-2 py-1 text-left hover:bg-surface-2"
                onClick={() => props.onOpen(path)}
              >
                <span className="block truncate text-sm text-muted">{path}</span>
                {snip && (
                  <span className="mt-0.5 block truncate text-xs text-faint">
                    {splitSnippet(snip.snippet, snip.highlights).map((seg, i) =>
                      seg.match ? (
                        <mark key={i} className="bg-transparent text-accent">
                          {seg.text}
                        </mark>
                      ) : (
                        <span key={i}>{seg.text}</span>
                      ),
                    )}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- SearchResults` — expect PASS (existing title/click tests + 2 new).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/SearchResults.tsx web/src/components/SearchResults.test.tsx
git commit -m "feat(search): render highlighted snippet line per result"
```

---

## Task 4: App wiring + e2e

**Files:** modify `web/src/app/App.tsx`, `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: App — select + pass searchSnippets**

In `web/src/app/App.tsx`:
(a) Add a selector next to `const searchResults = useCairn((s) => s.searchResults);`:
```tsx
  const searchSnippets = useCairn((s) => s.searchSnippets);
```
(b) Add the `snippets` prop to the `<SearchResults …/>` element (keep its existing `results`/`title`/`onOpen`/`onClose`):
```tsx
              snippets={searchSnippets ?? undefined}
```

- [ ] **Step 2: Gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 3: Add the e2e**

Append to `web/e2e/skeleton.spec.ts`:
```ts
test("search shows a highlighted snippet", async ({ page }) => {
  await page.goto("/");
  await page.getByPlaceholder("Search…").fill("Start");
  await page.getByPlaceholder("Search…").press("Enter");
  const overlay = page.getByTestId("search-results");
  await expect(overlay).toBeVisible();
  // index.md ("Start at [[ideas]] …") matches; its row shows a snippet.
  await expect(
    overlay.getByRole("button", { name: "index.md" }),
  ).toContainText("Start");
  // the matched term is rendered as a <mark> highlight.
  await expect(overlay.locator("mark")).toContainText("Start");
  // clicking the result opens the note + closes the overlay.
  await overlay.getByRole("button", { name: "index.md" }).click();
  await expect(page.getByTestId("search-results")).toHaveCount(0);
});
```

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e` — expect 15 passed (14 existing + this). If port 5273 busy: `lsof -ti :5273 | xargs kill 2>/dev/null` then retry once.
- The big existing search test still passes: search → `activeTag` null → `title` undefined → header "Results (N)"; and `getByRole("button", {name:"fresh.md"})` still matches via the new `aria-label`.
- If the new test fails because no `<mark>` renders, check the store consumes `search_results` and App passes `snippets`. STOP and report if a core assertion fails.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 6: Manual/visual check**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); confirm the dev log is error-free; stop it. (Human confirms: searching shows hits with a snippet line + highlighted term; tag-filter rows stay path-only; clicking a hit opens the note.)

- [ ] **Step 7: Commit**

```bash
git add web/src/app/App.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(search): wire snippets into the overlay + e2e"
```

---

## Notes for the executor

- **Only `Search` changed shape.** `runSearch` branches on `search_results`; `get_backlinks`/`refreshBacklinks` and `notes_by_tag`/`filterByTag` still use `paths` and are untouched.
- **`searchResults` stays `string[]`** (ranked paths) — `searchSnippets` is a parallel map, so existing search/tag tests and the overlay's path contract are unchanged. `searchSnippets` is cleared on tag-filter, close, and open-cairn so no stale snippet renders under a tag/backlink row.
- **Tasks 2 bundles mock + store** because committing the mock's new shape alone would leave the store search test red.
- **`aria-label={path}`** on result buttons keeps their accessible name exactly the path, so existing `getByRole("button", {name: path})` queries (the big search e2e) keep matching even with the snippet line added.
- **Highlight offsets** are treated as string indices (ASCII-correct); `splitSnippet` clamps so it never throws.
- **No contract/Rust change** — the `search_results` variant + `SearchResult` were already synced; this just adds the barrel export and consumes the shape.
```
