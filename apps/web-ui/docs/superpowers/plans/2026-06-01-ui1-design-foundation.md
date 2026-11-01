# Cairn UI‑1: Design Foundation + Navbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The frontend-design skill may be used by implementers for the primitives + navbar to hit the slickness bar.

**Goal:** Give the app a cohesive "Graphite" (Linear-like) design system — design tokens, Inter, primitive components, a restyled navbar with a logo — and sweep every existing component onto the tokens, with no behavior change.

**Architecture:** Define semantic color/type/radius tokens in Tailwind `theme.extend`; bundle Inter; add small primitive components (`Button`, `IconButton`, `Input`, `Logo`, `SectionLabel`) under `src/components/ui/`; restyle the navbar from them; then a visual-only token-adoption sweep across all components. Behavior/props/tests are unchanged (existing tests query by role/text/testid, so they guard against regressions).

**Tech Stack:** Tailwind 3 + `@fontsource-variable/inter`; React 18 + TS (existing). Vitest + Testing Library; Playwright e2e.

**Reference:** Spec `docs/superpowers/specs/2026-06-01-ui1-design-foundation-design.md`. Direction: Graphite/Linear-like, indigo `#6366f1` accent, Inter. UI‑1 is visual-only; dialogs (UI‑2), live-preview look (UI‑3), graph (UI‑4) are later cycles. All work under `web/`; run commands from `web/`.

---

## File Structure

```
web/tailwind.config.ts                  MOD  tokens: colors, fontFamily, radius
web/src/index.css                       MOD  @layer base body bg/text/font
web/src/main.tsx                        MOD  import Inter
web/src/components/ui/Button.tsx        NEW  + Button.test.tsx
web/src/components/ui/IconButton.tsx    NEW
web/src/components/ui/Input.tsx         NEW  + Input.test.tsx
web/src/components/ui/Logo.tsx          NEW  + Logo.test.tsx
web/src/components/ui/SectionLabel.tsx  NEW
web/src/app/App.tsx                     MOD  navbar: Logo + primitives
web/src/components/{Shell,NoteList,Backlinks,SearchBar,SearchResults,CommitBar,Settings,OpenCairn,ErrorToast,Editor}.tsx  MOD  token/primitive adoption
web/src/components/editor/livePreview.css  MOD  token colors
web/package.json                        MOD  + @fontsource-variable/inter
```

---

## Task 1: Design tokens + Inter

**Files:** Modify `web/tailwind.config.ts`, `web/src/index.css`, `web/src/main.tsx`, `web/package.json`.

- [ ] **Step 1: Install Inter**

From `web/`: `pnpm add @fontsource-variable/inter`

- [ ] **Step 2: Extend the Tailwind theme**

Replace the `theme` block in `web/tailwind.config.ts` so the file reads:
```ts
import type { Config } from "tailwindcss";
import typography from "@tailwindcss/typography";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#0e0e11",
        surface: "#141418",
        "surface-2": "#1d1d23",
        border: "#26262e",
        text: "#f1f1f4",
        muted: "#9a9ba6",
        faint: "#6b6c77",
        accent: "#6366f1",
        "accent-hover": "#7679f5",
        "accent-fg": "#ffffff",
        danger: "#f87171",
        "danger-bg": "#2a1416",
      },
      fontFamily: {
        sans: ['"Inter Variable"', "Inter", "system-ui", "sans-serif"],
      },
      borderRadius: { DEFAULT: "6px", sm: "4px", md: "6px", lg: "8px", xl: "12px" },
    },
  },
  plugins: [typography],
} satisfies Config;
```
(`danger`/`danger-bg` added for the error toast.)

- [ ] **Step 3: Base styles + Inter import**

`web/src/index.css`:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  body {
    @apply bg-bg text-text font-sans;
  }
}

html,
body,
#root {
  height: 100%;
  margin: 0;
}
```
In `web/src/main.tsx`, add (with the other imports, before `./index.css`):
```ts
import "@fontsource-variable/inter";
```

- [ ] **Step 4: Verify build + typecheck**

Run (from `web/`): `pnpm typecheck && pnpm build`
Expected: PASS (Inter bundles; new utilities available). Chunk-size advisory is fine.

- [ ] **Step 5: Commit**

```bash
git add web/tailwind.config.ts web/src/index.css web/src/main.tsx web/package.json web/pnpm-lock.yaml
git commit -m "feat(ui): graphite design tokens + Inter"
```

---

## Task 2: Primitive components

**Files:** Create `web/src/components/ui/{Button,IconButton,Input,Logo,SectionLabel}.tsx` and `Button.test.tsx`, `Input.test.tsx`, `Logo.test.tsx`.

- [ ] **Step 1: Write the failing tests**

`web/src/components/ui/Button.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button } from "./Button";

describe("Button", () => {
  it("renders children and fires onClick", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Go</Button>);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(onClick).toHaveBeenCalled();
  });
  it("applies the primary variant accent background", () => {
    render(<Button variant="primary">P</Button>);
    expect(screen.getByRole("button", { name: "P" }).className).toContain("bg-accent");
  });
  it("ghost variant has no accent background", () => {
    render(<Button variant="ghost">G</Button>);
    expect(screen.getByRole("button", { name: "G" }).className).not.toContain("bg-accent");
  });
});
```

`web/src/components/ui/Input.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Input } from "./Input";

describe("Input", () => {
  it("forwards placeholder and fires onChange", async () => {
    const onChange = vi.fn();
    render(<Input placeholder="Search…" onChange={onChange} />);
    await userEvent.type(screen.getByPlaceholderText("Search…"), "x");
    expect(onChange).toHaveBeenCalled();
  });
});
```

`web/src/components/ui/Logo.test.tsx`:
```tsx
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { Logo } from "./Logo";

describe("Logo", () => {
  it("renders an svg", () => {
    const { container } = render(<Logo />);
    expect(container.querySelector("svg")).not.toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- ui/`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement the primitives**

`web/src/components/ui/Button.tsx`:
```tsx
import type { ButtonHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost";

const VARIANT: Record<Variant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover",
  secondary: "bg-surface-2 text-text border border-border hover:bg-border",
  ghost: "text-muted hover:bg-surface-2 hover:text-text",
};

export function Button({
  variant = "secondary",
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent ${VARIANT[variant]} ${className}`}
      {...rest}
    />
  );
}
```

`web/src/components/ui/IconButton.tsx`:
```tsx
import type { ButtonHTMLAttributes, ReactNode } from "react";

export function IconButton({
  label,
  children,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button
      aria-label={label}
      className={`rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}
```

`web/src/components/ui/Input.tsx`:
```tsx
import type { InputHTMLAttributes } from "react";

export function Input({ className = "", ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`w-full rounded-md border border-border bg-bg px-2.5 py-1.5 text-sm text-text placeholder:text-faint focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent ${className}`}
      {...rest}
    />
  );
}
```

`web/src/components/ui/Logo.tsx`:
```tsx
export function Logo({ size = 18, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      className={`text-accent ${className}`}
      aria-hidden="true"
    >
      <rect x="3" y="3" width="10" height="10" rx="3" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <circle cx="8" cy="8" r="2" fill="currentColor" />
    </svg>
  );
}
```

`web/src/components/ui/SectionLabel.tsx`:
```tsx
import type { ReactNode } from "react";

export function SectionLabel({ children }: { children: ReactNode }) {
  return <span className="text-[10px] uppercase tracking-wide text-faint">{children}</span>;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test -- ui/`
Expected: PASS (Button 3, Input 1, Logo 1).

- [ ] **Step 5: typecheck + lint**

Run: `pnpm typecheck && pnpm lint`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/ui/
git commit -m "feat(ui): Button, IconButton, Input, Logo, SectionLabel primitives"
```

---

## Task 3: Navbar with logo

**Files:** Modify `web/src/app/App.tsx`.

- [ ] **Step 1: Restyle the navbar**

In `web/src/app/App.tsx`, import the primitives:
```tsx
import { Logo } from "../components/ui/Logo";
import { Button } from "../components/ui/Button";
```
Replace the `topBar={ … }` Shell prop's content. The bar has a left group (logo + brand + search + Graph toggle) and the right `CommitBar`. Use:
```tsx
      topBar={
        <div className="flex w-full items-center gap-3">
          <Logo />
          <span className="text-sm font-semibold text-text">Cairn</span>
          <SearchBar value={query} onChange={actions.setQuery} onSearch={actions.runSearch} />
          <Button
            variant="ghost"
            onClick={() => {
              const next = view === "graph" ? "editor" : "graph";
              setView(next);
              if (next === "graph") void actions.loadGraph();
            }}
          >
            {view === "graph" ? "Editor" : "Graph"}
          </Button>
          <span className="grow" />
          <CommitBar
            saving={saving}
            dirty={dirty}
            uncommitted={uncommitted}
            lastCommit={lastCommit}
            committing={committing}
            onCommit={actions.commitManual}
          />
        </div>
      }
```
(Keep the exact selectors/actions already in `App.tsx` — `query`, `view`, `setView`, `saving`, etc. Only the markup + the Graph toggle now use `Button`; the old inline graph-toggle `<button>` is replaced by this one.)

- [ ] **Step 2: Verify**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint`
Expected: PASS. The graph e2e clicks the button by name `/^graph$/i` / `/^editor$/i` — `Button` renders a real `<button>` with that text, so it still matches.

- [ ] **Step 3: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat(ui): navbar with logo + primitive buttons"
```

---

## Task 4: Token-adoption sweep

**Files:** Modify `web/src/components/{Shell,NoteList,Backlinks,SearchBar,SearchResults,CommitBar,Settings,OpenCairn,ErrorToast,Editor}.tsx`, `web/src/components/editor/livePreview.css`.

Apply this **token mapping** everywhere (visual only — do not change element roles, names, `data-testid`s, text, props, or handlers):

| Current | → Token class |
|---|---|
| `bg-neutral-900` (panes/app) | `bg-surface` (panes) / `bg-bg` (app root in Shell) |
| `bg-neutral-950` (inputs/editor) | `bg-bg` |
| `bg-neutral-800` (hover/active/inset) | `bg-surface-2` |
| `border-neutral-700` / `-800` | `border-border` |
| `text-neutral-100` / `-200` | `text-text` |
| `text-neutral-300` | `text-text` (or `text-muted` for clearly-secondary) |
| `text-neutral-400` / `-500` | `text-muted` |
| `text-neutral-500` / `-600` (captions/placeholders) | `text-faint` |
| `text-sky-400` / `-300` (links/accent) | `text-accent` |
| red error colors (`*-red-*`) | `text-danger` / `bg-danger-bg` / `border-danger` |

- [ ] **Step 1: Sweep each component** (one commit at the end, or per-file — your choice). Specifics:
  - **`Shell.tsx`**: root `bg-neutral-900` → `bg-bg`; the top bar + side `aside`s + borders → `bg-surface` / `border-border`; keep the three-pane structure.
  - **`NoteList.tsx`**: caps → `SectionLabel`; "+ New note" trigger → wrap text in a `Button variant="ghost"` (keep its `window.prompt` onClick unchanged); active row `bg-surface-2 text-text`, rows `text-muted hover:bg-surface-2`; delete ✕ → `text-faint hover:text-danger`.
  - **`Backlinks.tsx`**: caps → `SectionLabel`; empty state `text-faint`; link rows `text-muted hover:bg-surface-2 hover:text-text`.
  - **`SearchBar.tsx`**: replace the bare `<input>` with the `Input` primitive (keep `value`/`onChange`/`onKeyDown` + the `Search…` placeholder). `Input` is `w-full`; for the fixed search width, wrap it in a sized container: `<div className="w-64"><Input … /></div>` (don't pass `w-64` directly onto `Input` — it would conflict with the primitive's `w-full`).
  - **`SearchResults.tsx`**: overlay `bg-surface border border-border`; keep `data-testid="search-results"`; result rows `text-muted hover:bg-surface-2`; close `IconButton label="close"`.
  - **`CommitBar.tsx`**: status text `text-faint`/`text-muted`; `@commit` `text-faint`; the **Commit** trigger → `Button variant="primary"` (keep the `window.prompt` onClick unchanged).
  - **`Settings.tsx`**: caps → `SectionLabel`; labels `text-text`/`text-muted`; the number input → `Input` (or token classes); checkboxes keep behavior.
  - **`OpenCairn.tsx`**: `bg-bg text-text`; the "Open a cairn…" button → `Button variant="primary"`; subtitle `text-muted`/`text-faint`.
  - **`ErrorToast.tsx`**: `bg-danger-bg border border-danger text-danger`; dismiss → `IconButton label="dismiss"`.
  - **`Editor.tsx`**: header path `text-muted`; the mode-toggle button → `Button variant="ghost"` (keep onClick); container stays.
  - **`editor/livePreview.css`**: change hardcoded hex to the token hex — `.cm-lp-link` / `.cm-lp-wikilink.resolved` color → `#6366f1` (accent); inline-code bg → `#1d1d23` (surface-2); `.cm-lp-h6`/unresolved → `#6b6c77`/`#9a9ba6`. (CSS file can't read Tailwind tokens; use the literal hex matching the tokens.)

- [ ] **Step 2: Run the full gate**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: ALL PASS. If a test fails because it asserted a now-removed class, change that test to query by role/text instead (do NOT change component behavior). If `format:check` fails, run `pnpm format` and include changes.

- [ ] **Step 3: Run e2e**

Run: `pnpm e2e`
Expected: PASS (both tests) — markup roles/text/testids/`.cm-*` classes are unchanged, so selectors still match. (If port 5173 is busy with tau-ui, the Playwright `webServer` uses 5173 per `playwright.config.ts`; if it collides, kill the stray or note it — do not weaken assertions.)

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(ui): adopt design tokens + primitives across all components"
```

---

## Task 5: Manual visual check (controller)

Not a subagent task — the controller runs the app and screenshots.

- [ ] Start the dev server on a **non-5173 port** (5173 is tau-ui): `pnpm dev --port 5273 --strictPort`.
- [ ] Verify the graphite look: navbar (logo + Cairn + search + Graph + Commit), three panes on `surface`/`bg` with hairline borders, Inter type, indigo accents, consistent hover/active. Screenshot.
- [ ] Sanity: open a note, toggle Graph, open search — everything reads cohesively.

---

## Done criteria

- App uses a cohesive Graphite design system (tokens in Tailwind, Inter, primitives); navbar shows a logo + Cairn wordmark and primitive buttons; every component reads from the tokens — no stray `neutral-*`/`sky-*` left (`grep -rn "neutral-\|sky-" web/src` returns nothing, or only intentional exceptions).
- Behavior/props/tests unchanged: full unit suite + both e2e tests green; `typecheck`/`lint`/`format:check`/`build` clean. The `window.prompt`s, live-preview gutter, and React Flow graph are untouched (their cycles: UI‑2/3/4). Tauri/desktop unaffected.
```
