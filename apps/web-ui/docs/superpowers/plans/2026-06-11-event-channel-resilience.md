# Event-Channel Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the Tauri event channel fails to attach (or is reconnected), surface a "live updates unavailable" state and give the user a manual refresh that re-pulls all reactive data and re-attaches the channel.

**Architecture:** The reactive-refresh model depends on `client.subscribe(...)` push events. Today `TauriClient.subscribe` does `pending.then(...)` with no `.catch`, so a failed `listen()` registration is an unhandled rejection and the UI silently goes stale. We (1) widen the `subscribe` contract with an optional `onError` callback, (2) have `TauriClient` `.catch` the attach failure and call it, (3) track a `liveUpdates: "ok" | "down"` flag in the store set to `"down"` by that callback, and (4) add a `refreshAll()` store action (the manual-refresh affordance) that re-attaches the channel and re-pulls notes/tags/graph/backlinks, clearing the flag. A small banner in `App.tsx` surfaces the state and triggers `refreshAll()`.

**Tech Stack:** TypeScript, Zustand vanilla store, Tauri `@tauri-apps/api/event`, React, Vitest.

**Scope guard:** Only the event-channel attach/drop handling + surfaced state + manual refresh. NOT the broader floating-promise / observability Low findings, NOT a refresh-pipeline rewrite.

---

## File Structure

- `web/src/client/types.ts` — widen `CairnClient.subscribe` signature with optional `onError`.
- `web/src/client/tauri.ts` — `.catch` the `listen()` attach and forward to `onError`.
- `web/src/client/mock.ts` — accept (ignore) the new `onError` arg; never errors.
- `web/src/store/store.ts` — `liveUpdates` state, `connectEvents()` helper (re-attachable), `refreshAll()` action.
- `web/src/components/LiveUpdatesBanner.tsx` — new: banner + Refresh button.
- `web/src/app/App.tsx` — render the banner, wired to store.
- Tests: `web/src/client/tauri.test.ts`, `web/src/store/store.test.ts`, `web/src/components/LiveUpdatesBanner.test.tsx`.

---

### Task 1: Widen the subscribe contract

**Files:**
- Modify: `web/src/client/types.ts`
- Modify: `web/src/client/mock.ts:91-94`

- [ ] **Step 1: Update the interface**

In `types.ts`, change the `subscribe` line to:

```ts
  /** Subscribe to push events. `onError` fires if the channel fails to attach
   *  (or later drops); the UI surfaces a degraded "live updates unavailable"
   *  state. The mock never errors. */
  subscribe(cb: (e: Event) => void, onError?: (err: unknown) => void): Unsubscribe;
```

- [ ] **Step 2: Update the mock to match the signature**

In `mock.ts`, change the method signature (body unchanged; the mock never errors):

```ts
  subscribe(cb: (e: Event) => void, _onError?: (err: unknown) => void): Unsubscribe {
    this.subscribers.add(cb);
    return () => this.subscribers.delete(cb);
  }
```

- [ ] **Step 3: Typecheck**

Run: `cd web && pnpm tsc --noEmit`
Expected: PASS.

---

### Task 2: TauriClient catches attach failure

**Files:**
- Modify: `web/src/client/tauri.ts:23-35`
- Test: `web/src/client/tauri.test.ts`

- [ ] **Step 1: Write the failing test** (append inside `describe("TauriClient")`)

```ts
  it("subscribe reports a listen-registration failure via onError instead of an unhandled rejection", async () => {
    const boom = new Error("attach failed");
    listen.mockImplementationOnce(() => Promise.reject(boom));
    const c = new TauriClient();
    const onError = vi.fn();
    c.subscribe(vi.fn(), onError);
    await Promise.resolve();
    await Promise.resolve();
    expect(onError).toHaveBeenCalledWith(boom);
  });
```

- [ ] **Step 2: Run it — expect FAIL** (onError never called)

Run: `cd web && pnpm vitest run src/client/tauri.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement** — replace the `subscribe` method body:

```ts
  subscribe(cb: (e: Event) => void, onError?: (err: unknown) => void): Unsubscribe {
    const pending = listen<Event>("cairn://event", (e) => cb(e.payload));
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    pending.then(
      (fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      },
      (err) => {
        // The channel never attached: every reactive refresh depends on these
        // push events, so report it rather than leave an unhandled rejection.
        if (!cancelled) onError?.(err);
      },
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }
```

- [ ] **Step 4: Run it — expect PASS**

Run: `cd web && pnpm vitest run src/client/tauri.test.ts`
Expected: PASS (existing subscribe test still green).

---

### Task 3: Store surfaces `liveUpdates` and a `refreshAll()` action

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing tests** (append inside `describe("cairn store")`)

```ts
  it("surfaces a degraded state when the event channel fails to attach", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "subscribe").mockImplementation((_cb, onError) => {
      onError?.(new Error("attach failed"));
      return () => {};
    });
    await store.getState().init();
    expect(store.getState().liveUpdates).toBe("down");
  });

  it("refreshAll re-pulls note paths and clears the degraded state", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    // Stub the channel so pushed events never reach the store (simulates a dead
    // stream): the note list goes stale until a manual refresh.
    vi.spyOn(client, "subscribe").mockImplementation((_cb, onError) => {
      onError?.(new Error("attach failed"));
      return () => {};
    });
    await store.getState().init();
    await client.sendCommand({ type: "write_note", path: "c.md", contents: "hi" });
    expect(store.getState().notePaths).not.toContain("c.md"); // stale: no live event
    await store.getState().refreshAll();
    expect(store.getState().notePaths).toContain("c.md");
    expect(store.getState().liveUpdates).toBe("ok");
  });
```

- [ ] **Step 2: Run them — expect FAIL** (`liveUpdates` undefined / `refreshAll` not a function)

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: FAIL.

- [ ] **Step 3: Add the state field + action type**

In the `CairnState` interface, after `error: string | null;` add:

```ts
  // "down" when the push-event channel failed to attach/dropped — the reactive
  // refresh model is degraded and data may be stale until a manual refresh.
  liveUpdates: "ok" | "down";
```

And in the method list (near `dismissError(): void;`) add:

```ts
  refreshAll(): Promise<void>;
```

- [ ] **Step 4: Add the closure-level unsubscribe handle**

Near `let intervalHandle ...` (top of `createCairnStore`) add:

```ts
  let eventUnsub: Unsubscribe | null = null;
```

Add the import at the top of the method list usage — `Unsubscribe` comes from `../client/types`; update the existing import:

```ts
import type { CairnClient, Unsubscribe } from "../client/types";
```

- [ ] **Step 5: Set the initial state value**

In the returned object, after `error: null,` add:

```ts
      liveUpdates: "ok",
```

- [ ] **Step 6: Extract the event handler + a re-attachable connect helper**

Inside `createStore((set, get) => { ... })`, above the `return {`, add the handler and helper (move the body currently inlined in `init`):

```ts
    // The push-event handler. Extracted so `connectEvents` can (re)attach it.
    const onEvent = (e: Event) => {
      if (e.type === "note_changed" || e.type === "note_deleted") {
        let selfWrite = false;
        if (e.type === "note_changed") {
          const pending = pendingSelfWrites.get(e.path) ?? 0;
          if (pending > 0) {
            if (pending === 1) pendingSelfWrites.delete(e.path);
            else pendingSelfWrites.set(e.path, pending - 1);
            selfWrite = true;
          }
        }
        if (!selfWrite) {
          void get().refreshNotePaths();
          void get().loadTags();
          if (get().graph !== null) void get().loadGraph();
        }
        const tag = get().activeTag;
        if (tag) void get().filterByTag(tag);
        else if (get().searchResults !== null) void get().runSearch(get().query);
        if (get().activePath) void get().refreshBacklinks();
      } else if (e.type === "committed") {
        set({ lastCommit: e.commit, uncommitted: false });
      }
    };

    // (Re)attach the event channel. A failed attach flips liveUpdates to "down"
    // so the UI can surface the degraded state and offer a manual refresh.
    const connectEvents = () => {
      eventUnsub?.();
      eventUnsub = client.subscribe(onEvent, () => set({ liveUpdates: "down" }));
    };
```

Add the `Event` type to the contract import at the top of `store.ts` (the existing line imports from `../contract`):

```ts
import type { ContractError, TagCount, Event } from "../contract";
```

(Keep the existing `PluginSummary` import line as-is.)

- [ ] **Step 7: Replace the inline subscribe in `init`**

In `init()`, replace the whole `client.subscribe((e) => { ... });` block (the comment line `// Subscribe once ...` plus the call) with:

```ts
        // Attach the push-event channel once, for the store's lifetime — NOT
        // inside the path gate.
        connectEvents();
```

- [ ] **Step 8: Add the `refreshAll` action**

After `dismissError() { ... },` add:

```ts
      async refreshAll() {
        // The manual-refresh affordance: re-attach the channel (in case it
        // dropped) and re-pull everything the push events would have. Clearing
        // the flag is optimistic — if re-attach fails again, connectEvents'
        // onError flips it back to "down".
        connectEvents();
        set({ liveUpdates: "ok" });
        await get().refreshNotePaths();
        await get().loadTags();
        if (get().graph !== null) await get().loadGraph();
        if (get().activePath) await get().refreshBacklinks();
      },
```

- [ ] **Step 9: Run the store tests — expect PASS**

Run: `cd web && pnpm vitest run src/store/store.test.ts`
Expected: PASS (existing `note_changed` reaction test still green — it uses the real `MockClient.subscribe`).

---

### Task 4: LiveUpdatesBanner component

**Files:**
- Create: `web/src/components/LiveUpdatesBanner.tsx`
- Test: `web/src/components/LiveUpdatesBanner.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LiveUpdatesBanner } from "./LiveUpdatesBanner";

describe("LiveUpdatesBanner", () => {
  it("renders nothing when live updates are ok", () => {
    const { container } = render(
      <LiveUpdatesBanner status="ok" onRefresh={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a refresh affordance when down and fires onRefresh", () => {
    const onRefresh = vi.fn();
    render(<LiveUpdatesBanner status="down" onRefresh={onRefresh} />);
    expect(screen.getByRole("status")).toHaveTextContent(/live updates unavailable/i);
    fireEvent.click(screen.getByRole("button", { name: /refresh/i }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run it — expect FAIL** (module not found)

Run: `cd web && pnpm vitest run src/components/LiveUpdatesBanner.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement** (mirror `ErrorToast.tsx` styling; `bottom-28` so it stacks above the error/notice toasts)

```tsx
import { Button } from "./ui/Button";

export function LiveUpdatesBanner(props: {
  status: "ok" | "down";
  onRefresh: () => void;
}) {
  if (props.status === "ok") return null;
  return (
    <div
      role="status"
      className="fixed bottom-28 right-4 z-20 flex items-center gap-3 rounded border border-border bg-surface-2 px-3 py-2 text-sm text-text shadow-lg"
    >
      <span>Live updates unavailable — data may be stale.</span>
      <Button variant="ghost" onClick={props.onRefresh}>
        Refresh
      </Button>
    </div>
  );
}
```

- [ ] **Step 4: Run it — expect PASS**

Run: `cd web && pnpm vitest run src/components/LiveUpdatesBanner.test.tsx`
Expected: PASS.

---

### Task 5: Wire the banner into App.tsx

**Files:**
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Import the banner** (near the `NoticeToast` import)

```tsx
import { LiveUpdatesBanner } from "../components/LiveUpdatesBanner";
```

- [ ] **Step 2: Select the state** (near `const notice = useCairn((s) => s.notice);`)

```tsx
  const liveUpdates = useCairn((s) => s.liveUpdates);
```

- [ ] **Step 3: Render it** (after the `<NoticeToast ... />` line)

```tsx
      <LiveUpdatesBanner
        status={liveUpdates}
        onRefresh={() => void actions.refreshAll()}
      />
```

- [ ] **Step 4: Typecheck + full test run**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run`
Expected: PASS.

---

### Task 6: Verify + commit

- [ ] **Step 1: Full gate** (matches CI — lint, format, typecheck, tests, build)

Run: `cd web && pnpm lint && pnpm prettier --check . && pnpm tsc --noEmit && pnpm vitest run && pnpm build`
Expected: all PASS.

- [ ] **Step 2: Self-review the diff for scope creep** (requesting-code-review skill)

Run: `git diff main...HEAD --stat`

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "fix(events): surface degraded state + manual refresh when the event channel fails"
```
