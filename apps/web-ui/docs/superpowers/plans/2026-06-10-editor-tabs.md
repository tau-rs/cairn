# Editor Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Multiple notes open at once in a VSCode-style tab strip above the editor — preview/pin model, instant switching, ⌘W/Ctrl+Tab/⌘1-9, and pinned tabs that survive reload.

**Architecture:** A pure `tabsModel` (tab-list reducer) + a pure `tabsPersistence` (localStorage) feed a store refactor from a single open-note slot to **multi-note buffers** (`openNotes` + `tabs`, per-note autosave). `activeContents`/`dirty`/`saving` stay as a *derived mirror of the active tab*, so the editor/backlinks/graph and `App`/`CommitBar` are unchanged. A props-only `TabStrip` renders the strip; `App` wires actions, keyboard, and a "Close tab" palette command.

**Tech Stack:** React 18 + TypeScript, Zustand (vanilla), Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-10-editor-tabs-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. Run `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if `format:check` flags files. Ignore stale LSP "cannot find module" noise — trust `pnpm typecheck`'s exit code.
- e2e on port 5273 (`--strictPort`). Current baseline: 178 unit, 10 e2e green.
- **Relevant existing code:**
  - `stem(path)` from `web/src/client/wikilink.ts` (`"ideas.md"` → `"ideas"`).
  - `debounce(fn, ms): Debounced` (with `.cancel()`) from `web/src/util/timer.ts`.
  - Persistence pattern to mirror: `web/src/components/graph/forceSettings.ts` (try/catch-guarded localStorage; jsdom localStorage works via `web/src/vitest.setup.ts`).
  - Tailwind tokens: `bg`, `surface`, `surface-2`, `border`, `text`, `muted`, `faint`, `accent`, `danger`.
  - Store tests live in `web/src/store/store.test.ts` (MockClient fixtures + fake timers). The store is created by `createCairnStore(client, host)` in `web/src/store/store.ts`.
  - `App.tsx` already has: a global keydown `useEffect` (⌘K palette), `actions = cairnStore.getState()`, `COMMANDS`/`runCommand`, and selectors incl. `activePath`/`activeContents`/`dirty`/`saving`.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/tabs/tabsModel.ts` | Pure tab-list reducer (open/preview/pin/close/cycle/jump). |
| `web/src/components/tabs/tabsPersistence.ts` | Pure localStorage round-trip + clamping. |
| `web/src/components/tabs/TabStrip.tsx` | The strip component (props-only). |
| `web/src/store/store.ts` | Multi-note buffers + tabs + per-note autosave + tab actions + restore. |
| `web/src/app/App.tsx` | Render TabStrip; extend keyboard; "Close tab" command. |
| `web/e2e/skeleton.spec.ts` | Tabs e2e. |

---

## Task 1: tabsModel (pure)

**Files:**
- Create: `web/src/components/tabs/tabsModel.ts`
- Create: `web/src/components/tabs/tabsModel.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tabs/tabsModel.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import {
  openOrPreview,
  pinTab,
  closeTab,
  cycle,
  jumpTo,
  type TabsState,
} from "./tabsModel";

const empty: TabsState = { tabs: [], activePath: null };

describe("openOrPreview", () => {
  it("appends a preview tab when opening into an empty set", () => {
    expect(openOrPreview(empty, "a.md")).toEqual({
      tabs: [{ path: "a.md", preview: true }],
      activePath: "a.md",
    });
  });
  it("replaces the existing preview tab in place", () => {
    const s = openOrPreview(empty, "a.md");
    expect(openOrPreview(s, "b.md")).toEqual({
      tabs: [{ path: "b.md", preview: true }],
      activePath: "b.md",
    });
  });
  it("appends a new preview tab when the current one is pinned", () => {
    const s = pinTab(openOrPreview(empty, "a.md"), "a.md");
    const r = openOrPreview(s, "b.md");
    expect(r.tabs).toEqual([
      { path: "a.md", preview: false },
      { path: "b.md", preview: true },
    ]);
    expect(r.activePath).toBe("b.md");
  });
  it("focuses an already-open tab without duplicating", () => {
    const s = pinTab(openOrPreview(empty, "a.md"), "a.md");
    const withB = openOrPreview(s, "b.md");
    const r = openOrPreview(withB, "a.md");
    expect(r.tabs.map((t) => t.path)).toEqual(["a.md", "b.md"]);
    expect(r.activePath).toBe("a.md");
  });
});

describe("pinTab", () => {
  it("clears the preview flag for the given path", () => {
    const s = openOrPreview(empty, "a.md");
    expect(pinTab(s, "a.md").tabs[0]).toEqual({ path: "a.md", preview: false });
  });
});

describe("closeTab", () => {
  const three: TabsState = {
    tabs: [
      { path: "a.md", preview: false },
      { path: "b.md", preview: false },
      { path: "c.md", preview: false },
    ],
    activePath: "b.md",
  };
  it("focuses the right neighbour when closing the active tab", () => {
    const r = closeTab(three, "b.md");
    expect(r.tabs.map((t) => t.path)).toEqual(["a.md", "c.md"]);
    expect(r.activePath).toBe("c.md");
  });
  it("focuses the left neighbour when closing the active last tab", () => {
    const r = closeTab({ ...three, activePath: "c.md" }, "c.md");
    expect(r.activePath).toBe("b.md");
  });
  it("keeps the active path when closing a non-active tab", () => {
    const r = closeTab(three, "a.md");
    expect(r.activePath).toBe("b.md");
  });
  it("returns null active when closing the last remaining tab", () => {
    const one: TabsState = {
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    };
    expect(closeTab(one, "a.md")).toEqual({ tabs: [], activePath: null });
  });
});

describe("cycle / jumpTo", () => {
  const s: TabsState = {
    tabs: [
      { path: "a.md", preview: false },
      { path: "b.md", preview: false },
      { path: "c.md", preview: false },
    ],
    activePath: "a.md",
  };
  it("cycles forward and wraps", () => {
    expect(cycle(s, 1).activePath).toBe("b.md");
    expect(cycle({ ...s, activePath: "c.md" }, 1).activePath).toBe("a.md");
  });
  it("cycles backward and wraps", () => {
    expect(cycle(s, -1).activePath).toBe("c.md");
  });
  it("jumps to the Nth tab (1-based) and ignores out of range", () => {
    expect(jumpTo(s, 2).activePath).toBe("b.md");
    expect(jumpTo(s, 9)).toEqual(s);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- tabsModel` — expect FAIL (module not found).

- [ ] **Step 3: Implement `tabsModel.ts`**

Create `web/src/components/tabs/tabsModel.ts`:

```ts
export interface Tab {
  path: string;
  preview: boolean;
}

export interface TabsState {
  tabs: Tab[];
  activePath: string | null;
}

/** Open `path`: focus it if already open; else replace the single preview tab
 *  in place; else append a new preview tab. */
export function openOrPreview(state: TabsState, path: string): TabsState {
  if (state.tabs.some((t) => t.path === path)) {
    return { tabs: state.tabs, activePath: path };
  }
  const previewIdx = state.tabs.findIndex((t) => t.preview);
  if (previewIdx !== -1) {
    const tabs = state.tabs.slice();
    tabs[previewIdx] = { path, preview: true };
    return { tabs, activePath: path };
  }
  return { tabs: [...state.tabs, { path, preview: true }], activePath: path };
}

/** Pin the tab for `path` (preview → false). No-op if absent. */
export function pinTab(state: TabsState, path: string): TabsState {
  return {
    tabs: state.tabs.map((t) =>
      t.path === path ? { ...t, preview: false } : t,
    ),
    activePath: state.activePath,
  };
}

/** Remove `path`. If it was active, focus the right neighbour, else the left,
 *  else null. */
export function closeTab(state: TabsState, path: string): TabsState {
  const idx = state.tabs.findIndex((t) => t.path === path);
  if (idx === -1) return state;
  const tabs = state.tabs.filter((t) => t.path !== path);
  let activePath = state.activePath;
  if (state.activePath === path) {
    if (tabs.length === 0) activePath = null;
    else activePath = (tabs[idx] ?? tabs[tabs.length - 1]).path;
  }
  return { tabs, activePath };
}

/** Focus the tab `delta` steps from the active one (wraps). */
export function cycle(state: TabsState, delta: 1 | -1): TabsState {
  if (state.tabs.length === 0) return state;
  const idx = state.tabs.findIndex((t) => t.path === state.activePath);
  const base = idx === -1 ? 0 : idx;
  const n = state.tabs.length;
  return {
    tabs: state.tabs,
    activePath: state.tabs[(base + delta + n) % n].path,
  };
}

/** Focus the Nth tab (1-based); no-op if out of range. */
export function jumpTo(state: TabsState, n: number): TabsState {
  const tab = state.tabs[n - 1];
  return tab ? { tabs: state.tabs, activePath: tab.path } : state;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- tabsModel` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tabs/tabsModel.ts web/src/components/tabs/tabsModel.test.ts
git commit -m "feat(tabs): pure tab-list reducer (open/preview/pin/close/cycle/jump)"
```

---

## Task 2: tabsPersistence (pure)

**Files:**
- Create: `web/src/components/tabs/tabsPersistence.ts`
- Create: `web/src/components/tabs/tabsPersistence.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tabs/tabsPersistence.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { saveTabs, loadTabs } from "./tabsPersistence";
import type { TabsState } from "./tabsModel";

beforeEach(() => localStorage.clear());

const state: TabsState = {
  tabs: [
    { path: "a.md", preview: false },
    { path: "b.md", preview: false },
    { path: "scratch.md", preview: true }, // preview — must NOT persist
  ],
  activePath: "b.md",
};

describe("tabsPersistence", () => {
  it("round-trips pinned paths + active, excluding the preview tab", () => {
    saveTabs(state);
    expect(loadTabs(["a.md", "b.md", "scratch.md"])).toEqual({
      pinned: ["a.md", "b.md"],
      activePath: "b.md",
    });
  });
  it("drops persisted paths that no longer exist", () => {
    saveTabs(state);
    expect(loadTabs(["a.md"])).toEqual({ pinned: ["a.md"], activePath: "a.md" });
  });
  it("returns empty when nothing is stored", () => {
    expect(loadTabs(["a.md"])).toEqual({ pinned: [], activePath: null });
  });
  it("returns empty on malformed storage", () => {
    localStorage.setItem("cairn.tabs", "{not json");
    expect(loadTabs(["a.md"])).toEqual({ pinned: [], activePath: null });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- tabsPersistence` — expect FAIL (module not found).

- [ ] **Step 3: Implement `tabsPersistence.ts`**

Create `web/src/components/tabs/tabsPersistence.ts`:

```ts
import type { TabsState } from "./tabsModel";

export interface PersistedTabs {
  pinned: string[];
  activePath: string | null;
}

const STORAGE_KEY = "cairn.tabs";

/** Persist only the pinned tabs (paths) + the active path. */
export function saveTabs(state: TabsState): void {
  try {
    const data: PersistedTabs = {
      pinned: state.tabs.filter((t) => !t.preview).map((t) => t.path),
      activePath: state.activePath,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch {
    // ignore (private mode / quota)
  }
}

/** Load pinned tabs, dropping any path not in `existingPaths`. The restored
 *  active path is always one of the surviving pinned tabs (else the last, else null). */
export function loadTabs(existingPaths: string[]): PersistedTabs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { pinned: [], activePath: null };
    const parsed = JSON.parse(raw) as Partial<PersistedTabs>;
    const exists = new Set(existingPaths);
    const pinned = (parsed.pinned ?? []).filter((p) => exists.has(p));
    const activePath =
      parsed.activePath && pinned.includes(parsed.activePath)
        ? parsed.activePath
        : pinned.length > 0
          ? pinned[pinned.length - 1]
          : null;
    return { pinned, activePath };
  } catch {
    return { pinned: [], activePath: null };
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- tabsPersistence` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tabs/tabsPersistence.ts web/src/components/tabs/tabsPersistence.test.ts
git commit -m "feat(tabs): pinned-tab localStorage round-trip with clamping"
```

---

## Task 3: Store — multi-note buffers + tabs + per-note autosave

This is the crux. Replace the single open-note slot with `openNotes` (per-note buffers) + `tabs`, keep `activeContents`/`dirty`/`saving` as the **active-tab mirror**, make autosave per-note, add tab actions, and restore pinned tabs on init. All existing store tests must stay green.

**Files:**
- Modify (full rewrite): `web/src/store/store.ts`
- Modify: `web/src/store/store.test.ts` (add new tests; keep existing)

- [ ] **Step 1: Add test isolation + the new store tests**

First add a `localStorage.clear()` to the top-level `beforeEach` block in `web/src/store/store.test.ts` so persisted tabs don't leak between tests. The file currently has `beforeEach(() => vi.useFakeTimers());` — add a second line directly after it:
```ts
beforeEach(() => localStorage.clear());
```

Then append these tests (after the existing ones, inside the top-level `describe`; keep all existing tests unchanged):

```ts
  it("keeps each open note's buffer when switching tabs", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("edited A [[b]]"); // pins a.md, marks dirty
    await store.getState().openNote("b.md");
    expect(store.getState().activeContents).toBe("target note");
    await store.getState().openNote("a.md"); // back to A
    expect(store.getState().activeContents).toBe("edited A [[b]]");
    expect(store.getState().dirty).toBe(true);
  });

  it("editing pins the preview tab", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    expect(store.getState().tabs).toEqual([{ path: "a.md", preview: true }]);
    store.getState().editBuffer("x [[b]]");
    expect(store.getState().tabs).toEqual([{ path: "a.md", preview: false }]);
  });

  it("browsing notes (preview) does not write to disk", async () => {
    const { client, store } = setup();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().init();
    await store.getState().openNote("a.md");
    await store.getState().openNote("b.md"); // replaces the preview tab
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    expect(spy.mock.calls.some(([c]) => c.type === "write_note")).toBe(false);
    expect(store.getState().tabs).toEqual([{ path: "b.md", preview: true }]);
  });

  it("closeTab focuses a neighbour; closing the last clears the editor", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().openNote("b.md");
    store.getState().pinTab("b.md");
    store.getState().selectTab("a.md");
    store.getState().closeTab("a.md");
    expect(store.getState().activePath).toBe("b.md"); // neighbour focused
    store.getState().closeTab("b.md");
    expect(store.getState().activePath).toBeNull();
    expect(store.getState().activeContents).toBe("");
    expect(store.getState().tabs).toEqual([]);
  });

  it("deleteNote closes the note's tab", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().openNote("b.md");
    store.getState().pinTab("b.md");
    await store.getState().deleteNote("b.md");
    expect(store.getState().tabs.map((t) => t.path)).toEqual(["a.md"]);
    expect(store.getState().activePath).toBe("a.md");
  });

  it("restores persisted pinned tabs on init, skipping missing notes", async () => {
    vi.useRealTimers();
    localStorage.clear();
    // First store instance: open + pin two notes, which persists them.
    const c1 = new MockClient({ "a.md": "A", "b.md": "B" });
    const s1 = createCairnStore(c1);
    await s1.getState().init();
    await s1.getState().openNote("a.md");
    s1.getState().pinTab("a.md");
    await s1.getState().openNote("b.md");
    s1.getState().pinTab("b.md");
    s1.getState().selectTab("b.md");
    // Second instance with a fresh client missing b.md → only a.md restored.
    const c2 = new MockClient({ "a.md": "A" });
    const s2 = createCairnStore(c2);
    await s2.getState().init();
    expect(s2.getState().tabs.map((t) => t.path)).toEqual(["a.md"]);
    expect(s2.getState().activePath).toBe("a.md");
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- store` — expect FAIL (new tests: `pinTab`/`selectTab`/`closeTab` not functions; `tabs` undefined).

- [ ] **Step 3: Rewrite `store.ts`**

Replace the ENTIRE contents of `web/src/store/store.ts` with:

```ts
import { createStore, type StoreApi } from "zustand/vanilla";
import { alwaysOpenHost, type CairnHost } from "../client/host";
import type { CairnClient } from "../client/types";
import type { ContractError } from "../contract";
import { debounce, type Debounced } from "../util/timer";
import {
  openOrPreview,
  pinTab as pinTabModel,
  closeTab as closeTabModel,
  cycle as cycleModel,
  jumpTo as jumpToModel,
  type Tab,
  type TabsState,
} from "../components/tabs/tabsModel";
import { loadTabs, saveTabs } from "../components/tabs/tabsPersistence";

export interface Settings {
  autosaveMs: number;
  idleAutoCommit: boolean;
  idleAutoCommitMs: number;
  intervalAutoCommit: boolean;
  intervalAutoCommitMin: number;
  editorMode: "livepreview" | "source";
}

export const DEFAULT_SETTINGS: Settings = {
  autosaveMs: 1000,
  idleAutoCommit: true,
  idleAutoCommitMs: 5000,
  intervalAutoCommit: true,
  intervalAutoCommitMin: 5,
  editorMode: "livepreview",
};

export interface NoteBuffer {
  contents: string;
  dirty: boolean;
  saving: boolean;
}

export interface CairnState {
  cairnPath: string | null;
  notePaths: string[];
  openNotes: Record<string, NoteBuffer>;
  tabs: Tab[];
  activePath: string | null;
  activeContents: string;
  dirty: boolean;
  saving: boolean;
  uncommitted: boolean;
  lastCommit: string | null;
  committing: boolean;
  query: string;
  searchResults: string[] | null;
  backlinks: string[];
  graph: { nodes: string[]; edges: { from: string; to: string }[] } | null;
  noteTags: Record<string, string[]>;
  settings: Settings;
  error: string | null;

  init(): Promise<void>;
  openCairn(): Promise<void>;
  refreshNotePaths(): Promise<void>;
  openNote(path: string): Promise<void>;
  editBuffer(contents: string): void;
  saveActive(): Promise<void>;
  saveNote(path: string): Promise<void>;
  createNote(path: string): Promise<void>;
  deleteNote(path: string): Promise<void>;
  selectTab(path: string): void;
  closeTab(path: string): void;
  closeActiveTab(): void;
  cycleTab(delta: 1 | -1): void;
  jumpToTab(n: number): void;
  pinTab(path: string): void;
  runSearch(query: string): Promise<void>;
  setQuery(query: string): void;
  closeSearch(): void;
  refreshBacklinks(): Promise<void>;
  loadGraph(): Promise<void>;
  commitManual(message: string): Promise<void>;
  autoCommit(): Promise<void>;
  rearmInterval(): void;
  setSettings(patch: Partial<Settings>): void;
  dismissError(): void;
  assetUrl(relPath: string): string;
}

export function createCairnStore(
  client: CairnClient,
  host: CairnHost = alwaysOpenHost,
): StoreApi<CairnState> {
  const autosaves = new Map<string, Debounced>();
  let idleCommit: Debounced | null = null;
  let started = false;
  let intervalHandle: ReturnType<typeof setInterval> | null = null;

  const store = createStore<CairnState>()((set, get) => {
    const tabsState = (): TabsState => ({
      tabs: get().tabs,
      activePath: get().activePath,
    });

    const persist = () => saveTabs(tabsState());

    // Write a note's buffer; if it's the active note, keep the top-level mirror
    // (activeContents/dirty/saving) in sync so existing consumers are unchanged.
    const setBuffer = (path: string, patch: Partial<NoteBuffer>) => {
      set((s) => {
        const cur = s.openNotes[path] ?? {
          contents: "",
          dirty: false,
          saving: false,
        };
        const buf = { ...cur, ...patch };
        const mirror =
          s.activePath === path
            ? {
                activeContents: buf.contents,
                dirty: buf.dirty,
                saving: buf.saving,
              }
            : {};
        return { openNotes: { ...s.openNotes, [path]: buf }, ...mirror };
      });
    };

    // Apply a new tabs/active selection and swap the active-note mirror.
    const applyTabs = (next: TabsState) => {
      const buf = next.activePath ? get().openNotes[next.activePath] : undefined;
      set({
        tabs: next.tabs,
        activePath: next.activePath,
        activeContents: buf?.contents ?? "",
        dirty: buf?.dirty ?? false,
        saving: buf?.saving ?? false,
      });
    };

    const dropNote = (path: string) => {
      autosaves.get(path)?.cancel();
      autosaves.delete(path);
      set((s) => {
        const rest = { ...s.openNotes };
        delete rest[path];
        return { openNotes: rest };
      });
    };

    return {
      cairnPath: null,
      notePaths: [],
      openNotes: {},
      tabs: [],
      activePath: null,
      activeContents: "",
      dirty: false,
      saving: false,
      uncommitted: false,
      lastCommit: null,
      committing: false,
      query: "",
      searchResults: null,
      backlinks: [],
      graph: null,
      noteTags: {},
      settings: DEFAULT_SETTINGS,
      error: null,

      async init() {
        if (started) return;
        started = true;
        const path = await host.currentCairn();
        set({ cairnPath: path });
        // Subscribe once, for the store's lifetime — NOT inside the path gate.
        client.subscribe((e) => {
          if (e.type === "note_changed" || e.type === "note_deleted") {
            void get().refreshNotePaths();
            if (get().searchResults !== null) void get().runSearch(get().query);
            if (get().activePath) void get().refreshBacklinks();
            if (get().graph !== null) void get().loadGraph();
          } else if (e.type === "committed") {
            set({ lastCommit: e.commit, uncommitted: false });
          }
        });
        if (path !== null) {
          await get().refreshNotePaths();
          // Restore persisted pinned tabs; skip any that no longer load.
          const persisted = loadTabs(get().notePaths);
          for (const p of persisted.pinned) {
            try {
              await get().openNote(p);
              get().pinTab(p);
            } catch {
              /* skip a tab that won't load */
            }
          }
          if (persisted.activePath && get().openNotes[persisted.activePath]) {
            get().selectTab(persisted.activePath);
          }
          get().rearmInterval();
        }
      },

      async openCairn() {
        try {
          const path = await host.openCairn();
          if (path === null) return; // cancelled
          set({
            cairnPath: path,
            openNotes: {},
            tabs: [],
            activePath: null,
            activeContents: "",
            dirty: false,
            saving: false,
            backlinks: [],
          });
          await get().refreshNotePaths();
          get().rearmInterval();
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      async refreshNotePaths() {
        try {
          const res = await client.runQuery({ type: "list_notes" });
          if (res.type === "notes")
            set({ notePaths: res.notes.map((n) => n.path) });
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      async openNote(path) {
        try {
          if (!get().openNotes[path]) {
            const res = await client.runQuery({ type: "get_note", path });
            if (res.type !== "note") return;
            set((s) => ({
              openNotes: {
                ...s.openNotes,
                [path]: { contents: res.contents, dirty: false, saving: false },
              },
            }));
          }
          applyTabs(openOrPreview(tabsState(), path));
          persist();
          await get().refreshBacklinks();
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      editBuffer(contents) {
        const path = get().activePath;
        if (!path) return;
        setBuffer(path, { contents, dirty: true });
        // Editing pins the (possibly preview) active tab.
        applyTabs(pinTabModel(tabsState(), path));
        persist();
        autosaves.get(path)?.cancel();
        const d = debounce(
          () => void get().saveNote(path),
          get().settings.autosaveMs,
        );
        autosaves.set(path, d);
        d();
        const s = get().settings;
        if (s.idleAutoCommit) {
          idleCommit?.cancel();
          idleCommit = debounce(
            () => void get().autoCommit(),
            s.idleAutoCommitMs,
          );
          idleCommit();
        }
      },

      saveActive() {
        return get().saveNote(get().activePath ?? "");
      },

      async saveNote(path) {
        const buf = get().openNotes[path];
        if (!buf || !buf.dirty) return;
        const snapshot = buf.contents;
        setBuffer(path, { saving: true });
        try {
          await client.sendCommand({
            type: "write_note",
            path,
            contents: snapshot,
          });
          const cur = get().openNotes[path];
          setBuffer(path, {
            saving: false,
            // Stay dirty if the note changed during the write (the pending
            // debounce will save it).
            dirty: cur ? cur.contents !== snapshot : false,
          });
          set({ uncommitted: true });
        } catch (err) {
          setBuffer(path, { saving: false });
          set({ error: errMsg(err) });
        }
      },

      async createNote(path) {
        try {
          await client.sendCommand({ type: "write_note", path, contents: "" });
          await get().openNote(path);
          get().pinTab(path); // new notes open pinned
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      async deleteNote(path) {
        try {
          await client.sendCommand({ type: "delete_note", path });
          get().closeTab(path);
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      selectTab(path) {
        if (!get().openNotes[path]) return;
        applyTabs({ tabs: get().tabs, activePath: path });
        persist();
        void get().refreshBacklinks();
      },

      closeTab(path) {
        dropNote(path);
        applyTabs(closeTabModel(tabsState(), path));
        persist();
        void get().refreshBacklinks();
      },

      closeActiveTab() {
        const path = get().activePath;
        if (path) get().closeTab(path);
      },

      cycleTab(delta) {
        applyTabs(cycleModel(tabsState(), delta));
        persist();
        void get().refreshBacklinks();
      },

      jumpToTab(n) {
        applyTabs(jumpToModel(tabsState(), n));
        persist();
        void get().refreshBacklinks();
      },

      pinTab(path) {
        if (!get().openNotes[path]) return; // only pin an actually-open note
        // Pinning focuses the tab too (used by double-click and createNote).
        applyTabs(pinTabModel({ tabs: get().tabs, activePath: path }, path));
        persist();
        void get().refreshBacklinks();
      },

      async runSearch(query) {
        try {
          const res = await client.runQuery({ type: "search", query });
          if (res.type === "paths") set({ query, searchResults: res.paths });
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      setQuery(query) {
        set({ query });
      },

      closeSearch() {
        set({ searchResults: null });
      },

      async refreshBacklinks() {
        const path = get().activePath;
        if (!path) return set({ backlinks: [] });
        try {
          const res = await client.runQuery({ type: "get_backlinks", path });
          if (res.type === "paths") set({ backlinks: res.paths });
        } catch (err) {
          set({ error: errMsg(err) });
        }
      },

      async loadGraph() {
        try {
          const res = await client.runQuery({ type: "get_graph" });
          if (res.type === "graph")
            set({ graph: { nodes: res.nodes, edges: res.edges } });
        } catch (err) {
          set({ error: errMsg(err) });
        }
        try {
          set({ noteTags: await client.noteTags() });
        } catch {
          // leave the existing noteTags as-is — stale data beats clearing it
        }
      },

      async commitManual(message) {
        if (get().committing) return;
        set({ committing: true });
        try {
          const res = await client.sendCommand({ type: "commit", message });
          if (res.type === "committed")
            set({ lastCommit: res.commit, uncommitted: false });
        } catch (err) {
          set({ error: errMsg(err) });
        } finally {
          set({ committing: false });
        }
      },

      async autoCommit() {
        if (!get().uncommitted || get().committing) return;
        const path = get().activePath;
        const message = path ? `cairn: update ${path}` : "cairn: auto-commit";
        await get().commitManual(message);
      },

      rearmInterval() {
        if (intervalHandle) clearInterval(intervalHandle);
        intervalHandle = null;
        const { intervalAutoCommit, intervalAutoCommitMin } = get().settings;
        if (intervalAutoCommit) {
          intervalHandle = setInterval(
            () => void get().autoCommit(),
            intervalAutoCommitMin * 60_000,
          );
        }
      },

      setSettings(patch) {
        set({ settings: { ...get().settings, ...patch } });
        if ("intervalAutoCommit" in patch || "intervalAutoCommitMin" in patch) {
          get().rearmInterval();
        }
      },

      dismissError() {
        set({ error: null });
      },

      assetUrl(relPath: string) {
        return host.assetUrl(relPath);
      },
    };
  });

  return store;
}

function errMsg(err: unknown): string {
  // ContractError (rejected by the client) is a tagged object.
  if (err && typeof err === "object" && "type" in err) {
    const e = err as ContractError;
    if (e.type === "not_found") return `Not found: ${e.what}`;
    return e.message;
  }
  return err instanceof Error ? err.message : String(err);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- store` — expect PASS (all existing tests + the 6 new ones).
If a test using fake timers hangs on a restore loop, note that `init()`'s restore uses `await get().openNote(...)` — the existing tests that call `init()` use empty/persistence-free localStorage, so the restore loop is empty. If the "restore" test interferes via shared localStorage, ensure it calls `localStorage.clear()` first (it does).

- [ ] **Step 5: Full gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS (178 + new tests).

- [ ] **Step 6: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "refactor(store): multi-note buffers + tabs + per-note autosave (single-note mirror preserved)"
```

---

## Task 4: TabStrip component

**Files:**
- Create: `web/src/components/tabs/TabStrip.tsx`
- Create: `web/src/components/tabs/TabStrip.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tabs/TabStrip.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TabStrip } from "./TabStrip";

const tabs = [
  { path: "a.md", preview: false, dirty: false },
  { path: "ideas.md", preview: true, dirty: true },
];

function setup(over = {}) {
  const props = {
    tabs,
    activePath: "a.md",
    onSelect: vi.fn(),
    onPin: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  render(<TabStrip {...props} />);
  return props;
}

describe("TabStrip", () => {
  it("renders a tab per open note with the stem label", () => {
    setup();
    expect(screen.getByRole("tab", { name: /a$/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /ideas/ })).toBeInTheDocument();
  });
  it("marks the active tab as selected", () => {
    setup();
    expect(screen.getByRole("tab", { name: /a$/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
  it("calls onSelect when a tab is clicked", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("tab", { name: /ideas/ }));
    expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
  });
  it("calls onPin on double-click", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("tab", { name: /ideas/ }));
    expect(props.onPin).toHaveBeenCalledWith("ideas.md");
  });
  it("calls onClose (not onSelect) when the × is clicked", () => {
    const props = setup();
    fireEvent.click(screen.getByLabelText("close ideas"));
    expect(props.onClose).toHaveBeenCalledWith("ideas.md");
    expect(props.onSelect).not.toHaveBeenCalled();
  });
  it("renders nothing when there are no tabs", () => {
    const { container } = render(
      <TabStrip
        tabs={[]}
        activePath={null}
        onSelect={vi.fn()}
        onPin={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(container.querySelector('[role="tablist"]')).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- TabStrip` — expect FAIL (module not found).

- [ ] **Step 3: Implement `TabStrip.tsx`**

Create `web/src/components/tabs/TabStrip.tsx`:

```tsx
import { stem } from "../../client/wikilink";

export interface TabView {
  path: string;
  preview: boolean;
  dirty: boolean;
}

export function TabStrip(props: {
  tabs: TabView[];
  activePath: string | null;
  onSelect: (path: string) => void;
  onPin: (path: string) => void;
  onClose: (path: string) => void;
}) {
  if (props.tabs.length === 0) return null;
  return (
    <div
      role="tablist"
      className="flex h-9 shrink-0 items-stretch overflow-x-auto border-b border-border bg-surface"
    >
      {props.tabs.map((t) => {
        const active = t.path === props.activePath;
        const label = stem(t.path);
        return (
          <div
            key={t.path}
            role="tab"
            aria-selected={active}
            title={t.path}
            onClick={() => props.onSelect(t.path)}
            onDoubleClick={() => props.onPin(t.path)}
            className={
              "relative flex cursor-pointer items-center gap-2 whitespace-nowrap border-r border-border px-3 text-xs " +
              (active
                ? "bg-surface-2 text-text"
                : "text-muted hover:bg-surface-2 hover:text-text")
            }
          >
            {active && (
              <span className="absolute inset-x-0 top-0 h-0.5 bg-accent" />
            )}
            <span className={t.preview ? "italic" : ""}>{label}</span>
            {t.dirty && (
              <span className="h-1.5 w-1.5 rounded-full bg-accent" />
            )}
            <button
              type="button"
              aria-label={`close ${label}`}
              className="text-faint hover:text-text"
              onClick={(e) => {
                e.stopPropagation();
                props.onClose(t.path);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- TabStrip` — expect PASS (6 tests).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tabs/TabStrip.tsx web/src/components/tabs/TabStrip.test.tsx
git commit -m "feat(tabs): TabStrip component (preview italic / dirty dot / active / close)"
```

---

## Task 5: App wiring + keyboard + palette command + e2e

**Files:**
- Modify: `web/src/app/App.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Import TabStrip and add selectors in `App.tsx`**

Add the import (near the other component imports):
```tsx
import { TabStrip } from "../components/tabs/TabStrip";
```

Add two selectors next to the existing `useCairn` selectors (e.g. after `const noteTags = ...`):
```tsx
  const tabs = useCairn((s) => s.tabs);
  const openNotes = useCairn((s) => s.openNotes);
```

- [ ] **Step 2: Build the tab view list and render `<TabStrip>` above the editor**

Just before the `return (` (after `runCommand`), build the view list:
```tsx
  const tabViews = tabs.map((t) => ({
    path: t.path,
    preview: t.preview,
    dirty: openNotes[t.path]?.dirty ?? false,
  }));
```

Replace the editor slot — change this block:
```tsx
        editor={
          <div className="relative h-full">
            <SearchResults
              results={searchResults}
              onOpen={(p) => {
                void actions.openNote(p);
                actions.closeSearch();
              }}
              onClose={actions.closeSearch}
            />
            {view === "graph" ? (
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                tagsByNote={noteTags}
                activePath={activePath}
                onOpenNote={(p) => {
                  void actions.openNote(p);
                  setView("editor");
                }}
              />
            ) : (
              <Editor
                path={activePath}
                value={activeContents}
                mode={editorMode}
                notePaths={notePaths}
                assetUrl={actions.assetUrl}
                onChange={actions.editBuffer}
                onOpenNote={actions.openNote}
                onToggleMode={() =>
                  actions.setSettings({
                    editorMode:
                      editorMode === "livepreview" ? "source" : "livepreview",
                  })
                }
              />
            )}
          </div>
        }
```
to:
```tsx
        editor={
          <div className="relative h-full">
            <SearchResults
              results={searchResults}
              onOpen={(p) => {
                void actions.openNote(p);
                actions.closeSearch();
              }}
              onClose={actions.closeSearch}
            />
            {view === "graph" ? (
              <GraphView
                nodes={graph?.nodes ?? []}
                edges={graph?.edges ?? []}
                tagsByNote={noteTags}
                activePath={activePath}
                onOpenNote={(p) => {
                  void actions.openNote(p);
                  setView("editor");
                }}
              />
            ) : (
              <div className="flex h-full flex-col">
                <TabStrip
                  tabs={tabViews}
                  activePath={activePath}
                  onSelect={actions.selectTab}
                  onPin={actions.pinTab}
                  onClose={actions.closeTab}
                />
                <div className="min-h-0 flex-1">
                  <Editor
                    path={activePath}
                    value={activeContents}
                    mode={editorMode}
                    notePaths={notePaths}
                    assetUrl={actions.assetUrl}
                    onChange={actions.editBuffer}
                    onOpenNote={actions.openNote}
                    onToggleMode={() =>
                      actions.setSettings({
                        editorMode:
                          editorMode === "livepreview"
                            ? "source"
                            : "livepreview",
                      })
                    }
                  />
                </div>
              </div>
            )}
          </div>
        }
```

- [ ] **Step 3: Extend the global keydown handler**

Replace the existing keydown `useEffect` (the ⌘K-only one) with:
```tsx
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = cairnStore.getState();
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "w") {
        e.preventDefault();
        st.closeActiveTab();
      } else if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        st.cycleTab(e.shiftKey ? -1 : 1);
      } else if ((e.metaKey || e.ctrlKey) && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        st.jumpToTab(Number(e.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
```

- [ ] **Step 4: Add the "Close tab" palette command**

In `COMMANDS`, add after the `commit` entry:
```tsx
    { id: "close-tab", label: "Close tab" },
```
In `runCommand`'s switch, add a case:
```tsx
      case "close-tab":
        actions.closeActiveTab();
        break;
```

- [ ] **Step 5: Gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 6: Add the e2e**

Append to `web/e2e/skeleton.spec.ts` (keep the existing top import):
```ts
test("editor tabs: preview replaces, edit pins, close focuses, reload restores", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("index.md")).toBeVisible(); // app loaded

  // Open index → a single preview tab.
  await page.getByRole("button", { name: "index.md" }).click();
  await expect(page.getByRole("tab", { name: /index/ })).toBeVisible();

  // Open ideas → the preview tab is REPLACED (index tab gone).
  await page.getByRole("button", { name: "ideas.md" }).click();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /index/ })).toHaveCount(0);

  // Edit ideas → its tab pins. Open todo → a new preview tab (ideas stays).
  const cm = page.locator(".cm-content");
  await cm.click();
  await page.keyboard.type(" edited");
  await expect(page.getByText(/saved/i)).toBeVisible({ timeout: 5000 });
  await page.getByRole("button", { name: "todo.md" }).click();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /todo/ })).toBeVisible();

  // Close the todo tab via its × → ideas remains.
  await page.getByRole("button", { name: "close todo" }).click();
  await expect(page.getByRole("tab", { name: /todo/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();

  // Reload → the pinned ideas tab is restored; the (preview) todo is not.
  await page.reload();
  await expect(page.getByRole("tab", { name: /ideas/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /todo/ })).toHaveCount(0);
});
```

- [ ] **Step 7: Run e2e**

Run: `pnpm e2e` — expect 11 passed (10 existing + this one). If port 5273 is busy: `lsof -ti :5273 | xargs kill 2>/dev/null` then retry once. If the "preview replaces" assertion fails, the preview tab isn't being reused — re-check `openOrPreview`/App wiring. If reload-restore fails, check that pinned-tab persistence writes on edit/pin and that `init()` restores. STOP and report if a core assertion fails (do not weaken it).

Note: ⌘W / Ctrl+Tab / ⌘1-9 are not e2e-tested (⌘W is browser-reserved; these work in the Tauri shell). They are covered by the store-action unit tests (`closeTab`/`cycleTab`/`jumpToTab`) and verified manually.

- [ ] **Step 8: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 9: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); confirm the dev log is error-free; stop it. Report the app loads. (Human confirms: opening notes shows the strip; preview is italic and replaced on next open; editing pins + shows the dirty dot then Saved; ×/⌘W close + focus a neighbour; Ctrl+Tab cycles; ⌘1-9 jump; reload restores pinned tabs; the editor fills the height below the strip.)

- [ ] **Step 10: Commit**

```bash
git add web/src/app/App.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(tabs): TabStrip in App + ⌘W/Ctrl+Tab/⌘1-9 + Close tab command + e2e"
```

---

## Notes for the executor

- **The store refactor (Task 3) is the crux.** The contract: `openNotes` holds every open note's buffer; `tabs` holds order + preview flags; `activeContents`/`dirty`/`saving` are a *mirror* of `openNotes[activePath]` kept in sync by `setBuffer`/`applyTabs`. Never write those three directly except through those helpers. This is why App/CommitBar/Editor need no buffer changes.
- **Per-note autosave** uses `autosaves: Map<path, Debounced>`; `saveNote(path)` reads that path's buffer, not "active" — so a switched-away note still flushes. `saveActive()` is kept (one test calls it) and delegates to `saveNote(activePath)`.
- **Preview never writes.** Only `editBuffer` schedules autosave; browsing through preview tabs never hits disk. Pinning happens on edit (and on double-click / new note).
- **Restore order matters:** `init()` opens each persisted pinned path then immediately `pinTab`s it, so the next `openOrPreview` appends a fresh preview rather than replacing the just-restored tab.
- **⌘W is browser-reserved** — wired with `preventDefault` for the Tauri shell; the e2e closes via the tab's × instead.
- **Empty state:** closing the last tab sets `activePath = null`; the Editor already handles `path === null`. The strip renders nothing with zero tabs.
- **Layout:** the editor view becomes a flex column (`TabStrip` + `flex-1 min-h-0` editor). Verify the editor still fills the height in the manual check.
- **No new deps, no contract/host changes.** Tauri unaffected.
```
