# Cairn Web UI — UI‑4c: Graph Color Groups Design Spec

**Date:** 2026-06-09
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑4c — third of the UI‑4 graph cycles. Colors graph nodes by
user-defined groups (path or tag), on top of UI‑4a/UI‑4b.
**Builds on:** UI‑4a (`react-force-graph-2d` canvas, `paintNode`), UI‑4b (gear
settings overlay + localStorage settings pattern), the graphite design system.

---

## 1. Purpose

Let the user color graph nodes by **groups** (Obsidian's "Groups"), via a Groups
section in the graph settings panel. Each group is `{ kind: "path" | "tag";
query; color }`; a node takes the color of the **first** group it matches, else
the default. Tags come from a new `noteTags()` client seam (the mock parses real
fixture tags; the live engine stubs `[]` until it exposes tags).

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Group model | `{ kind: "path" \| "tag"; query: string; color: string }` — **explicit kind selector** (no `#` parsing / no path-vs-tag ambiguity). |
| Match | **path** = case-insensitive substring of the note path; **tag** = exact (case-insensitive) membership in the note's tags. **First matching group wins** (list order). Unmatched → default node color. |
| Tags source | New **client capability** `noteTags(): Promise<Record<path, string[]>>`. `MockClient` parses fixture content (`extractTags`); `TauriClient` returns `{}` (stub — engine has no tags yet). NOT a contract/Query change. |
| Persistence | localStorage (`cairn.graph.groups`), same pattern as UI‑4b's `forceSettings`. Default = `[]` (no groups). |
| UI | A **Groups** section in the existing gear overlay, above Forces: rows of [kind select · query · color · remove] + "Add group". |
| Active override | The active note stays accent-tinted (overrides its group color). Hover-dim keeps a node's group color at reduced alpha. |
| Out of scope | Group reordering (drag), filters/search, "Display" sliders, tag support in the live engine, UI‑4d (local graph). |

### Non-goals (deferred)

- Real tags on the live (Tauri) engine — needs a contract field; out of scope. The
  seam is in place so it drops in later.
- Drag-to-reorder groups (order = creation order for now).
- Tag substring matching, nested/AND queries, regex.
- Per-vault group profiles (single global list).

---

## 2. Architecture

Contained to the graph view + a thin client capability. **No contract DTO
changes** (`noteTags` is a `CairnClient` method, like `host.assetUrl` was a host
method).

```
web/src/components/graph/tags.ts                NEW (pure) — extractTags(markdown) → string[].
web/src/components/graph/tags.test.ts           NEW.
web/src/components/graph/colorGroups.ts         NEW (pure + localStorage) — ColorGroup, load/save, matchGroupColor.
web/src/components/graph/colorGroups.test.ts    NEW.
web/src/components/graph/GraphGroupsPanel.tsx    NEW (presentational) — group rows + add.
web/src/components/graph/GraphGroupsPanel.test.tsx  NEW.
web/src/client/types.ts (CairnClient)          MODIFY — add noteTags(): Promise<Record<string,string[]>>.
web/src/client/mock.ts (MockClient)            MODIFY — implement via extractTags over fixtures.
web/src/client/tauri.ts (TauriClient)          MODIFY — stub: return {}.
web/src/store/store.ts                         MODIFY — loadGraph also loads noteTags into state.
web/src/app/App.tsx                            MODIFY — pass tagsByNote to GraphView.
web/src/components/GraphView.tsx               MODIFY — groups state + GraphGroupsPanel + paintNode coloring.
web/e2e/skeleton.spec.ts                       MODIFY — gear → Groups section + add a group.
```

### `tags.ts` (pure)

```ts
// Tags from a note's markdown: frontmatter `tags:` (YAML list or comma list) +
// inline `#tag`. Lowercased, deduped, order preserved.
extractTags(markdown: string): string[]
```
- Frontmatter: a leading `---\n…\n---` block; within it a `tags:` key as either
  `tags: [a, b]` / `tags: a, b` or a block list (`- a` lines).
- Inline: `#tag` tokens in the body (word chars + `-`/`_`/`/`), excluding `#` in
  code spans/blocks is **out of scope** (simple regex scan is acceptable; note it).
- Returns `["a","b",…]` lowercased, no duplicates.

### `colorGroups.ts` (pure + localStorage)

```ts
export interface ColorGroup { kind: "path" | "tag"; query: string; color: string }

DEFAULT_COLOR_GROUPS: ColorGroup[]   // []
loadColorGroups(): ColorGroup[]      // localStorage["cairn.graph.groups"] → validated array, else []
saveColorGroups(groups): void        // JSON write, swallow errors

// First group that matches → its color; else null.
//  kind "path": path.toLowerCase().includes(query.toLowerCase()) (empty query → no match)
//  kind "tag":  tags (already lowercased) includes query.trim().toLowerCase()
matchGroupColor(path: string, tags: string[], groups: ColorGroup[]): string | null
```
- `loadColorGroups` tolerates missing/corrupt JSON and drops malformed entries
  (must have `kind ∈ {path,tag}`, string `query`, string `color`) → returns `[]`
  or the valid subset.

### `noteTags()` client seam

- `CairnClient` interface gains `noteTags(): Promise<Record<string, string[]>>`
  (path → tags for every note).
- `MockClient`: iterate its fixture notes, `extractTags(content)` per note → map.
- `TauriClient`: `return Promise.resolve({})` — documented stub; swap for an
  engine query when `NoteSummary`/a query exposes tags.
- Store: `loadGraph` (after fetching the graph) also `await client.noteTags()` and
  stores `noteTags: Record<string,string[]>` in state (default `{}`); failures →
  `{}` (non-fatal). `App` passes `tagsByNote={noteTags}` to `GraphView`.

### `GraphGroupsPanel.tsx` (presentational)

`props: { groups: ColorGroup[]; onChange: (next: ColorGroup[]) => void }`. Renders
a "Groups" titled block: one row per group — a `<select>` (Path/Tag) bound to
`kind`, an `<input type="text">` (query, aria-label e.g. "Group query"), an
`<input type="color">` (color, aria-label "Group color"), and a remove `×`; plus
an "Add group" button appending `{ kind: "path", query: "", color: "#6366f1" }`.
Each edit/add/remove calls `onChange(nextArray)`. No graph/store coupling.

### `GraphView.tsx` wiring

- New prop `tagsByNote: Record<string, string[]>` (from `App`/store).
- `const [groups, setGroups] = useState(loadColorGroups)`; `changeGroups(next)` =
  `setGroups` + `saveColorGroups`.
- Render `<GraphGroupsPanel groups={groups} onChange={changeGroups} />` in the gear
  overlay **above** `<GraphForcesPanel>` (same panel container).
- `paintNode` fill resolution (replaces the current `active ? accent : lit ? … : …`):
  - `base = active ? "#6366f1" : (matchGroupColor(node.id, tagsByNote[node.id] ?? [], groups) ?? "#cdd0e0")`.
  - When hovering and the node is NOT in the highlight set → draw `base` at reduced
    alpha (dim) instead of the flat `#6b6c7755`, so dimmed nodes keep their group
    hue; lit/active nodes draw `base` at full alpha.
  - `groups`/`tagsByNote` added to `paintNode`'s `useCallback` deps; they are NOT in
    `graphData` deps (no re-simulation on group/tag change — UI‑4a invariant).

---

## 3. Testing

- **Unit (Vitest):**
  - `extractTags`: frontmatter list + comma + inline `#tag`; lowercase + dedupe;
    empty when none.
  - `colorGroups`: `loadColorGroups` defaults on empty/corrupt, drops malformed
    entries; `saveColorGroups` round-trips + swallows errors; `matchGroupColor`
    path-substring, tag-exact, first-match-wins, empty-query no-match, no-match → null.
  - `MockClient.noteTags`: returns a path→tags map parsed from fixtures (add a
    tagged fixture note so the map is non-empty); `TauriClient.noteTags` → `{}`.
  - `GraphGroupsPanel`: Add appends a default group (onChange); editing kind/query/
    color fires onChange with the updated row; remove drops the row.
- **e2e (Playwright):** in the graph view, open the gear, assert the **Groups**
  section is visible, click **Add group**, and assert a new group row (e.g. the
  query input) appears. (Canvas recoloring is physics/canvas → manual-visual.)
- **Manual/visual check:** adding a "Path: <folder>" or "Tag: <tag>" group recolors
  the matching nodes live; first-match-wins; removing a group reverts; colors
  persist across reload; the active note stays accent.
- All existing tests stay green; Tauri unaffected (its `noteTags` returns `{}`).

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/graph/tags.ts` (+ test) | **New.** `extractTags`. |
| `web/src/components/graph/colorGroups.ts` (+ test) | **New.** Model + localStorage + `matchGroupColor`. |
| `web/src/components/graph/GraphGroupsPanel.tsx` (+ test) | **New.** Presentational group editor. |
| `web/src/client/types.ts` | **Modify.** `CairnClient.noteTags()`. |
| `web/src/client/mock.ts` | **Modify.** Implement `noteTags` via `extractTags`. |
| `web/src/client/tauri.ts` | **Modify.** Stub `noteTags` → `{}`. |
| `web/src/store/store.ts` (+ test) | **Modify.** Load `noteTags` in `loadGraph`; new state field. |
| `web/src/app/App.tsx` | **Modify.** Pass `tagsByNote` to `GraphView`. |
| `web/src/components/GraphView.tsx` | **Modify.** Groups state + panel + `paintNode` coloring. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Groups section + add-group e2e. |

No new npm dependencies. No contract DTO changes.

---

## 5. Risks

- **`noteTags` shape & call sites:** adding a required method to `CairnClient`
  means every implementation (Mock, Tauri) and any client mocks in tests must
  provide it — typecheck enforces. Keep it a `Promise<Record<string,string[]>>`.
- **Store load coupling:** load `noteTags` in `loadGraph` so tags arrive with the
  graph; a `noteTags` failure must not break graph load (catch → `{}`).
- **No re-simulation on group/tag change:** `groups`/`tagsByNote` feed `paintNode`
  (a `useCallback` dep → repaint) but NOT `graphData` deps — else the graph
  re-lays-out when you edit a group (UI‑4a invariant).
- **Tag parsing fidelity:** the inline `#tag` regex won't exclude `#` inside code
  spans/fences — acceptable for v1 (note it); frontmatter parsing is simple, not a
  full YAML parser (handles list + comma forms).
- **Empty on live engine:** on Tauri, `noteTags` is `{}`, so "Tag" groups match
  nothing until the engine exposes tags — expected; "Path" groups work everywhere.
- **localStorage in Tauri/private mode:** wrap read/write in try/catch (as UI‑4b).
- **Canvas testability:** model + panel unit-tested; recoloring manual-visual
  (consistent with UI‑4a/UI‑4b).
