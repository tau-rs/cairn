# Live-collab presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A one-way live-awareness layer over the daemon `/collab` WebSocket — an open note joins its collab session, and peer edits surface as an ambient presence pill plus a non-destructive content refresh.

**Architecture:** `DaemonClient.openCollab(note, handlers)` (transport, mirrors the `/collab` plumbing PR #146 added) → `collabSlice` (owns the session + presence state, token-guarded) → a corner `CollabPresencePill` + a `CollabPresenceHost` effect that follows the active note. Incoming block ops are treated as opaque "something changed" signals; content always comes from `get_note` via the existing `reloadNoteBuffer`. No client CRDT, no outgoing ops.

**Tech Stack:** TypeScript, React, Zustand (vanilla store + `useStore`), Vitest, the vendored `cairn-contract` DTOs (`CollabClientMsg`/`CollabServerMsg`/`WireBlockOp`).

**Spec:** `docs/superpowers/specs/2026-08-21-live-collab-presence-design.md`

## Global Constraints

- **Depends on PR #146** (recovery/restore) being merged to `main`. This plan reuses #146's `reloadNoteBuffer(path): Promise<void>` (clean-only external buffer reload) and its `/collab` client plumbing. **Task 1 rebases onto post-#146 `main` and must pass before any other task starts.**
- **Contract is vendored RAW ts-rs.** Never edit or reformat `web/src/contract/*`; it is in `web/.prettierignore` and byte-checked by `scripts/check-contract-drift.sh`. Consume the types only.
- **Contract guards stay thin (S5):** tag-check the union discriminant, do not validate inner fields.
- **`/collab` is token-gated** on the daemon (unlike origin-gated `/events`). A WS handshake can't carry an `Authorization` header, so the bearer token rides as a `?token=` query param — mirror #146's `openRecovery` exactly.
- **Presence is non-critical:** a WS failure or `error` frame must never surface a hard banner and never disrupt editing. It stays/goes dark silently.
- **Transport parity:** daemon-only this slice. `TauriClient.openCollab` is an honest-reject stub (desktop `/collab` is a later follow-on, like `/ask` #72→#79).
- **One-way only:** never send `op`. Out of scope: client `BlockDoc`, block-level patch-apply, cursors/roster, two-way CRDT.
- **Gate before claiming green:** `just web-ci` (includes `prettier --check` / `format:check` — the classic miss). PRs only; base `main`; merge via the queue ("Merge when ready").
- **Naming (verbatim):** presence label copy is the binary string `"Live edits"`. Constants: `LIVE_DECAY_MS = 6000`, `COLLAB_RELOAD_DEBOUNCE_MS = 300`.

---

### Task 1: Rebase onto post-#146 main + confirm baseline

**Files:** none created; this is a git + verification gate.

**Interfaces:**
- Consumes (from #146, must exist after rebase): `CairnClient.openRecovery`, `store.reloadNoteBuffer(path): Promise<void>`, the `assertTagged(x, allowed, what)` helper in `contractGuards.ts`, the `?token=` `/collab` URL pattern in `daemon.ts`.
- Produces: a green working tree on `feat/live-collab-presence` rebased on the merge commit of #146.

- [ ] **Step 1: Confirm #146 has merged to main**

Run: `git fetch origin && gh pr view 146 --json state,mergedAt`
Expected: `"state":"MERGED"`. If still `OPEN`, STOP — this plan is blocked until #146 lands (it is queued in the merge queue).

- [ ] **Step 2: Rebase the feature branch onto updated main**

Run:
```bash
git checkout feat/live-collab-presence
git rebase origin/main
```
Expected: clean rebase (this branch is only the flag commit + the design spec commit).

- [ ] **Step 3: Verify the reused #146 seams exist**

Run:
```bash
grep -n "reloadNoteBuffer" web/src/store/store.ts
grep -n "openRecovery" web/src/client/types.ts
grep -n "assertTagged" web/src/client/contractGuards.ts
```
Expected: each prints a match. If any is missing, the rebase did not pick up #146 — STOP and reconcile.

- [ ] **Step 4: Confirm a green baseline**

Run: `just web-ci`
Expected: PASS. (If it fails on a fresh live-audit advisory unrelated to this branch, see the merge-queue live-audit note; a vitest "Failed to start forks worker" with 0 tests is a Node-version mismatch, not a real break.)

- [ ] **Step 5: Push the rebased branch**

Run: `git push --force-with-lease`

---

### Task 2: `assertCollabServerMsg` contract guard

**Files:**
- Modify: `web/src/client/contractGuards.ts`
- Test: `web/src/client/contractGuards.test.ts`

**Interfaces:**
- Consumes: `assertTagged<T>(x, allowed, what)`, `ContractShapeError` (existing in `contractGuards.ts`); `CollabServerMsg` from `../contract`.
- Produces: `assertCollabServerMsg(x: unknown): CollabServerMsg`.

- [ ] **Step 1: Write the failing test**

Add to `web/src/client/contractGuards.test.ts`:
```ts
import { assertCollabServerMsg } from "./contractGuards";

describe("assertCollabServerMsg", () => {
  it("accepts each valid variant", () => {
    for (const type of ["joined", "snapshot", "op", "error", "recoverable"]) {
      expect(assertCollabServerMsg({ type, note: "n.md" }).type).toBe(type);
    }
  });
  it("rejects an unknown tag", () => {
    expect(() => assertCollabServerMsg({ type: "bogus" })).toThrow(
      /Malformed collab server message/,
    );
  });
  it("rejects a non-object", () => {
    expect(() => assertCollabServerMsg(null)).toThrow(
      /Malformed collab server message/,
    );
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test contractGuards -- --run`
Expected: FAIL — `assertCollabServerMsg` is not exported.

- [ ] **Step 3: Add the guard**

In `web/src/client/contractGuards.ts`, add `CollabServerMsg` to the type imports from `../contract`, then add near the other `*_TYPES` arrays:
```ts
const COLLAB_SERVER_MSG_TYPES = [
  "joined",
  "snapshot",
  "op",
  "error",
  "recoverable",
] as const;
```
and near the other `assert*` exports:
```ts
export const assertCollabServerMsg = (x: unknown): CollabServerMsg =>
  assertTagged(x, COLLAB_SERVER_MSG_TYPES, "collab server message");
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm --dir web test contractGuards -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/contractGuards.ts web/src/client/contractGuards.test.ts
git commit -m "feat(collab): assertCollabServerMsg wire guard"
```

---

### Task 3: `openCollab` transport seam + `MockClient` implementation

**Files:**
- Modify: `web/src/client/types.ts` (add `CollabHandlers`, `CollabSession`, `openCollab` to `CairnClient`)
- Modify: `web/src/client/mock.ts`
- Test: `web/src/client/mock.test.ts`

**Interfaces:**
- Consumes: `WireBlockOp` from `../contract`.
- Produces:
  ```ts
  export interface CollabHandlers {
    onSnapshot?(note: string): void;
    onForeignOp?(note: string, op: WireBlockOp): void;
    onError?(note: string, message: string): void;
  }
  export interface CollabSession {
    close(): void;
  }
  // on CairnClient:
  openCollab(note: string, handlers: CollabHandlers): CollabSession;
  ```
  `MockClient` also exposes a test hook `mockCollabHandlers: CollabHandlers | null` (the handlers from the most recent `openCollab`) so tests and the slice tests can drive `onForeignOp` deterministically. The mock session is otherwise inert (no simulated peers), so mock-mode app runs never spuriously light the pill.

- [ ] **Step 1: Write the failing test**

Add to `web/src/client/mock.test.ts`:
```ts
it("openCollab returns an inert session and exposes handlers for driving", () => {
  const c = new MockClient({ "n.md": "# N\n" });
  let opSeen: string | null = null;
  const session = c.openCollab("n.md", {
    onForeignOp: (note) => {
      opSeen = note;
    },
  });
  expect(typeof session.close).toBe("function");
  // No peers in the mock: nothing fired on its own.
  expect(opSeen).toBeNull();
  // Tests can drive a foreign op through the captured handlers.
  c.mockCollabHandlers?.onForeignOp?.("n.md", {
    op: "delete",
    id: { replica: 1, counter: 2 } as never,
    lamport: 5 as never,
  });
  expect(opSeen).toBe("n.md");
  session.close(); // idempotent, no throw
  session.close();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test mock -- --run`
Expected: FAIL — `openCollab` is not a function.

- [ ] **Step 3: Add the seam types**

In `web/src/client/types.ts`, add `WireBlockOp` to the type imports from `../contract`, then add above the `CairnClient` interface:
```ts
/** Handlers for a live `/collab` presence session (one-way: we receive only). */
export interface CollabHandlers {
  /** The join was confirmed (`snapshot` arrived). Content is authoritative from
   *  disk, so this carries no ops — it just confirms the session is live. */
  onSnapshot?(note: string): void;
  /** A foreign block op arrived (a peer edit, the daemon's fold-back of a foreign
   *  disk save, or a restore). Treated as an opaque "changed" signal. */
  onForeignOp?(note: string, op: WireBlockOp): void;
  /** A protocol `error` frame. Non-fatal — presence just stays dark. */
  onError?(note: string, message: string): void;
}
/** A live presence session for one note. `close()` sends `leave` and closes. */
export interface CollabSession {
  close(): void;
}
```
and inside `CairnClient` (after `openRecovery`):
```ts
  /** Join `note`'s `/collab` session for live presence (one-way: receive peer
   *  ops, never send). Client-level capability like `openRecovery`; Tauri stubs
   *  a rejection since `/collab` is daemon-only. */
  openCollab(note: string, handlers: CollabHandlers): CollabSession;
```

- [ ] **Step 4: Implement on MockClient**

In `web/src/client/mock.ts`, import the new types and add:
```ts
  mockCollabHandlers: CollabHandlers | null = null;

  openCollab(_note: string, handlers: CollabHandlers): CollabSession {
    // Inert: the mock has no peers, so nothing fires on its own. Tests drive
    // handlers via `mockCollabHandlers`.
    this.mockCollabHandlers = handlers;
    return { close: () => {} };
  }
```
(Add `CollabHandlers`, `CollabSession` to the existing `./types` import.)

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm --dir web test mock -- --run`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/client/types.ts web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(collab): openCollab presence seam + inert MockClient session"
```

---

### Task 4: `DaemonClient.openCollab` (real WS session)

**Files:**
- Modify: `web/src/client/daemon.ts`
- Test: `web/src/client/daemon.test.ts`

**Interfaces:**
- Consumes: `assertCollabServerMsg` (Task 2); `CollabHandlers`/`CollabSession` (Task 3); `CollabClientMsg`, `CollabServerMsg` from `../contract`; the injectable `this.WS`/`this.random` already on `DaemonClient`.
- Produces: `DaemonClient.openCollab(note, handlers): CollabSession` — opens `ws://…/collab?token=…`, sends `join` on open, routes `snapshot`→`onSnapshot`, `op`→`onForeignOp`, `error`→`onError` (ignores `joined`/`recoverable`), sends `leave` on `close()`.

- [ ] **Step 1: Write the failing test**

Add to `web/src/client/daemon.test.ts` a self-contained fake socket + test:
```ts
class CollabFakeWS {
  static last: CollabFakeWS | null = null;
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];
  closed = false;
  constructor(public url: string) {
    CollabFakeWS.last = this;
  }
  send(s: string) {
    this.sent.push(s);
  }
  close() {
    this.closed = true;
  }
  // test helpers
  open() {
    this.onopen?.();
  }
  message(obj: unknown) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
}

it("openCollab joins, routes frames, and leaves on close", () => {
  const client = new DaemonClient({
    url: "http://d",
    token: "t0",
    WebSocket: CollabFakeWS as unknown as { new (u: string): WebSocket },
    random: () => 0.5,
  });
  const ops: string[] = [];
  let snap = 0;
  const session = client.openCollab("n.md", {
    onSnapshot: () => (snap += 1),
    onForeignOp: (note) => ops.push(note),
  });
  const ws = CollabFakeWS.last!;
  expect(ws.url).toBe("ws://d/collab?token=t0"); // token-gated query param

  ws.open();
  expect(JSON.parse(ws.sent[0])).toMatchObject({ type: "join", note: "n.md" });

  ws.message({ type: "snapshot", note: "n.md", ops: [] });
  expect(snap).toBe(1);
  ws.message({
    type: "op",
    note: "n.md",
    op: { op: "delete", id: { replica: 1, counter: 2 }, lamport: 5 },
  });
  expect(ops).toEqual(["n.md"]);
  // A frame for a different note is ignored.
  ws.message({ type: "op", note: "other.md", op: { op: "delete", id: { replica: 1, counter: 3 }, lamport: 6 } });
  expect(ops).toEqual(["n.md"]);
  // A malformed frame is dropped, not thrown.
  expect(() => ws.message({ type: "bogus" })).not.toThrow();

  session.close();
  expect(JSON.parse(ws.sent[1])).toMatchObject({ type: "leave", note: "n.md" });
  expect(ws.closed).toBe(true);
  session.close(); // idempotent
  expect(ws.sent.length).toBe(2);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test daemon -- --run`
Expected: FAIL — `openCollab` is not a function on `DaemonClient`.

- [ ] **Step 3: Implement `openCollab`**

In `web/src/client/daemon.ts`: add `CollabServerMsg` to the `../contract` type imports (`CollabClientMsg` is already imported by #146), add `assertCollabServerMsg` to the `./contractGuards` imports, and `CollabHandlers`, `CollabSession` to the `./types` imports. Then add the method to `DaemonClient` (after `openRecovery`):
```ts
  openCollab(note: string, handlers: CollabHandlers): CollabSession {
    // `/collab` is token-gated; a WS handshake can't carry an Authorization
    // header, so the token rides as `?token=` (same as openRecovery).
    const base = this.url.replace(/^http/, "ws") + "/collab";
    const collabUrl = this.token
      ? `${base}?token=${encodeURIComponent(this.token)}`
      : base;
    // Passive read-only replica id (we never send ops); used only for the
    // daemon's participant set / echo-skip.
    const replica = Math.floor(this.random() * 2 ** 40);
    const ws = new this.WS(collabUrl);
    let open = false;
    let closed = false;
    const send = (msg: CollabClientMsg) =>
      ws.send(
        JSON.stringify(msg, (_k, v) => (typeof v === "bigint" ? Number(v) : v)),
      );

    ws.onopen = () => {
      open = true;
      if (!closed) send({ type: "join", note, replica: replica as unknown as bigint });
    };
    ws.onmessage = (ev: { data: unknown }) => {
      if (closed) return;
      let msg: CollabServerMsg;
      try {
        const text = typeof ev.data === "string" ? ev.data : String(ev.data);
        msg = assertCollabServerMsg(JSON.parse(text));
      } catch {
        return; // presence is non-critical: drop a malformed frame silently
      }
      if (msg.note !== note) return;
      switch (msg.type) {
        case "snapshot":
          handlers.onSnapshot?.(msg.note);
          break;
        case "op":
          handlers.onForeignOp?.(msg.note, msg.op);
          break;
        case "error":
          handlers.onError?.(msg.note, msg.message);
          break;
        // joined / recoverable: not used by the presence session
      }
    };

    return {
      close() {
        if (closed) return;
        closed = true;
        if (open) {
          try {
            send({ type: "leave", note });
          } catch {
            // socket already gone — nothing to leave
          }
        }
        ws.close();
      },
    };
  }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm --dir web test daemon -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/daemon.ts web/src/client/daemon.test.ts
git commit -m "feat(collab): DaemonClient.openCollab live presence session"
```

---

### Task 5: `TauriClient.openCollab` honest stub

**Files:**
- Modify: `web/src/client/tauri.ts`
- Test: `web/src/client/tauri.test.ts`

**Interfaces:**
- Consumes: `CollabHandlers`/`CollabSession` (Task 3).
- Produces: `TauriClient.openCollab(note, handlers): CollabSession` — returns an inert session and reports the daemon-only limitation via `handlers.onError` (never throws synchronously, so callers don't crash).

- [ ] **Step 1: Write the failing test**

Add to `web/src/client/tauri.test.ts`:
```ts
it("openCollab is an honest daemon-only stub", () => {
  const client = /* existing TauriClient construction in this file */;
  let err: string | null = null;
  const session = client.openCollab("n.md", {
    onError: (_note, message) => (err = message),
  });
  expect(typeof session.close).toBe("function");
  expect(err).toMatch(/daemon/i);
  session.close(); // no throw
});
```
(Use the same `TauriClient` construction the other tests in this file use.)

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test tauri -- --run`
Expected: FAIL — `openCollab` is not a function.

- [ ] **Step 3: Implement the stub**

In `web/src/client/tauri.ts`, add `CollabHandlers`, `CollabSession` to the `./types` import and add:
```ts
  openCollab(note: string, handlers: CollabHandlers): CollabSession {
    // `/collab` is daemon-only; there is no in-process Tauri collab transport.
    // Report it via onError (non-fatal) rather than throwing.
    handlers.onError?.(note, "live collab needs the daemon");
    return { close: () => {} };
  }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm --dir web test tauri -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/tauri.ts web/src/client/tauri.test.ts
git commit -m "feat(collab): TauriClient.openCollab honest daemon-only stub"
```

---

### Task 6: `collabSlice` (presence state, session-owned)

**Files:**
- Create: `web/src/store/collabSlice.ts`
- Create: `web/src/store/collabSlice.test.ts`
- Modify: `web/src/store/store.ts` (spread the slice into the store + extend `CairnState`)

**Interfaces:**
- Consumes: `CairnClient.openCollab` + `CollabSession` (Tasks 3–4); `store.reloadNoteBuffer(path)` and `store.openNotes[path].dirty` (from #146 / existing store).
- Produces:
  ```ts
  export interface CollabPresence { note: string | null; live: boolean; pendingCount: number }
  export interface CollabState {
    collab: CollabPresence;
    collabFollow(path: string): void;
    collabStop(): void;
    collabReloadNow(): void;
  }
  export function createCollabSlice(set, get, client): CollabState;
  export const LIVE_DECAY_MS = 6000;
  export const COLLAB_RELOAD_DEBOUNCE_MS = 300;
  ```

- [ ] **Step 1: Write the failing test**

Create `web/src/store/collabSlice.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";

const wireOp = { op: "delete", id: { replica: 9, counter: 1 }, lamport: 3 } as never;

const make = () => {
  const client = new MockClient({ "n.md": "# N\n", "m.md": "# M\n" });
  const store = createCairnStore(client);
  return { client, store };
};

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("collab slice", () => {
  it("follow opens a session and sets the followed note", () => {
    const { store } = make();
    store.getState().collabFollow("n.md");
    expect(store.getState().collab.note).toBe("n.md");
    expect(store.getState().collab.live).toBe(false);
  });

  it("a foreign op marks live and decays after LIVE_DECAY_MS", () => {
    const { client, store } = make();
    store.getState().collabFollow("n.md");
    client.mockCollabHandlers!.onForeignOp!("n.md", wireOp);
    expect(store.getState().collab.live).toBe(true);
    vi.advanceTimersByTime(6000);
    expect(store.getState().collab.live).toBe(false);
  });

  it("clean buffer: a foreign op debounce-reloads and does not bump pendingCount", () => {
    const { client, store } = make();
    const reload = vi.spyOn(store.getState(), "reloadNoteBuffer");
    store.getState().collabFollow("n.md"); // buffer for n.md is clean (not opened/edited)
    client.mockCollabHandlers!.onForeignOp!("n.md", wireOp);
    expect(store.getState().collab.pendingCount).toBe(0);
    vi.advanceTimersByTime(300);
    expect(reload).toHaveBeenCalledWith("n.md");
  });

  it("dirty buffer: a foreign op bumps pendingCount and does NOT reload", async () => {
    const { client, store } = make();
    // Make n.md dirty via the store's edit path.
    await store.getState().openNote("n.md");
    store.getState().editActiveNote("# N\nedited");
    const reload = vi.spyOn(store.getState(), "reloadNoteBuffer");
    store.getState().collabFollow("n.md");
    client.mockCollabHandlers!.onForeignOp!("n.md", wireOp);
    expect(store.getState().collab.pendingCount).toBe(1);
    vi.advanceTimersByTime(1000);
    expect(reload).not.toHaveBeenCalled();
  });

  it("collabReloadNow reloads and clears pendingCount", async () => {
    const { client, store } = make();
    await store.getState().openNote("n.md");
    store.getState().editActiveNote("# N\nedited");
    store.getState().collabFollow("n.md");
    client.mockCollabHandlers!.onForeignOp!("n.md", wireOp);
    const reload = vi.spyOn(store.getState(), "reloadNoteBuffer");
    store.getState().collabReloadNow();
    expect(reload).toHaveBeenCalledWith("n.md");
    expect(store.getState().collab.pendingCount).toBe(0);
  });

  it("switching notes drops a stale session's callbacks", () => {
    const { client, store } = make();
    store.getState().collabFollow("n.md");
    const staleHandlers = client.mockCollabHandlers!;
    store.getState().collabFollow("m.md"); // supersedes; token advances
    staleHandlers.onForeignOp!("n.md", wireOp); // late callback from the old session
    expect(store.getState().collab.note).toBe("m.md");
    expect(store.getState().collab.live).toBe(false); // stale op ignored
  });

  it("collabStop resets to the default presence", () => {
    const { store } = make();
    store.getState().collabFollow("n.md");
    store.getState().collabStop();
    expect(store.getState().collab).toEqual({ note: null, live: false, pendingCount: 0 });
  });
});
```
> Note: use the store's actual open/edit action names. Verify them first with `grep -n "openNote\|editActiveNote\|setBuffer" web/src/store/store.ts`; substitute the real names if they differ (the dirty-buffer setup just needs `openNotes["n.md"].dirty === true`).

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test collabSlice -- --run`
Expected: FAIL — `collabFollow` is not a function.

- [ ] **Step 3: Implement the slice**

Create `web/src/store/collabSlice.ts`:
```ts
import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, CollabSession } from "../client/types";
import type { CairnState } from "./store";

export interface CollabPresence {
  /** The note currently followed, or null when not following. */
  note: string | null;
  /** A foreign op arrived recently; decays to false after LIVE_DECAY_MS quiet. */
  live: boolean;
  /** Foreign changes seen while the buffer was dirty (feeds the reload nudge). */
  pendingCount: number;
}

export interface CollabState {
  collab: CollabPresence;
  /** Follow `path`'s live session (idempotent if already following it). */
  collabFollow(path: string): void;
  /** Leave the current session and reset presence. */
  collabStop(): void;
  /** User accepted the nudge: reload the buffer now and clear pendingCount. */
  collabReloadNow(): void;
}

export const DEFAULT_COLLAB: CollabPresence = {
  note: null,
  live: false,
  pendingCount: 0,
};
export const LIVE_DECAY_MS = 6000;
export const COLLAB_RELOAD_DEBOUNCE_MS = 300;

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

/** Live-collab presence slice. Closure-owns the CollabSession (like askSlice
 *  owns its stream) plus a monotonic token so a superseded session's late
 *  callbacks can't race stale state in after a note switch. One-way: we receive
 *  peer ops as opaque "changed" signals and reload content from get_note. */
export function createCollabSlice(
  set: Set,
  get: Get,
  client: CairnClient,
): CollabState {
  let session: CollabSession | null = null;
  let token = 0;
  let decayTimer: ReturnType<typeof setTimeout> | null = null;
  let reloadTimer: ReturnType<typeof setTimeout> | null = null;

  const clearTimers = () => {
    if (decayTimer) clearTimeout(decayTimer);
    if (reloadTimer) clearTimeout(reloadTimer);
    decayTimer = null;
    reloadTimer = null;
  };
  const teardown = () => {
    session?.close();
    session = null;
    clearTimers();
  };

  return {
    collab: DEFAULT_COLLAB,

    collabFollow(path) {
      if (get().collab.note === path && session) return; // already following
      teardown();
      const my = ++token;
      set({ collab: { note: path, live: false, pendingCount: 0 } });
      session = client.openCollab(path, {
        onForeignOp: (note) => {
          if (my !== token || get().collab.note !== note) return;
          set((s) => ({ collab: { ...s.collab, live: true } }));
          if (decayTimer) clearTimeout(decayTimer);
          decayTimer = setTimeout(() => {
            if (my !== token) return;
            set((s) => ({ collab: { ...s.collab, live: false } }));
          }, LIVE_DECAY_MS);

          const dirty = get().openNotes[note]?.dirty ?? false;
          if (dirty) {
            set((s) => ({
              collab: { ...s.collab, pendingCount: s.collab.pendingCount + 1 },
            }));
          } else {
            if (reloadTimer) clearTimeout(reloadTimer);
            reloadTimer = setTimeout(() => {
              if (my !== token) return;
              void get().reloadNoteBuffer(note);
            }, COLLAB_RELOAD_DEBOUNCE_MS);
          }
        },
        onError: () => {
          // Presence is non-critical: stay dark, never disrupt editing.
        },
      });
    },

    collabReloadNow() {
      const note = get().collab.note;
      if (!note) return;
      void get().reloadNoteBuffer(note);
      set((s) => ({ collab: { ...s.collab, pendingCount: 0 } }));
    },

    collabStop() {
      token++;
      teardown();
      set({ collab: DEFAULT_COLLAB });
    },
  };
}
```

- [ ] **Step 4: Wire the slice into the store**

In `web/src/store/store.ts`:
- add the import near the other slice imports:
  ```ts
  import { createCollabSlice, type CollabState } from "./collabSlice";
  ```
- extend the state interface (line ~142):
  ```ts
  export interface CairnState extends PluginGrantsState, HistorySlice, AskState, CollabState {
  ```
- spread the slice next to `createAskSlice` (line ~592):
  ```ts
        ...createCollabSlice(set, get, client),
  ```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pnpm --dir web test collabSlice -- --run`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/store/collabSlice.ts web/src/store/collabSlice.test.ts web/src/store/store.ts
git commit -m "feat(collab): collabSlice (session-owned presence, token-guarded)"
```

---

### Task 7: `CollabPresencePill` + `CollabPresenceHost`

**Files:**
- Create: `web/src/components/collab/CollabPresencePill.tsx`
- Create: `web/src/components/collab/CollabPresencePill.test.tsx`
- Create: `web/src/components/collab/CollabPresenceHost.tsx`
- Create: `web/src/components/collab/CollabPresenceHost.test.tsx`

**Interfaces:**
- Consumes: `CollabPresence` (Task 6); `useCairn`, `useActions` (`web/src/app/cairnStore.ts`); `Button` (`web/src/components/ui/Button`).
- Produces:
  - `CollabPresencePill(props: { collab: CollabPresence; dirty: boolean; onReload(): void })` — pure presentational.
  - `CollabPresenceHost()` — subscribes to `activePath`, drives `collabFollow`/`collabStop`, renders the pill.

- [ ] **Step 1: Write the failing pill test**

Create `web/src/components/collab/CollabPresencePill.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CollabPresencePill } from "./CollabPresencePill";

const base = { note: "n.md", live: false, pendingCount: 0 };

describe("CollabPresencePill", () => {
  it("renders nothing when quiet", () => {
    const { container } = render(
      <CollabPresencePill collab={base} dirty={false} onReload={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the binary label when live and buffer clean", () => {
    render(
      <CollabPresencePill collab={{ ...base, live: true }} dirty={false} onReload={() => {}} />,
    );
    expect(screen.getByText("Live edits")).toBeInTheDocument();
  });

  it("shows the reload nudge when live + dirty + pending, and fires onReload", async () => {
    const onReload = vi.fn();
    render(
      <CollabPresencePill
        collab={{ ...base, live: true, pendingCount: 3 }}
        dirty={true}
        onReload={onReload}
      />,
    );
    expect(screen.getByText(/3 live changes/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /reload/i }));
    expect(onReload).toHaveBeenCalledOnce();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir web test CollabPresencePill -- --run`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the pill**

Create `web/src/components/collab/CollabPresencePill.tsx` (mirrors `LiveUpdatesBanner`'s corner styling; stacks above it at `bottom-40` so the two never overlap):
```tsx
import { Button } from "../ui/Button";
import type { CollabPresence } from "../../store/collabSlice";

/** Bottom-right presence for the live-collab session on the open note. Binary
 *  "Live edits" (no roster is derivable from the wire). When the buffer is dirty
 *  and changes are pending, a non-destructive "N live changes — Reload" nudge. */
export function CollabPresencePill(props: {
  collab: CollabPresence;
  dirty: boolean;
  onReload: () => void;
}) {
  const { collab, dirty, onReload } = props;
  if (!collab.live) return null;

  if (dirty && collab.pendingCount > 0) {
    const n = collab.pendingCount;
    return (
      <div
        role="status"
        className="fixed bottom-40 right-4 z-20 flex items-center gap-3 rounded border border-border bg-surface-2 px-3 py-2 text-sm text-text shadow-lg"
      >
        <span>
          {n} live {n === 1 ? "change" : "changes"}
        </span>
        <Button variant="ghost" onClick={onReload}>
          Reload
        </Button>
      </div>
    );
  }

  return (
    <div
      role="status"
      className="fixed bottom-40 right-4 z-20 flex items-center gap-2 rounded-full border border-border bg-surface-2 px-3 py-1.5 text-sm text-muted shadow-lg"
    >
      <span aria-hidden className="h-2 w-2 rounded-full bg-green-500" />
      <span>Live edits</span>
    </div>
  );
}
```
> If `bg-green-500` is not in the theme palette, use the accent token used elsewhere for "success/live" (grep `text-success`/`bg-accent` in `web/src`); the dot color is cosmetic.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm --dir web test CollabPresencePill -- --run`
Expected: PASS.

- [ ] **Step 5: Write the failing host test**

Create `web/src/components/collab/CollabPresenceHost.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";

const follow = vi.fn();
const stop = vi.fn();

vi.mock("../../app/cairnStore", () => ({
  useCairn: (sel: (s: unknown) => unknown) =>
    sel({
      activePath: "n.md",
      collab: { note: "n.md", live: false, pendingCount: 0 },
      openNotes: { "n.md": { dirty: false } },
    }),
  useActions: () => ({ collabFollow: follow, collabStop: stop, collabReloadNow: vi.fn() }),
}));

import { CollabPresenceHost } from "./CollabPresenceHost";

describe("CollabPresenceHost", () => {
  it("follows the active note on mount", () => {
    render(<CollabPresenceHost />);
    expect(follow).toHaveBeenCalledWith("n.md");
  });
});
```

- [ ] **Step 6: Run test to verify it fails**

Run: `pnpm --dir web test CollabPresenceHost -- --run`
Expected: FAIL — module not found.

- [ ] **Step 7: Implement the host**

Create `web/src/components/collab/CollabPresenceHost.tsx`:
```tsx
import { useEffect } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import { CollabPresencePill } from "./CollabPresencePill";

/** Follows the focused pane's active note into a live `/collab` presence session
 *  and renders the corner pill. Session lifecycle lives in collabSlice; this is
 *  the thin per-corner subscription (mirrors AskPanelHost). */
export function CollabPresenceHost() {
  const activePath = useCairn((s) => s.activePath);
  const collab = useCairn((s) => s.collab);
  const dirty = useCairn((s) =>
    s.activePath ? (s.openNotes[s.activePath]?.dirty ?? false) : false,
  );
  const actions = useActions();

  useEffect(() => {
    if (activePath) actions.collabFollow(activePath);
    else actions.collabStop();
  }, [activePath, actions]);

  // Leave the session when the app tears down.
  useEffect(() => () => actions.collabStop(), [actions]);

  return (
    <CollabPresencePill
      collab={collab}
      dirty={dirty}
      onReload={actions.collabReloadNow}
    />
  );
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `pnpm --dir web test CollabPresence -- --run`
Expected: PASS (both pill and host).

- [ ] **Step 9: Commit**

```bash
git add web/src/components/collab
git commit -m "feat(collab): CollabPresencePill + CollabPresenceHost"
```

---

### Task 8: Mount in App, advance roadmap, full gate

**Files:**
- Modify: `web/src/app/App.tsx`
- Modify: `docs/roadmap.md`

**Interfaces:**
- Consumes: `CollabPresenceHost` (Task 7).
- Produces: the presence host mounted at the app root beside `LiveUpdatesBanner`; roadmap Phase 8 advanced.

- [ ] **Step 1: Mount the host in App.tsx**

In `web/src/app/App.tsx`, add the import:
```ts
import { CollabPresenceHost } from "../components/collab/CollabPresenceHost";
```
and render it next to `LiveUpdatesBanner` (inside the fragment, after `LiveUpdatesBanner`):
```tsx
      <CollabPresenceHost />
```

- [ ] **Step 2: Verify the app builds/tests**

Run: `pnpm --dir web test -- --run` then `pnpm --dir web build`
Expected: PASS (all suites) and a clean production build.

- [ ] **Step 3: Advance the roadmap**

In `docs/roadmap.md`, update the Phase 8 row (line ~79): change the status marker from 🟡 to 🔵 (in progress) and note that the recovery slice shipped in #146 and the live-collab presence slice is on `feat/live-collab-presence`. Keep it one line; do not restructure the table.

- [ ] **Step 4: Run the full gate**

Run: `just web-ci`
Expected: PASS (includes `prettier --check` / `format:check`, eslint, tsc, vitest, contract-drift).

- [ ] **Step 5: Commit**

```bash
git add web/src/app/App.tsx docs/roadmap.md
git commit -m "feat(collab): mount CollabPresenceHost + advance Phase 8 roadmap"
```

- [ ] **Step 6: Push and open the PR**

```bash
git push
gh pr create --base main --title "feat(collab): live-collab presence (Phase 8 follow-on)" \
  --body "One-way live-awareness layer over /collab: presence pill + non-destructive get_note reload. Consumes the live half of the collab DTOs (join/leave/snapshot/op + CollabServerMsg guard). Builds on recovery #146. Spec: docs/superpowers/specs/2026-08-21-live-collab-presence-design.md"
gh pr merge --auto   # queue: "already queued to merge" is the queue, not a failure
```

---

## Self-Review

**Spec coverage:**
- One-way presence-lite + live incoming apply (B1) → Tasks 3,4,6 (openCollab + collabSlice opaque-op → reload).
- No awareness frame / binary label → Task 7 (`"Live edits"`, no count).
- Whole-note reload, clean vs dirty → Task 6 (clean→`reloadNoteBuffer`, dirty→`pendingCount` nudge).
- Auto-join active note lifecycle → Task 7 host effect.
- Daemon-only + Tauri stub → Tasks 4,5.
- Thin wire guard (S5) → Task 2.
- Non-critical errors, no hard banner → Task 4 (`onError`), Task 6 (`onError` no-op).
- Token-guarded session, no stale races → Task 6 (monotonic `token`).
- Tests at slice + wire-parse + transport seams → Tasks 2,4,6,7.
- #146 reuse (`reloadNoteBuffer`, `/collab` plumbing) + rebase ordering → Task 1.
- Roadmap advance → Task 8.

**Placeholder scan:** No TBD/TODO; every code step has real code. Two explicit "verify the real name" notes (store edit-action names in Task 6; success color token in Task 7) are guarded with the exact grep to run and a safe fallback — not open-ended placeholders.

**Type consistency:** `openCollab(note, handlers): CollabSession` and `CollabHandlers.{onSnapshot,onForeignOp,onError}` are identical across Tasks 3,4,5,6. `CollabPresence.{note,live,pendingCount}`, `collabFollow/collabStop/collabReloadNow`, `LIVE_DECAY_MS=6000`, `COLLAB_RELOAD_DEBOUNCE_MS=300` match across Tasks 6,7. `reloadNoteBuffer(path)` matches #146's signature.

**Known simplification (intentional, in scope per spec):** the dirty-buffer nudge's `pendingCount` clears only on `collabReloadNow` or note switch — if the user saves first, the nudge persists until clicked. Acceptable for the MVP; a save-clears-nudge refinement is a follow-on.
