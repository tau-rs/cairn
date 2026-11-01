# Error Toast Queue + Structured Error Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-slot, lossy, unlogged `error` channel with a small toast queue (auto-dismiss + manual dismiss, multiple errors surface instead of clobbering) and `console.error` each caught command/query error with operation + context + typed `ContractError`.

**Architecture:** The store owns an `errors: Toast[]` queue. A single internal `pushError(operation, err, context?)` helper replaces every `set({ error: errMsg(err) })` call site: it `console.error`s the structured diagnostic, appends a toast via a *functional* `set` (so concurrent errors during a refresh storm queue rather than clobber), prefixes the surfaced message with the failing operation, and schedules auto-dismiss via `setTimeout`. `ErrorToast` renders the stack; each toast has its own dismiss button keyed by id.

**Tech Stack:** TypeScript, Zustand vanilla store, React, Vitest + Testing Library.

Addresses audit findings **U3** (`audit/design.md`) and **DG2** (`audit/diagnostics.md`).

---

### Task 1: Store — `Toast` type, `errors` queue, `pushError`, `dismissError(id)`

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write failing tests**

Add to `web/src/store/store.test.ts` (import `ERROR_TOAST_MS` alongside the existing imports: `import { createCairnStore, DEFAULT_SETTINGS, ERROR_TOAST_MS } from "./store";`).

```ts
it("queues multiple errors instead of clobbering", async () => {
  const { client, store } = setup();
  vi.spyOn(client, "sendCommand").mockRejectedValue(new Error("boom"));
  await store.getState().init();
  await store.getState().commitManual("one");
  await store.getState().commitManual("two");
  expect(store.getState().errors).toHaveLength(2);
});

it("auto-dismisses a queued error after the timeout", async () => {
  const { client, store } = setup();
  vi.spyOn(client, "sendCommand").mockRejectedValue(new Error("boom"));
  await store.getState().init();
  await store.getState().commitManual("x");
  expect(store.getState().errors).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(ERROR_TOAST_MS);
  expect(store.getState().errors).toHaveLength(0);
});

it("logs caught errors to console.error with operation context", async () => {
  const { client, store } = setup();
  const spy = vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(client, "runQuery").mockRejectedValueOnce({
    type: "internal",
    message: "boom",
  });
  await store.getState().init();
  expect(spy).toHaveBeenCalledWith(
    expect.stringContaining("List notes"),
    expect.objectContaining({
      operation: "List notes",
      error: expect.objectContaining({ type: "internal" }),
    }),
  );
  expect(store.getState().errors[0].message).toContain("List notes");
  spy.mockRestore();
});
```

Update the two existing error tests to the queue shape:

```ts
// was: expect(store.getState().error).toBe("boom");  (failing command test)
expect(store.getState().errors[0].message).toContain("boom");

// was: expect(store.getState().error).toBe("boom");  (failing list-notes test)
expect(store.getState().errors[0].message).toContain("boom");

// was: expect(store.getState().error).toBeTruthy();  (applyRenames stop-on-error test)
expect(store.getState().errors.length).toBeGreaterThan(0);
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: FAIL — `ERROR_TOAST_MS` is undefined / `errors` is undefined.

- [ ] **Step 3: Implement the queue in the store**

In `web/src/store/store.ts`:

a) Add the `Toast` type and timeout constant near the top (after imports, before `Settings`):

```ts
/** A queued, auto-dismissing error notification. */
export interface Toast {
  id: number;
  message: string;
}

/** How long a queued error toast stays before auto-dismissing (ms). */
export const ERROR_TOAST_MS = 6000;
```

b) In `CairnState`, replace `error: string | null;` with `errors: Toast[];` and change the action signature `dismissError(): void;` to `dismissError(id: number): void;`.

c) In `createCairnStore`, add a counter next to the existing `seq`/`pendingSelfWrites` declarations:

```ts
let errorSeq = 0;
```

d) Inside the `createStore` callback (alongside `setBuffer`/`applyTabs`/`dropNote`, which have `set`/`get` in scope), add the helper:

```ts
// Funnel for every caught command/query error. Logs a structured diagnostic
// (operation + context + typed ContractError) for devs, surfaces an
// operation-prefixed toast for users, and auto-dismisses it. Appends via a
// functional set() so concurrent errors during a refresh storm queue up
// instead of clobbering one another.
const pushError = (
  operation: string,
  err: unknown,
  context: Record<string, unknown> = {},
) => {
  console.error(`[cairn] ${operation} failed`, {
    operation,
    ...context,
    error: err,
  });
  const id = ++errorSeq;
  const message = `${operation}: ${errMsg(err)}`;
  set((s) => ({ errors: [...s.errors, { id, message }] }));
  setTimeout(() => {
    set((s) => ({ errors: s.errors.filter((t) => t.id !== id) }));
  }, ERROR_TOAST_MS);
};
```

e) In the returned state object, replace `error: null,` with `errors: [],`.

f) Replace `dismissError()` implementation:

```ts
dismissError(id) {
  set((s) => ({ errors: s.errors.filter((t) => t.id !== id) }));
},
```

g) Replace every `set({ error: errMsg(err) })` call site with a `pushError(...)` call carrying operation + context:

| Method | Replacement |
|---|---|
| `openCairn` | `pushError("Open vault", err);` |
| `refreshNotePaths` | `pushError("List notes", err);` |
| `openNote` | `pushError("Open note", err, { path });` |
| `saveNote` | `pushError("Save note", err, { path });` |
| `createNote` | `pushError("Create note", err, { path });` |
| `deleteNote` | `pushError("Delete note", err, { path });` |
| `applyRenames` | `pushError("Rename note", err, { from, to });` |
| `runSearch` | `pushError("Search", err, { query });` |
| `loadTags` | `pushError("Load tags", err);` |
| `filterByTag` | `pushError("Filter notes by tag", err, { tag });` |
| `loadPlugins` | `pushError("Load plugins", err);` |
| `invokePlugin` | `pushError("Run plugin command", err, { plugin, command });` |
| `refreshBacklinks` | `pushError("Load backlinks", err, { path });` |
| `loadGraph` | `pushError("Load graph", err);` |

Leave each method's other error-path logic (the `if (token !== seq.x) return;` guards, the `pendingSelfWrites` release in `saveNote`, the `break` in `applyRenames`, the buffer `saving:false` reset) exactly as-is — only the `set({ error: ... })` line changes.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: PASS (all store tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): error toast queue + structured error logging"
```

---

### Task 2: `ErrorToast` renders the queue

**Files:**
- Modify: `web/src/components/ErrorToast.tsx`
- Test: `web/src/components/ErrorToast.test.tsx`

- [ ] **Step 1: Rewrite the test for the queue API**

Replace `web/src/components/ErrorToast.test.tsx` body:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorToast } from "./ErrorToast";

describe("ErrorToast", () => {
  it("renders nothing when there are no errors", () => {
    const { container } = render(<ErrorToast errors={[]} onDismiss={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows each error and dismisses by id", async () => {
    const onDismiss = vi.fn();
    render(
      <ErrorToast
        errors={[
          { id: 1, message: "boom" },
          { id: 2, message: "kaboom" },
        ]}
        onDismiss={onDismiss}
      />,
    );
    expect(screen.getByText("boom")).toBeInTheDocument();
    expect(screen.getByText("kaboom")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: /dismiss/i });
    await userEvent.click(buttons[0]);
    expect(onDismiss).toHaveBeenCalledWith(1);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm vitest run src/components/ErrorToast.test.tsx`
Expected: FAIL — `ErrorToast` still expects `message`, not `errors`.

- [ ] **Step 3: Rewrite the component**

Replace `web/src/components/ErrorToast.tsx`:

```tsx
import type { Toast } from "../store/store";
import { IconButton } from "./ui/IconButton";

export function ErrorToast(props: {
  errors: Toast[];
  onDismiss: (id: number) => void;
}) {
  if (props.errors.length === 0) return null;
  return (
    <div className="fixed bottom-4 right-4 z-20 flex flex-col items-end gap-2">
      {props.errors.map((e) => (
        <div
          key={e.id}
          role="alert"
          className="flex items-center gap-3 rounded border border-danger bg-danger-bg px-3 py-2 text-sm text-danger shadow-lg"
        >
          <span>{e.message}</span>
          <IconButton label="dismiss" onClick={() => props.onDismiss(e.id)}>
            ✕
          </IconButton>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm vitest run src/components/ErrorToast.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ErrorToast.tsx web/src/components/ErrorToast.test.tsx
git commit -m "feat(ui): ErrorToast renders the error queue"
```

---

### Task 3: Wire `App.tsx` to the queue

**Files:**
- Modify: `web/src/app/App.tsx:101,343`

- [ ] **Step 1: Update the selector and render**

At `web/src/app/App.tsx:101` change:

```tsx
const error = useCairn((s) => s.error);
```
to
```tsx
const errors = useCairn((s) => s.errors);
```

At `web/src/app/App.tsx:343` change:

```tsx
<ErrorToast message={error} onDismiss={actions.dismissError} />
```
to
```tsx
<ErrorToast errors={errors} onDismiss={actions.dismissError} />
```

- [ ] **Step 2: Typecheck + full test run + build**

Run: `cd web && pnpm vitest run && pnpm exec tsc --noEmit && pnpm build`
Expected: tests PASS, no type errors, build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat(app): wire ErrorToast to the error queue"
```

---

## Self-Review

**Spec coverage:**
- Toast queue replacing single slot — Task 1 (`errors: Toast[]`). ✓
- Auto-dismiss + manual dismiss — Task 1 (`setTimeout` + `dismissError(id)`), Task 2 (per-toast ✕). ✓
- Surface multiple errors instead of clobbering — Task 1 (functional `set` append) + tests. ✓
- Stop unrelated `set` racing an error away — functional append; no success path writes `errors`. ✓
- `console.error` structured (operation + path + typed `ContractError`) — Task 1 `pushError`. ✓
- Surfaced message includes the failing operation — Task 1 (`${operation}: ...`). ✓
- Scoped, no retry/severity rebuild — confirmed; only the `error` slot + `ErrorToast` touched. ✓

**Placeholder scan:** none.

**Type consistency:** `Toast { id, message }` defined in store, imported by `ErrorToast`; `dismissError(id: number)`; `pushError(operation, err, context?)` — consistent across tasks.
