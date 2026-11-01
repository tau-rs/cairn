# Daemon "Reconnecting…" soft pill — design

**Date:** 2026-06-16
**Status:** Approved (brainstorm)
**Area:** `web/` — `cairn-web-ui` daemon transport + store seam + live-status UI

## Problem

`DaemonClient`'s WebSocket event channel already does silent exponential backoff
(`RECONNECT_BASE_MS=500`, `RECONNECT_MAX_MS=30_000`, escalate after
`ESCALATE_AFTER_ATTEMPTS=4`) — strategy B′. It fires `onError` **only** once
reconnection has clearly failed; the store then flips `liveUpdates` to `"down"`
and shows the hard `LiveUpdatesBanner`.

There is no signal for the transient "I'm retrying" window. A brief blip is
invisible, and a sustained outage jumps straight to the scary banner with
nothing in between. Goal: a calm "Reconnecting…" pill **during backoff, before
escalation**.

This is the deferred Wave-2 item from the daemon reconnect strategy; the "store
seam" it was waiting on is the `subscribe` callback set.

## Decisions (from brainstorm)

1. **Seam shape — third optional callback.** Add
   `onStatus?: (s: "reconnecting" | "live") => void` to `CairnClient.subscribe`,
   alongside the existing `onError`. Matches the existing plain-callback grain;
   transports with no reconnect concept (mock, tauri) simply never call it.
   Rejected: an options-object refactor (churns all transports + call sites for
   no gain today — YAGNI); a synthetic status `Event` on the domain stream
   (pollutes the `Event` union, fights the hexagonal boundary).

2. **Vocabulary — distinct.** The callback speaks the *channel*'s words
   (`"live"` / `"reconnecting"`); the store keeps its *user-facing* words
   (`"ok"` / `"reconnecting"` / `"down"`). The single mapping lives in the
   store's `connectEvents`.

3. **UI — extend `LiveUpdatesBanner`.** It already owns the bottom-right slot
   and already receives `status`. It switches on three states rather than
   spawning a second component. `App.tsx` is unchanged — the prop type just
   widens through.

## State machine

The store holds exactly one value, so the pill and banner are mutually
exclusive by construction.

```
            drop (attempt 1)              attempt >= 4 → onError
   ┌──────┐ ───────────────▶ ┌──────────────┐ ──────────────▶ ┌────────┐
   │ "ok" │                  │"reconnecting"│                 │ "down" │
   │ no UI│ ◀─────────────── │  calm pill   │ ◀───┐           │ banner │
   └──────┘  reopen→"live"   └──────────────┘     │           └────────┘
       ▲                       │     ▲            │ reopen→"live"  │  ▲
       │                       └─────┘            │                └──┘
       │       retry fails (2,3)→"reconnecting"   │        keeps retrying,
       └──────────────── reopen → "live" ─────────┘        but NO "reconnecting"
                         (only way out of "down")          (no downgrade to pill)
```

**Transitions**

| From | Trigger | Emission | To |
|------|---------|----------|----|
| ok | socket drops (attempt 1) | `onStatus("reconnecting")` | reconnecting |
| reconnecting | retry fails, attempt 2–3 | `onStatus("reconnecting")` | reconnecting |
| reconnecting | attempt ≥ 4 (escalate) | `onError(err)` | down |
| reconnecting | socket reopens | `onStatus("live")` | ok |
| down | retry fails again (attempt 5+) | `onError(err)` (silent re: pill) | down |
| down | socket reopens | `onStatus("live")` | ok |

**The invariant that makes it correct:** in `scheduleReconnect`, the two
emissions are mutually exclusive, so a `"down"` state is never downgraded back
to the calm pill:

```
attempt += 1
if (attempt >= ESCALATE_AFTER_ATTEMPTS) onError(err)            // → "down" (unchanged)
else                                    onStatus?.("reconnecting") // → pill, attempts 1–3 only
// onopen: attempt = 0; onStatus?.("live")                      // → "ok", recovers from pill OR banner
```

## Components & changes

### `web/src/client/types.ts`
Add the optional `onStatus` param to `CairnClient.subscribe`, documented as a
transient backoff signal that transports without reconnection never call.

### `web/src/client/daemon.ts`
- `subscribe(cb, onError?, onStatus?)`.
- `scheduleReconnect`: keep the existing `attempt >= ESCALATE_AFTER_ATTEMPTS →
  onError` branch; add an `else onStatus?.("reconnecting")` branch (mutually
  exclusive — preserves monotonicity, no downgrade once "down").
- `connect`/`sock.onopen`: after `attempt = 0`, call `onStatus?.("live")`.
- Mock (`MockClient` — `subscribe(cb)`) and Tauri (`TauriClient` —
  `subscribe(cb, onError?)`) need **no change**: omitting the new optional param
  stays a valid implementation of the widened interface. Behaviour unchanged.

### `web/src/store/store.ts`
- Widen `liveUpdates: "ok" | "reconnecting" | "down"`.
- `connectEvents` passes the third callback:
  ```ts
  client.subscribe(
    onEvent,
    () => set({ liveUpdates: "down" }),
    (s) => set({ liveUpdates: s === "live" ? "ok" : "reconnecting" }),
  );
  ```
- `refreshAll` (manual refresh) already optimistically sets `"ok"` — unchanged;
  a successful reopen will confirm via `onStatus("live")`, a failed one falls
  back to `"down"` via `onError`.

### `web/src/components/LiveUpdatesBanner.tsx`
- `status: "ok" | "reconnecting" | "down"`.
- `"ok"` → `null`; `"down"` → existing banner + Refresh (unchanged);
  `"reconnecting"` → calm pill: pulsing dot, muted text "Reconnecting…", **no**
  Refresh button, same positioning conventions (`fixed bottom-28 right-4 z-20`,
  `role="status"`).

### `web/src/app/App.tsx`
No change required — already renders `<LiveUpdatesBanner status={liveUpdates} … />`.

## Error handling

- An in-flight `"reconnecting"` that then succeeds → `"live"` → `"ok"` clears the
  pill.
- An in-flight `"reconnecting"` that escalates → `onError` → `"down"` shows the
  banner; subsequent retries stay silent (no pill flip-back).
- `onStatus` is optional everywhere; a transport that omits it degrades to
  today's two-state behaviour. The mock never errors and never reconnects, so it
  never calls either callback — its tests are unaffected.

## Testing (TDD)

**`web/src/client/daemon.test.ts`** (extends the existing B′ backoff block):
- On a transient drop (attempt 1) `onStatus("reconnecting")` fires and `onError`
  does not.
- Across attempts 1–3, `onStatus` is called with `"reconnecting"` and `onError`
  is never called.
- After escalation (attempt ≥ 4), `onError` fires and `onStatus` is **not**
  called again with `"reconnecting"` (no downgrade).
- On a successful reopen, `onStatus("live")` fires and the backoff resets
  (existing reset test extended to assert the emission).
- `onStatus` is optional — existing calls passing only `(cb)` / `(cb, onError)`
  still work.

**Store test** (`store.test.ts` or a focused new test):
- `ok → reconnecting → ok` via the status callback.
- `reconnecting → down` via `onError`, then `→ ok` via `onStatus("live")`.

**Component test** (if the suite covers `LiveUpdatesBanner`): renders the pill
(no Refresh button) for `"reconnecting"`, the banner (with Refresh) for
`"down"`, nothing for `"ok"`.

## Constraints

- **Hexagonal:** the React app stays transport-blind. Only the store seam +
  `DaemonClient` change behaviourally; mock/tauri keep their behaviour.
- **TDD:** tests precede implementation.
- **Full local gate before claiming green** — including `prettier --check` /
  `format:check`, which eslint won't catch.

## Definition of done

Pill shows during backoff, escalates to the existing banner on sustained
failure, clears on recovery; tests cover all transitions
(`ok→reconnecting→ok`, `reconnecting→down→ok`); full local gate green.
