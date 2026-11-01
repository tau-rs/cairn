# App.tsx Decomposition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decompose the 327-line `App.tsx` monolith into focused components and hooks that each subscribe only to the store slices they need, killing the whole-shell re-render storm — a behavior-preserving refactor.

**Architecture:** Move shared ephemeral UI state (dialog-open flags + keybinding overrides) into the Zustand store as a namespaced `ui` slice (Approach C). Extract six pane/host components (`TopBar`, `Sidebar`, `EditorPane`, `BacklinksPane`, `DialogHost`, `Toasts`) that each call `useCairn` for only their own slices, plus two hooks (`useCommands` for command list + dispatch, `useGlobalKeys` for the keydown effect). `App` becomes a thin coordinator. Tasks 1–10 are purely additive (the old `App` keeps working, e2e stays green); Task 11 swaps `App` to compose the new pieces in one atomic, mechanical change.

**Tech Stack:** React 18 + TypeScript, Zustand (vanilla store via `useStore`), react-router v7 (HashRouter), Vitest + React Testing Library, Playwright (e2e). Tailwind utility classes inline.

**Working directory:** all paths are under `web/`. Run commands from `web/`.

---

## File Structure

**New files:**
- `web/src/components/TopBar.tsx` — header: Logo, search, view toggle, settings, commit status
- `web/src/components/TopBar.test.tsx` — subscription-isolation pin
- `web/src/components/Sidebar.tsx` — left rail: folder tree + tags
- `web/src/components/EditorPane.tsx` — center: search results overlay + graph/editor switch
- `web/src/components/BacklinksPane.tsx` — right rail: backlinks
- `web/src/components/DialogHost.tsx` — settings/new-note/commit dialogs + command palette
- `web/src/components/Toasts.tsx` — error + notice toasts
- `web/src/components/shortcuts/useGlobalKeys.ts` — global keydown dispatch hook
- `web/src/components/shortcuts/useGlobalKeys.test.ts` — chord-dispatch pin
- `web/src/app/useCommands.ts` — command list + `runCommand` dispatcher hook
- `web/src/app/useCommands.test.tsx` — command-dispatch routing pin

**Modified files:**
- `web/src/app/routes.ts` — add `toggleViewTarget`
- `web/src/app/routes.test.ts` — test `toggleViewTarget`
- `web/src/store/store.ts` — add `ui` slice (`UiState`, `DEFAULT_UI`, `setUi`, `setKeybindingOverrides`, init seeding)
- `web/src/store/store.test.ts` — test the `ui` slice
- `web/src/app/App.tsx` — rewritten as a thin coordinator (Task 11)

---

## Task 1: `toggleViewTarget` route helper

The Graph/Editor toggle target (used by both the top-bar button and the
`toggle-view` command) currently lives inline in `App.tsx`. Extract it to
`routes.ts` so both consumers share one implementation.

**Files:**
- Modify: `web/src/app/routes.ts`
- Test: `web/src/app/routes.test.ts`

- [ ] **Step 1: Write the failing test** — append to `web/src/app/routes.test.ts`:

```ts
describe("toggleViewTarget", () => {
  it("from the graph, targets the active note", () => {
    expect(toggleViewTarget({ pathname: "/graph" }, "a.md")).toBe("/note/a.md");
  });
  it("from the graph with no active note, targets root", () => {
    expect(toggleViewTarget({ pathname: "/graph" }, null)).toBe("/");
  });
  it("from a note, targets the graph", () => {
    expect(toggleViewTarget({ pathname: "/note/a.md" }, "a.md")).toBe("/graph");
  });
});
```

Add `toggleViewTarget` to the existing import from `"./routes"` at the top of the file.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/app/routes.test.ts`
Expected: FAIL — `toggleViewTarget is not a function` / not exported.

- [ ] **Step 3: Add the helper** — append to `web/src/app/routes.ts`:

```ts
/**
 * Target URL for the Graph/Editor toggle: from the graph, back to the active
 * note (root if none); otherwise into the graph.
 */
export function toggleViewTarget(
  loc: RouteLocation,
  activePath: string | null,
): string {
  return isGraph(loc) ? (activePath ? noteUrl(activePath) : "/") : "/graph";
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/app/routes.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add web/src/app/routes.ts web/src/app/routes.test.ts
git commit -m "refactor(routes): extract toggleViewTarget helper"
```

---

## Task 2: Store `ui` slice

Add a namespaced `ui` slice to the store holding the dialog-open flags and the
keybinding overrides (migrated out of `App`'s `useState` + hand-rolled
persistence).

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing tests** — add to `web/src/store/store.test.ts`. First add this import near the top with the other imports:

```ts
import {
  loadOverrides,
  saveOverrides,
} from "../components/shortcuts/keybindingPersistence";
```

Then add this `describe` block:

```ts
describe("ui slice", () => {
  it("setUi patches ui flags without touching others", () => {
    const { store } = setup();
    store.getState().setUi({ commitOpen: true });
    expect(store.getState().ui.commitOpen).toBe(true);
    store.getState().setUi({ newNoteOpen: true, newNoteInitial: "folder/" });
    expect(store.getState().ui.newNoteOpen).toBe(true);
    expect(store.getState().ui.newNoteInitial).toBe("folder/");
    expect(store.getState().ui.commitOpen).toBe(true); // untouched
  });

  it("setKeybindingOverrides updates state and persists", () => {
    const { store } = setup();
    store.getState().setKeybindingOverrides({ "new-note": "Mod+Shift+N" });
    expect(store.getState().ui.keybindingOverrides).toEqual({
      "new-note": "Mod+Shift+N",
    });
    expect(loadOverrides()).toEqual({ "new-note": "Mod+Shift+N" });
  });

  it("init seeds keybindingOverrides from persistence", async () => {
    saveOverrides({ commit: null });
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().ui.keybindingOverrides).toEqual({ commit: null });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/store/store.test.ts -t "ui slice"`
Expected: FAIL — `setUi`/`setKeybindingOverrides`/`ui` not defined.

- [ ] **Step 3: Implement the slice** — in `web/src/store/store.ts`:

(a) Add imports near the existing imports:

```ts
import type { Overrides } from "../components/shortcuts/commands";
import {
  loadOverrides,
  saveOverrides,
} from "../components/shortcuts/keybindingPersistence";
```

(b) Add the `UiState` interface and default just above `export interface CairnState`:

```ts
export interface UiState {
  settingsOpen: boolean;
  newNoteOpen: boolean;
  newNoteInitial: string;
  commitOpen: boolean;
  paletteOpen: boolean;
  /** Per-command keybinding overrides (chord, or null = unbound). Persisted. */
  keybindingOverrides: Overrides;
}

export const DEFAULT_UI: UiState = {
  settingsOpen: false,
  newNoteOpen: false,
  newNoteInitial: "",
  commitOpen: false,
  paletteOpen: false,
  keybindingOverrides: {},
};
```

(c) In `interface CairnState`, add the field (next to `settings: Settings;`):

```ts
  ui: UiState;
```

and add the action signatures (next to `setSettings`):

```ts
  setUi(patch: Partial<UiState>): void;
  setKeybindingOverrides(overrides: Overrides): void;
```

(d) In the returned initial state object (next to `settings: DEFAULT_SETTINGS,`):

```ts
      ui: DEFAULT_UI,
```

(e) In `init()`, seed overrides immediately after `started = true;`:

```ts
        set((s) => ({
          ui: { ...s.ui, keybindingOverrides: loadOverrides() },
        }));
```

(f) Add the two actions (next to `setSettings`):

```ts
      setUi(patch) {
        set((s) => ({ ui: { ...s.ui, ...patch } }));
      },

      setKeybindingOverrides(overrides) {
        saveOverrides(overrides);
        set((s) => ({ ui: { ...s.ui, keybindingOverrides: overrides } }));
      },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/store/store.test.ts`
Expected: PASS (the new `ui slice` block and all existing store tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): add ui slice for dialog flags + keybinding overrides"
```

---

## Task 3: `useGlobalKeys` hook

Extract the global `keydown` effect (chord dispatch + built-in Ctrl+Tab / Mod+1-9
tab nav) into a hook. Named `useGlobalKeys`, not `KeyboardShortcuts`, because
`KeyboardShortcuts.tsx` already exists (the Settings rebind UI).

**Files:**
- Create: `web/src/components/shortcuts/useGlobalKeys.ts`
- Test: `web/src/components/shortcuts/useGlobalKeys.test.ts`

- [ ] **Step 1: Write the failing test** — create `web/src/components/shortcuts/useGlobalKeys.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { useGlobalKeys } from "./useGlobalKeys";
import { eventToChord } from "./keybinding";

describe("useGlobalKeys", () => {
  it("dispatches a mapped chord to runCommand", () => {
    const run = vi.fn();
    // Derive the canonical chord from the same event shape we dispatch, so the
    // test is independent of the platform's Mod-key mapping.
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "open-palette" }, run));
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    );
    expect(run).toHaveBeenCalledWith("open-palette");
  });

  it("ignores an unmapped chord", () => {
    const run = vi.fn();
    renderHook(() => useGlobalKeys({}, run));
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    );
    expect(run).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/shortcuts/useGlobalKeys.test.ts`
Expected: FAIL — module `./useGlobalKeys` not found.

- [ ] **Step 3: Implement the hook** — create `web/src/components/shortcuts/useGlobalKeys.ts`:

```ts
import { useEffect, useRef } from "react";
import { cairnStore } from "../../app/cairnStore";
import { eventToChord } from "./keybinding";

/**
 * Global keydown dispatch: maps a chord to a command id via `chordMap` and runs
 * it through `runCommand`, plus the built-in (non-rebindable) tab navigation
 * (Ctrl+Tab / Mod+1-9). `runCommand` is held in a ref so the window listener
 * binds once per `chordMap` change, not on every render.
 */
export function useGlobalKeys(
  chordMap: Record<string, string>,
  runCommand: (id: string) => void,
) {
  const runCommandRef = useRef(runCommand);
  runCommandRef.current = runCommand;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const chord = eventToChord(e);
      const id = chord ? chordMap[chord] : undefined;
      if (id) {
        e.preventDefault();
        runCommandRef.current(id);
        return;
      }
      // Built-in tab navigation (parameterized; not rebindable).
      const st = cairnStore.getState();
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        st.cycleTab(e.shiftKey ? -1 : 1);
      } else if ((e.metaKey || e.ctrlKey) && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        st.jumpToTab(Number(e.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chordMap]);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/shortcuts/useGlobalKeys.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shortcuts/useGlobalKeys.ts web/src/components/shortcuts/useGlobalKeys.test.ts
git commit -m "refactor(shortcuts): extract useGlobalKeys keydown hook"
```

---

## Task 4: `useCommands` hook

Extract the palette command list, the memoized `chordMap`, and the `runCommand`
dispatcher. Dialog-opening cases become `setUi(...)` store calls; nav cases keep
react-router's `navigate`/`location`. `editorMode`/`activePath` are read lazily
via `getState()` so the hook doesn't subscribe to them.

**Files:**
- Create: `web/src/app/useCommands.ts`
- Test: `web/src/app/useCommands.test.tsx`

- [ ] **Step 1: Write the failing test** — create `web/src/app/useCommands.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { useCommands } from "./useCommands";
import { cairnStore } from "./cairnStore";
import { DEFAULT_UI } from "../store/store";

const wrapper = ({ children }: { children: ReactNode }) => (
  <MemoryRouter>{children}</MemoryRouter>
);

beforeEach(() => {
  cairnStore.setState({ ui: { ...DEFAULT_UI } });
});

describe("useCommands runCommand", () => {
  it("opens the commit dialog", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("commit"));
    expect(cairnStore.getState().ui.commitOpen).toBe(true);
  });

  it("opens the new-note dialog with an empty initial path", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("new-note"));
    expect(cairnStore.getState().ui.newNoteOpen).toBe(true);
    expect(cairnStore.getState().ui.newNoteInitial).toBe("");
  });

  it("opens settings", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("open-settings"));
    expect(cairnStore.getState().ui.settingsOpen).toBe(true);
  });

  it("toggles the command palette", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    act(() => result.current.runCommand("open-palette"));
    expect(cairnStore.getState().ui.paletteOpen).toBe(true);
    act(() => result.current.runCommand("open-palette"));
    expect(cairnStore.getState().ui.paletteOpen).toBe(false);
  });

  it("exposes the built-in commands (minus open-palette) plus their hints", () => {
    const { result } = renderHook(() => useCommands(), { wrapper });
    const ids = result.current.commands.map((c) => c.id);
    expect(ids).toContain("commit");
    expect(ids).not.toContain("open-palette");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/app/useCommands.test.tsx`
Expected: FAIL — module `./useCommands` not found.

- [ ] **Step 3: Implement the hook** — create `web/src/app/useCommands.ts`:

```ts
import { useMemo } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { cairnStore, useCairn } from "./cairnStore";
import { noteUrl, toggleViewTarget } from "./routes";
import {
  COMMAND_DEFS,
  effectiveBinding,
  chordToId,
} from "../components/shortcuts/commands";
import { formatChord } from "../components/shortcuts/keybinding";
import {
  toPaletteCommands,
  parsePluginCommandId,
} from "../components/plugins/pluginCommands";
import type { PaletteCommand } from "../components/command-palette/CommandPalette";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /mac/i.test(navigator.platform || navigator.userAgent || "");

/**
 * Builds the command-palette command list, the chord→id map for global key
 * dispatch, and the `runCommand` dispatcher. Dialog-opening routes through the
 * store's `setUi`; navigation routes through react-router. State the dispatcher
 * only reads at call time (`editorMode`, `activePath`) is pulled lazily via
 * `getState()` so this hook subscribes to nothing high-frequency.
 */
export function useCommands(): {
  commands: PaletteCommand[];
  chordMap: Record<string, string>;
  runCommand: (id: string) => void;
} {
  const navigate = useNavigate();
  const location = useLocation();
  const overrides = useCairn((s) => s.ui.keybindingOverrides);
  const plugins = useCairn((s) => s.plugins);

  const chordMap = useMemo(() => chordToId(overrides), [overrides]);

  const commands = useMemo<PaletteCommand[]>(
    () => [
      ...COMMAND_DEFS.filter((d) => d.id !== "open-palette").map((d) => {
        const eff = effectiveBinding(d.id, overrides);
        return {
          id: d.id,
          label: d.label,
          hint: eff ? formatChord(eff, IS_MAC) : undefined,
        };
      }),
      ...toPaletteCommands(plugins),
    ],
    [overrides, plugins],
  );

  const runCommand = (id: string) => {
    const st = cairnStore.getState();
    const pluginCmd = parsePluginCommandId(id);
    if (pluginCmd) {
      void st.invokePlugin(pluginCmd.plugin, pluginCmd.command);
      st.setUi({ paletteOpen: false });
      return;
    }
    switch (id) {
      case "open-palette":
        st.setUi({ paletteOpen: !st.ui.paletteOpen });
        return;
      case "new-note":
        st.setUi({ newNoteInitial: "", newNoteOpen: true });
        break;
      case "commit":
        st.setUi({ commitOpen: true });
        break;
      case "close-tab":
        st.closeActiveTab();
        break;
      case "toggle-view":
        navigate(toggleViewTarget(location, st.activePath));
        break;
      case "open-settings":
        st.setUi({ settingsOpen: true });
        break;
      case "toggle-editor-mode":
        st.setSettings({
          editorMode:
            st.settings.editorMode === "livepreview" ? "source" : "livepreview",
        });
        break;
      case "nav-back":
        navigate(-1);
        break;
      case "nav-forward":
        navigate(1);
        break;
    }
    st.setUi({ paletteOpen: false });
  };

  return { commands, chordMap, runCommand };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/app/useCommands.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add web/src/app/useCommands.ts web/src/app/useCommands.test.tsx
git commit -m "refactor(app): extract useCommands hook (command list + dispatch)"
```

---

## Task 5: `TopBar` component

The header slot. Subscribes only to its own slices; reads `activePath` lazily in
the toggle handler so it doesn't re-render on note switches.

**Files:**
- Create: `web/src/components/TopBar.tsx`
- Test: `web/src/components/TopBar.test.tsx`

- [ ] **Step 1: Write the failing test** (subscription-isolation pin) — create `web/src/components/TopBar.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from "vitest";
import { Profiler, type ProfilerOnRenderCallback } from "react";
import { render, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { TopBar } from "./TopBar";
import { cairnStore } from "../app/cairnStore";

beforeEach(() => {
  cairnStore.setState({
    query: "",
    backlinks: [],
    saving: false,
    dirty: false,
    uncommitted: false,
    lastCommit: null,
    committing: false,
  });
});

function renderCounted() {
  let commits = 0;
  const onRender: ProfilerOnRenderCallback = () => {
    commits++;
  };
  render(
    <MemoryRouter>
      <Profiler id="topbar" onRender={onRender}>
        <TopBar />
      </Profiler>
    </MemoryRouter>,
  );
  return () => commits;
}

describe("TopBar subscription isolation", () => {
  it("does NOT re-render when an unrelated slice (backlinks) changes", () => {
    const commits = renderCounted();
    const before = commits();
    act(() => cairnStore.setState({ backlinks: ["x.md"] }));
    expect(commits()).toBe(before);
  });

  it("DOES re-render when its own slice (query) changes", () => {
    const commits = renderCounted();
    const before = commits();
    act(() => cairnStore.setState({ query: "hello" }));
    expect(commits()).toBe(before + 1);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/TopBar.test.tsx`
Expected: FAIL — module `./TopBar` not found.

- [ ] **Step 3: Implement the component** — create `web/src/components/TopBar.tsx`:

```tsx
import { useNavigate, useLocation } from "react-router-dom";
import { useCairn, cairnStore } from "../app/cairnStore";
import { isGraph, toggleViewTarget } from "../app/routes";
import { SearchBar } from "./SearchBar";
import { CommitBar } from "./CommitBar";
import { IconButton } from "./ui/IconButton";
import { Logo } from "./ui/Logo";
import { Button } from "./ui/Button";

export function TopBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = cairnStore.getState();
  const query = useCairn((s) => s.query);
  const saving = useCairn((s) => s.saving);
  const dirty = useCairn((s) => s.dirty);
  const uncommitted = useCairn((s) => s.uncommitted);
  const lastCommit = useCairn((s) => s.lastCommit);
  const committing = useCairn((s) => s.committing);
  const view = isGraph(location) ? "graph" : "editor";

  return (
    <div className="flex w-full items-center gap-3">
      <Logo />
      <span className="text-sm font-semibold text-text">Cairn</span>
      <SearchBar
        value={query}
        onChange={actions.setQuery}
        onSearch={actions.runSearch}
      />
      <Button
        variant="ghost"
        onClick={() =>
          navigate(toggleViewTarget(location, cairnStore.getState().activePath))
        }
      >
        {view === "graph" ? "Editor" : "Graph"}
      </Button>
      <span className="grow" />
      <IconButton
        label="Settings"
        onClick={() => actions.setUi({ settingsOpen: true })}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </IconButton>
      <CommitBar
        saving={saving}
        dirty={dirty}
        uncommitted={uncommitted}
        lastCommit={lastCommit}
        committing={committing}
        onRequestCommit={() => actions.setUi({ commitOpen: true })}
      />
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/components/TopBar.test.tsx`
Expected: PASS (both isolation and re-render cases).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/TopBar.tsx web/src/components/TopBar.test.tsx
git commit -m "refactor(app): extract TopBar component"
```

---

## Task 6: `Sidebar` component

The left-rail list slot: folder tree + tags panel.

**Files:**
- Create: `web/src/components/Sidebar.tsx`

- [ ] **Step 1: Implement the component** — create `web/src/components/Sidebar.tsx`:

```tsx
import { useNavigate } from "react-router-dom";
import { useCairn, cairnStore } from "../app/cairnStore";
import { noteUrl, tagUrl } from "../app/routes";
import { FolderTree } from "./tree/FolderTreeView";
import { TagsPanel } from "./tags/TagsPanel";

export function Sidebar() {
  const navigate = useNavigate();
  const actions = cairnStore.getState();
  const notePaths = useCairn((s) => s.notePaths);
  const activePath = useCairn((s) => s.activePath);
  const tags = useCairn((s) => s.tags);
  const activeTag = useCairn((s) => s.activeTag);

  return (
    <>
      <FolderTree
        paths={notePaths}
        activePath={activePath}
        onOpen={(p) => navigate(noteUrl(p))}
        onDelete={actions.deleteNote}
        onRequestNew={() =>
          actions.setUi({ newNoteInitial: "", newNoteOpen: true })
        }
        onRequestNewInFolder={(folder) =>
          actions.setUi({ newNoteInitial: folder + "/", newNoteOpen: true })
        }
        onApplyRenames={actions.applyRenames}
      />
      <TagsPanel
        tags={tags}
        activeTag={activeTag}
        onSelect={(t) => navigate(tagUrl(t))}
      />
    </>
  );
}
```

- [ ] **Step 2: Type-check it compiles**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/Sidebar.tsx
git commit -m "refactor(app): extract Sidebar component"
```

---

## Task 7: `EditorPane` component

The center slot: error boundary → search-results overlay + graph/editor switch.

**Files:**
- Create: `web/src/components/EditorPane.tsx`

- [ ] **Step 1: Implement the component** — create `web/src/components/EditorPane.tsx`:

```tsx
import { useNavigate, useLocation } from "react-router-dom";
import { useCairn, cairnStore } from "../app/cairnStore";
import { noteUrl, isGraph, tagFromLocation } from "../app/routes";
import { GraphView } from "./GraphView";
import { Editor } from "./Editor";
import { TabStrip } from "./tabs/TabStrip";
import { SearchResults } from "./SearchResults";
import { ErrorBoundary } from "./ErrorBoundary";
import { Button } from "./ui/Button";

export function EditorPane() {
  const navigate = useNavigate();
  const location = useLocation();
  const actions = cairnStore.getState();
  const notePaths = useCairn((s) => s.notePaths);
  const activePath = useCairn((s) => s.activePath);
  const activeContents = useCairn((s) => s.activeContents);
  const editorMode = useCairn((s) => s.settings.editorMode);
  const loadRemoteImages = useCairn((s) => s.settings.loadRemoteImages);
  const searchResults = useCairn((s) => s.searchResults);
  const searchSnippets = useCairn((s) => s.searchSnippets);
  const activeTag = useCairn((s) => s.activeTag);
  const graph = useCairn((s) => s.graph);
  const noteTags = useCairn((s) => s.noteTags);
  const tabs = useCairn((s) => s.tabs);
  const openNotes = useCairn((s) => s.openNotes);
  const view = isGraph(location) ? "graph" : "editor";

  const tabViews = tabs.map((t) => ({
    path: t.path,
    preview: t.preview,
    dirty: openNotes[t.path]?.dirty ?? false,
  }));

  return (
    // Retry clears the boundary so the pane re-renders; it recovers from
    // transient throws. If the cause is intrinsic to the open note (e.g. a
    // decoration-builder bug on its content), the crash recurs until the
    // user navigates away — still better than blanking the whole app.
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
          snippets={searchSnippets ?? undefined}
          title={activeTag ? `Tagged · ${activeTag}` : undefined}
          onOpen={(p) => navigate(noteUrl(p))}
          onClose={() => {
            // A tag filter is URL-owned (we're on /tags/:tag), so dismiss it by
            // navigating away; RouteSync then clears the overlay. A plain text
            // search is a store-only overlay with no route, so close it in the
            // store directly.
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
            onOpenNote={(p) => navigate(noteUrl(p))}
          />
        ) : (
          <div className="flex h-full flex-col">
            <TabStrip
              tabs={tabViews}
              activePath={activePath}
              onSelect={(p) => navigate(noteUrl(p))}
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
                loadRemoteImages={loadRemoteImages}
                onChange={actions.editBuffer}
                onOpenNote={(p) => navigate(noteUrl(p))}
                onToggleMode={() =>
                  actions.setSettings({
                    editorMode:
                      editorMode === "livepreview" ? "source" : "livepreview",
                  })
                }
              />
            </div>
          </div>
        )}
      </div>
    </ErrorBoundary>
  );
}
```

- [ ] **Step 2: Type-check it compiles**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/EditorPane.tsx
git commit -m "refactor(app): extract EditorPane component"
```

---

## Task 8: `BacklinksPane` component

The right-rail slot.

**Files:**
- Create: `web/src/components/BacklinksPane.tsx`

- [ ] **Step 1: Implement the component** — create `web/src/components/BacklinksPane.tsx`:

```tsx
import { useNavigate } from "react-router-dom";
import { useCairn } from "../app/cairnStore";
import { noteUrl } from "../app/routes";
import { Backlinks } from "./Backlinks";

export function BacklinksPane() {
  const navigate = useNavigate();
  const backlinks = useCairn((s) => s.backlinks);
  return <Backlinks paths={backlinks} onOpen={(p) => navigate(noteUrl(p))} />;
}
```

- [ ] **Step 2: Type-check it compiles**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/BacklinksPane.tsx
git commit -m "refactor(app): extract BacklinksPane component"
```

---

## Task 9: `DialogHost` component

The three dialogs + command palette. Subscribes to the `ui` slice and the data
each dialog needs; receives `commands`/`onRunCommand` as props (built by
`useCommands` in `App`).

**Files:**
- Create: `web/src/components/DialogHost.tsx`

- [ ] **Step 1: Implement the component** — create `web/src/components/DialogHost.tsx`:

```tsx
import { useNavigate } from "react-router-dom";
import { useCairn, cairnStore } from "../app/cairnStore";
import { noteUrl } from "../app/routes";
import { SettingsDialog } from "./SettingsDialog";
import { NewNoteDialog } from "./NewNoteDialog";
import { CommitDialog } from "./CommitDialog";
import {
  CommandPalette,
  type PaletteCommand,
} from "./command-palette/CommandPalette";

export function DialogHost(props: {
  commands: PaletteCommand[];
  onRunCommand: (id: string) => void;
}) {
  const navigate = useNavigate();
  const actions = cairnStore.getState();
  const ui = useCairn((s) => s.ui);
  const settings = useCairn((s) => s.settings);
  const plugins = useCairn((s) => s.plugins);
  const committing = useCairn((s) => s.committing);
  const notePaths = useCairn((s) => s.notePaths);

  return (
    <>
      <SettingsDialog
        open={ui.settingsOpen}
        onOpenChange={(o) => actions.setUi({ settingsOpen: o })}
        settings={settings}
        onChange={actions.setSettings}
        keybindingOverrides={ui.keybindingOverrides}
        onKeybindingsChange={actions.setKeybindingOverrides}
        plugins={plugins}
      />
      <NewNoteDialog
        open={ui.newNoteOpen}
        onOpenChange={(o) => actions.setUi({ newNoteOpen: o })}
        initialPath={ui.newNoteInitial}
        onCreate={actions.createNote}
      />
      <CommitDialog
        open={ui.commitOpen}
        onOpenChange={(o) => actions.setUi({ commitOpen: o })}
        committing={committing}
        onCommit={actions.commitManual}
      />
      <CommandPalette
        open={ui.paletteOpen}
        onClose={() => actions.setUi({ paletteOpen: false })}
        commands={props.commands}
        notes={notePaths}
        onRunCommand={props.onRunCommand}
        onOpenNote={(p) => {
          navigate(noteUrl(p));
          actions.setUi({ paletteOpen: false });
        }}
      />
    </>
  );
}
```

- [ ] **Step 2: Type-check it compiles**

Run: `npx tsc --noEmit`
Expected: no errors. (Confirms `setKeybindingOverrides` matches `SettingsDialog`'s `onKeybindingsChange: (o: Overrides) => void`.)

- [ ] **Step 3: Commit**

```bash
git add web/src/components/DialogHost.tsx
git commit -m "refactor(app): extract DialogHost component"
```

---

## Task 10: `Toasts` component

The error + notice toasts.

**Files:**
- Create: `web/src/components/Toasts.tsx`

- [ ] **Step 1: Implement the component** — create `web/src/components/Toasts.tsx`:

```tsx
import { useCairn, cairnStore } from "../app/cairnStore";
import { ErrorToast } from "./ErrorToast";
import { NoticeToast } from "./NoticeToast";

export function Toasts() {
  const actions = cairnStore.getState();
  const error = useCairn((s) => s.error);
  const notice = useCairn((s) => s.notice);
  return (
    <>
      <ErrorToast message={error} onDismiss={actions.dismissError} />
      <NoticeToast message={notice} onDismiss={actions.dismissNotice} />
    </>
  );
}
```

- [ ] **Step 2: Type-check it compiles**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/Toasts.tsx
git commit -m "refactor(app): extract Toasts component"
```

---

## Task 11: Rewrite `App.tsx` as a coordinator + full verification

Swap `App` to compose the new pieces. This deletes all the inline JSX, the
`runCommand`/keydown logic, the dialog `useState`, and the ~20 slice
subscriptions in one atomic, mechanical change. The new unit pins + e2e baseline
guard behavior.

**Files:**
- Modify: `web/src/app/App.tsx` (full rewrite)

- [ ] **Step 1: Replace the entire contents of `web/src/app/App.tsx`** with:

```tsx
import { useEffect } from "react";
import { Shell } from "../components/Shell";
import { OpenCairn } from "../components/OpenCairn";
import { RouteSync } from "./RouteSync";
import { cairnStore, useCairn } from "./cairnStore";
import { TopBar } from "../components/TopBar";
import { Sidebar } from "../components/Sidebar";
import { EditorPane } from "../components/EditorPane";
import { BacklinksPane } from "../components/BacklinksPane";
import { DialogHost } from "../components/DialogHost";
import { Toasts } from "../components/Toasts";
import { useCommands } from "./useCommands";
import { useGlobalKeys } from "../components/shortcuts/useGlobalKeys";

export default function App() {
  useEffect(() => {
    void cairnStore.getState().init();
  }, []);

  const cairnPath = useCairn((s) => s.cairnPath);
  const { commands, chordMap, runCommand } = useCommands();
  useGlobalKeys(chordMap, runCommand);

  if (cairnPath === null) {
    return <OpenCairn onOpen={() => void cairnStore.getState().openCairn()} />;
  }

  return (
    <>
      <RouteSync />
      <Shell
        topBar={<TopBar />}
        list={<Sidebar />}
        editor={<EditorPane />}
        backlinks={<BacklinksPane />}
      />
      <DialogHost commands={commands} onRunCommand={runCommand} />
      <Toasts />
    </>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `npx tsc --noEmit`
Expected: no errors, and no "unused import/variable" complaints (the old imports are gone).

- [ ] **Step 3: Run the full unit suite**

Run: `npx vitest run`
Expected: PASS — all existing tests plus the new `ui slice`, `useGlobalKeys`, `useCommands`, and `TopBar` tests.

- [ ] **Step 4: Lint + format gate** (the local gate that CI enforces — easy to miss)

Run: `npm run lint && npx prettier --check src`
Expected: no errors. If prettier reports files, run `npx prettier --write src` and re-check, then amend.

- [ ] **Step 5: Production build**

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 6: E2E baseline**

Run: `npx playwright test`
Expected: PASS — search, command palette ⌘K, rebind+persist, dialogs, tabs, graph toggle, commit all green (behavior preserved).

- [ ] **Step 7: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "refactor(app): compose decomposed components in thin App coordinator

App.tsx drops from ~327 lines and ~20 store subscriptions to a coordinator that
delegates to TopBar/Sidebar/EditorPane/BacklinksPane/DialogHost/Toasts and the
useCommands/useGlobalKeys hooks. Behavior-preserving (D5)."
```

---

## Notes for the implementer

- **Behavior-preserving only.** No feature changes. If a step seems to change
  behavior, stop — the extraction is meant to be a faithful move of existing code.
- **Out of scope (follow-ups, do NOT do here):** D6 (capture `actions` once at
  module scope) and U4 (global-keydown focus-check so shortcuts don't fire while
  typing in inputs). Leave them; note as follow-ups in the PR.
- **Coordination:** sessions 63 (dead-plugin) and 65 (loading-states/keyboard)
  also edit `App.tsx` / the keydown handler. The `useGlobalKeys` extraction will
  conflict with 65 — expect a rebase.
- **`actions = cairnStore.getState()` at render** is the existing idiom (store
  actions are stable for the store's lifetime; they read fresh state via `get()`),
  so capturing them per-render is safe and matches the original `App`.
- **Run vitest with `npx vitest run`** (single run, not watch) so steps terminate.
