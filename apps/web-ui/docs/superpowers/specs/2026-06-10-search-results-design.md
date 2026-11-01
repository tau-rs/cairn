# Cairn Web UI — Search Results (rich) Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Fix the search contract drift surfaced by the tags-sync — consume the
engine's `SearchResults` and show snippets + highlights in the results overlay.
**Builds on:** the synced contract (`SearchResult` + the `search_results` `QueryResponse`
variant already present), the store's `runSearch`/`searchResults` + shared results overlay,
and the `SearchResults` component (title + scroll, from the real-tags cycle).

---

## 1. Purpose

Syncing the contract revealed that the engine's `Query::Search` now returns
`QueryResponse::SearchResults { results: SearchResult[] }` — where
`SearchResult = { path; score; snippet; highlights: [start,end][] }` — but the UI's
`runSearch` and the mock still produce/expect `{type:"paths"}`. So against the real
engine, **search returns nothing**. This fixes that and uses the new data: search hits
show a **snippet line with the matched text highlighted**. (`get_backlinks` and
`notes_by_tag` keep `{type:"paths"}` — only `Search` changed.)

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Consume | `runSearch` reads `{type:"search_results", results}` (the contract variant already exists); the mock returns it too. |
| Show | Each **search** hit renders its path + a muted snippet line with `highlights` ranges emphasized (`<mark>`-style). |
| Tag/backlink rows | `notes_by_tag` still returns `{type:"paths"}`; those overlay rows stay **path-only** (no snippet). |
| Order | Search keeps the engine's ranked order; the overlay iterates `results.map(r => r.path)`. |
| State shape | Keep `searchResults: string[]` (ranked paths) **unchanged**; add a parallel `searchSnippets: Record<path, {snippet, highlights}> | null`. Avoids churning the existing search/tag tests + overlay path contract. |
| Highlights | Rendered by slicing the snippet at the offsets. Treated as string indices — correct for ASCII; a non-ASCII mis-align is cosmetic (never breaks the result). Documented. |
| Out of scope | Search ranking/scoring UI (score is consumed but not shown), regex/scoped search, snippet-click-to-jump-to-match, highlighting in the editor. |

---

## 2. Architecture

Contract is already synced (no contract change). A pure highlight-splitter + mock/store/overlay updates.

```
web/src/components/searchHighlight.ts (+test)  NEW (pure) — SearchSnippet type + splitSnippet(snippet, highlights) → segments.
web/src/client/mock.ts (+test)                 MODIFY — `search` returns {type:"search_results", results} (snippet/score/highlights).
web/src/store/store.ts (+test)                 MODIFY — add searchSnippets; runSearch consumes search_results; filterByTag/closeSearch/openCairn clear it.
web/src/components/SearchResults.tsx (+test)   MODIFY — optional `snippets` prop → render a highlighted snippet line per result.
web/src/app/App.tsx                            MODIFY — select + pass searchSnippets to <SearchResults>.
web/e2e/skeleton.spec.ts                        MODIFY — assert a search hit shows a highlighted snippet.
```

### 2.1 `searchHighlight.ts` (pure)

```ts
export interface SearchSnippet {
  snippet: string;
  highlights: [number, number][]; // [start, end) offsets within `snippet`
}

export interface SnippetSegment {
  text: string;
  match: boolean;
}

// Slice `snippet` into alternating plain/matched segments. Ranges are sorted,
// clamped to [0, snippet.length], invalid/empty ranges skipped, gaps filled with
// plain segments. No highlights → a single plain segment (or [] for an empty snippet).
export function splitSnippet(
  snippet: string,
  highlights: [number, number][],
): SnippetSegment[];
```

### 2.2 Mock `search`

Replace the `case "search"` handler so it returns `{type:"search_results", results}`. For
each note whose **body** (frontmatter-stripped) or **path** contains the (lowercased)
needle:
- `score` = count of needle occurrences in the lowercased body.
- If the body contains the needle at index `idx`: `snippet` = `body.slice(max(0, idx-20), min(len, idx+needle.length+20))`; `highlights` = `[[idx - start, idx - start + needle.length]]`.
- Else (matched only via path): `snippet` = `body.slice(0, 40)`; `highlights` = `[]`.
- Results sorted by `path` (deterministic; the mock isn't a true ranker — fine for dev).

(The mock stays a faithful-enough stand-in; exact relevance ordering is the engine's job.)

### 2.3 Store

- Add state `searchSnippets: Record<string, SearchSnippet> | null` (init `null`).
- `runSearch(query)`: on `{type:"search_results", results}` → `set({ query, searchResults: results.map(r => r.path), searchSnippets: Object.fromEntries(results.map(r => [r.path, { snippet: r.snippet, highlights: r.highlights }])), activeTag: null })`.
- `filterByTag(tag)`: unchanged except also `searchSnippets: null` (tag rows have no snippets).
- `closeSearch`: `set({ searchResults: null, searchSnippets: null, activeTag: null })`.
- `openCairn` reset: add `searchSnippets: null`.
- The activeTag-aware note-event refresh is unchanged (it re-runs `runSearch`/`filterByTag`, which set snippets correctly).

### 2.4 `SearchResults`

Add `snippets?: Record<string, SearchSnippet>`. Each result button gets `aria-label={path}`
(so existing exact-name tests/e2e still match) and shows the path; when `snippets[path]`
exists, a muted second line renders `splitSnippet(snippet, highlights)` with matched
segments emphasized (an accent/`<mark>` style) and plain segments normal. Tag/backlink rows
(no entry in `snippets`) render path-only, as today. Title + scroll-cap unchanged.

### 2.5 App

Select `const searchSnippets = useCairn((s) => s.searchSnippets);` and pass
`snippets={searchSnippets ?? undefined}` to `<SearchResults>`.

---

## 3. Testing

- **Unit (Vitest):**
  - `splitSnippet`: empty highlights → one plain segment (and `[]` for empty snippet);
    a single range splits into plain/match/plain; multiple + adjacent ranges; out-of-range
    clamped/skipped.
  - Mock `search`: returns `{type:"search_results"}`; a body match yields the right
    `snippet` + `highlights` + `score`; a path-only match yields `highlights: []`. (Update
    the existing "search matches body and path" test to the new shape with explicit
    expected `snippet`/`highlights`/`score`.)
  - Store: `runSearch` sets `searchResults` (paths) **and** `searchSnippets`; `filterByTag`
    leaves `searchSnippets` null; `closeSearch` clears both.
  - `SearchResults`: with a `snippets` entry, renders the snippet text and a highlighted
    (`mark`) segment; without it, renders path-only; clicking still calls `onOpen(path)`.
- **e2e (Playwright):** search a fixture term (e.g. `pointing`) → the hit shows a snippet
  containing the term, with a highlighted (`mark`) span; clicking the result opens the note.
  Keep all existing e2e green (the big search test's `getByRole("button", {name})` still
  matches via the path `aria-label`).
- All existing unit + e2e stay green. No Tauri change beyond the now-correct search path.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/searchHighlight.ts` (+test) | **New.** `SearchSnippet` type + `splitSnippet`. |
| `web/src/client/mock.ts` (+test) | **Modify.** `search` → `search_results`. |
| `web/src/store/store.ts` (+test) | **Modify.** `searchSnippets` + `runSearch`/`filterByTag`/`closeSearch`/`openCairn`. |
| `web/src/components/SearchResults.tsx` (+test) | **Modify.** `snippets` prop + highlighted snippet line. |
| `web/src/app/App.tsx` | **Modify.** Select + pass `searchSnippets`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Search-snippet e2e. |

No new npm dependencies. No contract change (already synced). No Rust change.

---

## 5. Risks

- **Search vs tag/backlink shape split.** Only `Search` returns `search_results`;
  `get_backlinks`/`notes_by_tag` still return `paths`. `runSearch` must branch on
  `search_results` (not `paths`); the mock's `search` is the only handler that changes.
  `refreshBacklinks`/`filterByTag` are untouched.
- **Shared overlay correctness.** `searchSnippets` must be cleared whenever the overlay
  shows non-search results (tag filter) or closes, or a stale snippet would render under a
  tag/backlink row. `filterByTag`/`closeSearch`/`openCairn` all null it; store tests cover it.
- **Highlight offsets.** Engine `highlights` are byte ranges; rendered as JS string indices
  — correct for ASCII, possibly mis-aligned for multi-byte snippets. Cosmetic only;
  `splitSnippet` clamps so it never throws or drops the result. Documented.
- **Accessible-name stability.** Adding a snippet line inside the result button could change
  its accessible name; `aria-label={path}` pins it to the path so existing
  `getByRole("button", {name: path})` queries (the big search e2e) keep working.
- **Mock fidelity.** The mock sorts by path, not true relevance; acceptable for dev (the
  real engine ranks). Snippet windowing is a simple ±20-char slice, not the engine's exact
  algorithm — fine, since assertions target the mock's own deterministic output.
- **Plain DOM, jsdom-safe.** `splitSnippet` is pure; `SearchResults` is plain DOM.
