# Responsive / Mobile Architecture — Design

**Date:** 2026-06-13
**Branch:** `responsive-mobile-architecture`
**Status:** Approved (design)

## Problem

The app is desktop-first with zero responsive code. `Shell.tsx` hardcodes a
three-column layout (`w-56` sidebar · `flex-1` editor · `w-56` backlinks) that
breaks below ~768px. There is no breakpoint detection, no mobile navigation, and
no touch affordances. We want a first-class mobile experience plus a graceful
tablet tier, built on an architecture that scales cleanly.

## Scope

**In scope:** Responsive layout only — pure frontend (React + Tailwind +
Zustand). Three layout tiers driven by viewport width. Reuse of existing leaf
components. Touch/safe-area/mobile-input polish.

**Out of scope (explicitly deferred):**
- Tauri Mobile (iOS/Android native build targets, touch-gesture plugins).
- A real web/networked transport — the browser still uses `MockClient` demo
  fixtures; this work makes the *layout* reusable if a real transport lands later.
- No engine/contract changes. No new backend commands.

The architecture is chosen so that adding Tauri-mobile or a web transport later
is additive, not a rewrite.

## Layout tiers

One source of truth — `useBreakpoint()` — maps viewport width to a tier. The
thresholds align with Tailwind's default `md` (768px) and `lg` (1024px) so JS and
CSS never disagree (no custom `screens` override needed).

| Tier      | Width        | Layout |
|-----------|--------------|--------|
| `mobile`  | `< 768px`    | Bottom-nav shell, one full-screen view at a time |
| `tablet`  | `768–1023px` | Two panes (tree + editor); backlinks = right slide-in drawer; split-panes stay enabled |
| `desktop` | `≥ 1024px`   | Today's full three-pane Shell, **unchanged** |

### Mobile (bottom-nav)

Bottom tab bar with five tabs: **Files · Editor · Search · Graph · More**.

- **Files** → the folder tree (`Sidebar`), full-screen. Opening a note
  auto-switches to the Editor tab.
- **Editor** → active note. Keeps a **compact, horizontally-scrollable tab
  strip**. **Split-panes are disabled** on mobile (single pane only). A "links"
  button in the top bar opens **Backlinks as a pull-up bottom sheet**.
- **Search** → search input + results, full-screen.
- **Graph** → graph view, full-screen.
- **More** → Tags, Plugins, Settings, Commit/Sync (lower-frequency surfaces).
- **⌘K command palette** → retained, reachable via a top-bar button (no physical
  keyboard on phone).

### Tablet

- Tree + editor side by side; editor keeps tabs and **split-panes stay enabled**
  (there is room).
- Backlinks collapses to a **right slide-in drawer** opened via a "links" button.
- No bottom nav — that is mobile-only.

### Desktop

Unchanged from today. `DesktopShell` is the current `Shell.tsx`.

## Architecture

**Approach: breakpoint hook + per-tier shell components, all composing the same
leaf components.**

```
useBreakpoint()  ──>  'mobile' | 'tablet' | 'desktop'   (matchMedia)
        │
   App.tsx renders exactly ONE shell for the active tier:
        ├─ DesktopShell   (today's Shell.tsx)
        ├─ TabletShell
        └─ MobileShell
                  │
   all three compose the SAME leaves, unchanged:
   <Sidebar> <EditorPane> <BacklinksPane> <SearchResults> <GraphView> <TopBar>
```

### Why this approach

- **vs. pure-CSS show/hide (Tailwind `md:`/`lg:` on one Shell):** the bottom-nav
  model is *behavioral*, not just visibility — "tap Files → switch to Editor",
  drawer open/close, single-view-at-a-time. CSS cannot drive that and would render
  all three layouts' DOM simultaneously.
- **vs. one mega-Shell with internal branching:** recreates the `App.tsx`
  monolith that the decomposition work (PR #39) deliberately broke apart. Thin
  per-tier shells stay readable and independently testable.

### Units and boundaries

- **`useBreakpoint()`** (`web/src/components/responsive/useBreakpoint.ts`)
  - *Does:* subscribes to two `matchMedia` queries; returns `'mobile' | 'tablet'
    | 'desktop'`.
  - *Uses:* call as a hook; re-renders on tier change.
  - *Depends on:* `window.matchMedia` only.
  - First paint is correct (matchMedia is synchronous) — no flash.
- **`DesktopShell`** = the existing `Shell.tsx` (renamed in usage; signature kept).
- **`TabletShell`** (`web/src/components/shells/TabletShell.tsx`)
  - *Does:* renders TopBar + tree + editor; owns the backlinks-drawer open state
    (from the `ui` slice).
- **`MobileShell`** (`web/src/components/shells/MobileShell.tsx`)
  - *Does:* renders a condensed TopBar, the active full-screen view, and the
    `BottomNav`. Owns active-tab selection and the backlinks bottom-sheet state.
- **`BottomNav`** (`web/src/components/shells/BottomNav.tsx`)
  - *Does:* five-tab bar; emits tab changes; reflects the active tab.
- **`Drawer` / `BottomSheet`** (`web/src/components/ui/`)
  - Small reusable overlay primitives (slide-in from edge / pull-up from bottom),
    used by Tablet (backlinks drawer) and Mobile (tree is a tab, backlinks is a
    sheet). Built on the existing Radix overlay patterns already in the repo.
- **`App.tsx`** picks the shell from `useBreakpoint()`; everything else
  (RouteSync, DialogHost, Toasts, LiveUpdatesBanner) stays at the App level,
  shared across tiers.

### State

Mobile/tablet UI state extends the existing `UiState` slice in
`web/src/store/store.ts` (alongside `paletteOpen`, `commitOpen`), set via the
existing `setUi(patch)` action:

```ts
// added to UiState
mobileTab: "files" | "editor" | "search" | "graph" | "more";  // default "editor"
backlinksOpen: boolean;   // tablet drawer + mobile sheet, default false
```

Where the URL already encodes a view, the bottom nav **projects** existing state
rather than duplicating it:
- **Graph** tab ↔ the graph route (`isGraph(location)`).
- **Editor** tab ↔ an active note route.
- **Search** tab ↔ search-active state (existing `searchResults` / `closeSearch`).

So `mobileTab` is only authoritative for **Files** and **More** (which have no
route today); the rest derive from router/store. Opening a note sets
`mobileTab = "editor"`.

### Breakpoint ↔ Tailwind alignment

Keep Tailwind defaults (`md` 768 / `lg` 1024). `useBreakpoint` uses the same
numbers. Leaf components may use `md:`/`lg:` utilities for in-component tweaks
(e.g. larger touch padding on mobile) with confidence that JS tiers agree.

## Cross-cutting responsive concerns

1. **Touch targets** — mobile nav items, tree rows, and tab-strip controls get
   ≥44px hit areas (padding, not just icon size).
2. **Safe-area insets** — add `viewport-fit=cover` to the viewport meta; pad the
   bottom nav with `env(safe-area-inset-bottom)` (and top bar with
   `safe-area-inset-top`) for notch/home-bar devices. Do **not** add
   `maximum-scale`/`user-scalable=no` (accessibility — keep pinch-zoom).
3. **iOS input zoom** — inputs use ≥16px font on mobile to prevent focus
   auto-zoom.
4. **Dialogs / palette on mobile** — render full-width / as bottom sheets on
   small screens, building on the existing modal-height cap (commit bc87940).
5. **Hover affordances** — hover-only controls (tree row actions, tab close
   buttons) become tap-reachable on touch (always-visible or via long-press/row
   tap), gated by a coarse-pointer check.

## Data flow

No new data flow. Leaf components consume the same store selectors and router
state as today. The only additions are `mobileTab`/`backlinksOpen` reads/writes
through `useCairn`/`setUi`, and `useBreakpoint()` reads at the `App` level.

## Error handling

- `useBreakpoint` defaults to `desktop` if `matchMedia` is unavailable
  (defensive; always present in target environments).
- The existing `ErrorBoundary` around `EditorPane` is preserved in every shell.
- No new failure modes — this is a presentational reorganization.

## Testing

- **`useBreakpoint`** — unit test with a `matchMedia` mock; assert tier mapping at
  767/768/1023/1024 boundaries and re-render on change events.
- **`MobileShell`** — render in jsdom; assert bottom nav renders, tab switching
  updates the visible view, "open note" forces the Editor tab, backlinks sheet
  toggles.
- **`TabletShell`** — assert two-pane layout + backlinks drawer toggle.
- **`BottomNav`** — tab selection callbacks + active state.
- **App tier selection** — mock `useBreakpoint` and assert the correct shell
  mounts per tier.
- Existing leaf-component and `Shell` tests remain valid (components reused
  unchanged; `Shell` becomes `DesktopShell`).
- Run the full local gate (lint, `prettier --check`, typecheck, unit) before
  claiming green — `prettier --check` is easy to miss.

## Implementation phasing (for the plan)

1. `useBreakpoint` hook + tests; align Tailwind/viewport meta + safe-area.
2. Extract `DesktopShell` (rename usage of current `Shell`); `App` selects shell
   by tier (desktop + tablet first, mobile stubbed).
3. `Drawer`/`BottomSheet` primitives; `TabletShell` with backlinks drawer.
4. `UiState` additions (`mobileTab`, `backlinksOpen`) + `MobileShell` + `BottomNav`
   + Editor/Backlinks mobile behavior (single pane, sheet).
5. Cross-cutting polish: touch targets, mobile dialog/palette sizing, hover→tap.

Each phase is independently testable and leaves the app working.
