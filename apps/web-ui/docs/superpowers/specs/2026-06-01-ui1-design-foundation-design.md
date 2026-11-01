# Cairn Web UI — UI‑1: Design Foundation + Navbar Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑1 of the UI/UX overhaul initiative (see §1). Phase 5-adjacent
("shell polish") on [`docs/roadmap.md`](../../roadmap.md).
**Builds on:** the current app (skeleton + Tauri + live-preview + graph). Visual
only — runs on mock + Tauri unchanged.

---

## 1. Context: the overhaul initiative

A UI/UX audit found the app has **no design system** (zero tokens; ad-hoc
`neutral-*`/`sky-*` per component), plus specific complaints (live preview looks
like a code editor; auto-commit settings live under Backlinks; no navbar logo;
graph isn't Obsidian-like; native `window.prompt` dialogs). The overhaul is
decomposed into focused cycles, each its own spec→plan→build:

- **UI‑1 (this spec): design foundation + navbar/logo + token-adoption sweep.**
- UI‑2: in-app dialog system → styled new-note + commit modals; relocate
  auto-commit settings into a Settings modal (out of Backlinks).
- UI‑3: live-preview document look (no gutter/line-numbers, prose typography).
- UI‑4: Obsidian-style graph (circular nodes sized by link count, faint links,
  hover/zoom labels, physics).

UI‑1 is the base the rest inherit. **Direction (chosen): "Graphite" —
Linear-like**: near-black graphite surfaces, hairline borders, a single indigo
accent, Inter, comfortable-but-tight density.

---

## 2. Locked decisions

| Decision | Choice |
|---|---|
| Visual direction | Graphite / Linear-like (dark, hairline borders, indigo accent). |
| Accent | indigo `#6366f1` (hover `#7679f5`, on-accent text `#fff`). |
| Font | **Inter**, bundled via `@fontsource-variable/inter` (no CDN — offline/Tauri-friendly). |
| Scope | Foundation (tokens + primitives + navbar/logo) **plus** a token-adoption sweep across all existing components. Behavior/props/tests unchanged. |
| Logo | A small inline-SVG placeholder mark (easily swapped for a final asset). |

---

## 3. Design tokens

Define in `tailwind.config.ts` `theme.extend` so utilities (`bg-surface`,
`text-muted`, `border-border`, `bg-accent`…) are available app-wide:

```ts
colors: {
  bg:        "#0e0e11",   // app background
  surface:   "#141418",   // panels / cards
  "surface-2":"#1d1d23",  // hover / active / inset
  border:    "#26262e",   // hairline borders
  text:      "#f1f1f4",   // primary text
  muted:     "#9a9ba6",   // secondary text
  faint:     "#6b6c77",   // placeholders, captions, disabled
  accent:    "#6366f1",
  "accent-hover":"#7679f5",
  "accent-fg":"#ffffff",
},
fontFamily: { sans: ['"Inter Variable"', "Inter", "system-ui", "sans-serif"] },
borderRadius: { DEFAULT: "6px", sm: "4px", md: "6px", lg: "8px", xl: "12px" },
```
`web/src/index.css` sets the body to `bg-bg text-text font-sans` (via `@layer
base`) and imports nothing color-specific otherwise. `web/src/main.tsx` imports
`@fontsource-variable/inter`.

Naming note: keep the existing `livePreview.css` `.cm-lp-*` classes but switch
their hardcoded hex to the token values (e.g. wikilink color → `accent`).

---

## 4. Primitive components (`web/src/components/ui/`)

Each small, focused, prop-driven, and unit-tested:

- **`Button`** — `props: { variant?: "primary" | "secondary" | "ghost"; type?; disabled?; onClick?; children; className? }`.
  - primary: `bg-accent text-accent-fg hover:bg-accent-hover`
  - secondary: `bg-surface-2 text-text border border-border hover:bg-[#24242c]`
  - ghost: `text-muted hover:bg-surface-2 hover:text-text`
  - shared: `rounded-md px-3 py-1.5 text-sm font-medium disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent`.
- **`IconButton`** — square ghost button for icon-only actions
  (`props: { label: string (aria-label); onClick?; children; className? }`),
  `rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-text`.
- **`Input`** — text field (`props` mirror `<input>`):
  `w-full rounded-md bg-bg border border-border px-2.5 py-1.5 text-sm text-text placeholder:text-faint focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent`.
- **`Logo`** — inline SVG placeholder mark (`props: { size?: number; className? }`),
  monochrome, currentColor-friendly, ~16–20px (a simple geometric mark; e.g. a
  rounded square outline with a centered dot). Easily replaced later.
- **`SectionLabel`** — the small uppercased panel caps
  (`text-[10px] uppercase tracking-wide text-faint`).

These are presentational; no store/contract coupling.

---

## 5. Navbar

Restyle the top bar (composed in `App.tsx`'s `topBar` Shell prop), built from the
primitives + tokens:

- Left: `<Logo/>` + a "Cairn" wordmark (`text-text font-semibold`).
- Then the search `Input` (compact, fixed/clamped width).
- Right (justified): the **Graph** toggle (`Button` ghost/secondary), the commit
  status text (`text-faint`), and **Commit** (`Button` primary). The graph
  toggle + commit behavior is unchanged — only styling/markup changes.
- The bar sits on `surface` with a `border-border` bottom hairline.

(The settings gear + moving auto-commit there is UI‑2, not here. The auto-commit
panel stays where it is for now, just restyled to tokens.)

---

## 6. Token-adoption sweep

Restyle these to tokens/primitives — **visual only, no prop/behavior changes**:
`Shell`, `NoteList`, `Backlinks`, `SearchBar`, `SearchResults`, `CommitBar`,
`Settings`, `OpenCairn`, `ErrorToast`, the `Editor` header, plus
`editor/livePreview.css` and the wikilink colors. Swap every `neutral-*`/`sky-*`/
raw hex to `bg`/`surface`/`surface-2`/`border`/`text`/`muted`/`faint`/`accent`;
use `Button`/`IconButton`/`Input`/`SectionLabel` where those components currently
hand-roll a button/input/cap. Keep all element roles, names, `data-testid`s, and
text the same so existing tests/e2e pass unchanged.

The two `window.prompt` calls (new-note, commit) are **left as-is** in UI‑1 (their
modal replacement is UI‑2) — only their trigger buttons adopt the `Button` style.

---

## 7. Testing

- **Unit (Vitest + Testing Library):**
  - `Button`: renders children, fires `onClick`, applies the variant (assert a
    representative class is present per variant, e.g. primary has `bg-accent`).
  - `Input`: forwards `value`/`onChange`/`placeholder`; typing fires `onChange`.
  - `Logo`: renders an `<svg>`.
- **Regression:** the entire existing unit suite + both e2e tests must stay green
  unchanged — the sweep is visual, and existing tests query by role/text/testid,
  not styles. If any test incidentally asserts a removed class, fix the test to
  query by role/text instead (do not change component behavior).
- **Visual check:** run the app and confirm the graphite look + navbar/logo
  render cohesively (manual; I'll screenshot).
- All green on the mock; Tauri/desktop unaffected.

---

## 8. Files & dependencies

| File | Change |
|---|---|
| `web/tailwind.config.ts` | **Modify.** `theme.extend` colors + fontFamily + borderRadius. |
| `web/src/index.css` | **Modify.** `@layer base` body `bg-bg text-text font-sans`. |
| `web/src/main.tsx` | **Modify.** import `@fontsource-variable/inter`. |
| `web/src/components/ui/Button.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/ui/IconButton.tsx` | **New.** |
| `web/src/components/ui/Input.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/ui/Logo.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/ui/SectionLabel.tsx` | **New.** |
| `web/src/app/App.tsx` | **Modify.** navbar markup using Logo + primitives. |
| `web/src/components/{Shell,NoteList,Backlinks,SearchBar,SearchResults,CommitBar,Settings,OpenCairn,ErrorToast,Editor}.tsx` | **Modify.** token/primitive adoption. |
| `web/src/components/editor/livePreview.css` | **Modify.** token colors. |
| `web/package.json` | **Modify.** add `@fontsource-variable/inter`. |

Implementation should use the **frontend-design** skill for the primitives +
navbar to hit the slickness bar.

---

## 9. Risks

- **Test brittleness:** unlikely (tests query by role/text/testid), but the sweep
  touches many files — run the full suite after each component and fix any
  incidental class-based assertion by switching it to role/text.
- **Inter packaging:** use `@fontsource-variable/inter` (bundled) not a CDN, so
  the Tauri desktop build stays offline-capable; import once in `main.tsx`.
- **Scope creep:** keep UI‑1 strictly visual — no structural/behavioral changes
  (dialogs, live-preview internals, graph) leak in; those are UI‑2/3/4.
- **Contrast/accessibility:** ensure text/`muted`/`faint` on `bg`/`surface` meet
  reasonable contrast; `focus-visible` rings on interactive primitives.

---

## 10. Build order (for the plan)

1. Tokens: `tailwind.config.ts` colors/font/radius + `index.css` base + Inter dep/import. Build check.
2. Primitives (TDD where testable): `Button` (+test), `Input` (+test), `Logo` (+test), `IconButton`, `SectionLabel`.
3. Navbar: restyle `App.tsx` top bar with `Logo` + primitives.
4. Token-adoption sweep across the remaining components + `livePreview.css` (run full suite after).
5. Full gate: `pnpm test`/`typecheck`/`lint`/`format:check`/`build` + `pnpm e2e`; manual screenshot.
```
