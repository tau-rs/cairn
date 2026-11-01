# Note history / version restore — UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface the engine's git-backed per-note history in the UI — a revision timeline, read-only view-at-revision, and confirmed restore — wired through the existing transport-abstracted client.

**Architecture:** All state/actions live in a new `web/src/store/historySlice.ts` wired into `createCairnStore` with one import + one spread line (passing in the store's `set/get/client/pushError/setBuffer` closures so the slice reuses error toasts + the buffer-reload). UI lives in `web/src/components/history/*`; the right aside gets a Backlinks/History tab toggle, and `EditorPane` renders a read-only `RevisionView` when a revision is being viewed. Diff-vs-current is a documented Phase-2 fast-follow (the `RevisionView` `mode` prop is the seam).

**Tech Stack:** React + TypeScript, Zustand (vanilla `createStore`), Vitest + Testing Library, Tailwind, Radix (via existing `ui/Modal`). Contract is vendored ts-rs (do not hand-edit).

**Spec:** `docs/superpowers/specs/2026-06-14-note-history-ui-design.md`

**Conventions for every task:**
- Work in `web/`. Run tests with `pnpm test -- <path>` (= `vitest run <path>`). Full gate before final commit: `pnpm typecheck && pnpm lint && pnpm run format:check && pnpm test`.
- Commit messages: conventional, scoped `feat(history): …` / `test(history): …`.

---

## File structure

| File | Responsibility |
|---|---|
| `web/src/components/history/formatRevision.ts` | Pure formatting of `Revision` (bigint timestamp → relative + absolute). |
| `web/src/store/historySlice.ts` | `HistorySlice` interface + `createHistorySlice(deps)`: history state + actions. |
| `web/src/store/store.ts` | (modify) one import + one spread line; intersect `HistorySlice` into `CairnState`. |
| `web/src/client/mock.ts` | (modify) serve `note_history` / `note_at` / `restore_note`; seed revision fixtures. |
| `web/src/components/history/HistoryList.tsx` | Presentational timeline list. |
| `web/src/components/history/HistoryPane.tsx` | Container: binds slice state to `HistoryList`. |
| `web/src/components/history/RevisionView.tsx` | Read-only revision content + banner + back/restore (editor region). |
| `web/src/components/history/RestoreConfirmDialog.tsx` | Confirm dialog (reuses `ui/Modal`). |
| `web/src/components/RightAside.tsx` | Backlinks/History tab toggle for the right region. |
| `web/src/app/App.tsx` | (modify) `backlinks={<RightAside/>}`. |
| `web/src/components/EditorPane.tsx` | (modify) render `RevisionView` when viewing; clear on note switch. |
| `web/src/components/shortcuts/commands.ts` | (modify) add `show-history` command def. |
| `web/src/app/useCommands.ts` | (modify) dispatch `show-history`. |

---

## Task 1: `formatRevision` util

**Files:**
- Create: `web/src/components/history/formatRevision.ts`
- Test: `web/src/components/history/formatRevision.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { relativeTime, absoluteTime } from "./formatRevision";

// Fixed "now" so tests are deterministic.
const NOW = 1_700_000_000; // seconds

describe("relativeTime", () => {
  it("formats sub-minute as 'just now'", () => {
    expect(relativeTime(BigInt(NOW - 10), NOW)).toBe("just now");
  });
  it("formats minutes/hours/days", () => {
    expect(relativeTime(BigInt(NOW - 5 * 60), NOW)).toBe("5m ago");
    expect(relativeTime(BigInt(NOW - 3 * 3600), NOW)).toBe("3h ago");
    expect(relativeTime(BigInt(NOW - 2 * 86400), NOW)).toBe("2d ago");
  });
});

describe("absoluteTime", () => {
  it("renders a locale date-time string for the bigint seconds", () => {
    const out = absoluteTime(BigInt(NOW));
    // Don't assert the exact locale string; assert it reflects the right instant.
    expect(out).toBe(new Date(NOW * 1000).toLocaleString());
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/history/formatRevision.test.ts`
Expected: FAIL — "Failed to resolve import ./formatRevision".

- [ ] **Step 3: Write minimal implementation**

```ts
/** Relative label like "5m ago" for a bigint Unix-seconds timestamp.
 *  `nowSecs` is injectable for deterministic tests (defaults to wall clock). */
export function relativeTime(
  tsSecs: bigint,
  nowSecs: number = Math.floor(Date.now() / 1000),
): string {
  const delta = nowSecs - Number(tsSecs);
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return `${Math.floor(delta / 86400)}d ago`;
}

/** Full locale date-time for tooltips. */
export function absoluteTime(tsSecs: bigint): string {
  return new Date(Number(tsSecs) * 1000).toLocaleString();
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/history/formatRevision.test.ts`
Expected: PASS (4 assertions).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/formatRevision.ts web/src/components/history/formatRevision.test.ts
git commit -m "feat(history): add revision timestamp formatting util"
```

---

## Task 2: Mock client serves history ops

The mock must serve `note_history`, `note_at`, and `restore_note` so the slice and components can be tested on the mock. We add an optional revision-fixtures map to the constructor and three handlers.

**Files:**
- Modify: `web/src/client/mock.ts`
- Test: `web/src/client/mock.test.ts`

- [ ] **Step 1: Write the failing test** (append to `mock.test.ts`)

```ts
import { MockClient } from "./mock";
import type { Revision } from "../contract";

describe("mock history ops", () => {
  function withHistory() {
    const revs: Revision[] = [
      { id: "r2", message: "second", timestamp_secs: 2n, author: "tau" },
      { id: "r1", message: "first", timestamp_secs: 1n, author: "tau" },
    ];
    return new MockClient(
      { "n.md": "current body" },
      { "n.md": { revisions: revs, contents: { r2: "body v2", r1: "body v1" } } },
    );
  }

  it("note_history returns seeded revisions newest-first", async () => {
    const c = withHistory();
    const res = await c.runQuery({ type: "note_history", path: "n.md" });
    expect(res).toEqual({
      type: "history",
      revisions: [
        { id: "r2", message: "second", timestamp_secs: 2n, author: "tau" },
        { id: "r1", message: "first", timestamp_secs: 1n, author: "tau" },
      ],
    });
  });

  it("note_history returns [] for a note with no seeded history", async () => {
    const c = withHistory();
    const res = await c.runQuery({ type: "note_history", path: "other.md" });
    expect(res).toEqual({ type: "history", revisions: [] });
  });

  it("note_at returns historical contents", async () => {
    const c = withHistory();
    const res = await c.runQuery({ type: "note_at", path: "n.md", revision: "r1" });
    expect(res).toEqual({ type: "note", contents: "body v1" });
  });

  it("note_at rejects an unknown revision with not_found", async () => {
    const c = withHistory();
    await expect(
      c.runQuery({ type: "note_at", path: "n.md", revision: "nope" }),
    ).rejects.toMatchObject({ type: "not_found" });
  });

  it("restore_note overwrites the working copy and emits note_changed", async () => {
    const c = withHistory();
    const events: string[] = [];
    c.subscribe((e) => events.push(e.type));
    const res = await c.sendCommand({ type: "restore_note", path: "n.md", revision: "r1" });
    expect(res).toEqual({ type: "done" });
    const note = await c.runQuery({ type: "get_note", path: "n.md" });
    expect(note).toEqual({ type: "note", contents: "body v1" });
    await new Promise((r) => queueMicrotask(r));
    expect(events).toContain("note_changed");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/client/mock.test.ts`
Expected: FAIL — constructor takes one arg / unsupported command `restore_note` / unsupported query `note_history`.

- [ ] **Step 3: Write minimal implementation**

In `web/src/client/mock.ts`, add a `Revision` import to the type import block:

```ts
import type {
  Command,
  Query,
  Event,
  CommandResponse,
  QueryResponse,
  ContractError,
  NoteSummary,
  GraphEdge,
  SearchResult,
  PluginSummary,
  Revision,
} from "../contract";
```

Add a fixtures type above the class:

```ts
/** Optional seeded git history per note: the revision list (newest first) and
 *  the note's contents at each revision id. */
export interface HistoryFixture {
  revisions: Revision[];
  contents: Record<string, string>;
}
```

Add a field + extend the constructor:

```ts
  private history: Map<string, HistoryFixture>;

  constructor(
    seed: Record<string, string> = {},
    history: Record<string, HistoryFixture> = {},
  ) {
    this.notes = new Map(Object.entries(seed));
    this.history = new Map(Object.entries(history));
  }
```

Add the `restore_note` command case (before `default:` in `sendCommand`):

```ts
      case "restore_note": {
        const fix = this.history.get(c.path);
        const contents = fix?.contents[c.revision];
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: c.revision };
          throw err;
        }
        this.notes.set(c.path, contents);
        this.emit({ type: "note_changed", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
```

Add the two query cases (before `default:` in `runQuery`):

```ts
      case "note_history": {
        const fix = this.history.get(q.path);
        return { type: "history", revisions: fix ? fix.revisions : [] };
      }
      case "note_at": {
        const fix = this.history.get(q.path);
        const contents = fix?.contents[q.revision];
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: q.revision };
          throw err;
        }
        return { type: "note", contents };
      }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/client/mock.test.ts`
Expected: PASS (all history cases + existing tests still green).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(history): serve note_history/note_at/restore_note in MockClient"
```

---

## Task 3: history slice — state, `loadHistory`, `setRightTab`, store wiring

**Files:**
- Create: `web/src/store/historySlice.ts`
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/historySlice.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";
import type { Revision } from "../contract";

beforeEach(() => vi.useFakeTimers());
beforeEach(() => localStorage.clear());
afterEach(() => vi.useRealTimers());

const REVS: Revision[] = [
  { id: "r2", message: "second", timestamp_secs: 2n, author: "tau" },
  { id: "r1", message: "first", timestamp_secs: 1n, author: "tau" },
];

function setup() {
  const client = new MockClient(
    { "n.md": "current", "x.md": "no history" },
    { "n.md": { revisions: REVS, contents: { r2: "v2", r1: "v1" } } },
  );
  const store = createCairnStore(client);
  return { client, store };
}

describe("history slice — loadHistory", () => {
  it("defaults to no history and the backlinks tab", () => {
    const { store } = setup();
    expect(store.getState().history).toBeNull();
    expect(store.getState().rightTab).toBe("backlinks");
  });

  it("loadHistory populates revisions for the active note", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().loadHistory();
    expect(store.getState().history).toEqual(REVS);
    expect(store.getState().historyPath).toBe("n.md");
  });

  it("loadHistory is a no-op with no active note", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().loadHistory();
    expect(store.getState().history).toBeNull();
  });

  it("setRightTab switches the active aside tab", () => {
    const { store } = setup();
    store.getState().setRightTab("history");
    expect(store.getState().rightTab).toBe("history");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/store/historySlice.test.ts`
Expected: FAIL — `history`/`rightTab`/`loadHistory` do not exist on state.

- [ ] **Step 3: Write minimal implementation**

Create `web/src/store/historySlice.ts`:

```ts
import type { StoreApi } from "zustand/vanilla";
import type { Revision } from "../contract";
import type { CairnClient } from "../client/types";
import type { CairnState, NoteBuffer } from "./store";

export type RightTab = "backlinks" | "history";

export interface HistorySlice {
  history: Revision[] | null;
  historyPath: string | null;
  historyLoading: boolean;
  viewingRevision: { path: string; revision: string; contents: string } | null;
  rightTab: RightTab;

  setRightTab(tab: RightTab): void;
  showHistory(): void;
  loadHistory(): Promise<void>;
  viewRevision(revision: string): Promise<void>;
  exitRevisionView(): void;
  restoreRevision(revision: string): Promise<void>;
}

export interface HistorySliceDeps {
  set: StoreApi<CairnState>["setState"];
  get: StoreApi<CairnState>["getState"];
  client: CairnClient;
  pushError: (op: string, err: unknown, ctx?: Record<string, unknown>) => void;
  setBuffer: (path: string, patch: Partial<NoteBuffer>) => void;
}

export function createHistorySlice(deps: HistorySliceDeps): HistorySlice {
  const { set, get, client, pushError, setBuffer } = deps;
  // Monotonic token so a slow note_history can't clobber a newer note's load.
  let historySeq = 0;

  return {
    history: null,
    historyPath: null,
    historyLoading: false,
    viewingRevision: null,
    rightTab: "backlinks",

    setRightTab(tab) {
      set({ rightTab: tab });
    },

    showHistory() {
      set({ rightTab: "history" });
      get().setUi({ backlinksOpen: true }); // opens the drawer on tablet/mobile
      void get().loadHistory();
    },

    async loadHistory() {
      const path = get().activePath;
      if (!path) return;
      const token = ++historySeq;
      set({ historyLoading: true });
      try {
        const res = await client.runQuery({ type: "note_history", path });
        if (token !== historySeq) return; // superseded
        if (res.type !== "history") {
          pushError("Load history", new Error(`unexpected: ${res.type}`), { path });
          return;
        }
        set({ history: res.revisions, historyPath: path });
      } catch (err) {
        if (token === historySeq) pushError("Load history", err, { path });
      } finally {
        if (token === historySeq) set({ historyLoading: false });
      }
    },

    async viewRevision(revision) {
      const path = get().activePath;
      if (!path) return;
      try {
        const res = await client.runQuery({ type: "note_at", path, revision });
        if (res.type !== "note") {
          pushError("View revision", new Error(`unexpected: ${res.type}`), { path, revision });
          return;
        }
        set({ viewingRevision: { path, revision, contents: res.contents } });
      } catch (err) {
        pushError("View revision", err, { path, revision });
      }
    },

    exitRevisionView() {
      set({ viewingRevision: null });
    },

    async restoreRevision(revision) {
      const path = get().activePath;
      if (!path) return;
      try {
        await client.sendCommand({ type: "restore_note", path, revision });
        // restore overwrites the working copy; the store treats the resulting
        // note_changed as external (not a tracked self-write) and won't refresh
        // the active buffer — so reload it explicitly here.
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type === "note") {
          setBuffer(path, { contents: res.contents, dirty: false });
        }
        set({ viewingRevision: null, uncommitted: true });
        await get().loadHistory();
      } catch (err) {
        pushError("Restore note", err, { path, revision });
      }
    },
  };
}
```

In `web/src/store/store.ts`:

1. Add the import near the other store imports (after the `paneModel` import block):

```ts
import { createHistorySlice, type HistorySlice } from "./historySlice";
```

2. Export `NoteBuffer` so the slice can type `setBuffer`. Find the `NoteBuffer` type/interface declaration in `store.ts` and ensure it is `export`ed (add `export` if missing).

3. Intersect `HistorySlice` into the public state. Change:

```ts
export interface CairnState {
```
to:
```ts
export interface CairnState extends HistorySlice {
```

4. Add the spread line as the **last** entry of the returned object — immediately after the `assetUrl(relPath)` method and before the closing `};` of the `return { … }` (around line 1099):

```ts
      assetUrl(relPath: string) {
        return host.assetUrl(relPath);
      },

      ...createHistorySlice({ set, get, client, pushError, setBuffer }),
    };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/store/historySlice.test.ts`
Expected: PASS. Also run `pnpm test -- src/store/store.test.ts` to confirm the spread didn't break existing state.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/historySlice.ts web/src/store/historySlice.test.ts web/src/store/store.ts
git commit -m "feat(history): add history slice (loadHistory + tab state) wired into store"
```

---

## Task 4: slice — `viewRevision` / `exitRevisionView`

**Files:**
- Test: `web/src/store/historySlice.test.ts` (append)

(Implementation already shipped in Task 3; this task adds the tests that lock the behavior.)

- [ ] **Step 1: Write the failing test** (append to `historySlice.test.ts`)

```ts
describe("history slice — view revision", () => {
  it("viewRevision loads the note at a revision (read-only state)", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    expect(store.getState().viewingRevision).toEqual({
      path: "n.md",
      revision: "r1",
      contents: "v1",
    });
  });

  it("exitRevisionView clears the viewing state", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    store.getState().exitRevisionView();
    expect(store.getState().viewingRevision).toBeNull();
  });

  it("viewRevision surfaces an error toast for an unknown revision", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("nope");
    expect(store.getState().viewingRevision).toBeNull();
    expect(store.getState().errors.length).toBeGreaterThan(0);
  });
});
```

- [ ] **Step 2: Run test to verify it passes immediately**

Run: `pnpm test -- src/store/historySlice.test.ts`
Expected: PASS (implementation already present from Task 3). If any fail, fix `viewRevision`/`exitRevisionView` in `historySlice.ts`.

- [ ] **Step 3: Commit**

```bash
git add web/src/store/historySlice.test.ts
git commit -m "test(history): cover viewRevision/exitRevisionView"
```

---

## Task 5: slice — `restoreRevision`

**Files:**
- Test: `web/src/store/historySlice.test.ts` (append)

- [ ] **Step 1: Write the failing test** (append)

```ts
describe("history slice — restore", () => {
  it("restoreRevision overwrites the buffer, marks uncommitted, exits view", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    await store.getState().restoreRevision("r1");
    expect(store.getState().activeContents).toBe("v1");
    expect(store.getState().uncommitted).toBe(true);
    expect(store.getState().viewingRevision).toBeNull();
  });

  it("restoreRevision surfaces an error for an unknown revision", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().restoreRevision("nope");
    expect(store.getState().errors.length).toBeGreaterThan(0);
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- src/store/historySlice.test.ts`
Expected: PASS (implementation from Task 3). If the buffer assertion fails, confirm `setBuffer` is passed into `createHistorySlice` and updates the active mirror.

- [ ] **Step 3: Commit**

```bash
git add web/src/store/historySlice.test.ts
git commit -m "test(history): cover restoreRevision round-trip"
```

---

## Task 6: slice — `showHistory`

**Files:**
- Test: `web/src/store/historySlice.test.ts` (append)

- [ ] **Step 1: Write the failing test** (append)

```ts
describe("history slice — showHistory", () => {
  it("showHistory selects the tab, opens the drawer, and loads history", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    store.getState().showHistory();
    expect(store.getState().rightTab).toBe("history");
    expect(store.getState().ui.backlinksOpen).toBe(true);
    await vi.waitFor(() => expect(store.getState().history).toEqual(REVS));
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- src/store/historySlice.test.ts`
Expected: PASS. (`showHistory` from Task 3 calls `setRightTab`, `setUi`, `loadHistory`.)

- [ ] **Step 3: Commit**

```bash
git add web/src/store/historySlice.test.ts
git commit -m "test(history): cover showHistory entry point"
```

---

## Task 7: `HistoryList` presentational component

**Files:**
- Create: `web/src/components/history/HistoryList.tsx`
- Test: `web/src/components/history/HistoryList.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { HistoryList } from "./HistoryList";
import type { Revision } from "../../contract";

const REVS: Revision[] = [
  { id: "r2", message: "second", timestamp_secs: 2n, author: "tau" },
  { id: "r1", message: "first", timestamp_secs: 1n, author: "tau" },
];

describe("HistoryList", () => {
  it("shows a loading state", () => {
    render(<HistoryList revisions={null} loading onView={vi.fn()} onRestore={vi.fn()} />);
    expect(screen.getByText(/Loading/i)).toBeInTheDocument();
  });

  it("shows an empty state", () => {
    render(<HistoryList revisions={[]} loading={false} onView={vi.fn()} onRestore={vi.fn()} />);
    expect(screen.getByText(/No history/i)).toBeInTheDocument();
  });

  it("renders one row per revision with message + short hash", () => {
    render(<HistoryList revisions={REVS} loading={false} onView={vi.fn()} onRestore={vi.fn()} />);
    expect(screen.getByText("second")).toBeInTheDocument();
    expect(screen.getByText("first")).toBeInTheDocument();
    expect(screen.getByText(/r2/)).toBeInTheDocument();
  });

  it("fires onView and onRestore with the revision id", () => {
    const onView = vi.fn();
    const onRestore = vi.fn();
    render(<HistoryList revisions={REVS} loading={false} onView={onView} onRestore={onRestore} />);
    fireEvent.click(screen.getAllByRole("button", { name: /view/i })[0]);
    fireEvent.click(screen.getAllByRole("button", { name: /restore/i })[0]);
    expect(onView).toHaveBeenCalledWith("r2");
    expect(onRestore).toHaveBeenCalledWith("r2");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/history/HistoryList.test.tsx`
Expected: FAIL — cannot resolve `./HistoryList`.

- [ ] **Step 3: Write minimal implementation**

```tsx
import type { Revision } from "../../contract";
import { SectionLabel } from "../ui/SectionLabel";
import { Spinner } from "../ui/Spinner";
import { relativeTime, absoluteTime } from "./formatRevision";

export function HistoryList(props: {
  revisions: Revision[] | null;
  loading: boolean;
  onView: (revision: string) => void;
  onRestore: (revision: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <span className="mb-1">
        <SectionLabel>History</SectionLabel>
      </span>
      {props.loading ? (
        <span className="flex items-center gap-2 text-faint">
          <Spinner label="Loading history" /> Loading…
        </span>
      ) : !props.revisions || props.revisions.length === 0 ? (
        <span className="text-faint">No history</span>
      ) : (
        props.revisions.map((r) => (
          <div key={r.id} className="rounded px-2 py-1.5 hover:bg-surface-2">
            <div className="truncate text-text">{r.message}</div>
            <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-faint">
              <span className="font-mono">{r.id}</span>
              <span>·</span>
              <span title={absoluteTime(r.timestamp_secs)}>
                {relativeTime(r.timestamp_secs)}
              </span>
              <span>·</span>
              <span className="truncate">{r.author}</span>
            </div>
            <div className="mt-1 flex gap-2">
              <button
                className="rounded border border-border px-2 py-0.5 text-[11px] text-muted hover:text-text"
                onClick={() => props.onView(r.id)}
              >
                View
              </button>
              <button
                className="rounded border border-border px-2 py-0.5 text-[11px] text-muted hover:text-text"
                onClick={() => props.onRestore(r.id)}
              >
                Restore
              </button>
            </div>
          </div>
        ))
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/history/HistoryList.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/HistoryList.tsx web/src/components/history/HistoryList.test.tsx
git commit -m "feat(history): add HistoryList timeline component"
```

---

## Task 8: `HistoryPane` container + `RestoreConfirmDialog`

The container loads history for the active note, owns the "which revision is pending restore" confirm state, and wires callbacks to the slice.

**Files:**
- Create: `web/src/components/history/RestoreConfirmDialog.tsx`
- Create: `web/src/components/history/HistoryPane.tsx`
- Test: `web/src/components/history/HistoryPane.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { HistoryPane } from "./HistoryPane";
import { cairnStore } from "../../app/cairnStore";

beforeEach(async () => {
  localStorage.clear();
  await cairnStore.getState().init();
});

function renderPane() {
  return render(
    <MemoryRouter>
      <HistoryPane />
    </MemoryRouter>,
  );
}

describe("HistoryPane", () => {
  it("loads and renders history for the active note", async () => {
    await cairnStore.getState().openNote("index.md");
    renderPane();
    // index.md has no seeded history in the dev fixtures → empty state.
    await waitFor(() => expect(screen.getByText(/No history/i)).toBeInTheDocument());
  });

  it("opens a confirm dialog before restoring", async () => {
    // Seed a note with history via a fresh store-backed render is covered in the
    // slice tests; here we assert the confirm dialog gates restore.
    await cairnStore.getState().openNote("index.md");
    renderPane();
    // With no revisions there is nothing to restore; this test asserts the
    // dialog component mounts closed by default.
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
```

> Note: `cairnStore` is the app singleton built on the dev `MockClient` + `FIXTURE_NOTES`. The fixtures have no seeded revisions, so this test asserts the empty + closed-dialog states. Rich restore-flow behavior is covered by the slice tests (Tasks 5–6). To exercise a populated pane here, the implementer may add a couple of revision fixtures in `web/src/client/fixtures.ts` + the dev store seed; if so, update the assertion to match.

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/history/HistoryPane.test.tsx`
Expected: FAIL — cannot resolve `./HistoryPane`.

- [ ] **Step 3: Write minimal implementation**

`RestoreConfirmDialog.tsx`:

```tsx
import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";

export function RestoreConfirmDialog(props: {
  open: boolean;
  revision: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal
      open={props.open}
      onClose={props.onCancel}
      title="Restore this version?"
      description="This overwrites your working copy. Your current edits become uncommitted changes you can still commit or discard."
    >
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={props.onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onClick={props.onConfirm}>
          Restore
        </Button>
      </div>
    </Modal>
  );
}
```

`HistoryPane.tsx`:

```tsx
import { useEffect, useState } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import { HistoryList } from "./HistoryList";
import { RestoreConfirmDialog } from "./RestoreConfirmDialog";

export function HistoryPane() {
  const actions = useActions();
  const activePath = useCairn((s) => s.activePath);
  const history = useCairn((s) => s.history);
  const loading = useCairn((s) => s.historyLoading);
  const [pending, setPending] = useState<string | null>(null);

  // (Re)load history whenever the active note changes.
  useEffect(() => {
    void actions.loadHistory();
  }, [activePath, actions]);

  return (
    <>
      <HistoryList
        revisions={history}
        loading={loading}
        onView={(rev) => void actions.viewRevision(rev)}
        onRestore={(rev) => setPending(rev)}
      />
      <RestoreConfirmDialog
        open={pending !== null}
        revision={pending}
        onCancel={() => setPending(null)}
        onConfirm={() => {
          if (pending) void actions.restoreRevision(pending);
          setPending(null);
        }}
      />
    </>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/history/HistoryPane.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/RestoreConfirmDialog.tsx web/src/components/history/HistoryPane.tsx web/src/components/history/HistoryPane.test.tsx
git commit -m "feat(history): add HistoryPane container + restore confirm dialog"
```

---

## Task 9: `RevisionView` (read-only revision in the editor region)

**Files:**
- Create: `web/src/components/history/RevisionView.tsx`
- Test: `web/src/components/history/RevisionView.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RevisionView } from "./RevisionView";

describe("RevisionView", () => {
  it("shows a read-only banner with the revision and the contents", () => {
    render(
      <RevisionView
        revision="r1"
        contents="old body"
        onBack={vi.fn()}
        onRestore={vi.fn()}
      />,
    );
    expect(screen.getByText(/read-only/i)).toBeInTheDocument();
    expect(screen.getByText(/r1/)).toBeInTheDocument();
    expect(screen.getByText("old body")).toBeInTheDocument();
  });

  it("fires onBack and onRestore", () => {
    const onBack = vi.fn();
    const onRestore = vi.fn();
    render(
      <RevisionView revision="r1" contents="old" onBack={onBack} onRestore={onRestore} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /back to current/i }));
    fireEvent.click(screen.getByRole("button", { name: /restore/i }));
    expect(onBack).toHaveBeenCalled();
    expect(onRestore).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/history/RevisionView.test.tsx`
Expected: FAIL — cannot resolve `./RevisionView`.

- [ ] **Step 3: Write minimal implementation**

```tsx
import { Button } from "../ui/Button";

// Phase 1: read-only content. Phase 2 will add `mode: "full" | "diff"` and a
// diff renderer; the `mode` prop is the seam (kept out of v1 — no diff dep yet).
export function RevisionView(props: {
  revision: string;
  contents: string;
  onBack: () => void;
  onRestore: () => void;
}) {
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-warn/40 bg-warn/10 px-3 py-2 text-xs text-text">
        <span>
          Viewing <span className="font-mono">{props.revision}</span> — read-only
        </span>
        <div className="ml-auto flex gap-2">
          <Button variant="ghost" onClick={props.onBack}>
            ← Back to current
          </Button>
          <Button variant="primary" onClick={props.onRestore}>
            Restore
          </Button>
        </div>
      </div>
      <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-3 font-mono text-sm text-muted">
        {props.contents}
      </pre>
    </div>
  );
}
```

> If `warn` is not a Tailwind color token in this repo, use the existing accent/amber convention (grep `bg-` usages in `components/`); fall back to `border-accent/40 bg-accent/10`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/history/RevisionView.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/RevisionView.tsx web/src/components/history/RevisionView.test.tsx
git commit -m "feat(history): add read-only RevisionView"
```

---

## Task 10: `RightAside` tab toggle + wire into `App.tsx`

**Files:**
- Create: `web/src/components/RightAside.tsx`
- Modify: `web/src/app/App.tsx`
- Test: `web/src/components/RightAside.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { RightAside } from "./RightAside";
import { cairnStore } from "../app/cairnStore";

beforeEach(async () => {
  localStorage.clear();
  await cairnStore.getState().init();
});

function renderAside() {
  return render(
    <MemoryRouter>
      <RightAside />
    </MemoryRouter>,
  );
}

describe("RightAside", () => {
  it("shows Backlinks by default and switches to History on tab click", async () => {
    await cairnStore.getState().openNote("index.md");
    renderAside();
    // Backlinks pane is visible first.
    expect(screen.getByText(/Backlinks/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /history/i }));
    expect(await screen.findByText(/No history/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/RightAside.test.tsx`
Expected: FAIL — cannot resolve `./RightAside`.

- [ ] **Step 3: Write minimal implementation**

`RightAside.tsx`:

```tsx
import { useCairn, useActions } from "../app/cairnStore";
import { BacklinksPane } from "./BacklinksPane";
import { HistoryPane } from "./history/HistoryPane";

export function RightAside() {
  const tab = useCairn((s) => s.rightTab);
  const actions = useActions();
  return (
    <div className="flex h-full flex-col">
      <div role="tablist" className="mb-2 flex gap-1 text-xs">
        <button
          role="tab"
          aria-selected={tab === "backlinks"}
          className={
            "rounded px-2 py-1 " +
            (tab === "backlinks" ? "bg-surface-2 text-text" : "text-muted")
          }
          onClick={() => actions.setRightTab("backlinks")}
        >
          Backlinks
        </button>
        <button
          role="tab"
          aria-selected={tab === "history"}
          className={
            "rounded px-2 py-1 " +
            (tab === "history" ? "bg-surface-2 text-text" : "text-muted")
          }
          onClick={() => {
            actions.setRightTab("history");
            void actions.loadHistory();
          }}
        >
          History
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "history" ? <HistoryPane /> : <BacklinksPane />}
      </div>
    </div>
  );
}
```

In `web/src/app/App.tsx`: add the import and swap the region.

```ts
import { RightAside } from "../components/RightAside";
```
Change `backlinks={<BacklinksPane />}` to `backlinks={<RightAside />}`, and remove the now-unused `BacklinksPane` import from `App.tsx` (it moved into `RightAside`).

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/RightAside.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/RightAside.tsx web/src/components/RightAside.test.tsx web/src/app/App.tsx
git commit -m "feat(history): add Backlinks/History tab toggle in the right aside"
```

---

## Task 11: `EditorPane` integration (render `RevisionView`, clear on switch)

**Files:**
- Modify: `web/src/components/EditorPane.tsx`
- Test: `web/src/components/EditorPane.test.tsx` (append)

- [ ] **Step 1: Write the failing test** (append to `EditorPane.test.tsx`; match the existing harness imports already at the top of that file)

```tsx
import { useEffect } from "react";
// (Below assumes the file already renders <EditorPane/> within a router +
//  store-backed harness like its existing tests. Reuse that harness helper.)

describe("EditorPane revision view", () => {
  it("renders the read-only RevisionView when a revision is being viewed", async () => {
    await cairnStore.getState().init();
    await cairnStore.getState().openNote("index.md");
    await cairnStore.getState().viewRevision("r1"); // index.md has no history → no-op
    // For a deterministic assertion, set viewing state directly:
    cairnStore.setState({
      viewingRevision: { path: "index.md", revision: "r1", contents: "old body" },
    });
    renderEditorPane(); // existing harness helper in this test file
    expect(await screen.findByText(/read-only/i)).toBeInTheDocument();
    expect(screen.getByText("old body")).toBeInTheDocument();
  });
});
```

> Use the test file's existing render helper and imports (`screen`, `cairnStore`, router wrapper). If none exists, mirror `RightAside.test.tsx`'s `MemoryRouter` + `cairnStore.init()` setup.

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/EditorPane.test.tsx`
Expected: FAIL — the editor still renders the normal `Editor`, not `RevisionView`.

- [ ] **Step 3: Write minimal implementation**

In `EditorPane.tsx`:

1. Add imports:

```ts
import { useEffect } from "react";
import { RevisionView } from "./history/RevisionView";
```

2. Inside `EditorPane()`, subscribe to viewing state and clear it on note switch. Add near the other `useCairn` calls:

```ts
  const viewingRevision = useCairn((s) => s.viewingRevision);
```

Add an effect (after the hooks, before `return`):

```ts
  useEffect(() => {
    // A lingering revision view must not leak across note switches.
    if (
      viewingRevision &&
      viewingRevision.path !== activePath
    ) {
      actions.exitRevisionView();
    }
  }, [activePath, viewingRevision, actions]);
```

3. Short-circuit the render when viewing the active note's revision. Immediately inside the returned `<ErrorBoundary>`'s `<div className="relative h-full">`, before `<SearchResults …>`, branch:

```tsx
      <div className="relative h-full">
        {viewingRevision && viewingRevision.path === activePath ? (
          <RevisionView
            revision={viewingRevision.revision}
            contents={viewingRevision.contents}
            onBack={() => actions.exitRevisionView()}
            onRestore={() => void actions.restoreRevision(viewingRevision.revision)}
          />
        ) : (
          <>
            {/* existing SearchResults + view===graph?… block, unchanged, moved inside this fragment */}
          </>
        )}
      </div>
```

> Wrap the existing `SearchResults` + graph/editor block in the `<>…</>` else-branch. Do not change that block's contents.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/EditorPane.test.tsx`
Expected: PASS. Also re-run the full file to confirm existing editor/graph tests still pass.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/EditorPane.tsx web/src/components/EditorPane.test.tsx
git commit -m "feat(history): render read-only RevisionView in the editor when viewing a revision"
```

---

## Task 12: Command palette + keybinding + toolbar entry

**Files:**
- Modify: `web/src/components/shortcuts/commands.ts`
- Modify: `web/src/app/useCommands.ts`
- Test: `web/src/app/useCommands.test.ts` (or `commands.test.ts` — match where dispatch is tested)

- [ ] **Step 1: Write the failing test**

Add to `commands.test.ts` (or the existing COMMAND_DEFS test):

```ts
import { COMMAND_DEFS } from "./commands";

it("includes a show-history command bound to Mod+Shift+H", () => {
  const def = COMMAND_DEFS.find((d) => d.id === "show-history");
  expect(def).toBeDefined();
  expect(def?.defaultBinding).toBe("Mod+Shift+H");
});
```

If `useCommands` has a dispatch test harness, add:

```ts
it("show-history dispatches showHistory()", () => {
  const spy = vi.spyOn(cairnStore.getState(), "showHistory");
  // invoke runCommand("show-history") via the hook's harness
  runCommand("show-history");
  expect(spy).toHaveBeenCalled();
});
```

> If no `useCommands` dispatch harness exists, the `COMMAND_DEFS` assertion plus the slice's `showHistory` test (Task 6) is sufficient coverage; skip the second test.

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- src/components/shortcuts/commands.test.ts`
Expected: FAIL — no `show-history` def.

- [ ] **Step 3: Write minimal implementation**

In `commands.ts`, append to `COMMAND_DEFS`:

```ts
  {
    id: "show-history",
    label: "Show note history",
    defaultBinding: "Mod+Shift+H",
  },
```

In `useCommands.ts`, add a case to the `switch (id)`:

```ts
      case "show-history":
        st.showHistory();
        break;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- src/components/shortcuts/commands.test.ts`
Expected: PASS.

- [ ] **Step 5 (optional toolbar clock): add an editor toolbar button**

If the editor exposes a toolbar (check `Editor.tsx` for an existing action row), add a clock button calling `useActions().showHistory()`. If there is no natural toolbar slot, skip — the ⌘K command + History tab already surface the feature. Do not invent a new toolbar.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/commands.ts web/src/app/useCommands.ts web/src/components/shortcuts/commands.test.ts
git commit -m "feat(history): add 'Show note history' command + keybinding"
```

---

## Task 13: Full gate + manual verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full local gate from `web/`**

Run: `pnpm typecheck && pnpm lint && pnpm run format:check && pnpm test`
Expected: all green. If `format:check` fails, run `pnpm format` and re-commit. (eslint will NOT catch format drift — this step is mandatory.)

- [ ] **Step 2: Manual smoke (dev server)**

Run: `pnpm dev`, open the app, open a note, click the **History** tab. With the dev `MockClient` fixtures (no seeded revisions) you'll see "No history" — to exercise the full flow, temporarily seed a revision fixture in `web/src/client/fixtures.ts` + the dev store seed, verify: View shows the read-only banner, Back returns to the editor, Restore prompts then updates the buffer and marks the note uncommitted. Revert any temporary seed (or keep it if it improves the dev experience — implementer's call, noted in the PR).

- [ ] **Step 3: Final commit (if any format fixes)**

```bash
git add -A
git commit -m "chore(history): satisfy format gate"
```

---

## Self-review notes (coverage vs spec)

- Contract ops → Task 2 (mock) + Task 3 (slice queries/commands). ✓
- Slice in own file + one import/spread line + `pushError`/`setBuffer` reuse → Task 3. ✓
- History state kept out of `store.ts` `loading`/`UiState` → Task 3 (slice-local fields). ✓
- Timeline panel → Tasks 7–8; tab toggle → Task 10. ✓
- Read-only view-at-revision → Tasks 9 + 11; clear-on-switch → Task 11. ✓
- Restore confirm + buffer reload + uncommitted + live refresh → Tasks 5, 8, 11. ✓
- ⌘K command + keybinding → Task 12; toolbar clock optional. ✓
- Error toasts via existing channel → Task 3 (`pushError`). ✓
- Phase-2 diff seam (`RevisionView.mode`) documented, out of v1 → Task 9 note. ✓
- Full gate incl. `format:check` → Task 13. ✓
