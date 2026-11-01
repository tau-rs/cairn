# Editor Split-Panes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a side-by-side, two-pane editor split where each pane has its own independent tab state, while keeping routing, persistence, and every existing single-pane behaviour intact.

**Architecture:** Generalise the store's single tab group (`tabs` + `activePath`) into an array of panes (`panes: PaneState[]` + `activePane`), keeping a derived focused-pane mirror (`activePath`/`activeContents`/`dirty`/`saving`) so RouteSync, the Editor body, autosave, and backlinks are undisturbed. A new pure `paneModel.ts` sits above the untouched `tabsModel.ts`. The note `index` is opened in panes; `openNotes` buffers stay path-keyed and shared. Note selection for the second pane is explicit, via a new tree context menu ("Open to the side").

**Tech Stack:** React 18 + TypeScript, Zustand (vanilla store + `useStore`), react-router v7 (HashRouter), Vitest + Testing Library, Tailwind. Build/test via `just` (web recipes fan out to `pnpm`).

**Reference:** Design doc `docs/superpowers/specs/2026-06-12-editor-split-panes-design.md`.

**Conventions for every commit step:** run the relevant `pnpm` script from `web/`. The full gate is `just web-ci` (`pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build`). Run `pnpm format` before committing so `format:check` passes (easy to miss — eslint won't catch it).

---

## Task 1: Pure pane-layout model (`paneModel.ts`)

A pure module above `tabsModel`. Operates on an array of pane tab-states plus a focused index. All layout edge logic (clamping, refuse-last-pane, seeding) lives and is tested here.

**Files:**
- Create: `web/src/components/tabs/paneModel.ts`
- Test: `web/src/components/tabs/paneModel.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// web/src/components/tabs/paneModel.test.ts
import { describe, it, expect } from "vitest";
import { splitPane, closePane, focusPane, type PanesState } from "./paneModel";

const single: PanesState = {
  panes: [
    {
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    },
  ],
  activePane: 0,
};

describe("splitPane", () => {
  it("appends a second pane seeded (pinned) with seedPath and focuses it", () => {
    const r = splitPane(single, "a.md");
    expect(r.panes).toHaveLength(2);
    expect(r.panes[1]).toEqual({
      tabs: [{ path: "a.md", preview: false }],
      activePath: "a.md",
    });
    expect(r.activePane).toBe(1);
  });
  it("is a no-op when already split", () => {
    const split = splitPane(single, "a.md");
    expect(splitPane(split, "b.md")).toBe(split);
  });
  it("is a no-op when seedPath is null", () => {
    expect(splitPane(single, null)).toBe(single);
  });
});

describe("closePane", () => {
  it("removes the pane and clamps activePane to the survivor", () => {
    const split = splitPane(single, "a.md"); // activePane 1
    const r = closePane(split, 1);
    expect(r.panes).toHaveLength(1);
    expect(r.activePane).toBe(0);
  });
  it("keeps activePane valid when closing pane 0 while focused on it", () => {
    const split = { ...splitPane(single, "a.md"), activePane: 0 };
    const r = closePane(split, 0);
    expect(r.panes).toHaveLength(1);
    expect(r.activePane).toBe(0);
  });
  it("refuses to remove the last pane", () => {
    expect(closePane(single, 0)).toBe(single);
  });
});

describe("focusPane", () => {
  it("sets activePane within range", () => {
    const split = splitPane(single, "a.md");
    expect(focusPane(split, 0).activePane).toBe(0);
  });
  it("ignores out-of-range indices", () => {
    expect(focusPane(single, 5)).toBe(single);
    expect(focusPane(single, -1)).toBe(single);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm vitest run src/components/tabs/paneModel.test.ts`
Expected: FAIL — `Cannot find module './paneModel'`.

- [ ] **Step 3: Write minimal implementation**

```ts
// web/src/components/tabs/paneModel.ts
import type { TabsState } from "./tabsModel";

/** One pane is exactly a tab group — reuse TabsState so tabsModel applies per pane. */
export type PaneState = TabsState;

export interface PanesState {
  /** 1 (single) or 2 (split). Modelled as an array so N-way is a later UI-only change. */
  panes: PaneState[];
  /** Index of the focused pane. */
  activePane: number;
}

/** Maximum panes the layout allows (v1: side-by-side only). */
const MAX_PANES = 2;

/** Append a second pane seeded with `seedPath` (pinned) and focus it. No-op if
 *  already at MAX_PANES or seedPath is null. */
export function splitPane(s: PanesState, seedPath: string | null): PanesState {
  if (s.panes.length >= MAX_PANES || seedPath === null) return s;
  const seeded: PaneState = {
    tabs: [{ path: seedPath, preview: false }],
    activePath: seedPath,
  };
  return { panes: [...s.panes, seeded], activePane: s.panes.length };
}

/** Remove pane `index`, clamping activePane to a surviving pane. Never drops the
 *  last pane. */
export function closePane(s: PanesState, index: number): PanesState {
  if (s.panes.length <= 1 || index < 0 || index >= s.panes.length) return s;
  const panes = s.panes.filter((_, i) => i !== index);
  const activePane = Math.min(s.activePane > index ? s.activePane - 1 : s.activePane, panes.length - 1);
  return { panes, activePane };
}

/** Focus pane `index` (guarded). */
export function focusPane(s: PanesState, index: number): PanesState {
  if (index < 0 || index >= s.panes.length || index === s.activePane) return s;
  return { panes: s.panes, activePane: index };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm vitest run src/components/tabs/paneModel.test.ts`
Expected: PASS (all cases).

- [ ] **Step 5: Commit**

```bash
cd web && pnpm format
git add web/src/components/tabs/paneModel.ts web/src/components/tabs/paneModel.test.ts
git commit -m "feat(tabs): pure pane-layout model (split/close/focus)"
```

---

## Task 2: Pane-aware persistence (`tabsPersistence.ts`)

Add `savePanes`/`loadPanes` that persist all panes + focused index + split ratio, migrating the legacy single-group format. Keep the existing `saveTabs`/`loadTabs` for now — Task 3 swaps the store over and deletes them.

**Files:**
- Modify: `web/src/components/tabs/tabsPersistence.ts`
- Test: `web/src/components/tabs/tabsPersistence.test.ts`

- [ ] **Step 1: Write the failing test (append to the existing test file)**

```ts
// add to web/src/components/tabs/tabsPersistence.test.ts
import { savePanes, loadPanes } from "./tabsPersistence";
import type { PanesState } from "./paneModel";

describe("savePanes / loadPanes", () => {
  beforeEach(() => localStorage.clear());

  it("round-trips pinned tabs per pane, focused index, and ratio", () => {
    const state: PanesState = {
      panes: [
        { tabs: [{ path: "a.md", preview: false }, { path: "x.md", preview: true }], activePath: "a.md" },
        { tabs: [{ path: "b.md", preview: false }], activePath: "b.md" },
      ],
      activePane: 1,
    };
    savePanes({ ...state, ratio: 0.6 });
    const r = loadPanes(["a.md", "b.md", "x.md"]);
    // preview tab x.md is NOT persisted (only pinned)
    expect(r.panes).toEqual([
      { pinned: ["a.md"], activePath: "a.md" },
      { pinned: ["b.md"], activePath: "b.md" },
    ]);
    expect(r.activePane).toBe(1);
    expect(r.ratio).toBe(0.6);
  });

  it("drops paths that no longer exist", () => {
    savePanes({
      panes: [{ tabs: [{ path: "gone.md", preview: false }], activePath: "gone.md" }],
      activePane: 0,
      ratio: 0.5,
    });
    const r = loadPanes(["a.md"]);
    expect(r.panes[0].pinned).toEqual([]);
    expect(r.panes[0].activePath).toBeNull();
  });

  it("migrates the legacy single-group format", () => {
    // write the OLD shape under the same key
    localStorage.setItem(
      "cairn.tabs",
      JSON.stringify({ pinned: ["a.md", "b.md"], activePath: "b.md" }),
    );
    const r = loadPanes(["a.md", "b.md"]);
    expect(r.panes).toEqual([{ pinned: ["a.md", "b.md"], activePath: "b.md" }]);
    expect(r.activePane).toBe(0);
    expect(r.ratio).toBe(0.5);
  });

  it("returns a single empty pane when nothing is stored", () => {
    const r = loadPanes(["a.md"]);
    expect(r.panes).toEqual([{ pinned: [], activePath: null }]);
    expect(r.activePane).toBe(0);
    expect(r.ratio).toBe(0.5);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm vitest run src/components/tabs/tabsPersistence.test.ts`
Expected: FAIL — `savePanes`/`loadPanes` not exported.

- [ ] **Step 3: Write the implementation (append to `tabsPersistence.ts`)**

```ts
// add imports at top of web/src/components/tabs/tabsPersistence.ts
import type { PanesState } from "./paneModel";

// add below the existing code:

export interface PersistedPane {
  pinned: string[];
  activePath: string | null;
}
export interface PersistedPanes {
  panes: PersistedPane[];
  activePane: number;
  ratio: number;
}

const DEFAULT_RATIO = 0.5;

/** Persist all panes (pinned tabs + active), the focused pane, and the ratio. */
export function savePanes(state: PanesState & { ratio: number }): void {
  try {
    const data: PersistedPanes = {
      panes: state.panes.map((p) => ({
        pinned: p.tabs.filter((t) => !t.preview).map((t) => t.path),
        activePath: p.activePath,
      })),
      activePane: state.activePane,
      ratio: state.ratio,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch {
    // ignore (private mode / quota)
  }
}

function resolvePane(p: PersistedPane, exists: Set<string>): PersistedPane {
  const pinned = (p.pinned ?? []).filter((x) => exists.has(x));
  const activePath =
    p.activePath && pinned.includes(p.activePath)
      ? p.activePath
      : pinned.length > 0
        ? pinned[pinned.length - 1]
        : null;
  return { pinned, activePath };
}

/** Load panes, dropping vanished paths. Tolerates the legacy single-group shape
 *  (`{ pinned, activePath }`) by lifting it into one pane. */
export function loadPanes(existingPaths: string[]): PersistedPanes {
  const fallback: PersistedPanes = {
    panes: [{ pinned: [], activePath: null }],
    activePane: 0,
    ratio: DEFAULT_RATIO,
  };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<PersistedPanes> & Partial<PersistedTabs>;
    const exists = new Set(existingPaths);

    // Legacy format: has `pinned`/`activePath` at top level, no `panes`.
    const rawPanes: PersistedPane[] = Array.isArray(parsed.panes)
      ? parsed.panes
      : [{ pinned: parsed.pinned ?? [], activePath: parsed.activePath ?? null }];

    const panes = rawPanes.map((p) => resolvePane(p, exists));
    const safePanes = panes.length > 0 ? panes : fallback.panes;
    const activePane = Math.min(
      Math.max(0, parsed.activePane ?? 0),
      safePanes.length - 1,
    );
    const ratio =
      typeof parsed.ratio === "number" ? parsed.ratio : DEFAULT_RATIO;
    return { panes: safePanes, activePane, ratio };
  } catch {
    return fallback;
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm vitest run src/components/tabs/tabsPersistence.test.ts`
Expected: PASS (all old + new cases).

- [ ] **Step 5: Commit**

```bash
cd web && pnpm format
git add web/src/components/tabs/tabsPersistence.ts web/src/components/tabs/tabsPersistence.test.ts
git commit -m "feat(tabs): pane-aware persistence with legacy migration"
```

---

## Task 3: Store — generalise to a pane array (single-pane behaviour unchanged)

Replace top-level `tabs`/`activePath`-as-source with `panes`/`activePane`/`splitRatio`, keeping the focused-pane mirror. Make per-pane tab actions accept an optional `paneIndex` (defaulting to focused). This task adds **no new user-facing behaviour** — all existing store tests stay green with a single pane.

**Files:**
- Modify: `web/src/store/store.ts`
- Modify: `web/src/store/store.test.ts` (one persistence test imports `saveTabs`)
- Modify: `web/src/components/EditorPane.tsx` (reads `s.tabs` → derive from `s.panes[s.activePane]` so it compiles; full split UI comes in Task 5)

- [ ] **Step 1: Update imports and types in `store.ts`**

Replace the tabs-model import block (lines 12-21) with:

```ts
import {
  openOrPreview,
  pinTab as pinTabModel,
  closeTab as closeTabModel,
  cycle as cycleModel,
  jumpTo as jumpToModel,
  type Tab,
  type TabsState,
} from "../components/tabs/tabsModel";
import {
  splitPane as splitPaneModel,
  closePane as closePaneModel,
  type PaneState,
  type PanesState,
} from "../components/tabs/paneModel";
import { loadPanes, savePanes } from "../components/tabs/tabsPersistence";
```

In `interface CairnState`, replace `tabs: Tab[];` with:

```ts
  panes: PaneState[];
  activePane: number;
  splitRatio: number;
```

Keep `activePath`, `activeContents`, `dirty`, `saving` (they remain the focused-pane mirror). Update the action signatures in the interface:

```ts
  selectTab(path: string, paneIndex?: number): void;
  closeTab(path: string, paneIndex?: number): void;
  closeActiveTab(): void;
  cycleTab(delta: 1 | -1): void;
  jumpToTab(n: number): void;
  pinTab(path: string, paneIndex?: number): void;
  openNote(path: string, opts?: { pane?: number }): Promise<void>;
  splitPane(): void;
  openToSide(path: string): Promise<void>;
  closePane(index?: number): void;
  focusPane(index: number): void;
  setSplitRatio(ratio: number): void;
```

(`splitPane`/`openToSide`/`closePane`/`focusPane`/`setSplitRatio` are implemented in Task 4 — declare them now and add stub bodies in Step 4 of this task so the type checks; Task 4 fills them in via TDD.)

- [ ] **Step 2: Replace the store helper closures**

Replace `tabsState`, `persist`, and `applyTabs` (lines 186-227) with:

```ts
    const tabsState = (paneIndex = get().activePane): TabsState =>
      get().panes[paneIndex];

    const panesState = (): PanesState => ({
      panes: get().panes,
      activePane: get().activePane,
    });

    const persist = () =>
      savePanes({ ...panesState(), ratio: get().splitRatio });

    // Recompute the focused-pane mirror (activePath/contents/dirty/saving) so the
    // many existing consumers that read these top-level fields stay unchanged.
    const syncMirror = () =>
      set((s) => {
        const pane = s.panes[s.activePane];
        const buf = pane.activePath ? s.openNotes[pane.activePath] : undefined;
        return {
          activePath: pane.activePath,
          activeContents: buf?.contents ?? "",
          dirty: buf?.dirty ?? false,
          saving: buf?.saving ?? false,
        };
      });

    // Write a TabsState into one pane, then refresh the mirror from the focused
    // pane (whether or not the written pane was the focused one).
    const applyTabs = (next: TabsState, paneIndex = get().activePane) => {
      set((s) => {
        const panes = s.panes.slice();
        panes[paneIndex] = next;
        return { panes };
      });
      syncMirror();
    };

    // Load a note's buffer if not already open. Returns false on failure.
    const ensureNote = async (path: string): Promise<boolean> => {
      if (get().openNotes[path]) return true;
      setLoading("note", true);
      try {
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type !== "note") {
          unexpected("Open note", res, { path });
          return false;
        }
        set((s) => ({
          openNotes: {
            ...s.openNotes,
            [path]: { contents: res.contents, dirty: false, saving: false },
          },
        }));
        return true;
      } finally {
        setLoading("note", false);
      }
    };
```

Note: `setLoading` and `unexpected` are defined further down in the closure but are hoisted function expressions assigned to `const` — `ensureNote` is only *called* at runtime (inside async actions), after all consts are initialised, so referencing them here is safe. (They are already used the same way by sibling closures.)

- [ ] **Step 3: Update `setBuffer`** — its mirror condition already keys off `s.activePath === path`, which now means "the focused pane's active note." No change needed; leave `setBuffer` as-is.

- [ ] **Step 4: Update the initial state and the tab actions**

In the returned state object, replace `tabs: [],` (initial, ~line 381) with:

```ts
      panes: [{ tabs: [], activePath: null }],
      activePane: 0,
      splitRatio: 0.5,
```

Rewrite `openNote`:

```ts
      async openNote(path, opts) {
        const pane = opts?.pane ?? get().activePane;
        try {
          if (!(await ensureNote(path))) return;
          applyTabs(openOrPreview(tabsState(pane), path), pane);
          persist();
          await get().refreshBacklinks();
        } catch (err) {
          pushError("Open note", err, { path });
        }
      },
```

`editBuffer` — only the `applyTabs(pinTabModel(tabsState(), path))` line stays; `tabsState()` now defaults to the focused pane. No change to the function body.

Rewrite `selectTab`, `closeTab`, `closeActiveTab`, `cycleTab`, `jumpToTab`, `pinTab`:

```ts
      selectTab(path, paneIndex = get().activePane) {
        if (!get().openNotes[path]) return;
        set({ activePane: paneIndex });
        applyTabs({ ...tabsState(paneIndex), activePath: path }, paneIndex);
        persist();
        void get().refreshBacklinks();
      },

      closeTab(path, paneIndex = get().activePane) {
        // Flush any pending edit before the buffer may be dropped.
        void get().saveNote(path);
        applyTabs(closeTabModel(tabsState(paneIndex), path), paneIndex);
        // Drop the buffer only if no pane still references the path.
        const stillOpen = get().panes.some((p) =>
          p.tabs.some((t) => t.path === path),
        );
        if (!stillOpen) dropNote(path);
        // Collapse an emptied second pane back to single.
        const ai = paneIndex;
        if (get().panes.length > 1 && get().panes[ai].tabs.length === 0) {
          const r = closePaneModel(panesState(), ai);
          set({ panes: r.panes, activePane: r.activePane });
          syncMirror();
        }
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

      pinTab(path, paneIndex = get().activePane) {
        if (!get().openNotes[path]) return;
        set({ activePane: paneIndex });
        applyTabs(
          pinTabModel({ ...tabsState(paneIndex), activePath: path }, path),
          paneIndex,
        );
        persist();
        void get().refreshBacklinks();
      },
```

Add **stub** bodies for the Task-4 actions so the file type-checks (Task 4 replaces them via TDD):

```ts
      splitPane() {},
      async openToSide(_path) {},
      closePane(_index) {},
      focusPane(_index) {},
      setSplitRatio(_ratio) {},
```

- [ ] **Step 5: Update `applyRenames` to remap across all panes**

Replace its `set((s) => { ... })` block (lines 582-595) with:

```ts
          set((s) => {
            const openNotes = { ...s.openNotes };
            if (from in openNotes) {
              openNotes[to] = openNotes[from];
              delete openNotes[from];
            }
            const panes = s.panes.map((p) => ({
              tabs: p.tabs.map((t) =>
                t.path === from ? { ...t, path: to } : t,
              ),
              activePath: p.activePath === from ? to : p.activePath,
            }));
            const active = panes[s.activePane];
            const buf = active.activePath ? openNotes[active.activePath] : undefined;
            return {
              openNotes,
              panes,
              activePath: active.activePath,
              activeContents: buf?.contents ?? "",
              dirty: buf?.dirty ?? false,
              saving: buf?.saving ?? false,
            };
          });
```

- [ ] **Step 6: Update `loadCairn` reset + restore**

In the reset `set({ ... })` (lines 339-356), replace `tabs: [], activePath: null,` with:

```ts
        panes: [{ tabs: [], activePath: null }],
        activePane: 0,
        splitRatio: 0.5,
        activePath: null,
```

Replace the restore block (lines 360-372, from `// Restore persisted pinned tabs` to the `rearmInterval()` call) with:

```ts
      // Restore persisted panes; skip any pinned note that no longer loads, and
      // collapse a pane that restores empty.
      const persisted = loadPanes(get().notePaths);
      const restored: PaneState[] = [];
      for (const pp of persisted.panes) {
        const tabs: Tab[] = [];
        for (const p of pp.pinned) {
          if (await ensureNote(p)) tabs.push({ path: p, preview: false });
        }
        const activePath =
          pp.activePath && tabs.some((t) => t.path === pp.activePath)
            ? pp.activePath
            : tabs.length > 0
              ? tabs[tabs.length - 1].path
              : null;
        restored.push({ tabs, activePath });
      }
      const nonEmpty = restored.filter((p) => p.tabs.length > 0);
      const panes = nonEmpty.length > 0 ? nonEmpty : [{ tabs: [], activePath: null }];
      const activePane = Math.min(persisted.activePane, panes.length - 1);
      set({ panes, activePane, splitRatio: persisted.ratio });
      syncMirror();
      if (get().activePath) await get().refreshBacklinks();
      get().rearmInterval();
```

- [ ] **Step 7: Fix `EditorPane.tsx` so it compiles** (full split UI is Task 5)

Replace `const tabs = useCairn((s) => s.tabs);` with:

```ts
  const panes = useCairn((s) => s.panes);
  const activePane = useCairn((s) => s.activePane);
  const tabs = panes[activePane].tabs;
```

- [ ] **Step 8: Fix the store persistence test**

In `web/src/store/store.test.ts`, the import `import { saveTabs } from "../components/tabs/tabsPersistence";` and any test seeding the legacy shape with `saveTabs(...)` still works (legacy migration is supported). If a test asserts on `store.getState().tabs`, change it to `store.getState().panes[store.getState().activePane].tabs`. Run the suite (next step) to find exactly which assertions need updating, and update only those — do not change behaviour expectations.

- [ ] **Step 9: Run the full web test suite + typecheck**

Run: `cd web && pnpm typecheck && pnpm test`
Expected: PASS. All existing store/RouteSync/EditorPane tests stay green (single-pane behaviour is identical). Fix any `s.tabs` assertion drift surfaced here per Step 8.

- [ ] **Step 10: Commit**

```bash
cd web && pnpm format
git add web/src/store/store.ts web/src/store/store.test.ts web/src/components/EditorPane.tsx
git commit -m "refactor(store): generalise tabs into a pane array with focused mirror"
```

---

## Task 4: Store — split actions (`splitPane`, `openToSide`, `closePane`, `focusPane`, `setSplitRatio`)

Replace the Task-3 stubs with real implementations, TDD'd against the store.

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing tests (append to `store.test.ts`)**

```ts
describe("split panes", () => {
  async function ready() {
    const { store } = setup(); // setup() seeds a.md, b.md
    await store.getState().init();
    return store;
  }

  it("splitPane duplicates the focused note into a new focused pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    store.getState().splitPane();
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[1].activePath).toBe("a.md");
    expect(s.activePath).toBe("a.md"); // mirror follows the new focused pane
  });

  it("openToSide opens a chosen note in the other pane and focuses it", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[0].activePath).toBe("a.md"); // origin pane unchanged
    expect(s.panes[1].activePath).toBe("b.md");
    expect(s.activePath).toBe("b.md");
  });

  it("openToSide on an existing split targets the non-focused pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md"); // panes: [a][b], focus 1
    store.getState().focusPane(0); // focus pane 0
    await store.getState().openToSide("a.md"); // open into pane 1 (the non-focused)
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[1].activePath).toBe("a.md");
  });

  it("focusPane moves the mirror to the target pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    store.getState().focusPane(0);
    expect(store.getState().activePath).toBe("a.md");
  });

  it("closePane collapses to a single pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    store.getState().closePane(1);
    const s = store.getState();
    expect(s.panes).toHaveLength(1);
    expect(s.activePane).toBe(0);
    expect(s.activePath).toBe("a.md");
  });

  it("setSplitRatio clamps to [0.2, 0.8]", async () => {
    const store = await ready();
    store.getState().setSplitRatio(0.95);
    expect(store.getState().splitRatio).toBe(0.8);
    store.getState().setSplitRatio(0.05);
    expect(store.getState().splitRatio).toBe(0.2);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && pnpm vitest run src/store/store.test.ts -t "split panes"`
Expected: FAIL — stubs do nothing (`panes` length stays 1, etc.).

- [ ] **Step 3: Replace the stub bodies in `store.ts`**

```ts
      splitPane() {
        const seed = get().activePath; // duplicate the focused note
        const r = splitPaneModel(panesState(), seed);
        if (r === panesState()) return; // no-op (already split or no active note)
        set({ panes: r.panes, activePane: r.activePane });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      async openToSide(path) {
        if (!(await ensureNote(path))) return;
        if (get().panes.length < 2) {
          // Create the second pane seeded directly with `path` (NOT the current
          // note) and focus it.
          const r = splitPaneModel(panesState(), path);
          set({ panes: r.panes, activePane: r.activePane });
        } else {
          const target = get().activePane === 0 ? 1 : 0;
          applyTabs(openOrPreview(tabsState(target), path), target);
          set({ activePane: target });
        }
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      closePane(index = get().activePane) {
        if (get().panes.length < 2) return;
        const closing = get().panes[index];
        const survivorPaths = new Set(
          get()
            .panes.filter((_, i) => i !== index)
            .flatMap((p) => p.tabs.map((t) => t.path)),
        );
        for (const t of closing.tabs) {
          if (!survivorPaths.has(t.path)) {
            void get().saveNote(t.path);
            dropNote(t.path);
          }
        }
        const r = closePaneModel(panesState(), index);
        set({ panes: r.panes, activePane: r.activePane });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      focusPane(index) {
        if (index < 0 || index >= get().panes.length || index === get().activePane)
          return;
        set({ activePane: index });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      setSplitRatio(ratio) {
        set({ splitRatio: Math.min(0.8, Math.max(0.2, ratio)) });
        persist();
      },
```

Note on the `splitPane` no-op check: `splitPaneModel` returns the *same reference* when it no-ops, but `panesState()` builds a fresh object each call, so `r === panesState()` never matches. Replace that guard with a length/seed check:

```ts
      splitPane() {
        const seed = get().activePath;
        if (seed === null || get().panes.length >= 2) return;
        const r = splitPaneModel(panesState(), seed);
        set({ panes: r.panes, activePane: r.activePane });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && pnpm vitest run src/store/store.test.ts -t "split panes"`
Expected: PASS. Then run the whole suite: `cd web && pnpm test` — all green.

- [ ] **Step 5: Commit**

```bash
cd web && pnpm format
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): split/openToSide/closePane/focusPane/setSplitRatio actions"
```

---

## Task 5: EditorPane → split layout (per-pane view + resizable divider)

Render 1 or 2 panes side by side. Extract the current single-pane body into a per-pane view; add a `Divider`; focus a pane on interaction.

**Files:**
- Modify: `web/src/components/EditorPane.tsx`
- Create: `web/src/components/editor/Divider.tsx`
- Test: `web/src/components/editor/Divider.test.tsx`
- Test: `web/src/components/EditorPane.test.tsx` (create if absent)

- [ ] **Step 1: Write the failing Divider test**

```tsx
// web/src/components/editor/Divider.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { Divider } from "./Divider";

describe("Divider", () => {
  it("nudges ratio with arrow keys", () => {
    const onRatio = vi.fn();
    render(<Divider ratio={0.5} onRatio={onRatio} />);
    const sep = screen.getByRole("separator");
    fireEvent.keyDown(sep, { key: "ArrowRight" });
    expect(onRatio).toHaveBeenCalledWith(expect.closeTo(0.52, 5));
    fireEvent.keyDown(sep, { key: "ArrowLeft" });
    expect(onRatio).toHaveBeenCalledWith(expect.closeTo(0.48, 5));
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && pnpm vitest run src/components/editor/Divider.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `Divider.tsx`**

```tsx
// web/src/components/editor/Divider.tsx
import { useRef } from "react";

/** Draggable vertical divider between split panes. Reports a new left-pane
 *  fraction via `onRatio`. Keyboard-accessible (←/→ nudge by 0.02). */
export function Divider(props: {
  ratio: number;
  onRatio: (ratio: number) => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);

  const onPointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    const parent = ref.current?.parentElement;
    if (!parent) return;
    const rect = parent.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      props.onRatio((ev.clientX - rect.left) / rect.width);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowRight") {
      e.preventDefault();
      props.onRatio(props.ratio + 0.02);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      props.onRatio(props.ratio - 0.02);
    }
  };

  return (
    <div
      ref={ref}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize panes"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
      className="w-1.5 shrink-0 cursor-col-resize bg-border hover:bg-accent focus:bg-accent focus:outline-none"
    />
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && pnpm vitest run src/components/editor/Divider.test.tsx`
Expected: PASS.

- [ ] **Step 5: Refactor `EditorPane.tsx` into a split layout**

Extract the per-pane body into a local `PaneView` and render the panes. Replace the whole `EditorPane` component with:

```tsx
import { useNavigate, useLocation } from "react-router-dom";
import { useCairn, useActions } from "../app/cairnStore";
import { noteUrl, isGraph, tagFromLocation } from "../app/routes";
import { GraphView } from "./GraphView";
import { Editor } from "./Editor";
import { TabStrip } from "./tabs/TabStrip";
import { SearchResults } from "./SearchResults";
import { ErrorBoundary } from "./ErrorBoundary";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";
import { Divider } from "./editor/Divider";
import type { PaneState } from "./tabs/paneModel";

/** One editor pane: its own tab strip + editor, bound to pane `index`. */
function PaneView(props: { pane: PaneState; index: number; focused: boolean; split: boolean }) {
  const navigate = useNavigate();
  const actions = useActions();
  const notePaths = useCairn((s) => s.notePaths);
  const openNotes = useCairn((s) => s.openNotes);
  const editorMode = useCairn((s) => s.settings.editorMode);
  const loadRemoteImages = useCairn((s) => s.settings.loadRemoteImages);
  const loading = useCairn((s) => s.loading);

  const { pane, index, focused, split } = props;
  const activePath = pane.activePath;
  const buffer = activePath ? (openNotes[activePath]?.contents ?? "") : "";

  const tabViews = pane.tabs.map((t) => ({
    path: t.path,
    preview: t.preview,
    dirty: openNotes[t.path]?.dirty ?? false,
  }));

  return (
    <div
      className={
        "flex min-w-0 flex-1 flex-col " +
        (split && focused ? "ring-1 ring-inset ring-accent/60" : "")
      }
      style={split ? { flexGrow: undefined } : undefined}
      onMouseDownCapture={() => actions.focusPane(index)}
    >
      <TabStrip
        tabs={tabViews}
        activePath={activePath}
        onSelect={(p) => actions.selectTab(p, index)}
        onPin={(p) => actions.pinTab(p, index)}
        onClose={(p) => actions.closeTab(p, index)}
        onSplit={split ? undefined : actions.splitPane}
        onClosePane={split ? () => actions.closePane(index) : undefined}
      />
      <div className="relative min-h-0 flex-1">
        {loading.note && focused && (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-bg/50">
            <Spinner label="Loading note" />
          </div>
        )}
        <Editor
          path={activePath}
          value={buffer}
          mode={editorMode}
          notePaths={notePaths}
          assetUrl={actions.assetUrl}
          loadRemoteImages={loadRemoteImages}
          onChange={actions.editBuffer}
          onOpenNote={(p) => navigate(noteUrl(p))}
          onToggleMode={() =>
            actions.setSettings({
              editorMode: editorMode === "livepreview" ? "source" : "livepreview",
            })
          }
        />
      </div>
    </div>
  );
}

export function EditorPane() {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = useActions();
  const activePath = useCairn((s) => s.activePath);
  const searchResults = useCairn((s) => s.searchResults);
  const searchSnippets = useCairn((s) => s.searchSnippets);
  const activeTag = useCairn((s) => s.activeTag);
  const graph = useCairn((s) => s.graph);
  const noteTags = useCairn((s) => s.noteTags);
  const panes = useCairn((s) => s.panes);
  const activePane = useCairn((s) => s.activePane);
  const splitRatio = useCairn((s) => s.splitRatio);
  const loading = useCairn((s) => s.loading);
  const view = isGraph(location) ? "graph" : "editor";
  const split = panes.length > 1;

  return (
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
        <SearchResults
          results={searchResults}
          loading={loading.search}
          snippets={searchSnippets ?? undefined}
          title={activeTag ? `Tagged · ${activeTag}` : undefined}
          onOpen={(p) => navigate(noteUrl(p))}
          onClose={() => {
            if (tagFromLocation(location) !== null) {
              navigate(activePath ? noteUrl(activePath) : "/");
            } else {
              actions.closeSearch();
            }
          }}
        />
        {view === "graph" ? (
          <GraphView
            nodes={graph?.nodes ?? []}
            edges={graph?.edges ?? []}
            tagsByNote={noteTags}
            activePath={activePath}
            loading={loading.graph}
            onOpenNote={(p) => navigate(noteUrl(p))}
          />
        ) : (
          <div className="flex h-full">
            {panes.map((pane, i) => (
              <div
                key={i}
                className="flex min-w-0 flex-col"
                style={{ flexGrow: split ? (i === 0 ? splitRatio : 1 - splitRatio) : 1, flexBasis: 0 }}
              >
                <PaneView pane={pane} index={i} focused={i === activePane} split={split} />
              </div>
            ))}
            {split && (
              <Divider ratio={splitRatio} onRatio={actions.setSplitRatio} />
            )}
          </div>
        )}
      </div>
    </ErrorBoundary>
  );
}
```

Note: the `Divider` renders after the panes for simplicity; for a between-panes divider, render it between the two pane wrappers instead. Adjust to: map pane 0 wrapper, then `{split && <Divider/>}`, then pane 1 wrapper. Use this structure:

```tsx
<div className="flex h-full">
  <div className="flex min-w-0 flex-col" style={{ flexGrow: split ? splitRatio : 1, flexBasis: 0 }}>
    <PaneView pane={panes[0]} index={0} focused={activePane === 0} split={split} />
  </div>
  {split && <Divider ratio={splitRatio} onRatio={actions.setSplitRatio} />}
  {split && (
    <div className="flex min-w-0 flex-col" style={{ flexGrow: 1 - splitRatio, flexBasis: 0 }}>
      <PaneView pane={panes[1]} index={1} focused={activePane === 1} split={split} />
    </div>
  )}
</div>
```

- [ ] **Step 6: Write an EditorPane split-render test**

```tsx
// web/src/components/EditorPane.test.tsx
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, it, expect, beforeEach } from "vitest";
import { EditorPane } from "./EditorPane";
import { cairnStore } from "../app/cairnStore";

// Drive the real store directly (it is a module singleton).
function seedSplit() {
  cairnStore.setState({
    notePaths: ["a.md", "b.md"],
    openNotes: {
      "a.md": { contents: "alpha", dirty: false, saving: false },
      "b.md": { contents: "beta", dirty: false, saving: false },
    },
    panes: [
      { tabs: [{ path: "a.md", preview: false }], activePath: "a.md" },
      { tabs: [{ path: "b.md", preview: false }], activePath: "b.md" },
    ],
    activePane: 1,
    splitRatio: 0.5,
    activePath: "b.md",
    activeContents: "beta",
  });
}

describe("EditorPane split", () => {
  beforeEach(() => seedSplit());

  it("renders both panes' tab strips and a resize separator", () => {
    render(
      <MemoryRouter initialEntries={["/note/b.md"]}>
        <EditorPane />
      </MemoryRouter>,
    );
    expect(screen.getByLabelText("a.md")).toBeInTheDocument();
    expect(screen.getByLabelText("b.md")).toBeInTheDocument();
    expect(screen.getByRole("separator")).toBeInTheDocument();
  });
});
```

- [ ] **Step 7: Run the tests**

Run: `cd web && pnpm vitest run src/components/EditorPane.test.tsx src/components/editor/Divider.test.tsx`
Expected: PASS. Then `pnpm typecheck` — green.

- [ ] **Step 8: Commit**

```bash
cd web && pnpm format
git add web/src/components/EditorPane.tsx web/src/components/editor/Divider.tsx web/src/components/editor/Divider.test.tsx web/src/components/EditorPane.test.tsx
git commit -m "feat(editor): side-by-side split layout with resizable divider"
```

---

## Task 6: TabStrip split / close-pane action buttons

Add a trailing actions slot to `TabStrip` with the ⫷ split (single-pane) and ⊟ close-pane (split) controls. The strip already exists; this adds two optional callbacks.

**Files:**
- Modify: `web/src/components/tabs/TabStrip.tsx`
- Modify: `web/src/components/tabs/TabStrip.test.tsx`

- [ ] **Step 1: Write the failing tests (append to `TabStrip.test.tsx`)**

(`render`, `screen`, `fireEvent`, and `vi` are already imported at the top of this file — do not re-import.)

```tsx
it("renders a Split right button and fires onSplit", () => {
  const onSplit = vi.fn();
  render(
    <TabStrip
      tabs={[{ path: "a.md", preview: false, dirty: false }]}
      activePath="a.md"
      onSelect={() => {}}
      onPin={() => {}}
      onClose={() => {}}
      onSplit={onSplit}
    />,
  );
  fireEvent.click(screen.getByLabelText("Split editor right"));
  expect(onSplit).toHaveBeenCalled();
});

it("renders a Close pane button and fires onClosePane", () => {
  const onClosePane = vi.fn();
  render(
    <TabStrip
      tabs={[{ path: "a.md", preview: false, dirty: false }]}
      activePath="a.md"
      onSelect={() => {}}
      onPin={() => {}}
      onClose={() => {}}
      onClosePane={onClosePane}
    />,
  );
  fireEvent.click(screen.getByLabelText("Close pane"));
  expect(onClosePane).toHaveBeenCalled();
});
```

(Ensure `vi` and `screen`/`render` are imported in the test file; they already are for existing cases.)

- [ ] **Step 2: Run to verify failure**

Run: `cd web && pnpm vitest run src/components/tabs/TabStrip.test.tsx`
Expected: FAIL — buttons not found / props unknown.

- [ ] **Step 3: Implement the actions slot**

In `TabStrip.tsx`, extend the props:

```tsx
export function TabStrip(props: {
  tabs: TabView[];
  activePath: string | null;
  onSelect: (path: string) => void;
  onPin: (path: string) => void;
  onClose: (path: string) => void;
  onSplit?: () => void;
  onClosePane?: () => void;
}) {
```

The component currently early-returns `null` when `tabs.length === 0`. Keep that, but the pane actions should still show on an empty split pane — change the guard so the strip renders when there are tabs **or** a pane action exists:

```tsx
  if (props.tabs.length === 0 && !props.onSplit && !props.onClosePane) return null;
```

Then, inside the `role="tablist"` container, after the `{props.tabs.map(...)}` block and before the closing `</div>`, add the actions slot:

```tsx
      {(props.onSplit || props.onClosePane) && (
        <div className="ml-auto flex shrink-0 items-center gap-1 border-l border-border px-1.5">
          {props.onSplit && (
            <button
              type="button"
              aria-label="Split editor right"
              title="Split editor right"
              onClick={props.onSplit}
              className="flex h-6 w-6 items-center justify-center rounded text-faint hover:bg-surface-2 hover:text-text"
            >
              <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4">
                <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
                <line x1="8" y1="2.5" x2="8" y2="13.5" />
              </svg>
            </button>
          )}
          {props.onClosePane && (
            <button
              type="button"
              aria-label="Close pane"
              title="Close pane"
              onClick={props.onClosePane}
              className="flex h-6 w-6 items-center justify-center rounded text-faint hover:bg-surface-2 hover:text-text"
            >
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4">
                <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
                <line x1="6" y1="6" x2="10" y2="10" />
                <line x1="10" y1="6" x2="6" y2="10" />
              </svg>
            </button>
          )}
        </div>
      )}
```

The `role="tablist"` container already uses `flex`; the `ml-auto` pushes the actions to the right edge.

- [ ] **Step 4: Run to verify pass**

Run: `cd web && pnpm vitest run src/components/tabs/TabStrip.test.tsx`
Expected: PASS (old + new cases).

- [ ] **Step 5: Commit**

```bash
cd web && pnpm format
git add web/src/components/tabs/TabStrip.tsx web/src/components/tabs/TabStrip.test.tsx
git commit -m "feat(tabs): split / close-pane action buttons in the tab strip"
```

---

## Task 7: Tree context menu (Open · Open to the side · Rename · Delete)

A new positioned context menu for tree note rows. Rename/Delete reuse existing tree handlers; Open / Open to the side call the parent.

**Files:**
- Create: `web/src/components/tree/TreeContextMenu.tsx`
- Test: `web/src/components/tree/TreeContextMenu.test.tsx`
- Modify: `web/src/components/tree/FolderTreeView.tsx`
- Modify: `web/src/components/Sidebar.tsx`

- [ ] **Step 1: Write the failing TreeContextMenu test**

```tsx
// web/src/components/tree/TreeContextMenu.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { TreeContextMenu } from "./TreeContextMenu";

function setup(overrides = {}) {
  const handlers = {
    x: 10,
    y: 10,
    onOpen: vi.fn(),
    onOpenToSide: vi.fn(),
    onRename: vi.fn(),
    onDelete: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<TreeContextMenu {...handlers} />);
  return handlers;
}

describe("TreeContextMenu", () => {
  it("renders the four items", () => {
    setup();
    expect(screen.getByText("Open")).toBeInTheDocument();
    expect(screen.getByText("Open to the side")).toBeInTheDocument();
    expect(screen.getByText("Rename")).toBeInTheDocument();
    expect(screen.getByText("Delete")).toBeInTheDocument();
  });

  it("fires Open to the side then closes", () => {
    const h = setup();
    fireEvent.click(screen.getByText("Open to the side"));
    expect(h.onOpenToSide).toHaveBeenCalled();
    expect(h.onClose).toHaveBeenCalled();
  });

  it("closes on Escape", () => {
    const h = setup();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(h.onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && pnpm vitest run src/components/tree/TreeContextMenu.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `TreeContextMenu.tsx`**

```tsx
// web/src/components/tree/TreeContextMenu.tsx
import { useEffect, useRef } from "react";

/** A positioned right-click menu for a tree note row. Closes on Escape, on an
 *  outside click, or after any item runs. */
export function TreeContextMenu(props: {
  x: number;
  y: number;
  onOpen: () => void;
  onOpenToSide: () => void;
  onRename: () => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    ref.current?.focus();
    const onDocDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) props.onClose();
    };
    document.addEventListener("mousedown", onDocDown);
    return () => document.removeEventListener("mousedown", onDocDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = (fn: () => void) => () => {
    fn();
    props.onClose();
  };

  const item = (label: string, fn: () => void, danger = false, hint?: string) => (
    <button
      type="button"
      role="menuitem"
      onClick={run(fn)}
      className={
        "flex w-full items-center justify-between gap-6 rounded px-2.5 py-1.5 text-left text-xs hover:bg-surface-2 " +
        (danger ? "text-danger" : "text-text")
      }
    >
      <span>{label}</span>
      {hint && <span className="text-faint">{hint}</span>}
    </button>
  );

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
      }}
      style={{ left: props.x, top: props.y }}
      className="fixed z-50 w-44 rounded-lg border border-border bg-surface p-1 shadow-xl outline-none"
    >
      {item("Open", props.onOpen, false, "↵")}
      {item("Open to the side", props.onOpenToSide, false, "⌘↵")}
      <div className="my-1 h-px bg-border" />
      {item("Rename", props.onRename, false, "F2")}
      {item("Delete", props.onDelete, true)}
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && pnpm vitest run src/components/tree/TreeContextMenu.test.tsx`
Expected: PASS.

- [ ] **Step 5: Wire the menu into `FolderTreeView.tsx`**

Add `onOpenToSide: (path: string) => void;` to the `FolderTree` props (after `onOpen`).

Add menu state near the other `useState` hooks:

```tsx
  const [menu, setMenu] = useState<{ path: string; x: number; y: number } | null>(null);
```

Import the menu at the top:

```tsx
import { TreeContextMenu } from "./TreeContextMenu";
```

On the note-row `<div>` (the `node.kind !== "folder"` branch, the element with `draggable`), add a context handler:

```tsx
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ path: node.path, x: e.clientX, y: e.clientY });
          }}
```

At the end of the component's returned JSX, render the menu (inside the outer `<div className="flex flex-col ...">`, after `{renderNodes(tree, 0)}`):

```tsx
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          onOpen={() => props.onOpen(menu.path)}
          onOpenToSide={() => props.onOpenToSide(menu.path)}
          onRename={() => setEditingPath(menu.path)}
          onDelete={() => props.onDelete(menu.path)}
          onClose={() => setMenu(null)}
        />
      )}
```

Also add the keyboard twin on the note-row `<button>` `onKeyDown` (the existing F2 handler) — open to the side on Cmd/Ctrl+Enter:

```tsx
              onKeyDown={(e) => {
                if (e.key === "F2") {
                  e.preventDefault();
                  setEditingPath(node.path);
                } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  props.onOpenToSide(node.path);
                }
              }}
```

- [ ] **Step 6: Wire `Sidebar.tsx`**

Add the prop to the `<FolderTree>` element (after `onOpen`):

```tsx
        onOpenToSide={actions.openToSide}
```

- [ ] **Step 7: Update the existing FolderTreeView tests for the new required prop**

`web/src/components/tree/FolderTreeView.test.tsx` and `FolderTreeView.dnd.test.tsx` render `<FolderTree>`; add `onOpenToSide={() => {}}` to each render call's props so they type-check and pass.

- [ ] **Step 8: Run the tree tests + typecheck**

Run: `cd web && pnpm typecheck && pnpm vitest run src/components/tree`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
cd web && pnpm format
git add web/src/components/tree/TreeContextMenu.tsx web/src/components/tree/TreeContextMenu.test.tsx web/src/components/tree/FolderTreeView.tsx web/src/components/tree/FolderTreeView.test.tsx web/src/components/tree/FolderTreeView.dnd.test.tsx web/src/components/Sidebar.tsx
git commit -m "feat(tree): right-click context menu with Open to the side"
```

---

## Task 8: Commands — `split-right` and `close-pane` in the palette + global keys

Register the two layout commands so they appear in ⌘K and dispatch globally. (Open-to-side stays a tree-only gesture per the spec.)

**Files:**
- Modify: `web/src/components/shortcuts/commands.ts`
- Modify: `web/src/app/useCommands.ts`
- Modify: `web/src/app/useCommands.test.tsx`

- [ ] **Step 1: Write the failing test (append to `useCommands.test.tsx`)**

First add `vi` to the existing vitest import at the top of the file:
`import { describe, it, expect, beforeEach, vi } from "vitest";`

Then append (the file's wrapper helper is the lowercase `wrapper`):

```tsx
it("split-right command calls splitPane", () => {
  const spy = vi.spyOn(cairnStore.getState(), "splitPane");
  const { result } = renderHook(() => useCommands(), { wrapper });
  act(() => result.current.runCommand("split-right"));
  expect(spy).toHaveBeenCalled();
});

it("close-pane command calls closePane", () => {
  const spy = vi.spyOn(cairnStore.getState(), "closePane");
  const { result } = renderHook(() => useCommands(), { wrapper });
  act(() => result.current.runCommand("close-pane"));
  expect(spy).toHaveBeenCalled();
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && pnpm vitest run src/app/useCommands.test.tsx`
Expected: FAIL — unknown command ids (no dispatch).

- [ ] **Step 3: Add the command defs**

In `commands.ts`, add to `COMMAND_DEFS` (after `close-tab`):

```ts
  { id: "split-right", label: "Split editor right", defaultBinding: "Mod+\\" },
  { id: "close-pane", label: "Close pane", defaultBinding: "Mod+Shift+W" },
```

- [ ] **Step 4: Add the dispatch cases**

In `useCommands.ts`, inside the `switch (id)` block, add:

```ts
      case "split-right":
        st.splitPane();
        break;
      case "close-pane":
        st.closePane();
        break;
```

- [ ] **Step 5: Run to verify pass**

Run: `cd web && pnpm vitest run src/app/useCommands.test.tsx`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd web && pnpm format
git add web/src/components/shortcuts/commands.ts web/src/app/useCommands.ts web/src/app/useCommands.test.tsx
git commit -m "feat(shortcuts): split-right and close-pane commands"
```

---

## Task 9: Full gate, manual verification, PR

**Files:** none (verification + integration).

- [ ] **Step 1: Run the complete web gate**

Run: `cd web && pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build`
Expected: all PASS. Fix anything that fails before proceeding. (This is exactly what CI runs via `just web-ci`.)

- [ ] **Step 2: Run the full repo gate**

Run: `just ci`
Expected: web + rust gates PASS.

- [ ] **Step 3: Manual smoke test**

Run: `cd web && pnpm dev`, open the app, and verify:
- Open a note, click ⫷ in the strip → second pane appears with the same note, focused.
- Right-click a different note in the tree → "Open to the side" → it opens in the other pane.
- Click each pane → focus ring + URL (`/note/...`) follow the focused pane.
- Drag the divider → panes resize; reload the app → ratio + panes persist.
- ⊟ → collapses back to a single pane.
- ⌘K → "Split editor right" / "Close pane" present and working.

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin editor-split-panes
gh pr create --base main --title "feat(editor): side-by-side split panes" --body "$(cat <<'EOF'
## Summary
Adds a side-by-side, two-pane editor split. Each pane has independent tab state; note buffers are shared (edit once, reflected in both). Choosing the note for the second pane is explicit, via a new tree context menu ("Open to the side"); the strip's split icon duplicates the current note. Resizable + persisted divider.

Generalises the store's single tab group into a `panes` array + `activePane`, keeping a derived focused-pane mirror so routing/autosave/backlinks are unchanged. New pure `paneModel.ts` above the untouched `tabsModel.ts`.

Design: `docs/superpowers/specs/2026-06-12-editor-split-panes-design.md`
Plan: `docs/superpowers/plans/2026-06-12-editor-split-panes.md`

## Test plan
- Unit: paneModel, pane-aware persistence (incl. legacy migration), store split actions, Divider, TreeContextMenu, TabStrip actions, EditorPane split render, commands.
- `just ci` green; manual smoke test of split/open-to-side/focus/resize/persist/collapse.
EOF
)"
```

- [ ] **Step 5: Merge via the merge queue**

Per the brief and repo policy (branch protection, merge queue active): use "Merge when ready" on the PR. This task lands **after task B, before task D** so D rebases onto these shell changes.

---

## Self-review notes (addressed)

- **Spec coverage:** routing-untouched mirror (Task 3), pane array + N-way-ready model (Task 1/3), tree "Open to the side" + context menu w/ Rename+Delete (Task 7), split icon duplicates (Task 6 + store splitPane Task 4), focused click + ring (Task 5), resizable+persisted divider (Task 2/4/5), commands (Task 8), shared buffers (Task 3 `closeTab` guard), legacy persistence migration (Task 2). All present.
- **Type consistency:** `PaneState`/`PanesState`, `splitPane`/`closePane`/`focusPane`, `savePanes`/`loadPanes`, `selectTab(path, paneIndex?)`, `openNote(path, opts?)`, `openToSide`, `setSplitRatio` — names used consistently across store, components, and tests.
- **Open detail resolved during planning:** default keybindings `Mod+\\` (split right) and `Mod+Shift+W` (close pane); both user-rebindable via the existing registry. Flag in PR for review.
