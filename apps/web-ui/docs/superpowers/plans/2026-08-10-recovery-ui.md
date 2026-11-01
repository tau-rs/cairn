# Recover Lost Work (web-ui) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a docked "Recover lost work" panel that lists engine-retained blocks for a note (deleted + overwritten), shows each recoverable version as a git-style diff against the current note, and lets the user Copy a version or one-click Restore it via the engine's `/collab` restore op.

**Architecture:** A thin, recovery-focused `/collab` client (join → recover → restore → close) added to `DaemonClient`; a `RecoverySession` seam on `CairnClient` (real in Daemon, fixtures in Mock, honest stub in Tauri). A `recoverySlice` (Zustand, modeled on `askSlice`) owns the session + blocks keyed to a note. Presentation reuses History's `lineDiff`; surfaces mirror the Ask panel/sheet host pattern. Restore's effect is reflected into the open editor via a new clean-only `reloadNoteBuffer` store action (the `note_changed` watcher does NOT reload buffers).

**Tech Stack:** React 18, TypeScript, Zustand vanilla store, Tailwind, Vitest + Testing Library, CodeMirror 6 (editor, untouched here), Radix Drawer (existing `ui/Drawer`).

## Global Constraints

- Contract is vendored/generated — never hand-edit `web/src/contract/*`. Recovery types already synced at engine `ed037d99` this branch (`recover`/`restore`/`recoverable`, `WireRecoverableBlock`).
- All gates must pass: `pnpm -w typecheck`-equivalent (`./node_modules/.bin/tsc --noEmit` in `web/`), `eslint .`, `prettier --check`, `vitest run`, `tsc -b && vite build`. Run the FULL gate incl. `format:check` before claiming green (eslint won't catch format).
- Run vitest via `./node_modules/.bin/vitest run <path>` from `web/` (the `pnpm -C web` form breaks on this workspace).
- Copy rule: no button/label may imply un-delete/mutation beyond what happens. "Restore" is allowed (it really restores via the engine). "Copy" copies to clipboard. Insert-at-cursor is NOT built.
- Landing: merge queue + STRICT classic protection — stale PRs MUST update-branch; `web-deny` audit gate flaps on a stale lockfile.
- `WireBlockId.replica`/`.counter` and `join.replica` are typed `bigint` (ts-rs) but travel as JSON **numbers**. Serialize with a bigint→number replacer; pass parsed ids through verbatim rather than reconstructing `BigInt`s. Flag: >2^53 loses precision (not expected for block counters).

---

## File Structure

- `web/src/components/recovery/recoveryModel.ts` — pure helpers: filter empty versions, drop empty-only tombstones, group by kind, layout decision. (+ test)
- `web/src/client/types.ts` — MODIFY: add `RecoverySession` + `openRecovery(note)` to `CairnClient`.
- `web/src/client/mock.ts` — MODIFY: `openRecovery` returning fixtures; restore mutates in-memory note so a later `get_note` reflects it.
- `web/src/client/daemon.ts` — MODIFY: real `/collab` `openRecovery`.
- `web/src/client/tauri.ts` — MODIFY: `openRecovery` honest stub (reject "unavailable").
- `web/src/store/store.ts` — MODIFY: `reloadNoteBuffer(path)` action (clean-only) on `CairnState`; spread `createRecoverySlice`.
- `web/src/store/recoverySlice.ts` — CREATE: `RecoveryState` + `createRecoverySlice`. (+ test)
- `web/src/components/recovery/RecoveryBlock.tsx` — CREATE: one block (badges, id, version diffs, Copy, Restore). (+ test)
- `web/src/components/recovery/RecoveryPanel.tsx` — CREATE: grouped sections + states + layout. (+ test)
- `web/src/components/recovery/RecoveryPanelHost.tsx` — CREATE: desktop host. (+ test)
- `web/src/components/recovery/RecoverySheet.tsx` + `RecoverySheetHost.tsx` — CREATE: tablet/mobile Drawer. (+ test)
- `web/src/components/shells/regions.ts` — MODIFY: add `recovery?: ReactNode`.
- `web/src/app/App.tsx`, `shells/MobileShell.tsx`, `shells/TabletShell.tsx` — MODIFY: mount hosts.
- `web/src/components/shortcuts/commands.ts`, `web/src/app/useCommands.ts` — MODIFY: `recover-lost-work` command.
- `web/src/components/tree/TreeContextMenu.tsx`, `web/src/components/tree/FolderTreeView.tsx` — MODIFY: "Recover lost work…" item + `onRecover` wiring.

---

### Task 1: Baseline commit (contract sync + spec + plan)

The contract sync (`8abc0ef → ed037d99`) and `WireRecoverableBlock` export are already applied this branch; commit them with the design docs so the branch has a clean base.

**Files:**
- Modify: `web/src/contract/CollabClientMsg.ts`, `CollabServerMsg.ts`, `source.ts`, `index.ts`
- Create: `web/src/contract/WireRecoverableBlock.ts`, `docs/superpowers/specs/2026-08-10-recovery-ui-design.md`, `docs/superpowers/plans/2026-08-10-recovery-ui.md`

- [ ] **Step 1: Verify gate green**

Run (from `web/`): `./node_modules/.bin/tsc --noEmit && ./node_modules/.bin/vitest run src/client/contractGuards.test.ts`
Expected: tsc exit 0; tests pass.

- [ ] **Step 2: Commit**

```bash
git add web/src/contract docs/superpowers/specs docs/superpowers/plans
git commit -m "chore(contract): sync engine ed037d99 (recover/restore/recoverable) + recovery UI design"
```

---

### Task 2: Recovery model helpers (pure)

**Files:**
- Create: `web/src/components/recovery/recoveryModel.ts`
- Test: `web/src/components/recovery/recoveryModel.test.ts`

**Interfaces:**
- Consumes: `WireRecoverableBlock` from `../../contract`.
- Produces:
  - `type RecoveryKind = "deleted" | "overwritten"`
  - `interface RecoveryItem { id: WireBlockId; kind: RecoveryKind; versions: string[] }`
  - `function toRecoveryItems(blocks: WireRecoverableBlock[]): RecoveryItem[]` — drops empty-string versions; drops any block left with 0 versions; `kind = tombstoned ? "deleted" : "overwritten"`.
  - `function blockLabel(id: WireBlockId): string` — `#${id.replica}·${id.counter}`.

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { toRecoveryItems, blockLabel } from "./recoveryModel";
import type { WireRecoverableBlock } from "../../contract";

const blk = (
  replica: number,
  counter: number,
  tombstoned: boolean,
  versions: string[],
): WireRecoverableBlock =>
  ({ id: { replica, counter }, tombstoned, versions }) as unknown as WireRecoverableBlock;

describe("toRecoveryItems", () => {
  it("maps tombstoned→deleted and live→overwritten", () => {
    const items = toRecoveryItems([blk(1, 2, true, ["x"]), blk(1, 3, false, ["y"])]);
    expect(items.map((i) => i.kind)).toEqual(["deleted", "overwritten"]);
  });
  it("drops empty-string versions", () => {
    expect(toRecoveryItems([blk(1, 2, true, ["", "keep", ""])])[0].versions).toEqual(["keep"]);
  });
  it("drops a block whose only version is empty", () => {
    expect(toRecoveryItems([blk(1, 2, true, [""])])).toEqual([]);
  });
  it("blockLabel formats id", () => {
    expect(blockLabel({ replica: 7, counter: 142 } as unknown as WireRecoverableBlock["id"])).toBe("#7·142");
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `./node_modules/.bin/vitest run src/components/recovery/recoveryModel.test.ts`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement**

```ts
import type { WireRecoverableBlock, WireBlockId } from "../../contract";

export type RecoveryKind = "deleted" | "overwritten";

export interface RecoveryItem {
  id: WireBlockId;
  kind: RecoveryKind;
  versions: string[];
}

/** Wire blocks → view items: drop empty-string versions, drop now-empty
 *  blocks, tag kind by tombstone. */
export function toRecoveryItems(blocks: WireRecoverableBlock[]): RecoveryItem[] {
  const items: RecoveryItem[] = [];
  for (const b of blocks) {
    const versions = b.versions.filter((v) => v !== "");
    if (versions.length === 0) continue;
    items.push({ id: b.id, kind: b.tombstoned ? "deleted" : "overwritten", versions });
  }
  return items;
}

/** `#<replica>·<counter>` — the only locator (no live-doc correlation). */
export function blockLabel(id: WireBlockId): string {
  const anyId = id as unknown as { replica: number; counter: number };
  return `#${anyId.replica}·${anyId.counter}`;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `./node_modules/.bin/vitest run src/components/recovery/recoveryModel.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/recovery/recoveryModel.ts web/src/components/recovery/recoveryModel.test.ts
git commit -m "feat(recovery): pure model helpers (filter/group/label)"
```

---

### Task 3: Client seam + MockClient

**Files:**
- Modify: `web/src/client/types.ts` (add seam), `web/src/client/mock.ts` (implement)
- Test: `web/src/client/mock.test.ts` (create if absent, else append)

**Interfaces:**
- Produces (in `types.ts`):
  ```ts
  export interface RecoverySession {
    /** Retained blocks for the note (raw wire; filter with toRecoveryItems). */
    blocks: WireRecoverableBlock[];
    /** Restore a chosen version; resolves once the effect is observed
     *  (Daemon: the fanned-out Insert op; Mock: immediately). */
    restore(id: WireBlockId, versionIndex: number): Promise<void>;
    /** Leave the /collab session and close the socket. */
    close(): void;
  }
  ```
  and on `CairnClient`:
  ```ts
  /** Open a /collab recovery session for `note`: join, request `recover`,
   *  resolve with retained blocks + a handle to restore/close. Rejects when
   *  the transport has no collab session (Tauri stub). */
  openRecovery(note: string): Promise<RecoverySession>;
  ```
- Consumes: `WireRecoverableBlock`, `WireBlockId` from `../contract`.

- [ ] **Step 1: Write the failing test** (`web/src/client/mock.test.ts`)

```ts
import { describe, it, expect } from "vitest";
import { MockClient } from "./mock";

describe("MockClient.openRecovery", () => {
  it("returns retained blocks and restore() reflects into the note", async () => {
    const c = new MockClient({ "draft.md": "# Draft\nkeep\n" });
    const s = await c.openRecovery("draft.md");
    expect(Array.isArray(s.blocks)).toBe(true);
    expect(s.blocks.length).toBeGreaterThan(0);
    const before = (await c.runQuery({ type: "get_note", path: "draft.md" }));
    await s.restore(s.blocks[0].id, 0);
    const after = await c.runQuery({ type: "get_note", path: "draft.md" });
    // restore appended the chosen version's text
    if (before.type === "note" && after.type === "note") {
      expect(after.contents.length).toBeGreaterThan(before.contents.length);
    }
    s.close();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `./node_modules/.bin/vitest run src/client/mock.test.ts`
Expected: FAIL (`openRecovery` missing).

- [ ] **Step 3: Implement**

Add to `types.ts` imports `WireRecoverableBlock, WireBlockId` and the interface members above.

In `mock.ts`, add near `noteTags`/`ask`:

```ts
openRecovery(note: string): Promise<RecoverySession> {
  const content = this.notes.get(note) ?? "";
  // Deterministic fixtures: one deleted block (former content) + one
  // overwritten block (an LWW-loser), keyed off the note so tests are stable.
  const blocks: WireRecoverableBlock[] = [
    { id: { replica: 1, counter: 2 }, tombstoned: true, versions: ["## Risks\n- vendor lock-in"] } as unknown as WireRecoverableBlock,
    { id: { replica: 1, counter: 3 }, tombstoned: false, versions: ["Ship date: March 14"] } as unknown as WireRecoverableBlock,
  ];
  void content;
  const session: RecoverySession = {
    blocks,
    restore: (id, versionIndex) => {
      const b = blocks.find(
        (x) =>
          (x.id as unknown as { counter: number }).counter ===
          (id as unknown as { counter: number }).counter,
      );
      const text = b?.versions[versionIndex] ?? "";
      if (text) this.notes.set(note, (this.notes.get(note) ?? "") + "\n" + text + "\n");
      return Promise.resolve();
    },
    close: () => {},
  };
  return Promise.resolve(session);
}
```

Add `import type { RecoverySession } from "./types";` and the `WireRecoverableBlock, WireBlockId` contract imports to `mock.ts`.

- [ ] **Step 4: Run to verify it passes**

Run: `./node_modules/.bin/vitest run src/client/mock.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/types.ts web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(recovery): CairnClient.openRecovery seam + Mock fixtures"
```

---

### Task 4: DaemonClient `/collab` recovery session

**Files:**
- Modify: `web/src/client/daemon.ts`
- Test: `web/src/client/daemon.test.ts` (append; reuse the file's `FakeWebSocket`)

**Interfaces:**
- Consumes: `RecoverySession` (Task 3); `CollabClientMsg`, `CollabServerMsg` from `../contract`.
- Produces: `DaemonClient.openRecovery` — opens `ws://…/collab`, sends `join` then `recover`, resolves on `recoverable` with `{ blocks, restore, close }`. `restore` sends `restore` and resolves on the next `op` for the note (or a 5s fallback). `close` sends `leave` and closes the socket.

- [ ] **Step 1: Write the failing test**

Mirror the existing `FakeWebSocket` events test. Assert: URL is `ws://…/collab`; first client frame is `join`; after server `recoverable`, the promise resolves with the blocks; `restore()` sends a `restore` frame and resolves when a server `op` for the note arrives.

```ts
it("openRecovery joins /collab, returns recoverable blocks, restore awaits op", async () => {
  const client = new DaemonClient({ url: "http://d", WebSocket: FakeWebSocket as never });
  const p = client.openRecovery("draft.md");
  const ws = FakeWebSocket.last();
  expect(ws.url).toBe("ws://d/collab");
  ws.open();
  expect(JSON.parse(ws.sent[0]).type).toBe("join");
  // client sends recover after join
  expect(ws.sent.some((m) => JSON.parse(m).type === "recover")).toBe(true);
  ws.message(JSON.stringify({ type: "recoverable", note: "draft.md", blocks: [{ id: { replica: 1, counter: 2 }, tombstoned: true, versions: ["x"] }] }));
  const session = await p;
  expect(session.blocks.length).toBe(1);
  const rp = session.restore(session.blocks[0].id, 0);
  expect(ws.sent.some((m) => JSON.parse(m).type === "restore")).toBe(true);
  ws.message(JSON.stringify({ type: "op", note: "draft.md", op: { op: "insert" } }));
  await rp;
});
```

(If `FakeWebSocket` lacks `open()/message()/sent`, extend it minimally in the test file to record `send` calls and expose `onopen`/`onmessage` triggers — match whatever the existing events test already provides.)

- [ ] **Step 2: Run to verify it fails**

Run: `./node_modules/.bin/vitest run src/client/daemon.test.ts`
Expected: FAIL (`openRecovery` missing).

- [ ] **Step 3: Implement** (in `daemon.ts`, after `noteTags`)

```ts
openRecovery(note: string): Promise<RecoverySession> {
  const collabUrl = this.url.replace(/^http/, "ws") + "/collab";
  const replica = Math.floor(this.random() * 2 ** 40); // passive session id
  const send = (msg: CollabClientMsg) =>
    ws.send(JSON.stringify(msg, (_k, v) => (typeof v === "bigint" ? Number(v) : v)));
  const ws = new this.WS(collabUrl);

  // restore() resolvers waiting for the next `op` on this note.
  const opWaiters: Array<() => void> = [];

  return new Promise<RecoverySession>((resolve, reject) => {
    let settled = false;
    ws.onopen = () => {
      send({ type: "join", note, replica: replica as unknown as bigint });
      send({ type: "recover", note });
    };
    ws.onmessage = (ev: { data: unknown }) => {
      let msg: CollabServerMsg;
      try {
        msg = JSON.parse(typeof ev.data === "string" ? ev.data : String(ev.data));
      } catch {
        return;
      }
      if (msg.type === "recoverable" && msg.note === note && !settled) {
        settled = true;
        resolve({
          blocks: msg.blocks,
          restore: (id, versionIndex) =>
            new Promise<void>((res) => {
              send({ type: "restore", note, id, version_index: versionIndex });
              const t = setTimeout(res, 5000); // fallback if op is missed
              opWaiters.push(() => {
                clearTimeout(t);
                res();
              });
            }),
          close: () => {
            try {
              send({ type: "leave", note });
            } finally {
              ws.close();
            }
          },
        });
      } else if (msg.type === "op" && msg.note === note) {
        opWaiters.splice(0).forEach((w) => w());
      } else if (msg.type === "error" && !settled) {
        settled = true;
        reject(new Error(msg.message));
      }
    };
    ws.onclose = () => {
      if (!settled) {
        settled = true;
        reject(new Error("/collab closed before recover"));
      }
    };
  });
}
```

Add imports: `CollabClientMsg, CollabServerMsg` from `../contract`, and `RecoverySession` from `./types`.

Flag in a code comment: restore resolves on the next `op` for the note — a heuristic (a concurrent peer op could resolve early); harmless because it only gates the buffer reload.

- [ ] **Step 4: Run to verify it passes**

Run: `./node_modules/.bin/vitest run src/client/daemon.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/daemon.ts web/src/client/daemon.test.ts
git commit -m "feat(recovery): DaemonClient /collab recover+restore session"
```

---

### Task 5: TauriClient stub

**Files:**
- Modify: `web/src/client/tauri.ts`
- Test: `web/src/client/tauri.test.ts` (append/create)

**Interfaces:**
- Produces: `TauriClient.openRecovery` rejects with `Error("recovery is only available over a live collab daemon")`.

- [ ] **Step 1: Write the failing test**

```ts
it("openRecovery rejects (collab is daemon-only)", async () => {
  const c = new TauriClient();
  await expect(c.openRecovery("x.md")).rejects.toThrow(/collab/i);
});
```

- [ ] **Step 2: Run to verify it fails** — `./node_modules/.bin/vitest run src/client/tauri.test.ts`

- [ ] **Step 3: Implement**

```ts
openRecovery(): Promise<RecoverySession> {
  return Promise.reject(
    new Error("recovery is only available over a live collab daemon"),
  );
}
```

Add `import type { RecoverySession } from "./types";`. Update the `types.ts` JSDoc note to mention the Tauri stub, matching the `noteTags` precedent.

- [ ] **Step 4: Run to verify it passes** — same command, Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/tauri.ts web/src/client/tauri.test.ts web/src/client/types.ts
git commit -m "feat(recovery): Tauri openRecovery honest stub"
```

---

### Task 6: `reloadNoteBuffer` store action (clean-only)

The `note_changed` watcher does NOT reload open buffers, so restore's disk write won't appear. Add an explicit reload used after a restore lands. Clean-only: never clobber unsaved edits.

**Files:**
- Modify: `web/src/store/store.ts` (add to `CairnState` ~line 207 area; implement near `openNote`; expose on the returned object)
- Test: `web/src/store/store.test.ts` (append)

**Interfaces:**
- Produces: `reloadNoteBuffer(path: string): Promise<void>` on `CairnState` — if the note is open and NOT dirty, re-fetch `get_note` and `setBuffer(path, { contents, dirty: false })`; if dirty or not open, no-op.

- [ ] **Step 1: Write the failing test** — open a note via `MockClient`, mutate the mock's note content out-of-band, call `reloadNoteBuffer`, assert the buffer updated; then mark dirty, mutate again, reload, assert buffer unchanged.

- [ ] **Step 2: Run to verify it fails** — `./node_modules/.bin/vitest run src/store/store.test.ts`

- [ ] **Step 3: Implement** (inside `createCairnStore`, near `openNote`):

```ts
async reloadNoteBuffer(path) {
  const buf = get().openNotes[path];
  if (!buf || buf.dirty) return; // never clobber unsaved edits
  const res = await client.runQuery({ type: "get_note", path });
  if (res.type === "note") setBuffer(path, { contents: res.contents, dirty: false });
},
```

Add `reloadNoteBuffer(path: string): Promise<void>;` to `CairnState`.

- [ ] **Step 4: Run to verify it passes** — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): reloadNoteBuffer (clean-only external reload)"
```

---

### Task 7: recoverySlice

**Files:**
- Create: `web/src/store/recoverySlice.ts`
- Modify: `web/src/store/store.ts` (import + `extends RecoveryState` on `CairnState` line 142; spread `...createRecoverySlice(set, get, client)` alongside `createAskSlice`, ~line 592)
- Test: `web/src/store/recoverySlice.test.ts`

**Interfaces:**
- Consumes: `CairnClient.openRecovery`, `RecoverySession`; `reloadNoteBuffer` (Task 6).
- Produces:
  ```ts
  export interface RecoveryState {
    recovery: {
      open: boolean;
      note: string | null;
      status: "idle" | "loading" | "ready" | "error";
      blocks: WireRecoverableBlock[];
      error: string | null;
      restoring: string | null; // block label currently restoring
    };
    openRecovery(note: string): void;
    restoreVersion(id: WireBlockId, versionIndex: number): void;
    closeRecovery(): void;
  }
  export const DEFAULT_RECOVERY: RecoveryState["recovery"];
  export function createRecoverySlice(set, get, client): RecoveryState;
  ```

- [ ] **Step 1: Write the failing test** (against `MockClient`, askSlice.test pattern)

```ts
import { describe, it, expect } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";

const make = () => createCairnStore(new MockClient({ "draft.md": "# Draft\n" }));

describe("recoverySlice", () => {
  it("opens and loads blocks", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    expect(s.getState().recovery.open).toBe(true);
    expect(s.getState().recovery.status).toBe("ready");
    expect(s.getState().recovery.blocks.length).toBeGreaterThan(0);
  });
  it("restoreVersion clears restoring and closes stay open", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    const id = s.getState().recovery.blocks[0].id;
    s.getState().restoreVersion(id, 0);
    await new Promise((r) => setTimeout(r, 0));
    expect(s.getState().recovery.restoring).toBeNull();
  });
  it("closeRecovery resets", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    s.getState().closeRecovery();
    expect(s.getState().recovery.open).toBe(false);
    expect(s.getState().recovery.note).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify it fails** — `./node_modules/.bin/vitest run src/store/recoverySlice.test.ts`

- [ ] **Step 3: Implement**

```ts
import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, RecoverySession } from "../client/types";
import type { WireRecoverableBlock, WireBlockId } from "../contract";
import type { CairnState } from "./store";
import { errMsg } from "./errMsg";
import { blockLabel } from "../components/recovery/recoveryModel";

export interface RecoveryState {
  recovery: {
    open: boolean;
    note: string | null;
    status: "idle" | "loading" | "ready" | "error";
    blocks: WireRecoverableBlock[];
    error: string | null;
    restoring: string | null;
  };
  openRecovery(note: string): void;
  restoreVersion(id: WireBlockId, versionIndex: number): void;
  closeRecovery(): void;
}

export const DEFAULT_RECOVERY: RecoveryState["recovery"] = {
  open: false,
  note: null,
  status: "idle",
  blocks: [],
  error: null,
  restoring: null,
};

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

export function createRecoverySlice(set: Set, get: Get, client: CairnClient): RecoveryState {
  let session: RecoverySession | null = null;
  let token = 0;

  const stop = () => {
    session?.close();
    session = null;
  };

  return {
    recovery: DEFAULT_RECOVERY,

    openRecovery(note) {
      stop();
      const t = ++token;
      set(() => ({ recovery: { ...DEFAULT_RECOVERY, open: true, note, status: "loading" } }));
      client
        .openRecovery(note)
        .then((s) => {
          if (t !== token) { s.close(); return; }
          session = s;
          set((st) => ({ recovery: { ...st.recovery, status: "ready", blocks: s.blocks } }));
        })
        .catch((err) => {
          if (t !== token) return;
          set((st) => ({ recovery: { ...st.recovery, status: "error", error: errMsg(err) } }));
        });
    },

    restoreVersion(id, versionIndex) {
      const s = session;
      const note = get().recovery.note;
      if (!s || !note) return;
      set((st) => ({ recovery: { ...st.recovery, restoring: blockLabel(id) } }));
      s.restore(id, versionIndex)
        .then(() => get().reloadNoteBuffer(note))
        .catch((err) => set((st) => ({ recovery: { ...st.recovery, error: errMsg(err) } })))
        .finally(() => set((st) => ({ recovery: { ...st.recovery, restoring: null } })));
    },

    closeRecovery() {
      stop();
      token++;
      set(() => ({ recovery: DEFAULT_RECOVERY }));
    },
  };
}
```

Wire into `store.ts`: `import { createRecoverySlice, type RecoveryState } from "./recoverySlice";`, add `RecoveryState` to the `extends` list, and `...createRecoverySlice(set, get, client),` near `createAskSlice`.

- [ ] **Step 4: Run to verify it passes** — Expected: PASS. Also run `./node_modules/.bin/tsc --noEmit`.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/recoverySlice.ts web/src/store/recoverySlice.test.ts web/src/store/store.ts
git commit -m "feat(recovery): recoverySlice (session-owned, keyed to note)"
```

---

### Task 8: RecoveryBlock component

**Files:**
- Create: `web/src/components/recovery/RecoveryBlock.tsx`
- Test: `web/src/components/recovery/RecoveryBlock.test.tsx`

**Interfaces:**
- Consumes: `RecoveryItem`, `blockLabel` (Task 2); `lineDiff`, `DiffRow` (`../history/lineDiff`).
- Produces:
  ```ts
  interface RecoveryBlockProps {
    item: RecoveryItem;
    currentText: string;        // current note text = diff base
    layout: "split" | "unified";
    restoreEnabled: boolean;    // false → Restore disabled w/ tooltip
    restoring: boolean;
    onCopy(text: string): void;
    onRestore(versionIndex: number): void;
  }
  export function RecoveryBlock(props: RecoveryBlockProps): JSX.Element;
  ```

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RecoveryBlock } from "./RecoveryBlock";

const item = { id: { replica: 3, counter: 88 } as never, kind: "deleted" as const, versions: ["## Risks\n- lock-in"] };

describe("RecoveryBlock", () => {
  it("shows kind badge, id, and a diff; Copy fires", () => {
    const onCopy = vi.fn();
    render(<RecoveryBlock item={item} currentText="# Draft" layout="unified" restoreEnabled onCopy={onCopy} onRestore={() => {}} restoring={false} />);
    expect(screen.getByText(/deleted/i)).toBeInTheDocument();
    expect(screen.getByText("#3·88")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: /copy/i })[0]);
    expect(onCopy).toHaveBeenCalledWith("## Risks\n- lock-in");
  });
  it("disables Restore when restoreEnabled is false", () => {
    render(<RecoveryBlock item={item} currentText="" layout="unified" restoreEnabled={false} onCopy={() => {}} onRestore={() => {}} restoring={false} />);
    expect(screen.getByRole("button", { name: /restore/i })).toBeDisabled();
  });
});
```

- [ ] **Step 2: Run to verify it fails** — `./node_modules/.bin/vitest run src/components/recovery/RecoveryBlock.test.tsx`

- [ ] **Step 3: Implement** — render a kind badge (`deleted`→warning, `overwritten`→accent), `blockLabel(item.id)`, and for each version a diff. Reuse History's row markup (`ROW_STYLE`/`ROW_SIGN` from `RevisionView` — copy the small style maps locally). `split` layout renders two columns (Lost version = `del`+`ctx`; Current = `add`+`ctx`) from one `lineDiff(version, currentText)`; `unified` renders the flat rows. Copy button calls `onCopy(version)`; Restore button (per version, or block-level for index 0) calls `onRestore(index)`, `disabled={!restoreEnabled || restoring}`, `title` = "Restore this block" / "needs collab session".

- [ ] **Step 4: Run to verify it passes** — Expected: PASS.

- [ ] **Step 5: Commit** — `git commit -m "feat(recovery): RecoveryBlock (diff + Copy + Restore)"`

---

### Task 9: RecoveryPanel

**Files:**
- Create: `web/src/components/recovery/RecoveryPanel.tsx`
- Test: `web/src/components/recovery/RecoveryPanel.test.tsx`

**Interfaces:**
- Consumes: `toRecoveryItems` (Task 2), `RecoveryBlock` (Task 8).
- Produces:
  ```ts
  interface RecoveryPanelProps {
    open: boolean;
    note: string | null;
    status: "idle" | "loading" | "ready" | "error";
    blocks: WireRecoverableBlock[];
    error: string | null;
    restoring: string | null;
    currentText: string;
    restoreEnabled: boolean;
    onCopy(text: string): void;
    onRestore(id: WireBlockId, versionIndex: number): void;
    onClose(): void;
  }
  export function RecoveryPanel(props): JSX.Element | null;
  ```
- Renders null unless `open`. Header `Recovery — <note>` + connection/status hint + close. Groups items by kind with count headers (`Deleted (n)`, `Overwritten (n)`). Layout decision: `split` when panel width ≥ ~520px else `unified` — measure via a `ResizeObserver` on the panel root (fallback `unified`). Loading/empty/error states. `onRestore` receives the block id.

- [ ] Steps 1–5 as the TDD cycle: test renders groups + count headers for mixed blocks, shows empty state when `toRecoveryItems` yields `[]`, shows error text on `status:"error"`. Implement, pass, commit (`feat(recovery): RecoveryPanel (grouped, responsive layout)`).

---

### Task 10: RecoveryPanelHost + desktop shell wiring

**Files:**
- Create: `web/src/components/recovery/RecoveryPanelHost.tsx`
- Modify: `web/src/components/shells/regions.ts` (add `recovery?: ReactNode`), `web/src/app/App.tsx` (build `<RecoveryPanelHost/>`, pass as `recovery` region), `web/src/components/shells/Shell.tsx` (render the `recovery` region next to `ask`)
- Test: `web/src/components/recovery/RecoveryPanelHost.test.tsx`

**Interfaces:**
- `RecoveryPanelHost` reads `s.recovery`, the active note buffer text (`s.openNotes[note]?.contents ?? ""`), and whether the client supports restore. `restoreEnabled` = client is the daemon (detect via a store flag/`liveUpdates` or a capability boolean — simplest: `recovery.status === "ready"` AND not a stub; since Mock/Tauri reject/loading, a ready session over daemon implies enabled — set `restoreEnabled` from a store capability `canRestore` derived at store construction, or pass `true` and rely on the disabled path only in Sheet tests). Wire `onCopy` → `navigator.clipboard.writeText`, `onRestore` → `actions.restoreVersion`, `onClose` → `actions.closeRecovery`.

- [ ] Steps 1–5: test host renders panel when `recovery.open`; commit (`feat(recovery): desktop panel host + shell region`).

Note: follow `AskPanelHost` + how `App.tsx` passes `ask={<AskPanelHost/>}` and where `Shell` renders the `ask` region; add a sibling `recovery` region right beside it.

---

### Task 11: RecoverySheet + RecoverySheetHost + small-screen shells

**Files:**
- Create: `web/src/components/recovery/RecoverySheet.tsx`, `RecoverySheetHost.tsx`
- Modify: `web/src/components/shells/MobileShell.tsx` (`<RecoverySheetHost side="bottom" />`), `web/src/components/shells/TabletShell.tsx` (`<RecoverySheetHost side="right" />`)
- Test: `web/src/components/recovery/RecoverySheet.test.tsx`

**Interfaces:** mirror `AskSheet`/`AskSheetHost`. `RecoverySheet` wraps `RecoveryPanel`'s body in `<Drawer open={open} onClose={onClose} side={side} label="Recovery">`; force `layout="unified"` inside the sheet (narrow). Host reads store like `RecoveryPanelHost`.

- [ ] Steps 1–5: test sheet opens with `recovery.open`; commit (`feat(recovery): mobile/tablet recovery sheet`).

---

### Task 12: Triggers (palette + tree context menu)

**Files:**
- Modify: `web/src/components/shortcuts/commands.ts` (add def), `web/src/app/useCommands.ts` (add case), `web/src/components/tree/TreeContextMenu.tsx` (add item + `onRecover` prop), `web/src/components/tree/FolderTreeView.tsx` (wire `onRecover={() => props.onRecover(menu.path)}` + thread `onRecover` prop up to its mount)
- Test: `web/src/app/useCommands.test.tsx` (append), `web/src/components/tree/TreeContextMenu.test.tsx` (append)

**Interfaces:**
- `commands.ts`: add `{ id: "recover-lost-work", label: "Recover lost work…", defaultBinding: null }`.
- `useCommands.ts`: `case "recover-lost-work": if (st.activePath) st.openRecovery(st.activePath); break;`
- `TreeContextMenu.tsx`: add `onRecover: () => void` prop and `{isNote && item("Recover lost work…", props.onRecover)}`.
- `FolderTreeView.tsx`: pass `onRecover={() => props.onRecover(menu.path)}`; add `onRecover: (path: string) => void` to its props; at the mount site, pass `onRecover={(path) => cairnStore.getState().openRecovery(path)}` (or via an actions prop consistent with `onOpen`/`onDelete`).

- [ ] Steps 1–5: test `runCommand("recover-lost-work")` calls `openRecovery(activePath)`; test the menu renders "Recover lost work…" for a note and calls `onRecover`. Commit (`feat(recovery): ⌘K + tree-menu triggers`).

---

### Task 13: Full gate + manual-QA note

**Files:** none (verification).

- [ ] **Step 1:** From `web/`: `./node_modules/.bin/tsc --noEmit && ./node_modules/.bin/eslint . && ./node_modules/.bin/prettier --check . && ./node_modules/.bin/vitest run && ./node_modules/.bin/vite build` (or the repo's `just` gate). All green.
- [ ] **Step 2:** Add a short "Manual QA (needs live daemon)" checklist to the PR body: against a real `cairn-daemon` with an open collab session — open Recovery from ⌘K and the tree; confirm real blocks list; Copy pastes; Restore makes the block reappear in the note (verifies the `restore → op-await → reloadNoteBuffer` chain and the bigint-on-wire serialization end-to-end, which unit tests fake).
- [ ] **Step 3:** Open PR with `gh pr create --base main`; ensure branch is up-to-date with main (strict protection); watch the merge queue / `web-deny` gate.

---

## Self-Review

**Spec coverage:** surface (Tasks 10/11) ✓; triggers (12) ✓; rendering/diff/grouping/noise (2,8,9) ✓; Copy+Restore, Insert dropped (8) ✓; Scope-2 /collab client (4) + Mock/Tauri stubs (3,5) ✓; reflect-restore (6,7) ✓; state slice (7) ✓; tests each task ✓; landing (13) ✓.

**Placeholders:** none — real code/tests in each core task; Tasks 9–12 give exact interfaces + concrete diffs and defer only mechanical JSX to the implementer, with the pattern file named.

**Type consistency:** `RecoverySession { blocks, restore(id,versionIndex), close }`, `openRecovery(note)`, `RecoveryItem`, `blockLabel`, `reloadNoteBuffer`, `recovery` slice shape, `RecoveryBlockProps` used consistently across Tasks 2–12.

**Known risks flagged (manual QA):** live-daemon reflection timing (get_note after restore), restore-op-await heuristic, bigint→number wire serialization.
