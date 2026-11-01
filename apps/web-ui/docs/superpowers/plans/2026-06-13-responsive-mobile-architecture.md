# Responsive / Mobile Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the desktop-only UI responsive across three viewport tiers — a mobile bottom-nav shell, a tablet two-pane shell, and the unchanged desktop three-pane shell — by selecting a per-tier shell component that reuses the existing leaf components.

**Architecture:** A single `useBreakpoint()` hook (matchMedia, thresholds aligned to Tailwind's `md`/`lg` defaults) is the source of truth. `AppShell` reads it and renders exactly one of `MobileShell` / `TabletShell` / `Shell` (desktop). All three compose the same leaves (`TopBar`, `Sidebar`, `EditorPane`, `BacklinksPane`). Mobile/tablet-only UI state (`mobileTab`, `backlinksOpen`) lives in the existing `UiState` store slice.

**Tech Stack:** React 18, TypeScript, Zustand (store), Tailwind CSS, Radix UI (`@radix-ui/react-dialog`), lucide-react (icons), react-router v7, Vitest + Testing Library.

---

## File Structure

**New files:**
- `web/src/components/responsive/useBreakpoint.ts` — breakpoint hook (the one source of truth).
- `web/src/components/responsive/useBreakpoint.test.ts`
- `web/src/components/shells/regions.ts` — shared `ShellRegions` interface.
- `web/src/components/shells/AppShell.tsx` — picks the shell by tier.
- `web/src/components/shells/AppShell.test.tsx`
- `web/src/components/ui/Drawer.tsx` — Radix-Dialog overlay; `side: "right" | "bottom"` (tablet drawer + mobile sheet).
- `web/src/components/ui/Drawer.test.tsx`
- `web/src/components/shells/TabletShell.tsx`
- `web/src/components/shells/TabletShell.test.tsx`
- `web/src/components/shells/BottomNav.tsx` — five-tab mobile nav bar.
- `web/src/components/shells/BottomNav.test.tsx`
- `web/src/components/shells/MoreMenu.tsx` — mobile "More" tab content.
- `web/src/components/shells/MoreMenu.test.tsx`
- `web/src/components/shells/MobileShell.tsx` — bottom-nav shell.
- `web/src/components/shells/MobileShell.test.tsx`

**Modified files:**
- `web/src/store/store.ts` — add `mobileTab`, `backlinksOpen` to `UiState` + `DEFAULT_UI`.
- `web/src/app/App.tsx` — render `AppShell` instead of `Shell`.
- `web/src/components/Shell.tsx` — adopt the shared `ShellRegions` type (no behavior change).
- `web/src/components/EditorPane.tsx` — force single pane on mobile.
- `web/src/components/TopBar.tsx` — hide wordmark + Graph toggle below `md` (declutter mobile header).
- `web/src/vitest.setup.ts` — add a default `matchMedia` polyfill (jsdom lacks it).
- `web/index.html` — `viewport-fit=cover` for safe-area.
- `web/src/index.css` — 16px min input font on small screens (stop iOS focus zoom).

---

## Task 1: Foundations (viewport, safe-area, matchMedia test polyfill)

**Files:**
- Modify: `web/index.html`
- Modify: `web/src/vitest.setup.ts`
- Modify: `web/src/index.css`

- [ ] **Step 1: Add `viewport-fit=cover` to the viewport meta**

In `web/index.html`, replace the viewport meta line:

```html
    <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
```

(Do **not** add `maximum-scale`/`user-scalable=no` — pinch-zoom must stay enabled for accessibility.)

- [ ] **Step 2: Add a default `matchMedia` polyfill to the test setup**

jsdom has no `matchMedia`. Append to `web/src/vitest.setup.ts` (after the existing `ResizeObserver` block). Default `matches: true` resolves to the **desktop** tier, preserving every existing component test's behavior:

```ts
if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  window.matchMedia = ((query: string) => ({
    matches: true, // default to the largest tier (desktop) in tests
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
```

- [ ] **Step 3: Add a 16px input font rule for small screens**

Append to `web/src/index.css`:

```css
/* Stop iOS Safari from auto-zooming when focusing an input on small screens. */
@media (max-width: 767px) {
  input,
  textarea,
  select {
    font-size: 16px;
  }
}
```

- [ ] **Step 4: Run the existing suite to confirm nothing broke**

Run: `cd web && pnpm test -- --run`
Expected: PASS (same as before — the polyfill defaults to desktop).

- [ ] **Step 5: Commit**

```bash
git add web/index.html web/src/vitest.setup.ts web/src/index.css
git commit -m "feat(responsive): viewport-fit, matchMedia test polyfill, mobile input font"
```

---

## Task 2: `useBreakpoint` hook

**Files:**
- Create: `web/src/components/responsive/useBreakpoint.ts`
- Test: `web/src/components/responsive/useBreakpoint.test.ts`

- [ ] **Step 1: Write the failing test**

`web/src/components/responsive/useBreakpoint.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useBreakpoint } from "./useBreakpoint";

/** Build a matchMedia stub driven by a single viewport width. */
function installMatchMedia(width: number) {
  const listeners = new Set<() => void>();
  const mql = (query: string) => {
    const min = Number(/min-width:\s*(\d+)px/.exec(query)?.[1] ?? "0");
    return {
      matches: width >= min,
      media: query,
      addEventListener: (_: string, cb: () => void) => listeners.add(cb),
      removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
    } as unknown as MediaQueryList;
  };
  window.matchMedia = mql as unknown as typeof window.matchMedia;
  return {
    resize(next: number) {
      width = next;
      act(() => listeners.forEach((cb) => cb()));
    },
  };
}

describe("useBreakpoint", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("maps width to tier at the boundaries", () => {
    installMatchMedia(500);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("mobile");
    installMatchMedia(767);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("mobile");
    installMatchMedia(768);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("tablet");
    installMatchMedia(1023);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("tablet");
    installMatchMedia(1024);
    expect(renderHook(() => useBreakpoint()).result.current).toBe("desktop");
  });

  it("updates when the viewport crosses a breakpoint", () => {
    const mm = installMatchMedia(500);
    const { result } = renderHook(() => useBreakpoint());
    expect(result.current).toBe("mobile");
    mm.resize(1200);
    expect(result.current).toBe("desktop");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run useBreakpoint`
Expected: FAIL with "Cannot find module './useBreakpoint'".

- [ ] **Step 3: Write the implementation**

`web/src/components/responsive/useBreakpoint.ts`:

```ts
import { useEffect, useState } from "react";

export type Breakpoint = "mobile" | "tablet" | "desktop";

// Aligned with Tailwind's default `md` (768px) and `lg` (1024px) so CSS
// utilities and this hook always agree on tier boundaries.
const TABLET_QUERY = "(min-width: 768px)";
const DESKTOP_QUERY = "(min-width: 1024px)";

function read(): Breakpoint {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "desktop";
  }
  if (window.matchMedia(DESKTOP_QUERY).matches) return "desktop";
  if (window.matchMedia(TABLET_QUERY).matches) return "tablet";
  return "mobile";
}

/** The active responsive tier; re-renders when the viewport crosses 768/1024. */
export function useBreakpoint(): Breakpoint {
  const [bp, setBp] = useState<Breakpoint>(read);
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const tablet = window.matchMedia(TABLET_QUERY);
    const desktop = window.matchMedia(DESKTOP_QUERY);
    const update = () => setBp(read());
    tablet.addEventListener("change", update);
    desktop.addEventListener("change", update);
    update();
    return () => {
      tablet.removeEventListener("change", update);
      desktop.removeEventListener("change", update);
    };
  }, []);
  return bp;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run useBreakpoint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/responsive/
git commit -m "feat(responsive): add useBreakpoint hook"
```

---

## Task 3: `ShellRegions` type + `AppShell` selector, wired into App

**Files:**
- Create: `web/src/components/shells/regions.ts`
- Create: `web/src/components/shells/AppShell.tsx`
- Test: `web/src/components/shells/AppShell.test.tsx`
- Modify: `web/src/components/Shell.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Create the shared region type**

`web/src/components/shells/regions.ts`:

```ts
import type { ReactNode } from "react";

/** The four leaf regions every shell composes. */
export interface ShellRegions {
  topBar: ReactNode;
  list: ReactNode;
  editor: ReactNode;
  backlinks: ReactNode;
}
```

- [ ] **Step 2: Point `Shell.tsx` at the shared type (no behavior change)**

In `web/src/components/Shell.tsx`, replace the inline prop type. Change the import block and signature:

```tsx
import type { ShellRegions } from "./shells/regions";

export function Shell(props: ShellRegions) {
```

Leave the JSX body unchanged. (Remove the now-unused `import type { ReactNode } from "react";` line.)

- [ ] **Step 3: Write the failing test for AppShell**

`web/src/components/shells/AppShell.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import type { Breakpoint } from "../responsive/useBreakpoint";

const bp = vi.hoisted(() => ({ value: "desktop" as Breakpoint }));
vi.mock("../responsive/useBreakpoint", () => ({
  useBreakpoint: () => bp.value,
}));

import { AppShell } from "./AppShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

function renderAt(tier: Breakpoint) {
  bp.value = tier;
  return render(
    <MemoryRouter>
      <AppShell {...regions} />
    </MemoryRouter>,
  );
}

describe("AppShell", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders the desktop three-pane shell at desktop tier", () => {
    renderAt("desktop");
    // Backlinks region is always mounted in the desktop shell.
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
    expect(screen.getByText("LIST")).toBeInTheDocument();
  });

  it("renders the bottom nav at mobile tier", () => {
    renderAt("mobile");
    expect(screen.getByRole("navigation")).toBeInTheDocument();
  });
});
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cd web && pnpm test -- --run AppShell`
Expected: FAIL with "Cannot find module './AppShell'".

- [ ] **Step 5: Write AppShell**

`web/src/components/shells/AppShell.tsx`:

```tsx
import { Shell } from "../Shell";
import { MobileShell } from "./MobileShell";
import { TabletShell } from "./TabletShell";
import { useBreakpoint } from "../responsive/useBreakpoint";
import type { ShellRegions } from "./regions";

/** Selects the layout shell for the active viewport tier. */
export function AppShell(props: ShellRegions) {
  const bp = useBreakpoint();
  if (bp === "mobile") return <MobileShell {...props} />;
  if (bp === "tablet") return <TabletShell {...props} />;
  return <Shell {...props} />;
}
```

(This imports `MobileShell` and `TabletShell`, created in Tasks 6 and 9. The test will not pass until those exist — that's expected; AppShell is committed together with them at the end of Task 9. For now, create stub files so the module resolves: see Step 6.)

- [ ] **Step 6: Create temporary stubs so AppShell compiles**

`web/src/components/shells/TabletShell.tsx` (replaced in Task 6):

```tsx
import type { ShellRegions } from "./regions";
export function TabletShell(props: ShellRegions) {
  return <div>{props.editor}</div>;
}
```

`web/src/components/shells/MobileShell.tsx` (replaced in Task 9):

```tsx
import type { ShellRegions } from "./regions";
export function MobileShell(props: ShellRegions) {
  return (
    <div>
      <nav>stub</nav>
      {props.editor}
    </div>
  );
}
```

- [ ] **Step 7: Wire App.tsx to use AppShell**

In `web/src/app/App.tsx`, change the import and usage:

```tsx
import { AppShell } from "../components/shells/AppShell";
```

Replace the `<Shell ... />` element with:

```tsx
      <AppShell
        topBar={<TopBar />}
        list={<Sidebar />}
        editor={<EditorPane />}
        backlinks={<BacklinksPane />}
      />
```

(Remove the now-unused `import { Shell } from "../components/Shell";`.)

- [ ] **Step 8: Run tests to verify AppShell + Shell pass**

Run: `cd web && pnpm test -- --run AppShell Shell`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add web/src/components/shells/ web/src/components/Shell.tsx web/src/app/App.tsx
git commit -m "feat(responsive): AppShell tier selector + shared ShellRegions"
```

---

## Task 4: `Drawer` overlay primitive

**Files:**
- Create: `web/src/components/ui/Drawer.tsx`
- Test: `web/src/components/ui/Drawer.test.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/ui/Drawer.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Drawer } from "./Drawer";

describe("Drawer", () => {
  it("renders children when open and is labelled", () => {
    render(
      <Drawer open onClose={() => {}} side="right" label="Backlinks">
        <div>panel body</div>
      </Drawer>,
    );
    expect(screen.getByText("panel body")).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Backlinks" })).toBeInTheDocument();
  });

  it("does not render children when closed", () => {
    render(
      <Drawer open={false} onClose={() => {}} side="right" label="Backlinks">
        <div>panel body</div>
      </Drawer>,
    );
    expect(screen.queryByText("panel body")).not.toBeInTheDocument();
  });

  it("calls onClose on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Drawer open onClose={onClose} side="bottom" label="Sheet">
        <div>body</div>
      </Drawer>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run Drawer`
Expected: FAIL with "Cannot find module './Drawer'".

- [ ] **Step 3: Write the implementation**

`web/src/components/ui/Drawer.tsx`:

```tsx
import * as Dialog from "@radix-ui/react-dialog";
import type { ReactNode } from "react";

/** A slide-in overlay panel. `right` = side drawer (tablet); `bottom` = sheet (mobile). */
export function Drawer({
  open,
  onClose,
  side,
  label,
  children,
}: {
  open: boolean;
  onClose: () => void;
  side: "right" | "bottom";
  label: string;
  children: ReactNode;
}) {
  const pos =
    side === "right"
      ? "right-0 top-0 bottom-0 w-[min(85vw,320px)] border-l"
      : "left-0 right-0 bottom-0 max-h-[70vh] rounded-t-xl border-t pb-[env(safe-area-inset-bottom)]";
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className={
            "fixed z-50 overflow-y-auto border-border bg-surface p-3 text-text shadow-2xl focus:outline-none " +
            pos
          }
        >
          <Dialog.Title className="sr-only">{label}</Dialog.Title>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run Drawer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ui/Drawer.tsx web/src/components/ui/Drawer.test.tsx
git commit -m "feat(responsive): add Drawer overlay primitive"
```

---

## Task 5: `UiState` additions (`mobileTab`, `backlinksOpen`)

**Files:**
- Modify: `web/src/store/store.ts:84-101`
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing test**

`store.test.ts` already imports `createCairnStore` from `"./store"` and `MockClient` from `"../client/mock"`; it builds instances via `createCairnStore(...)`. Follow that pattern. Add a new `describe` block (and import `MockHost` + `FIXTURE_NOTES` alongside the existing imports if not already present — check the file header; `MockHost` comes from `"../client/host"`, `FIXTURE_NOTES` from `"../client/fixtures"`):

```ts
describe("mobile ui state", () => {
  function freshStore() {
    return createCairnStore(new MockClient(FIXTURE_NOTES), new MockHost());
  }

  it("defaults mobileTab to editor and backlinksOpen to false", () => {
    const ui = freshStore().getState().ui;
    expect(ui.mobileTab).toBe("editor");
    expect(ui.backlinksOpen).toBe(false);
  });

  it("setUi patches mobile fields", () => {
    const store = freshStore();
    store.getState().setUi({ mobileTab: "files", backlinksOpen: true });
    expect(store.getState().ui.mobileTab).toBe("files");
    expect(store.getState().ui.backlinksOpen).toBe(true);
  });
});
```

(Check the existing imports in `store.test.ts` first — the snippet earlier in this plan shows it already imports several `../client/*` helpers; reuse whatever is already imported and only add what's missing.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run store`
Expected: FAIL — `ui.mobileTab` is `undefined`.

- [ ] **Step 3: Extend `UiState` and `DEFAULT_UI`**

In `web/src/store/store.ts`, add a shared tab type above `UiState` (line ~84):

```ts
export type MobileTab = "files" | "editor" | "search" | "graph" | "more";
```

Add these two fields to the `UiState` interface (after `paletteOpen`):

```ts
  /** Active mobile bottom-nav tab (mobile shell only). */
  mobileTab: MobileTab;
  /** Backlinks drawer/sheet open (tablet + mobile shells). */
  backlinksOpen: boolean;
```

Add the matching defaults to `DEFAULT_UI`:

```ts
  mobileTab: "editor",
  backlinksOpen: false,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run store`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(responsive): add mobileTab + backlinksOpen to UiState"
```

---

## Task 6: `TabletShell`

**Files:**
- Create (replace stub): `web/src/components/shells/TabletShell.tsx`
- Test: `web/src/components/shells/TabletShell.test.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/shells/TabletShell.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { cairnStore } from "../../app/cairnStore";
import { TabletShell } from "./TabletShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

describe("TabletShell", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ backlinksOpen: false });
  });

  it("shows tree + editor and hides backlinks until toggled", () => {
    render(<TabletShell {...regions} />);
    expect(screen.getByText("LIST")).toBeInTheDocument();
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(screen.queryByText("BACKLINKS")).not.toBeInTheDocument();
  });

  it("opens the backlinks drawer via the Links button", async () => {
    render(<TabletShell {...regions} />);
    await userEvent.click(screen.getByRole("button", { name: /links/i }));
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run TabletShell`
Expected: FAIL — the stub renders only `EDITOR`, no Links button.

- [ ] **Step 3: Write the implementation**

Replace `web/src/components/shells/TabletShell.tsx`:

```tsx
import { useCairn, useActions } from "../../app/cairnStore";
import { Drawer } from "../ui/Drawer";
import { Button } from "../ui/Button";
import type { ShellRegions } from "./regions";

/** Tablet (768–1023px): tree + editor side by side; backlinks in a right drawer. */
export function TabletShell({ topBar, list, editor, backlinks }: ShellRegions) {
  const actions = useActions();
  const backlinksOpen = useCairn((s) => s.ui.backlinksOpen);
  return (
    <div className="flex h-full flex-col bg-bg text-text">
      <header className="flex items-center gap-2 border-b border-border bg-surface px-3 pt-[env(safe-area-inset-top)] [&>*:first-child]:flex-1 py-2">
        {topBar}
        <Button
          variant="ghost"
          onClick={() => actions.setUi({ backlinksOpen: true })}
        >
          Links
        </Button>
      </header>
      <div className="flex min-h-0 flex-1">
        <aside className="w-56 shrink-0 overflow-auto border-r border-border bg-surface p-2">
          {list}
        </aside>
        <main className="min-w-0 flex-1 overflow-auto p-3">{editor}</main>
      </div>
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="right"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run TabletShell`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shells/TabletShell.tsx web/src/components/shells/TabletShell.test.tsx
git commit -m "feat(responsive): TabletShell two-pane layout + backlinks drawer"
```

---

## Task 7: `BottomNav`

**Files:**
- Create: `web/src/components/shells/BottomNav.tsx`
- Test: `web/src/components/shells/BottomNav.test.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/shells/BottomNav.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BottomNav } from "./BottomNav";

describe("BottomNav", () => {
  it("renders five tabs and marks the active one", () => {
    render(<BottomNav active="graph" onSelect={() => {}} />);
    for (const label of ["Files", "Editor", "Search", "Graph", "More"]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Graph" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("calls onSelect with the tab id", async () => {
    const onSelect = vi.fn();
    render(<BottomNav active="editor" onSelect={onSelect} />);
    await userEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(onSelect).toHaveBeenCalledWith("files");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run BottomNav`
Expected: FAIL with "Cannot find module './BottomNav'".

- [ ] **Step 3: Write the implementation**

`web/src/components/shells/BottomNav.tsx`:

```tsx
import { Folder, FileText, Search, Share2, MoreHorizontal } from "lucide-react";
import type { ComponentType } from "react";
import type { MobileTab } from "../../store/store";

const TABS: { id: MobileTab; label: string; Icon: ComponentType<{ size?: number }> }[] = [
  { id: "files", label: "Files", Icon: Folder },
  { id: "editor", label: "Editor", Icon: FileText },
  { id: "search", label: "Search", Icon: Search },
  { id: "graph", label: "Graph", Icon: Share2 },
  { id: "more", label: "More", Icon: MoreHorizontal },
];

/** The mobile bottom tab bar. Stateless — parent owns `active`. */
export function BottomNav({
  active,
  onSelect,
}: {
  active: MobileTab;
  onSelect: (tab: MobileTab) => void;
}) {
  return (
    <nav className="flex shrink-0 border-t border-border bg-surface pb-[env(safe-area-inset-bottom)]">
      {TABS.map(({ id, label, Icon }) => (
        <button
          key={id}
          type="button"
          aria-label={label}
          aria-current={active === id ? "page" : undefined}
          onClick={() => onSelect(id)}
          className={
            "flex min-h-[44px] flex-1 flex-col items-center justify-center gap-0.5 py-1.5 text-[10px] " +
            (active === id ? "text-accent" : "text-faint")
          }
        >
          <Icon size={18} />
          {label}
        </button>
      ))}
    </nav>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run BottomNav`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shells/BottomNav.tsx web/src/components/shells/BottomNav.test.tsx
git commit -m "feat(responsive): BottomNav mobile tab bar"
```

---

## Task 8: `MoreMenu`

**Files:**
- Create: `web/src/components/shells/MoreMenu.tsx`
- Test: `web/src/components/shells/MoreMenu.test.tsx`

Note: Tags already live in the Files tab (the `Sidebar` renders `TagsPanel`), and Plugins are surfaced inside the Settings dialog (`SettingsDialog` renders `PluginsPanel`). So the More tab only needs entries for Settings and Commit/Sync.

- [ ] **Step 1: Write the failing test**

`web/src/components/shells/MoreMenu.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { cairnStore } from "../../app/cairnStore";
import { MoreMenu } from "./MoreMenu";

describe("MoreMenu", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ settingsOpen: false, commitOpen: false });
  });

  it("opens Settings", async () => {
    render(<MoreMenu />);
    await userEvent.click(screen.getByRole("button", { name: /settings/i }));
    expect(cairnStore.getState().ui.settingsOpen).toBe(true);
  });

  it("opens Commit", async () => {
    render(<MoreMenu />);
    await userEvent.click(screen.getByRole("button", { name: /commit/i }));
    expect(cairnStore.getState().ui.commitOpen).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run MoreMenu`
Expected: FAIL with "Cannot find module './MoreMenu'".

- [ ] **Step 3: Write the implementation**

`web/src/components/shells/MoreMenu.tsx`:

```tsx
import { Settings as SettingsIcon, GitCommit } from "lucide-react";
import type { ComponentType } from "react";
import { useActions } from "../../app/cairnStore";

/** Mobile "More" tab: lower-frequency surfaces. Tags live in Files; Plugins in Settings. */
export function MoreMenu() {
  const actions = useActions();
  const items: { label: string; Icon: ComponentType<{ size?: number }>; onClick: () => void }[] = [
    {
      label: "Settings",
      Icon: SettingsIcon,
      onClick: () => actions.setUi({ settingsOpen: true }),
    },
    {
      label: "Commit changes",
      Icon: GitCommit,
      onClick: () => actions.setUi({ commitOpen: true }),
    },
  ];
  return (
    <div className="flex flex-col p-2">
      {items.map(({ label, Icon, onClick }) => (
        <button
          key={label}
          type="button"
          onClick={onClick}
          className="flex min-h-[44px] items-center gap-3 rounded-md px-3 text-left text-sm text-text hover:bg-surface-2"
        >
          <Icon size={18} />
          {label}
        </button>
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run MoreMenu`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shells/MoreMenu.tsx web/src/components/shells/MoreMenu.test.tsx
git commit -m "feat(responsive): MoreMenu for mobile More tab"
```

---

## Task 9: `MobileShell`

**Files:**
- Create (replace stub): `web/src/components/shells/MobileShell.tsx`
- Test: `web/src/components/shells/MobileShell.test.tsx`

Behavior: the main region shows `list` (Files tab), `MoreMenu` (More tab), or `editor` (Editor/Search/Graph — `EditorPane` already renders the note, the search-results overlay, or the graph based on route/state). The bottom-nav highlight is **derived**: Files/More from `mobileTab`, otherwise from the route (graph) and search state. Selecting a tab updates `mobileTab` and navigates where a route exists. Backlinks is a bottom sheet toggled from the header.

- [ ] **Step 1: Write the failing test**

`web/src/components/shells/MobileShell.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { cairnStore } from "../../app/cairnStore";
import { MobileShell } from "./MobileShell";

const regions = {
  topBar: <div>TOPBAR</div>,
  list: <div>LIST</div>,
  editor: <div>EDITOR</div>,
  backlinks: <div>BACKLINKS</div>,
};

function renderShell() {
  return render(
    <MemoryRouter>
      <MobileShell {...regions} />
    </MemoryRouter>,
  );
}

describe("MobileShell", () => {
  beforeEach(() => {
    cairnStore.getState().setUi({ mobileTab: "editor", backlinksOpen: false });
  });

  it("shows the editor by default and the bottom nav", () => {
    renderShell();
    expect(screen.getByText("EDITOR")).toBeInTheDocument();
    expect(screen.getByRole("navigation")).toBeInTheDocument();
  });

  it("switches to the Files view when the Files tab is tapped", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Files" }));
    expect(screen.getByText("LIST")).toBeInTheDocument();
    expect(screen.queryByText("EDITOR")).not.toBeInTheDocument();
  });

  it("switches to the More view when the More tab is tapped", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "More" }));
    expect(screen.getByRole("button", { name: /settings/i })).toBeInTheDocument();
  });

  it("opens the backlinks bottom sheet from the header", async () => {
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: /backlinks/i }));
    expect(screen.getByText("BACKLINKS")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run MobileShell`
Expected: FAIL — the stub has no Files/More switching, no backlinks button.

- [ ] **Step 3: Write the implementation**

Replace `web/src/components/shells/MobileShell.tsx`:

```tsx
import { Link2 } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";
import { useCairn, useActions, cairnStore } from "../../app/cairnStore";
import { isGraph, noteUrl } from "../../app/routes";
import { IconButton } from "../ui/IconButton";
import { Drawer } from "../ui/Drawer";
import { BottomNav } from "./BottomNav";
import { MoreMenu } from "./MoreMenu";
import type { MobileTab } from "../../store/store";
import type { ShellRegions } from "./regions";

/** Mobile (<768px): one full-screen view at a time, driven by a bottom tab bar. */
export function MobileShell({ topBar, list, editor, backlinks }: ShellRegions) {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = useActions();
  const mobileTab = useCairn((s) => s.ui.mobileTab);
  const backlinksOpen = useCairn((s) => s.ui.backlinksOpen);
  const searchActive = useCairn((s) => s.searchResults !== null);

  // Active highlight is derived: Files/More are authoritative; the content tabs
  // (editor/search/graph) are read back from the route + search state.
  const active: MobileTab =
    mobileTab === "files"
      ? "files"
      : mobileTab === "more"
        ? "more"
        : isGraph(location)
          ? "graph"
          : searchActive
            ? "search"
            : "editor";

  function select(tab: MobileTab) {
    actions.setUi({ mobileTab: tab });
    if (tab === "graph") {
      navigate("/graph");
    } else if (tab === "editor" || tab === "search") {
      if (isGraph(location)) {
        const path = cairnStore.getState().activePath;
        navigate(path ? noteUrl(path) : "/");
      }
    }
  }

  const main =
    mobileTab === "files" ? list : mobileTab === "more" ? <MoreMenu /> : editor;

  return (
    <div className="flex h-full flex-col bg-bg text-text">
      <header className="flex items-center gap-1 border-b border-border bg-surface px-2 pt-[env(safe-area-inset-top)] [&>*:first-child]:min-w-0 [&>*:first-child]:flex-1 py-2">
        {topBar}
        <IconButton
          label="Backlinks"
          onClick={() => actions.setUi({ backlinksOpen: true })}
        >
          <Link2 size={18} />
        </IconButton>
      </header>
      <main className="min-h-0 flex-1 overflow-auto">{main}</main>
      <BottomNav active={active} onSelect={select} />
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="bottom"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run MobileShell`
Expected: PASS.

- [ ] **Step 5: Run the AppShell test (now that real shells exist)**

Run: `cd web && pnpm test -- --run AppShell`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shells/MobileShell.tsx web/src/components/shells/MobileShell.test.tsx
git commit -m "feat(responsive): MobileShell bottom-nav layout"
```

---

## Task 10: Force single editor pane on mobile

**Files:**
- Modify: `web/src/components/EditorPane.tsx`
- Test: `web/src/components/EditorPane.test.tsx`

- [ ] **Step 1: Write the failing test**

The existing `EditorPane.test.tsx` has a `seedSplit()` helper (two panes a.md/b.md) and a "split" test asserting `getByRole("separator")` plus tab labels `"a"` and `"b"` (TabStrip uses the note stem as the aria-label).

First, add the `vi` import and a controllable mock of the breakpoint hook at the top of the file (the existing import line is `import { render, screen } from ...`; add `vi` to the vitest import: `import { describe, it, expect, beforeEach, vi } from "vitest";`). Then add the mock just below the imports:

```tsx
const bp = vi.hoisted(() => ({ value: "desktop" as "desktop" | "mobile" }));
vi.mock("./responsive/useBreakpoint", () => ({
  useBreakpoint: () => bp.value,
}));
```

Add this test inside the existing `describe("EditorPane split", ...)` block (it reuses `seedSplit()` from `beforeEach`):

```tsx
  it("renders only the first pane on mobile even when two are open", () => {
    bp.value = "mobile";
    render(
      <MemoryRouter initialEntries={["/note/b.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    expect(screen.getByLabelText("a")).toBeInTheDocument(); // pane 0 tab strip
    expect(screen.queryByLabelText("b")).not.toBeInTheDocument(); // pane 1 gone
    expect(screen.queryByRole("separator")).not.toBeInTheDocument();
    bp.value = "desktop";
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run EditorPane`
Expected: FAIL — both panes still render on mobile.

- [ ] **Step 3: Implement the gate**

In `web/src/components/EditorPane.tsx`, import the hook at the top:

```tsx
import { useBreakpoint } from "./responsive/useBreakpoint";
```

Inside `EditorPane`, where `split` is computed, gate it on tier:

```tsx
  const bp = useBreakpoint();
  const split = panes.length > 1 && bp !== "mobile";
```

(Replace the existing `const split = panes.length > 1;` line.) The rest of the component already renders only pane 0 plus the divider/pane 1 when `split` is true, so no other change is needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run EditorPane`
Expected: PASS (the existing split test still passes — its mock defaults to `desktop`).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/EditorPane.tsx web/src/components/EditorPane.test.tsx
git commit -m "feat(responsive): single editor pane on mobile"
```

---

## Task 11: Declutter the TopBar on small screens

**Files:**
- Modify: `web/src/components/TopBar.tsx`
- Test: `web/src/components/TopBar.test.tsx`

The Graph toggle is redundant on mobile (Graph is a bottom-nav tab) and the "Cairn" wordmark wastes width. Hide both below `md` using Tailwind responsive utilities (pure CSS — no JS needed here).

- [ ] **Step 1: Write the failing test**

`TopBar.test.tsx` already imports `render, screen` and `MemoryRouter` and seeds the store in `beforeEach`. Add a new `describe` block at the end of the file:

```tsx
describe("TopBar responsive declutter", () => {
  it("hides the wordmark and graph toggle below md", () => {
    render(
      <MemoryRouter>
        <TopBar />
      </MemoryRouter>,
    );
    expect(screen.getByText("Cairn")).toHaveClass("hidden");
    expect(screen.getByRole("button", { name: "Graph" })).toHaveClass("hidden");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test -- --run TopBar`
Expected: FAIL — neither element has the `hidden` class.

- [ ] **Step 3: Implement responsive hiding**

In `web/src/components/TopBar.tsx`:

- Add `className="hidden md:inline"` to the wordmark span:

```tsx
      <span className="hidden text-sm font-semibold text-text md:inline">
        Cairn
      </span>
```

- Add `className="hidden md:inline-flex"` to the Graph toggle `Button`. The `Button` component must forward `className`; if it does, write:

```tsx
      <Button
        variant="ghost"
        className="hidden md:inline-flex"
        onClick={() =>
          navigate(toggleViewTarget(location, cairnStore.getState().activePath))
        }
      >
        {view === "graph" ? "Editor" : "Graph"}
      </Button>
```

Check `web/src/components/ui/Button.tsx` first: if it does not already merge an incoming `className`, add `className` to its props and append it to the internal class string (e.g. `className={base + " " + (props.className ?? "")}`). Keep that change minimal and covered by the existing `Button.test.tsx`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test -- --run TopBar Button`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/TopBar.tsx web/src/components/ui/Button.tsx web/src/components/TopBar.test.tsx
git commit -m "feat(responsive): declutter TopBar below md"
```

---

## Final verification

- [ ] **Step 1: Run the full local gate** (per the project's CI-local-gates note — `prettier --check` is easy to miss and eslint won't catch it):

Run from the repo root:

```bash
just web-check
```

If `just web-check` is not defined, run the equivalent in `web/`:

```bash
cd web && pnpm lint && pnpm exec prettier --check . && pnpm exec tsc --noEmit && pnpm test -- --run
```

Expected: all green.

- [ ] **Step 2: Manual smoke (optional but recommended)**

Run `cd web && pnpm dev`, open the app, and use the browser devtools device toolbar to resize across <768 / 768–1023 / ≥1024. Confirm: mobile shows the bottom nav and switches Files/Editor/Search/Graph/More; tablet shows tree+editor with a Links drawer; desktop is unchanged.

- [ ] **Step 3: Final commit if any formatting fixes were applied**

```bash
git add -A
git commit -m "chore(responsive): formatting + lint fixes"
```

---

## Notes on coverage vs. the design spec

- **Three tiers / breakpoints (768/1024):** Tasks 2, 3.
- **Mobile bottom-nav (Files/Editor/Search/Graph/More):** Tasks 7, 9.
- **Editor single-pane on mobile + compact tab strip:** Task 10 (single-pane). The tab strip is already horizontally laid out; no change needed for it to scroll on narrow widths — verify in Step 2 of Final verification and add `overflow-x-auto` to the `TabStrip` container only if it clips.
- **Backlinks as bottom sheet (mobile) / right drawer (tablet):** Tasks 4, 6, 9.
- **Tablet two-pane + split-panes retained:** Task 6 (TabletShell does not gate panes; only `MobileShell`/Task 10 forces single pane).
- **More = Settings + Commit; Tags in Files; Plugins in Settings:** Task 8.
- **Touch targets ≥44px:** Tasks 7, 8 (`min-h-[44px]`); verify tree rows in Step 2.
- **Safe-area insets:** Tasks 1, 6, 7, 9 (`viewport-fit=cover` + `env(safe-area-inset-*)`).
- **iOS input zoom:** Task 1.
- **⌘K palette reachable on mobile:** the palette is already command-driven; it is openable via `setUi({ paletteOpen: true })`. If a visible trigger is wanted on mobile, add an `IconButton` to the `MobileShell` header mirroring the Backlinks button (`onClick={() => actions.setUi({ paletteOpen: true })}`) — left out of the core tasks to avoid header crowding; add during Step 2 if desired.
- **Mobile dialog/palette full-width sizing:** the existing `Modal` already uses `w-[min(92vw,360px)]` and a `max-h` cap, so dialogs are already mobile-safe; no task required. Revisit only if Step 2 shows clipping.
