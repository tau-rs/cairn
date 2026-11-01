# Design: Real routing for Cairn (react-router v7)

**Date:** 2026-06-11
**Status:** Approved (pending spec review)

## Background

PR #18 bumped `react-router-dom` 6 → 7 (major). Investigation showed the
dependency is **vestigial**: `web/src/main.tsx` wraps `<App />` in
`<BrowserRouter>`, but nothing in `web/src` defines a route, reads a param, or
calls `navigate`/`Link`. All navigation is Zustand-store-driven. The upgrade
passed CI precisely because no routed code exists to break.

This design makes the dependency earn its place by introducing real routing,
chosen to fit Cairn's scope: an Obsidian-style local-first notes app, Tauri-first
desktop plus web, whose navigable surface is notes, a graph view, tag filters,
and search.

## Goals

1. **Back/forward navigation history across notes** — the highest-value item.
   Cairn is built around following links (wikilinks `[[...]]`, backlinks, graph
   clicks, search results); each is a navigation. A real history stack gives
   Obsidian-style back/forward in both desktop (mouse back button, `Cmd+[ / ]`)
   and web, independent of a visible address bar.
2. **The active note as a real URL** (`/note/*`) — the natural unit of
   navigation; bookmarkable/shareable on web, restorable on launch.
3. **Top-level view and tag filter as routes** (`/graph`, `/tags/:tag`) — low
   cost, clean mapping.

## Non-goals (YAGNI)

- **Dialogs in the URL** (settings, commit, new-note, palette) — ephemeral
  modals; stay as local `useState`.
- **The full tab set in the URL** — multi-tab is not a single-URL concept; tabs
  and open-note contents stay in the store + localStorage where they already
  persist.
- **Search query in the URL** — transient; deferred.
- **v7 data-router / loaders** (`createBrowserRouter`) — would fight the
  Zustand + transport-abstraction loading model and the always-mounted shell.

## Principle — who owns what

- **URL owns exactly one fact: what is foregrounded** — the active note, or the
  graph view, or a tag filter.
- **The store keeps owning everything else** — tabs, open buffers, dirty state,
  autosave, search results, settings, persistence. Unchanged.

We use declarative `<Routes>` only. The persistent `<Shell>` stays mounted across
all routes; routes select what renders in the main pane.

## Route map

| Route        | Shows                                                            |
| ------------ | --------------------------------------------------------------- |
| `/`          | Editor pane, no note focused (Lane B replaces to restored note) |
| `/note/*`    | Editor with that note active (splat: note paths contain `/`)    |
| `/graph`     | Graph view                                                      |
| `/tags/:tag` | Tag-filtered results (current `filterByTag` UX)                 |

Semantic note: today a tag filter overlays on top of the active note. A URL holds
one pointer, so `/tags/:tag` forgoes the note in the URL while filtering; the
back button then dismisses the filter (arguably better than today's
close-button-only UX).

## Data flow — one controller, two narrow lanes

A purely one-directional URL→store flow is **not** achievable: some active-note
changes originate in the store, not from user navigation — `closeTab` picks the
next tab, `deleteNote`/rename of the active note, and `init()` restoring persisted
tabs. The URL must follow those.

So: **one controller component, two guarded lanes, converging on equality.**

- **Lane A (URL → store), on location change:**
  - `/note/X` and `activePath !== X` → `openNote(X)` (already handles
    open-or-select via `openOrPreview`).
  - `/tags/:tag` → `filterByTag(tag)`; leaving a `/tags` route → `closeSearch()`.
  - `/graph` entry → `loadGraph()`.
- **Lane B (store → URL), on `activePath` change:**
  - Acts **only when the current location is a note route or `/`** — never while
    on `/graph` or `/tags/:tag`. Otherwise a store-side `activePath` change (e.g.
    `closeTab`) on the graph route would bounce the user off the graph back to a
    note. Graph and tag routes are explicit, user-chosen foregrounds that Lane B
    must not override.
  - When it acts: `activePath !==` URL's note →
    `navigate(noteUrl(activePath), { replace: true })`.
  - `replace` because tab-closes/restores should not pollute history.

Both lanes are guarded by exact inequality, so they converge in one step and go
quiet — no flags, no echo suppression. Loop-safe by construction, which matters
given the store's recent stale-response / autosave-echo hardening (PR #30).

**`toggle-view` target:** toggling *into* graph navigates to `/graph`. Toggling
*out of* graph navigates to the active note's URL (`noteUrl(activePath)`) when one
exists, else `/` — this avoids a `/` → Lane-B-replace flash and lands directly on
the note.

All **user-initiated** note opens become `navigate(noteUrl(path))` (creating real
history entries): folder tree, wikilinks, backlinks, graph node clicks, search
results, command palette, tab clicks, and `cycleTab`/`jumpToTab` (after computing
the target path). This is what makes back/forward work.

## Router choice: HashRouter

Swap `<BrowserRouter>` → `<HashRouter>` in `main.tsx` (`/#/note/...`).
Reload-safe under Tauri's asset protocol and any static web host with zero server
config. Cairn is Tauri-first with no address bar, so the cosmetic cost is nil.
Swapping to `BrowserRouter` + an SPA fallback later is a one-line change if clean
web URLs ever matter.

## Components / files

New, isolated:

- **`web/src/app/routes.ts`** — pure helpers: `noteUrl(path)`,
  `notePathFromLocation(location)`, `tagFromLocation`, `isGraph`. Per-segment
  `encodeURIComponent` (note names can contain `#`, `?`, `%`, spaces). Pure and
  unit-tested via round-trips.
- **`web/src/app/RouteSync.tsx`** — the controller above; the **only** file where
  URL and store meet. Renders nothing (returns `null`); runs two effects.

Changed:

- **`web/src/main.tsx`** — `BrowserRouter` → `HashRouter`.
- **`web/src/app/App.tsx`** — delete the `view` `useState`; derive view from the
  route (`useLocation`/`isGraph`). Nav handlers (`onOpen`, `onOpenNote`, search
  open, graph open, palette open) call `navigate(noteUrl(path))` instead of
  `actions.openNote`. The `toggle-view` command navigates to `/graph` ↔ `/`.
  The `toggle-view` command navigates per the rule above (`/graph` in; active
  note or `/` out). Mount `<RouteSync />`.
- **`web/src/components/shortcuts/commands.ts`** — add `nav-back` / `nav-forward`
  commands (default `Cmd+[` / `Cmd+]`) → `navigate(-1)` / `navigate(1)`.
  Obsidian-style history nav on desktop where there is no browser chrome.

## Edge cases & error handling

- **Deep link beats restore:** launched at `/note/X`, Lane A wins after `init()`;
  launched at `/`, Lane B replaces to the restored note.
- **Unknown note in URL:** `openNote` already sets `error` on a failed
  `get_note`; surface the existing error toast and replace the location to `/`.
- **No vault open** (`cairnPath === null`): `OpenCairn` renders regardless of
  route; `RouteSync` stays inert until `init()` completes and a vault is present.
- **Tag route with a tag that no longer exists:** `filterByTag` yields empty
  results (existing behavior); no crash.

## Testing

- **`routes.ts`** — encode/decode round-trips: nested slashes, spaces, `#`, `%`,
  unicode; `noteUrl(notePathFromLocation(x)) === x`.
- **`RouteSync`** — `MemoryRouter` + a real store with a mock client:
  - URL `/note/X` drives `openNote(X)`.
  - Store `activePath` change drives a `replace` navigation.
  - Convergence: a single change does not re-fire the opposite lane.
  - Unknown note → error path + redirect to `/`.
- **App-level** — folder-tree click updates the location; browser back restores
  the prior note; `/graph` shows the graph.
- **Regression** — existing vitest unit/component suites and the e2e suite stay
  green.

## Rollout / reversibility

Incremental and reversible: the store API is untouched; `RouteSync` is additive;
the only deletions are `App.tsx`'s local `view` state and direct `openNote` calls
at navigation sites (replaced by `navigate`). Reverting is removing `RouteSync`
and restoring the `view` `useState`.
