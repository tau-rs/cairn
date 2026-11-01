# Top-Level Error Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a React error boundary so a render-time throw shows a recoverable
fallback (with reload/retry) and logs a diagnostic, instead of unmounting the
whole tree to a blank window.

**Architecture:** A reusable class-component `ErrorBoundary` (React error
boundaries must be class components — there is no hook equivalent). It catches
errors via `getDerivedStateFromError` / `componentDidCatch`, logs the error +
component stack through an injectable `onError` hook (default `console.error`),
and renders a fallback. A default full-app fallback offers "Reload" (full page
reload). The boundary also accepts a custom `fallback` render-prop so we can wrap
the graph/editor region in a *secondary* boundary whose fallback offers an
in-app "Retry" that resets the boundary without reloading — one widget crashing
no longer nukes the whole UI.

**Tech Stack:** React 19, TypeScript, Vitest + @testing-library/react + jsdom,
Tailwind v4 (theme tokens in `web/tailwind.config.ts`).

---

## File Structure

- `web/src/components/ErrorBoundary.tsx` — new. The class component + the default
  full-app fallback UI. One responsibility: catch render errors and present a
  recoverable surface.
- `web/src/components/ErrorBoundary.test.tsx` — new. Behavior tests.
- `web/src/main.tsx` — modify. Wrap `<App />` in the top-level boundary.
- `web/src/app/App.tsx` — modify. Wrap the editor/graph region (the `editor`
  slot, lines ~246-300) in a secondary boundary with a compact retry fallback.

Theme tokens available (from `web/tailwind.config.ts`): `bg`, `surface`,
`surface-2`, `border`, `text`, `muted`, `accent`, `accent-fg`, `danger`,
`danger-bg`. Reuse the `Button` component (`web/src/components/ui/Button.tsx`)
for actions.

---

### Task 1: ErrorBoundary component + tests

**Files:**
- Create: `web/src/components/ErrorBoundary.tsx`
- Test: `web/src/components/ErrorBoundary.test.tsx`

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/components/ErrorBoundary.test.tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorBoundary } from "./ErrorBoundary";

function Boom({ when = true }: { when?: boolean }): JSX.Element {
  if (when) throw new Error("kaboom");
  return <div>safe child</div>;
}

describe("ErrorBoundary", () => {
  // React logs caught errors to console.error; silence it for clean output.
  let spy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    spy = vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => spy.mockRestore());

  it("renders children when nothing throws", () => {
    render(
      <ErrorBoundary>
        <div>safe child</div>
      </ErrorBoundary>,
    );
    expect(screen.getByText("safe child")).toBeInTheDocument();
  });

  it("renders the fallback instead of unmounting when a child throws", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    // The thrown child is gone; the recoverable fallback is shown.
    expect(screen.queryByText("safe child")).not.toBeInTheDocument();
    expect(screen.getByText(/something went wrong/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /reload/i }),
    ).toBeInTheDocument();
  });

  it("reports the error and component stack via onError", () => {
    const onError = vi.fn();
    render(
      <ErrorBoundary onError={onError}>
        <Boom />
      </ErrorBoundary>,
    );
    expect(onError).toHaveBeenCalledTimes(1);
    const [err, info] = onError.mock.calls[0];
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).message).toBe("kaboom");
    // componentStack is the diagnostic React provides.
    expect(info).toHaveProperty("componentStack");
    expect(typeof info.componentStack).toBe("string");
  });

  it("uses a custom fallback render-prop and can reset on retry", async () => {
    let shouldThrow = true;
    function Toggle(): JSX.Element {
      if (shouldThrow) throw new Error("kaboom");
      return <div>recovered child</div>;
    }
    render(
      <ErrorBoundary
        fallback={(reset) => (
          <button onClick={reset}>retry-widget</button>
        )}
      >
        <Toggle />
      </ErrorBoundary>,
    );
    expect(screen.getByText("retry-widget")).toBeInTheDocument();
    // Fix the underlying condition, then retry resets the boundary.
    shouldThrow = false;
    await userEvent.click(screen.getByText("retry-widget"));
    expect(screen.getByText("recovered child")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd web && pnpm vitest run src/components/ErrorBoundary.test.tsx`
Expected: FAIL — `Failed to resolve import "./ErrorBoundary"` (file not created yet).

- [ ] **Step 3: Implement the component**

```tsx
// web/src/components/ErrorBoundary.tsx
import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "./ui/Button";

type Props = {
  children: ReactNode;
  /** Diagnostic hook. Defaults to console.error. */
  onError?: (error: Error, info: ErrorInfo) => void;
  /**
   * Custom fallback. Receives a `reset` callback that clears the error state so
   * the boundary re-renders its children (use after the cause is resolved).
   * When omitted, the default full-app fallback (with a page reload) is shown.
   */
  fallback?: (reset: () => void) => ReactNode;
};

type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Log the error + component stack so a render throw leaves a diagnostic
    // trail instead of a silent blank window.
    if (this.props.onError) {
      this.props.onError(error, info);
    } else {
      console.error("ErrorBoundary caught an error:", error, info.componentStack);
    }
  }

  reset = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error === null) return this.props.children;
    if (this.props.fallback) return this.props.fallback(this.reset);
    return <DefaultFallback error={error} />;
  }
}

function DefaultFallback({ error }: { error: Error }) {
  return (
    <div
      role="alert"
      className="flex h-full min-h-[12rem] w-full flex-col items-center justify-center gap-4 bg-bg p-8 text-center text-text"
    >
      <div className="flex flex-col items-center gap-2">
        <h1 className="text-lg font-semibold">Something went wrong</h1>
        <p className="max-w-md text-sm text-muted">
          The app hit an unexpected error and can&apos;t continue. Reloading
          usually fixes it. If it keeps happening, the details below help with
          diagnosis.
        </p>
      </div>
      <Button variant="primary" onClick={() => window.location.reload()}>
        Reload
      </Button>
      <pre className="max-h-40 max-w-md overflow-auto rounded border border-border bg-surface px-3 py-2 text-left text-xs text-danger">
        {error.message}
      </pre>
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm vitest run src/components/ErrorBoundary.test.tsx`
Expected: PASS — all 4 tests green.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ErrorBoundary.tsx web/src/components/ErrorBoundary.test.tsx
git commit -m "feat(ui): add recoverable ErrorBoundary with diagnostic hook"
```

---

### Task 2: Wire the top-level boundary into main.tsx

**Files:**
- Modify: `web/src/main.tsx:9-15`

- [ ] **Step 1: Wrap `<App />` in the boundary**

```tsx
// web/src/main.tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./app/App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "@fontsource-variable/inter";
import "./index.css";
import "./components/editor/livePreview.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </ErrorBoundary>
  </React.StrictMode>,
);
```

- [ ] **Step 2: Verify the suite + typecheck still pass**

Run: `cd web && pnpm vitest run && pnpm typecheck`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add web/src/main.tsx
git commit -m "feat(ui): wrap app in top-level ErrorBoundary"
```

---

### Task 3: Secondary boundary around the editor/graph region

**Files:**
- Modify: `web/src/app/App.tsx` (import + the `editor` slot, ~lines 246-300)

- [ ] **Step 1: Import the boundary**

Add to the import block near the other component imports (after the
`ErrorToast` import on line 12):

```tsx
import { ErrorBoundary } from "../components/ErrorBoundary";
```

- [ ] **Step 2: Wrap the editor/graph content**

Wrap the existing `editor={ ... }` slot's inner content so a crash in the graph
library or editor decoration builder is contained to that pane. The existing
content is `<div className="relative h-full"> ... </div>`; wrap that div:

```tsx
editor={
  <ErrorBoundary
    fallback={(reset) => (
      <div
        role="alert"
        className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-text"
      >
        <p className="text-sm font-medium">This view crashed.</p>
        <p className="max-w-sm text-xs text-muted">
          The rest of the app is still usable. Retry to reload just this pane.
        </p>
        <Button variant="primary" onClick={reset}>
          Retry
        </Button>
      </div>
    )}
  >
    <div className="relative h-full">
      {/* ...existing SearchResults / GraphView / Editor content unchanged... */}
    </div>
  </ErrorBoundary>
}
```

(`Button` is already imported in App.tsx.)

- [ ] **Step 3: Verify suite, typecheck, and build**

Run: `cd web && pnpm vitest run && pnpm typecheck && pnpm build`
Expected: PASS; build completes.

- [ ] **Step 4: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat(ui): contain editor/graph crashes with a secondary ErrorBoundary"
```

---

## Self-Review

**Spec coverage** (brief §1):
- Top-level boundary wrapping the app → Task 2. ✓
- Fallback with recoverable message + reload/retry → `DefaultFallback` (Task 1) +
  retry fallback (Task 3). ✓
- Logs error + component stack for diagnostics → `componentDidCatch` + `onError`
  default `console.error` with `info.componentStack` (Task 1); test asserts it. ✓
- Secondary boundary around graph/editor → Task 3. ✓
- FAILING test: child throws → fallback shown + error reported (brief §2) →
  Task 1 Step 1 tests 2 & 3. ✓

**Placeholder scan:** The `{/* ...existing... */}` in Task 3 Step 2 references
real, unchanged code already in `App.tsx:247-299` — the engineer wraps it, not
authors it. No other placeholders.

**Type consistency:** `ErrorBoundary` props (`children`, `onError`, `fallback`),
`reset: () => void`, and `fallback: (reset) => ReactNode` are consistent across
Tasks 1, 3 and the tests.

**Constraint check (brief):** One finding (DX2), one PR. No app-wide error
handling rebuild — only the boundary, fallback, and diagnostic hook.
