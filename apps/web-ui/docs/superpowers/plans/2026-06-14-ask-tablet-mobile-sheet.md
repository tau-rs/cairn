# Ask Tablet/Mobile Sheet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the cairn `ask` conversation (`mode: "panel"`) as a slide-in sheet on tablet/mobile, and un-gate the bar's `⤢` promote so those breakpoints can reach it.

**Architecture:** Two new presentational/wiring files in `components/ask/` reuse the existing `AnswerView`, `AskSurfaceProps`, and `ui/Drawer`. `AskSheet` wraps the conversation in `Drawer` (`side="bottom"` mobile, `side="right"` tablet); `AskSheetHost` wires it to the store/router and is mounted directly by `MobileShell`/`TabletShell`. `DialogHost` drops the desktop-only promote gate; `AskPanel` gains Escape-to-close. No store/reducer/contract changes.

**Tech Stack:** React + TypeScript, Zustand store (`cairnStore`), Radix `@radix-ui/react-dialog` (via `ui/Drawer`), Vitest + `@testing-library/react` (jsdom).

---

## File Structure

- **Create** `web/src/components/ask/AskSheet.tsx` — presentational sheet: conversation (turn list + composer) inside `Drawer`. Props: `AskSurfaceProps & { side: "bottom" | "right" }`.
- **Create** `web/src/components/ask/AskSheet.test.tsx` — unit tests.
- **Create** `web/src/components/ask/AskSheetHost.tsx` — store/router wiring; props `{ side: "bottom" | "right" }`.
- **Create** `web/src/components/ask/AskSheetHost.test.tsx` — wiring tests.
- **Modify** `web/src/components/shells/MobileShell.tsx` — render `<AskSheetHost side="bottom" />`.
- **Modify** `web/src/components/shells/TabletShell.tsx` — render `<AskSheetHost side="right" />`.
- **Modify** `web/src/components/DialogHost.tsx` — remove the `canPromote` desktop gate + unused `useBreakpoint`.
- **Modify** `web/src/components/ask/AskPanel.tsx` — add Escape-to-close.
- **Modify** `web/src/components/ask/AskPanel.test.tsx` — add Escape test.

All commands below run from `web/` (`cd web` first). Run a single test file with `pnpm exec vitest run <path>` (NOT `pnpm test -- <file>`, which runs the whole suite).

---

### Task 1: `AskSheet` presentational component

**Files:**
- Create: `web/src/components/ask/AskSheet.tsx`
- Test: `web/src/components/ask/AskSheet.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/ask/AskSheet.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskSheet } from "./AskSheet";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  side: "bottom" as const,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskSheet", () => {
  it("renders nothing when closed", () => {
    render(<AskSheet {...base} open={false} />);
    expect(screen.queryByTestId("ask-sheet")).toBeNull();
  });

  it("renders all turns when open", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q1", citations: [], tools: [] },
      { role: "assistant", text: "a1", citations: [], tools: [] },
    ];
    render(<AskSheet {...base} turns={turns} />);
    expect(screen.getByText("q1")).toBeInTheDocument();
    expect(screen.getByText("a1")).toBeInTheDocument();
  });

  it("submits a follow-up", () => {
    const onSubmit = vi.fn();
    render(<AskSheet {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "more?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("more?");
  });

  it("closes via the ✕ button", () => {
    const onClose = vi.fn();
    render(<AskSheet {...base} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /close ask/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it("renders as a right side sheet too", () => {
    render(<AskSheet {...base} side="right" />);
    expect(screen.getByTestId("ask-sheet")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/ask/AskSheet.test.tsx`
Expected: FAIL — `Failed to resolve import "./AskSheet"` / `AskSheet is not defined`.

- [ ] **Step 3: Write minimal implementation**

Create `web/src/components/ask/AskSheet.tsx`:

```tsx
import { useState } from "react";
import { Drawer } from "../ui/Drawer";
import { AnswerView } from "./AnswerView";
import type { AskSurfaceProps } from "./AskBar";

/** Tablet/mobile conversation surface: the full turn list + composer inside a
 *  slide-in Drawer. `side="bottom"` is the mobile sheet, `side="right"` the
 *  tablet side sheet. The Drawer's Radix dialog supplies scrim + Escape close. */
export function AskSheet(props: AskSurfaceProps & { side: "bottom" | "right" }) {
  const { open, turns, streaming, error, onSubmit, onClose, onOpenNote, side } =
    props;
  const [value, setValue] = useState("");

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };
  const lastIdx = turns.length - 1;

  return (
    <Drawer open={open} onClose={onClose} side={side} label="Ask">
      <div data-testid="ask-sheet" className="flex h-full flex-col">
        <div className="mb-2 flex items-center justify-between text-sm font-semibold text-accent">
          <span>Ask ✦</span>
          <button
            aria-label="Close ask"
            className="text-faint"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {turns.map((t, i) => (
            <div key={i} className="my-1.5">
              <AnswerView
                turn={t}
                streaming={streaming && i === lastIdx && t.role === "assistant"}
                onOpenNote={onOpenNote}
              />
            </div>
          ))}
          {error && (
            <div data-testid="ask-error" className="m-1.5 text-sm text-danger">
              ⚠ {error}
            </div>
          )}
        </div>
        <div className="mt-2 flex gap-2 border-t border-border pt-2">
          <input
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                submit();
              }
            }}
            placeholder="Ask a follow-up…"
            className="flex-1 rounded-md border border-border bg-bg px-2 py-1.5 text-sm text-text focus:border-accent focus:outline-none"
          />
          <button
            aria-label="Send"
            className="rounded-md bg-accent px-3 text-accent-fg disabled:opacity-40"
            onClick={submit}
            disabled={streaming}
          >
            ↑
          </button>
        </div>
      </div>
    </Drawer>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/ask/AskSheet.test.tsx`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ask/AskSheet.tsx src/components/ask/AskSheet.test.tsx
git commit -m "feat(ask): add AskSheet drawer surface for small screens"
```

---

### Task 2: `AskSheetHost` store/router wiring

**Files:**
- Create: `web/src/components/ask/AskSheetHost.tsx`
- Test: `web/src/components/ask/AskSheetHost.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/ask/AskSheetHost.test.tsx` (mirrors `AskPanelHost.test.tsx`):

```tsx
import { describe, it, expect } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { AskSheetHost } from "./AskSheetHost";
import { cairnStore } from "../../app/cairnStore";

describe("AskSheetHost", () => {
  it("shows the sheet only in panel mode and submits follow-ups", () => {
    render(
      <MemoryRouter>
        <AskSheetHost side="bottom" />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId("ask-sheet")).toBeNull();

    act(() => {
      cairnStore.getState().askOpen();
      cairnStore.getState().askPromote();
    });
    expect(screen.getByTestId("ask-sheet")).toBeInTheDocument();

    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "hi" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(cairnStore.getState().ask.turns.length).toBeGreaterThan(0);

    act(() => {
      cairnStore.getState().askClose();
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/ask/AskSheetHost.test.tsx`
Expected: FAIL — `Failed to resolve import "./AskSheetHost"`.

- [ ] **Step 3: Write minimal implementation**

Create `web/src/components/ask/AskSheetHost.tsx` (mirrors `AskPanelHost.tsx`):

```tsx
import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../../app/cairnStore";
import { noteUrl } from "../../app/routes";
import { AskSheet } from "./AskSheet";
import { resolveStem } from "./citation";

/** Wires AskSheet to the store + router for the small-screen shells. `side` is
 *  supplied by the shell (mobile = bottom, tablet = right). Shows the sheet only
 *  when the conversation is in panel mode. */
export function AskSheetHost({ side }: { side: "bottom" | "right" }) {
  const navigate = useNavigate();
  const actions = useActions();
  const ask = useCairn((s) => s.ask);
  const notePaths = useCairn((s) => s.notePaths);

  return (
    <AskSheet
      open={ask.mode === "panel"}
      side={side}
      turns={ask.turns}
      streaming={ask.streaming}
      error={ask.error}
      onSubmit={actions.askSubmit}
      onClose={actions.askClose}
      onOpenNote={(target) => {
        const path = resolveStem(notePaths, target);
        if (path) navigate(noteUrl(path));
      }}
    />
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/ask/AskSheetHost.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/ask/AskSheetHost.tsx src/components/ask/AskSheetHost.test.tsx
git commit -m "feat(ask): add AskSheetHost wiring for small-screen shells"
```

---

### Task 3: Mount the sheet in the mobile + tablet shells

Glue wiring; behavior is already covered by Task 2's host test. Verified by typecheck/lint (no separate shell render test — the shells need heavy store/router setup for marginal value).

**Files:**
- Modify: `web/src/components/shells/MobileShell.tsx`
- Modify: `web/src/components/shells/TabletShell.tsx`

- [ ] **Step 1: Wire MobileShell**

In `web/src/components/shells/MobileShell.tsx`, add the import alongside the other component imports:

```tsx
import { AskSheetHost } from "../ask/AskSheetHost";
```

Then render it inside the root `<div>`, immediately after the backlinks `<Drawer>` block (before the closing `</div>`):

```tsx
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="bottom"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
      <AskSheetHost side="bottom" />
    </div>
```

- [ ] **Step 2: Wire TabletShell**

In `web/src/components/shells/TabletShell.tsx`, add the import:

```tsx
import { AskSheetHost } from "../ask/AskSheetHost";
```

Then render it after the backlinks `<Drawer>` block (before the closing `</div>`):

```tsx
      <Drawer
        open={backlinksOpen}
        onClose={() => actions.setUi({ backlinksOpen: false })}
        side="right"
        label="Backlinks"
      >
        {backlinks}
      </Drawer>
      <AskSheetHost side="right" />
    </div>
```

- [ ] **Step 3: Verify typecheck + lint pass**

Run: `pnpm typecheck && pnpm lint`
Expected: both PASS, no unused-import or type errors.

- [ ] **Step 4: Commit**

```bash
git add src/components/shells/MobileShell.tsx src/components/shells/TabletShell.tsx
git commit -m "feat(ask): mount AskSheetHost in mobile + tablet shells"
```

---

### Task 4: Un-gate `⤢` promote in `DialogHost`

**Files:**
- Modify: `web/src/components/DialogHost.tsx`

- [ ] **Step 1: Remove the desktop gate and the now-unused breakpoint read**

In `web/src/components/DialogHost.tsx`:

1. Delete the import line:

```tsx
import { useBreakpoint } from "./responsive/useBreakpoint";
```

2. Delete the local:

```tsx
  const breakpoint = useBreakpoint();
```

3. In the `<AskBar … />` element, delete this prop line entirely (leave the rest of the props unchanged):

```tsx
        canPromote={breakpoint === "desktop"}
```

`AskBar`'s `canPromote` defaults to `true`, so removing the prop enables `⤢` on every breakpoint.

- [ ] **Step 2: Verify typecheck + lint pass**

Run: `pnpm typecheck && pnpm lint`
Expected: both PASS — confirms no other use of `breakpoint`/`useBreakpoint` remained in the file.

- [ ] **Step 3: Verify AskBar promote coverage still passes**

Run: `pnpm exec vitest run src/components/ask/AskBar.test.tsx`
Expected: PASS — the existing "promotes to the panel" test exercises `⤢` with `canPromote` defaulting to `true`.

- [ ] **Step 4: Commit**

```bash
git add src/components/DialogHost.tsx
git commit -m "feat(ask): un-gate bar promote on tablet + mobile"
```

---

### Task 5: Escape-to-close in `AskPanel` (desktop)

`AskPanel` is a plain `<aside>` with no Radix dialog, so it has no native Escape handling. Add a window `keydown` listener active only while open.

**Files:**
- Modify: `web/src/components/ask/AskPanel.tsx`
- Modify: `web/src/components/ask/AskPanel.test.tsx`

- [ ] **Step 1: Write the failing test**

Add this test inside the `describe("AskPanel", …)` block in `web/src/components/ask/AskPanel.test.tsx`:

```tsx
  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<AskPanel {...base} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/ask/AskPanel.test.tsx`
Expected: FAIL — the new `closes on Escape` test fails (`onClose` not called); the other 4 still pass.

- [ ] **Step 3: Add the Escape listener**

In `web/src/components/ask/AskPanel.tsx`:

1. Change the React import to include `useEffect`:

```tsx
import { useEffect, useState } from "react";
```

2. Add the effect immediately after `const [value, setValue] = useState("");` and **before** the `if (!open) return null;` line (hooks must run before the early return):

```tsx
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/ask/AskPanel.test.tsx`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ask/AskPanel.tsx src/components/ask/AskPanel.test.tsx
git commit -m "feat(ask): close docked panel on Escape"
```

---

### Task 6: Full gate

- [ ] **Step 1: Run the complete gate**

Run: `pnpm format:check && pnpm typecheck && pnpm lint && pnpm test`
Expected: all PASS. If `format:check` fails, run `pnpm format` (or `pnpm exec prettier --write <files>`), re-stage, amend the relevant commit, and re-run.

- [ ] **Step 2: Push branch and open PR**

Base the PR on `ask-ui-track` (PR #62 not yet merged); retarget to `main` once #62 lands. Repo uses a merge queue — use "Merge when ready", do not manually update the branch.

```bash
git push -u origin ask-tablet-mobile-sheet
gh pr create --base ask-ui-track --title "feat(ask): tablet + mobile chat sheet" --body "<summary>"
```

---

## Notes / gotchas

- IDE diagnostics may emit false `Cannot find module 'react'`/`'vitest'` errors (tsc runs from repo root). Ignore — trust `pnpm typecheck`.
- Only one shell mounts per viewport, so `mode: "panel"` renders exactly one surface (desktop `AskPanel`, or one `AskSheet`). No double-render.
- `Drawer` (Radix `Dialog`, modal) provides scrim + Escape close for the sheet; the explicit ✕ is for discoverability. The desktop `AskPanel` needs its own Escape (Task 5) because it is not a Radix dialog.
