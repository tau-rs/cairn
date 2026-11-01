# Design findings — cairn-web-ui

Subsections: **Code design**, **DX**, **UX**.

---

## Code design

### D1. Stale-response races across every async store action
**Severity: High**
**Location:** `web/src/store/store.ts:518-527` (`refreshBacklinks`), `:434-453` (`runSearch`), `:529-542` (`loadGraph`), `:464-476` (`filterByTag`)

Async actions read state, `await` a query, then unconditionally `set(...)` the
result — with no check that the request is still relevant. Example:

```ts
async refreshBacklinks() {
  const path = get().activePath;     // note A
  ...
  const res = await client.runQuery({ type: "get_backlinks", path });
  if (res.type === "paths") set({ backlinks: res.paths });  // may now be note B
}
```

`selectTab`/`openNote`/`cycleTab`/`jumpToTab` all fire `refreshBacklinks`
(`store.ts:396,417,423,431`). Rapidly switching notes lets an earlier
(slower) response overwrite the current note's backlinks. The same pattern
makes a slow `runSearch`/`loadGraph` clobber newer results. There is no request
sequencing, abort, or "is this still the active target?" guard.

**Impact:** Backlinks/search/graph that don't match the visible note — a subtle,
hard-to-reproduce correctness bug that worsens on slower (daemon) transports.

**Recommendation:** Tag each async action with a monotonic request id or
re-read the relevant key after `await` and bail if it changed
(`if (get().activePath !== path) return;`). Consider an AbortController-style
cancellation in `CairnClient`.

### D2. Self-induced refresh storm on every autosave
**Severity: High**
**Location:** `web/src/store/store.ts:197-206` (subscribe handler) + `:288-311` (`editBuffer`) + `:317-344` (`saveNote`)

Each keystroke debounces a `saveNote`, which sends `write_note`. The engine
then emits `note_changed` **for the user's own write**, and the subscribe
handler reacts by running, unconditionally: `refreshNotePaths()`,
`loadTags()`, plus `filterByTag`/`runSearch` (if a filter/search is active),
`refreshBacklinks` (if a note is open), and `loadGraph` (if the graph was ever
loaded). So a single autosave triggers 2-5 backend queries plus a full graph
rebuild + `noteTags()` scan — while the user keeps typing.

**Impact:** On the real Tauri/daemon transport this is a continuous query
storm and a graph-simulation reheat on every save; needless backend load, jank,
and battery drain. It also amplifies D1's races.

**Recommendation:** Distinguish self-originated writes (echo suppression via a
pending-write set keyed by path) from external changes; debounce/coalesce the
index-derived refreshes; don't reload the whole graph on every note change.

### D3. `openCairn` leaves derived state stale after switching cairns
**Severity: Medium**
**Location:** `web/src/store/store.ts:232-256`

`init()` (`:191-230`) loads notes, tags, plugins, persisted tabs and arms the
interval. `openCairn()` resets state — explicitly clearing `tags: []`,
`plugins: []` — but then only calls `refreshNotePaths()` and `rearmInterval()`.
It never reloads tags or plugins, never re-runs persisted-tab restoration, and
never resets `graph`/`backlinks` consistently. After opening a second cairn the
Tags panel is empty until an unrelated `note_changed` happens to fire
`loadTags`, and plugins are gone for good.

**Impact:** Inconsistent UI when switching vaults; tags/plugins silently
missing.

**Recommendation:** Factor a single `loadCairn()` used by both `init` and
`openCairn` (load notes + tags + plugins + tabs), so the two paths can't drift.

### D4. Dead / unwired features: plugins panel, notice toast, plugin palette commands
**Severity: Medium**
**Status: Resolved** — the "wire them up" recommendation was implemented in
commit `2d25265` ("feat(plugins): plugin commands in the palette + Plugins panel
+ e2e"), an ancestor of `main` that predates this audit being committed (the
finding was authored against an earlier snapshot). In current `main`:
`PluginsPanel` renders in `SettingsDialog.tsx:36`; `NoticeToast` renders next to
`ErrorToast` in `App.tsx:343-344`; `toPaletteCommands(plugins)` is merged into
the palette (`App.tsx:137`) and `invokePlugin` is reachable via
`parsePluginCommandId` → `runCommand` (`App.tsx:140-142`). The full flow has a
dedicated e2e (`web/e2e/skeleton.spec.ts:465`). No dead code remains.

**Location:** `web/src/components/plugins/PluginsPanel.tsx`, `web/src/components/plugins/pluginCommands.ts` (`toPaletteCommands`), `web/src/components/NoticeToast.tsx`, `web/src/store/store.ts:478-508` (`loadPlugins`/`invokePlugin`/`notice`), `web/src/app/App.tsx`

`App.tsx` renders neither `PluginsPanel` nor `NoticeToast`, and never calls
`toPaletteCommands`. The store loads plugins (`init` → `loadPlugins`) into state
that nothing displays; `invokePlugin` (the only thing that sets `notice`) is
unreachable because no UI dispatches it and the plugin palette commands are
never merged into `COMMANDS` (`App.tsx:109-118`). All of this is tested in
isolation but wired into nothing.

**Impact:** ~4 components/functions of dead weight; plugin support appears
implemented but is inert; the `notice` state + `NoticeToast` + `dismissNotice`
are unreachable. Confusing for the next contributor.

**Recommendation:** Either wire them up (render `PluginsPanel`, merge
`toPaletteCommands(plugins)` into the palette, render `NoticeToast` next to
`ErrorToast`) or delete them until plugins are real.

### D5. `App.tsx` is a 327-line monolith with heavy prop drilling
**Severity: Medium**
**Location:** `web/src/app/App.tsx:73-94` (≈20 individual `useCairn` selectors), `:160-326` (one giant JSX tree)

The root component subscribes to ~20 store slices individually and threads them
through `Shell` via four large render-prop slots, mixing local UI state
(dialog open flags, `view`, keybinding overrides) with global store data and
command dispatch. The command-dispatch `runCommand` and the global keydown
effect also live here. This concentrates re-renders and makes the file the
single hardest thing to change safely.

**Impact:** Re-render breadth (any of 20 slices changing re-renders the whole
shell), poor separation of concerns, high merge-conflict surface.

**Recommendation:** Extract `TopBar`, `EditorPane`, and dialog-host into their
own components that pull the slices they need; move command dispatch into a hook
(`useCommands`) and the keymap into `KeyboardShortcuts`/a hook.

### D6. Captured `actions = cairnStore.getState()` and stable-identity assumption
**Severity: Low**
**Location:** `web/src/app/App.tsx:103` (and `:43,60`)

`const actions = cairnStore.getState()` is captured during render and its
methods passed as props. This works only because Zustand action identities are
stable for the store's lifetime (the code comments this). It's a fragile
convention: any refactor that recreates the store or memoizes actions
differently silently breaks it, and it bypasses the reactive `useCairn`
subscription model used elsewhere.

**Recommendation:** Select actions via `useCairn((s) => s.openNote)` (etc.) or a
dedicated `useActions()` hook, so the dependency is explicit and consistent.

### D7. Silent type-narrowing: unexpected response types are dropped without error
**Severity: Low**
**Location:** `web/src/store/store.ts:261-262, 271-272, 437, 458, 467, 481, 523, 532` (`if (res.type === …)` with no `else`)

Every query handler checks `res.type` and silently no-ops on a mismatch (e.g.
`refreshNotePaths` does nothing if the response isn't `notes`). A backend that
returns an unexpected variant produces neither updated state nor an error —
the UI just stays stale with no diagnostic.

**Recommendation:** Add an `else` that surfaces an `error` (or at least
`console.error`) on an unexpected response variant.

### D8. `editBuffer` allocates a fresh debounce per keystroke
**Severity: Low**
**Location:** `web/src/store/store.ts:295-301`

Each keystroke cancels the prior debounce and constructs a new
`debounce(...)`/closure, stored in the `autosaves` map. Functionally correct but
needlessly allocates on the hot path; a per-path persistent debounce that you
merely re-trigger would be cleaner and cheaper.

### D9. `react-force-graph` typed with `as` casts and runtime mutation assumptions
**Severity: Low**
**Location:** `web/src/components/GraphView.tsx:131-134` (`d3Force("link") as {…}`), `:289` (`graphData={data as {…}}`), `:33-41` (comment: the lib mutates node/link objects)

The graph relies on the library mutating `data.nodes`/`links` in place and on
several unchecked casts. The mitigations (building adjacency from a *fresh*
string-keyed copy) are thoughtful and commented, but the type holes mean a lib
upgrade can break silently. Acceptable given the library, but worth a thin typed
wrapper.

---

## DX

### DX1. ESLint config omits accessibility and React rules
**Severity: Medium**
**Location:** `web/.eslintrc.cjs`

Extends only `eslint:recommended`, `@typescript-eslint/recommended`,
`react-hooks/recommended`, `prettier`. No `eslint-plugin-jsx-a11y` and no
`eslint-plugin-react`. Given the a11y gaps in U1/U2 below, the linter cannot
catch the most common ones (non-interactive elements with handlers, missing
roles, etc.). `@typescript-eslint` is at `recommended`, not
`recommended-type-checked`, so the unchecked casts in D7/D9 go unflagged.

**Recommendation:** Add `plugin:jsx-a11y/recommended` and
`plugin:react/recommended`; consider the type-checked tier for the store/client
layers.

### DX2. No top-level error boundary
**Severity: Medium**
**Location:** `web/src/main.tsx:9-15`

The tree is mounted with no React error boundary. Any render-time throw (e.g.
the graph lib choking on malformed data, or a decoration builder bug in
`livePreview.ts`) unmounts to a blank white window with no recovery and no
user-visible diagnostic. See diagnostics.md DG1.

### DX3. Contract is vendored by a manual script with no drift check
**Severity: Low**
**Location:** `scripts/sync-contract.sh`, `web/src/contract/source.ts`

The TS contract is copied from the engine repo and the source commit recorded in
`source.ts`, but nothing in CI verifies the vendored commit matches the engine's
current `cairn-contract`. Drift is invisible until something breaks at runtime
(and S5 means it breaks silently).

**Recommendation:** Add a CI job that re-runs `sync-contract.sh` against a pinned
engine ref and fails on a diff.

### DX4. Test coverage is strong; quality nit on integration boundaries
**Severity: Low (informational)**
**Location:** `web/src/**/*.test.*`, `web/e2e/skeleton.spec.ts`, `web/stryker.config.json`

Nearly every module has a unit test and mutation testing (Stryker) is
configured — genuinely good. Gap: there is no test that exercises the
store↔subscribe feedback loop (D2) or the async races (D1), and `App.tsx`'s
command wiring is only lightly covered by the single e2e happy-path (the plugin
paths from D4 now have their own e2e). These are exactly the integration seams where the
real bugs live.

---

## UX

### U1. Tabs are not keyboard operable
**Severity: Medium**
**Location:** `web/src/components/tabs/TabStrip.tsx:24-58`

Each tab is a `<div role="tab">` with `onClick`/`onDoubleClick` but no
`tabIndex` and no `onKeyDown`. A keyboard user cannot focus or activate tabs
(the ARIA `tablist`/`tab` roles further promise keyboard semantics that aren't
implemented). The folder-tree drag-and-drop (`FolderTreeView.tsx:101-127`) is
also mouse-only with no keyboard alternative for moving notes.

**Recommendation:** Make tabs real `<button>`s or add `tabIndex={0}` + arrow/
Enter handling per the WAI-ARIA tabs pattern; provide a keyboard path
(rename-to-path, or a move command) for tree reorganization.

### U2. No loading states for async operations
**Severity: Medium**
**Location:** `web/src/store/store.ts` (no `loading` flags for `runSearch`,
`loadGraph`, `openNote`, `refreshBacklinks`); consumers `SearchResults.tsx`,
`GraphView.tsx`, `Backlinks.tsx`

Opening a note, searching, switching to the graph, and loading backlinks have no
pending/loading indication. On the mock everything is instant so it's invisible,
but on the real (potentially networked) transport the UI just appears frozen or
shows a stale/empty panel until data lands. Empty vs. loading vs. error are
indistinguishable (e.g. `Backlinks` shows "No backlinks" during a slow load).

**Recommendation:** Add `loading` flags (or a request-state enum) per async area
and render spinners/skeletons; differentiate "loading" from "empty".

### U3. Error toast: single-slot, overwrites, never auto-dismisses
**Severity: Medium**
**Location:** `web/src/store/store.ts:67` (`error: string | null`), `web/src/components/ErrorToast.tsx`

Errors are a single `string | null`. A new error overwrites the previous one
(silently losing it), only one can ever be shown, and it persists until the
user clicks ✕ (no timeout). During the refresh storm (D2) a transient backend
error can flash and be immediately overwritten by a later success-path `set`,
so the user may never see it. There is no severity/affordance (retry) on the
toast.

**Recommendation:** Use a small toast queue with auto-dismiss + manual dismiss;
include an action (retry) where applicable; don't let unrelated `set` calls
race the error away.

### U4. Global keydown fires command chords regardless of focus target
**Severity: Low**
**Location:** `web/src/app/App.tsx:50-71`

The window `keydown` listener matches chords and `preventDefault()`s them
without checking whether focus is in a text input, the CodeMirror editor, or a
dialog. Mod-based chords are usually safe, but e.g. `Mod+E` (toggle editor
mode) or `Mod+W` (close tab) will trigger while the user is mid-edit in a
dialog field or the editor, and the `Mod+1..9` tab-jump fires even while typing
in the search box. The editable-table code already had to defensively
`stopPropagation` `Mod+A` (`editableTableWidget.ts:138-147`) to avoid a
whole-document selection — evidence the global handler is too eager.

**Recommendation:** Skip the global dispatch when the event target is an
editable element / inside CodeMirror, except for a small allowlist (palette,
commit).

### U5. Local images render broken with no fallback
**Severity: Low**
**Location:** `web/src/components/editor/widgets/imageWidget.ts:21-30` + S4

Because the asset protocol isn't enabled (S4), local relative images resolve to
an `asset://` URL that doesn't load, producing a broken-image glyph with no alt
fallback styling or "image unavailable" affordance.

**Recommendation:** Add an `onerror` fallback in `ImageWidget` (show alt text /
a placeholder), and fix S4.
