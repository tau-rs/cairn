# Daemon "Reconnecting…" soft pill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a calm "Reconnecting…" pill during the daemon WebSocket's silent backoff window, before the existing hard "live updates unavailable" banner escalates.

**Architecture:** `DaemonClient` gains a third optional `onStatus` callback on `subscribe`, emitting `"reconnecting"` while it retries (attempts 1–3) and `"live"` on a successful (re)open — mutually exclusive with the existing `onError` escalation so a `"down"` state never downgrades back to the pill. The store maps that callback into a widened `liveUpdates: "ok" | "reconnecting" | "down"`. `LiveUpdatesBanner` renders a pill for the new middle state. The React app stays transport-blind; mock/tauri are untouched.

**Tech Stack:** TypeScript, React, Zustand store, Vitest + @testing-library/react, Tailwind (project color tokens), pnpm + `just`.

---

## File structure

| File | Change | Responsibility |
|------|--------|----------------|
| `web/src/client/types.ts` | Modify | Add optional `onStatus` to the `CairnClient.subscribe` seam |
| `web/src/client/daemon.ts` | Modify | Emit `"reconnecting"` on retry, `"live"` on reopen |
| `web/src/client/daemon.test.ts` | Modify | Cover the status emissions + no-downgrade invariant |
| `web/src/store/store.ts` | Modify | Widen `liveUpdates`; map `onStatus` in `connectEvents` |
| `web/src/store/store.test.ts` | Modify | Cover `ok→reconnecting→ok` and `reconnecting→down→ok` |
| `web/src/components/LiveUpdatesBanner.tsx` | Modify | Render the calm pill for `"reconnecting"` |
| `web/src/components/LiveUpdatesBanner.test.tsx` | Modify | Cover the pill (no Refresh button) |

`web/src/client/mock.ts`, `web/src/client/tauri.ts`, `web/src/app/App.tsx` need **no change** (the new param is optional; App already passes `status` through).

All commands below run from the `web/` directory unless noted. Final gate: `just web-ci` (run from repo root).

---

### Task 1: DaemonClient emits reconnect status

**Files:**
- Modify: `web/src/client/types.ts` (the `subscribe` signature, ~lines 24–27)
- Modify: `web/src/client/daemon.ts` (`subscribe` ~line 192, `scheduleReconnect` ~line 202, `onopen` ~line 222)
- Test: `web/src/client/daemon.test.ts` (inside the existing `describe("reconnect/backoff (B′)")` block, ~line 201)

- [ ] **Step 1: Write the failing tests**

Add these three tests inside the `describe("reconnect/backoff (B′)", ...)` block in `web/src/client/daemon.test.ts`, after the existing `"resets the backoff after a successful reconnect"` test:

```ts
it("emits 'reconnecting' on a transient drop without erroring", () => {
  const onError = vi.fn();
  const onStatus = vi.fn();
  const c = new DaemonClient({
    url: URL,
    fetch: vi.fn(),
    WebSocket: WS,
    random: () => 1,
  });
  c.subscribe(vi.fn(), onError, onStatus);
  FakeWebSocket.last().emitClose(); // first drop → attempt 1
  expect(onStatus).toHaveBeenCalledWith("reconnecting");
  expect(onError).not.toHaveBeenCalled();
});

it("emits 'live' on a successful (re)open", () => {
  const onStatus = vi.fn();
  const c = new DaemonClient({
    url: URL,
    fetch: vi.fn(),
    WebSocket: WS,
    random: () => 1,
  });
  c.subscribe(vi.fn(), vi.fn(), onStatus);
  FakeWebSocket.last().emitClose();
  vi.advanceTimersByTime(backoffDelay(1));
  FakeWebSocket.last().emitOpen(); // healed
  expect(onStatus).toHaveBeenCalledWith("live");
});

it("stops emitting 'reconnecting' once escalated to onError (no downgrade)", () => {
  const onError = vi.fn();
  const onStatus = vi.fn();
  const c = new DaemonClient({
    url: URL,
    fetch: vi.fn(),
    WebSocket: WS,
    random: () => 1,
  });
  c.subscribe(vi.fn(), onError, onStatus);
  // Each reconnect immediately drops again → 5 failed attempts.
  for (let i = 1; i <= 5; i++) {
    FakeWebSocket.last().emitClose();
    vi.advanceTimersByTime(backoffDelay(i));
  }
  expect(onError).toHaveBeenCalled();
  // Pill only for attempts 1–3; attempts 4,5 escalate via onError instead.
  const reconnecting = onStatus.mock.calls.filter((a) => a[0] === "reconnecting");
  expect(reconnecting).toHaveLength(3);
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && pnpm test src/client/daemon.test.ts`
Expected: the three new tests FAIL (`onStatus` is never called — it isn't wired yet), existing tests still PASS.

- [ ] **Step 3: Add `onStatus` to the seam type**

In `web/src/client/types.ts`, replace the `subscribe` member of `CairnClient`:

```ts
  /** Subscribe to push events. `onError` fires if the channel fails to attach,
   *  so the UI can surface a degraded "live updates unavailable" state and
   *  offer a manual refresh. The mock never errors. `onStatus` is an optional
   *  transient signal for transports that reconnect: `"reconnecting"` during
   *  silent backoff (before `onError` escalation), `"live"` on a successful
   *  (re)open. Transports with no reconnect concept never call it. */
  subscribe(
    cb: (e: Event) => void,
    onError?: (err: unknown) => void,
    onStatus?: (s: "reconnecting" | "live") => void,
  ): Unsubscribe;
```

- [ ] **Step 4: Emit the status in `DaemonClient`**

In `web/src/client/daemon.ts`, change the `subscribe` signature (~line 192):

```ts
  subscribe(
    cb: (e: Event) => void,
    onError?: (err: unknown) => void,
    onStatus?: (s: "reconnecting" | "live") => void,
  ): Unsubscribe {
```

In `scheduleReconnect` (~line 202), replace the escalation line so the two emissions are mutually exclusive:

```ts
    const scheduleReconnect = (err: unknown) => {
      if (closed) return;
      attempt += 1;
      // Stay silent for brief blips; surface a degraded state only once the
      // socket has clearly failed to come back (B′). Below the threshold we
      // emit the calm "reconnecting" pill instead — mutually exclusive with
      // onError, so a "down" state is never downgraded back to the pill.
      if (attempt >= ESCALATE_AFTER_ATTEMPTS) onError?.(err);
      else onStatus?.("reconnecting");
      // Full jitter over the capped exponential ceiling.
      timer = setTimeout(connect, this.random() * backoffDelay(attempt));
    };
```

In `connect`, extend the `onopen` handler (~line 222) so a healed socket announces it:

```ts
      sock.onopen = () => {
        attempt = 0;
        onStatus?.("live");
      };
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd web && pnpm test src/client/daemon.test.ts`
Expected: all tests PASS (the three new ones + every existing reconnect/HTTP/ask test).

- [ ] **Step 6: Commit**

```bash
git add web/src/client/types.ts web/src/client/daemon.ts web/src/client/daemon.test.ts
git commit -m "feat(daemon): emit reconnecting/live status during ws backoff

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Store maps the status into `liveUpdates`

**Files:**
- Modify: `web/src/store/store.ts` (type ~line 170, `connectEvents` ~lines 426–431)
- Test: `web/src/store/store.test.ts` (after the existing `"surfaces a degraded state…"` test, ~line 109)

- [ ] **Step 1: Write the failing tests**

Add these two tests in `web/src/store/store.test.ts`, after the `"surfaces a degraded state when the event channel fails to attach"` test. `Event` is already imported at the top of the file.

```ts
it("maps onStatus reconnecting/live onto liveUpdates", async () => {
  const { client, store } = setup();
  let statusCb: ((s: "reconnecting" | "live") => void) | undefined;
  vi.spyOn(client, "subscribe").mockImplementation(
    (
      _cb: (e: Event) => void,
      _onError?: (err: unknown) => void,
      onStatus?: (s: "reconnecting" | "live") => void,
    ) => {
      statusCb = onStatus;
      return () => {};
    },
  );
  await store.getState().init();
  expect(store.getState().liveUpdates).toBe("ok");
  statusCb!("reconnecting");
  expect(store.getState().liveUpdates).toBe("reconnecting");
  statusCb!("live");
  expect(store.getState().liveUpdates).toBe("ok");
});

it("escalates reconnecting to down, then recovers on live", async () => {
  const { client, store } = setup();
  let errorCb: ((err: unknown) => void) | undefined;
  let statusCb: ((s: "reconnecting" | "live") => void) | undefined;
  vi.spyOn(client, "subscribe").mockImplementation(
    (
      _cb: (e: Event) => void,
      onError?: (err: unknown) => void,
      onStatus?: (s: "reconnecting" | "live") => void,
    ) => {
      errorCb = onError;
      statusCb = onStatus;
      return () => {};
    },
  );
  await store.getState().init();
  statusCb!("reconnecting");
  expect(store.getState().liveUpdates).toBe("reconnecting");
  errorCb!(new Error("sustained failure"));
  expect(store.getState().liveUpdates).toBe("down");
  statusCb!("live");
  expect(store.getState().liveUpdates).toBe("ok");
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && pnpm test src/store/store.test.ts`
Expected: the two new tests FAIL — `liveUpdates` never becomes `"reconnecting"` (the callback isn't passed yet). TypeScript may also flag `"reconnecting"` as not assignable to the current `"ok" | "down"` type; both are fixed in Step 3.

- [ ] **Step 3: Widen the type and wire the callback**

In `web/src/store/store.ts`, widen the state field (~line 170):

```ts
  // "reconnecting" while the push channel is in silent backoff (calm pill);
  // "down" once it has clearly failed (hard banner). Both mean data may be
  // stale until reconnection or a manual refresh.
  liveUpdates: "ok" | "reconnecting" | "down";
```

In `connectEvents` (~line 426), pass the third callback:

```ts
    const connectEvents = () => {
      eventUnsub?.();
      eventUnsub = client.subscribe(
        onEvent,
        () => set({ liveUpdates: "down" }),
        (s) => set({ liveUpdates: s === "live" ? "ok" : "reconnecting" }),
      );
    };
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && pnpm test src/store/store.test.ts`
Expected: all store tests PASS (the two new ones + the existing `down`/`refreshAll` tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): map daemon reconnect status to liveUpdates state

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: LiveUpdatesBanner renders the calm pill

**Files:**
- Modify: `web/src/components/LiveUpdatesBanner.tsx` (whole component)
- Test: `web/src/components/LiveUpdatesBanner.test.tsx` (add one test, ~line 21)

- [ ] **Step 1: Write the failing test**

Add this test in `web/src/components/LiveUpdatesBanner.test.tsx`, inside the existing `describe("LiveUpdatesBanner", ...)`:

```ts
it("shows a calm reconnecting pill with no refresh button", () => {
  render(<LiveUpdatesBanner status="reconnecting" onRefresh={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent(/reconnecting/i);
  expect(screen.queryByRole("button")).toBeNull();
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && pnpm test src/components/LiveUpdatesBanner.test.tsx`
Expected: FAIL — `status="reconnecting"` is a TS type error today and the component renders nothing for it, so `getByRole("status")` throws.

- [ ] **Step 3: Add the pill branch**

Replace the whole body of `web/src/components/LiveUpdatesBanner.tsx`:

```tsx
import { Button } from "./ui/Button";

/** Surfaces the live-update channel state in the bottom-right slot (above the
 *  error/notice toasts, bottom-28). Three states, mutually exclusive:
 *  - "ok": nothing.
 *  - "reconnecting": a calm pill (pulsing dot, no action) during silent backoff.
 *  - "down": the hard "data may be stale" banner with a manual Refresh. */
export function LiveUpdatesBanner(props: {
  status: "ok" | "reconnecting" | "down";
  onRefresh: () => void;
}) {
  if (props.status === "ok") return null;

  if (props.status === "reconnecting") {
    return (
      <div
        role="status"
        className="fixed bottom-28 right-4 z-20 flex items-center gap-2 rounded-full border border-border bg-surface-2 px-3 py-1.5 text-sm text-muted shadow-lg"
      >
        <span
          aria-hidden
          className="h-2 w-2 rounded-full bg-accent animate-pulse"
        />
        <span>Reconnecting…</span>
      </div>
    );
  }

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

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd web && pnpm test src/components/LiveUpdatesBanner.test.tsx`
Expected: all three tests PASS (ok → empty, reconnecting → pill, down → banner+Refresh).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/LiveUpdatesBanner.tsx web/src/components/LiveUpdatesBanner.test.tsx
git commit -m "feat(ui): render calm reconnecting pill in LiveUpdatesBanner

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Full local gate

**Files:** none (verification only).

- [ ] **Step 1: Run the full web gate**

Run from the repo root: `just web-ci`
(Equivalent to `cd web && pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build`.)
Expected: PASS at every stage. If `format:check` fails, run `cd web && pnpm format` and amend the relevant commit — `prettier --check` is easy to miss and eslint won't catch it.

- [ ] **Step 2: Confirm done**

Verify against the Definition of Done: pill shows during backoff, escalates to the banner on sustained failure, clears on recovery; all transitions covered by tests; gate green. No further commit needed.

---

## Notes for the implementer

- **Why `bg-accent` for the dot, not amber:** the project palette (`tailwind.config.ts`) has no warning/amber token; `accent` (indigo) is the established "something is happening" color — `AnswerView` already uses `bg-accent animate-pulse` for the streaming cursor. Staying in-palette keeps it calm and theme-correct.
- **Why mock/tauri don't change:** the new `onStatus` param is optional, so `mock.ts`'s `subscribe(cb)` and `tauri.ts`'s `subscribe(cb, onError?)` remain valid implementations of the widened interface (a function with fewer params is assignable to one expecting more). They never reconnect, so they'd never call it anyway.
- **The invariant under test (Task 1, test 3):** once `attempt >= ESCALATE_AFTER_ATTEMPTS` the client emits `onError` and stops emitting `"reconnecting"`, so the store's `"down"` never falls back to the calm pill. The only exit from `"down"` is a real reopen → `onStatus("live")` → `"ok"`.
