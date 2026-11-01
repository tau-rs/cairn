# A11y & Loading States Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make tabs keyboard-operable, give a keyboard path for tree note-moves, add per-area loading states that render distinctly from empty, and enable the jsx-a11y/react ESLint rules that catch these gaps.

**Architecture:** (U1) Keep `<div role="tab">` (it wraps a close `<button>`, so it can't itself be a button) but add roving `tabIndex` + arrow/Enter/Space key handling per the WAI-ARIA tabs pattern; add F2-to-rename + rename-to-path move on tree rows. (U2) Add a `loading` object to the store (`search`/`graph`/`backlinks`/`note`), set around each async call (token-guarded clears so a superseded request never clears a newer one's flag), and render a shared `Spinner` in the three named consumers plus an editor-pane overlay. (DX1) Add `eslint-plugin-jsx-a11y` + `eslint-plugin-react` to the flat config; fix only the violations they surface in touched files.

**Tech Stack:** React 19 + TypeScript, Zustand vanilla store, Vitest + Testing Library, ESLint 10 flat config, Tailwind.

**Note on the brief:** it cites `web/.eslintrc.cjs`, but the repo has migrated to flat config (`web/eslint.config.js`). Apply DX1 there.

---

## File Structure

- `web/src/components/ui/Spinner.tsx` — **new** shared accessible spinner (`role="status"`).
- `web/src/components/tabs/TabStrip.tsx` — roving tabindex + keyboard handlers.
- `web/src/components/tree/treeMoves.ts` — new `planRenameNotePath` (rename-or-move via typed path).
- `web/src/components/tree/FolderTreeView.tsx` — F2-to-rename on rows; note rename uses `planRenameNotePath`.
- `web/src/store/store.ts` — `loading` state + per-area set/clear.
- `web/src/components/Backlinks.tsx`, `web/src/components/SearchResults.tsx`, `web/src/components/GraphView.tsx` — render loading distinctly from empty.
- `web/src/app/App.tsx` — pass `loading.*` to consumers; editor-pane note-loading overlay. (No keydown changes — avoids colliding with sessions 63/64.)
- `web/eslint.config.js` — jsx-a11y + react plugins.
- Tests alongside each.

---

## Task 1: Accessible Spinner

**Files:**
- Create: `web/src/components/ui/Spinner.tsx`
- Test: `web/src/components/ui/Spinner.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Spinner } from "./Spinner";

describe("Spinner", () => {
  it("exposes a status role with an accessible label", () => {
    render(<Spinner label="Searching" />);
    expect(screen.getByRole("status", { name: "Searching" })).toBeInTheDocument();
  });
  it("defaults the label to Loading", () => {
    render(<Spinner />);
    expect(screen.getByRole("status", { name: "Loading" })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify it fails** — `pnpm test -- Spinner` → FAIL (module not found).

- [ ] **Step 3: Implement**

```tsx
/** Small inline spinner. `role="status"` + a label so screen readers announce it. */
export function Spinner(props: { label?: string; className?: string }) {
  return (
    <span
      role="status"
      aria-label={props.label ?? "Loading"}
      className={
        "inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent " +
        (props.className ?? "")
      }
    />
  );
}
```

- [ ] **Step 4: Run** — `pnpm test -- Spinner` → PASS.
- [ ] **Step 5: Commit** — `feat(ui): accessible Spinner`.

---

## Task 2: Keyboard-operable tabs (U1)

**Files:**
- Modify: `web/src/components/tabs/TabStrip.tsx`
- Test: `web/src/components/tabs/TabStrip.test.tsx`

- [ ] **Step 1: Add failing tests** (append cases)

```tsx
it("makes the active tab focusable and others not (roving tabindex)", () => {
  setup();
  expect(screen.getByRole("tab", { name: /a$/ })).toHaveAttribute("tabindex", "0");
  expect(screen.getByRole("tab", { name: /ideas/ })).toHaveAttribute("tabindex", "-1");
});
it("activates a tab on Enter", () => {
  const props = setup();
  fireEvent.keyDown(screen.getByRole("tab", { name: /ideas/ }), { key: "Enter" });
  expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
});
it("activates a tab on Space", () => {
  const props = setup();
  fireEvent.keyDown(screen.getByRole("tab", { name: /ideas/ }), { key: " " });
  expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
});
it("moves selection to the next tab on ArrowRight (wrapping)", () => {
  const props = setup(); // active = a.md (index 0)
  fireEvent.keyDown(screen.getByRole("tab", { name: /a$/ }), { key: "ArrowRight" });
  expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
});
it("moves selection to the previous tab on ArrowLeft (wrapping to last)", () => {
  const props = setup(); // active = a.md (index 0) -> wraps to ideas.md
  fireEvent.keyDown(screen.getByRole("tab", { name: /a$/ }), { key: "ArrowLeft" });
  expect(props.onSelect).toHaveBeenCalledWith("ideas.md");
});
```

- [ ] **Step 2: Run** — `pnpm test -- TabStrip` → FAIL (no tabindex / handlers).

- [ ] **Step 3: Implement.** Add `useRef`; store tab element refs; roving tabindex; keyboard handler with automatic activation.

Replace the imports/signature top and the mapped `<div role="tab">` with:

```tsx
import { useRef } from "react";
import { stem } from "../../client/wikilink";
```

Inside the component body, before `return`:

```tsx
const tabRefs = useRef<(HTMLDivElement | null)[]>([]);

const onTabKeyDown = (e: React.KeyboardEvent, index: number) => {
  const tabs = props.tabs;
  const focusAt = (i: number) => {
    tabRefs.current[i]?.focus();
    props.onSelect(tabs[i].path); // automatic activation (WAI-ARIA tabs)
  };
  switch (e.key) {
    case "ArrowRight":
      e.preventDefault();
      focusAt((index + 1) % tabs.length);
      break;
    case "ArrowLeft":
      e.preventDefault();
      focusAt((index - 1 + tabs.length) % tabs.length);
      break;
    case "Home":
      e.preventDefault();
      focusAt(0);
      break;
    case "End":
      e.preventDefault();
      focusAt(tabs.length - 1);
      break;
    case "Enter":
    case " ":
      e.preventDefault();
      props.onSelect(tabs[index].path);
      break;
  }
};
```

Change the `props.tabs.map((t) => {` to `props.tabs.map((t, index) => {` and the tab `<div>` to:

```tsx
<div
  key={t.path}
  ref={(el) => {
    tabRefs.current[index] = el;
  }}
  role="tab"
  tabIndex={active ? 0 : -1}
  aria-selected={active}
  aria-label={label}
  title={t.path}
  onClick={() => props.onSelect(t.path)}
  onDoubleClick={() => props.onPin(t.path)}
  onKeyDown={(e) => onTabKeyDown(e, index)}
  className={ /* unchanged */ }
>
```

(Leave the inner close `<button>` and everything else unchanged.)

- [ ] **Step 4: Run** — `pnpm test -- TabStrip` → PASS (all, incl. existing).
- [ ] **Step 5: Commit** — `feat(a11y): keyboard-operable tabs (U1)`.

---

## Task 3: Keyboard tree-move planner (U1)

**Files:**
- Modify: `web/src/components/tree/treeMoves.ts`
- Test: `web/src/components/tree/treeMoves.test.ts` (append)

- [ ] **Step 1: Add failing tests**

```tsx
describe("planRenameNotePath", () => {
  it("renames within the folder when no slash is given", () => {
    expect(planRenameNotePath("dir/a.md", "b")).toEqual([{ from: "dir/a.md", to: "dir/b.md" }]);
  });
  it("moves the note when a path with slashes is given", () => {
    expect(planRenameNotePath("a.md", "dir/sub/a")).toEqual([{ from: "a.md", to: "dir/sub/a.md" }]);
  });
  it("strips a trailing .md and a leading slash", () => {
    expect(planRenameNotePath("a.md", "/dir/a.md")).toEqual([{ from: "a.md", to: "dir/a.md" }]);
  });
  it("returns [] for an empty input or a no-op", () => {
    expect(planRenameNotePath("a.md", "  ")).toEqual([]);
    expect(planRenameNotePath("dir/a.md", "dir/a")).toEqual([]);
  });
});
```

(Add `planRenameNotePath` to the existing import in the test file.)

- [ ] **Step 2: Run** — `pnpm test -- treeMoves` → FAIL (not exported).

- [ ] **Step 3: Implement** — append to `treeMoves.ts`:

```ts
/** Rename or move a note via a typed relative path. A bare name (no slash)
 *  renames within the current folder (delegates to planRenameNote); a path with
 *  slashes re-homes the note to that folder. Trailing `.md` and a leading `/`
 *  are stripped. [] when empty or a no-op. */
export function planRenameNotePath(notePath: string, input: string): Rename[] {
  const trimmed = input
    .trim()
    .replace(/^\/+/, "")
    .replace(/\.md$/, "");
  if (!trimmed) return [];
  if (!trimmed.includes("/")) return planRenameNote(notePath, trimmed);
  const to = `${trimmed}.md`;
  return to === notePath ? [] : [{ from: notePath, to }];
}
```

- [ ] **Step 4: Run** — `pnpm test -- treeMoves` → PASS.
- [ ] **Step 5: Commit** — `feat(a11y): rename-to-path move planner (U1)`.

---

## Task 4: Wire F2 rename + rename-to-path into the tree (U1)

**Files:**
- Modify: `web/src/components/tree/FolderTreeView.tsx`
- Test: `web/src/components/tree/FolderTreeView.test.tsx` (create if absent; otherwise append)

- [ ] **Step 1: Add failing tests** — F2 on a focused note row opens the rename input; committing a slashed value applies a move.

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FolderTree } from "./FolderTreeView";

function tree(over = {}) {
  const props = {
    paths: ["a.md", "dir/b.md"],
    activePath: null,
    onOpen: vi.fn(),
    onDelete: vi.fn(),
    onRequestNew: vi.fn(),
    onRequestNewInFolder: vi.fn(),
    onApplyRenames: vi.fn(),
    ...over,
  };
  render(<FolderTree {...props} />);
  return props;
}

it("opens rename on F2 and moves a note when a slashed path is committed", async () => {
  const props = tree();
  const row = screen.getByRole("button", { name: "a.md" });
  fireEvent.keyDown(row, { key: "F2" });
  const input = screen.getByDisplayValue("a");
  await userEvent.clear(input);
  await userEvent.type(input, "dir/a{Enter}");
  expect(props.onApplyRenames).toHaveBeenCalledWith([{ from: "a.md", to: "dir/a.md" }]);
});
```

- [ ] **Step 2: Run** — `pnpm test -- FolderTreeView` → FAIL.

- [ ] **Step 3: Implement.**
  1. Import the new planner: add `planRenameNotePath` to the `treeMoves` import.
  2. In `commitRename`, change the note branch from `planRenameNote(node.path, newName)` to `planRenameNotePath(node.path, newName)`.
  3. Add an `onKeyDown` to BOTH the folder open/toggle `<button>` (the one with `onClick={() => toggle(node.path)}`) and the note `<button>` (the one with `onClick={() => props.onOpen(node.path)}`):

```tsx
onKeyDown={(e) => {
  if (e.key === "F2") {
    e.preventDefault();
    setEditingPath(node.path);
  }
}}
```

- [ ] **Step 4: Run** — `pnpm test -- FolderTreeView treeMoves` → PASS.
- [ ] **Step 5: Commit** — `feat(a11y): F2 rename + keyboard note-move in tree (U1)`.

---

## Task 5: Store loading state (U2)

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts` (append)

- [ ] **Step 1: Add failing tests** (real timers; deferred mock to hold a request in-flight).

```tsx
it("exposes a loading.search flag while a search is in flight", async () => {
  vi.useRealTimers();
  const { client, store } = setup();
  await store.getState().init();
  let resolve!: (v: unknown) => void;
  vi.spyOn(client, "runQuery").mockReturnValueOnce(
    new Promise((r) => (resolve = r)),
  );
  const p = store.getState().runSearch("x");
  expect(store.getState().loading.search).toBe(true);
  resolve({ type: "search_results", results: [] });
  await p;
  expect(store.getState().loading.search).toBe(false);
});

it("sets loading.note while a note's contents load", async () => {
  vi.useRealTimers();
  const { client, store } = setup();
  await store.getState().init();
  let resolve!: (v: unknown) => void;
  vi.spyOn(client, "runQuery").mockReturnValueOnce(
    new Promise((r) => (resolve = r)),
  );
  const p = store.getState().openNote("a.md");
  expect(store.getState().loading.note).toBe(true);
  resolve({ type: "note", contents: "hi" });
  await p;
  expect(store.getState().loading.note).toBe(false);
});

it("starts with all loading flags false", () => {
  const { store } = setup();
  expect(store.getState().loading).toEqual({
    search: false, graph: false, backlinks: false, note: false,
  });
});
```

- [ ] **Step 2: Run** — `pnpm test -- store` → FAIL (`loading` undefined).

- [ ] **Step 3: Implement.**
  1. In `CairnState` add (after `error: string | null;`):

```ts
loading: { search: boolean; graph: boolean; backlinks: boolean; note: boolean };
```

  2. In the returned initial-state object add (after `error: null,`):

```ts
loading: { search: false, graph: false, backlinks: false, note: false },
```

  3. Add a helper near `dropNote`:

```ts
const setLoading = (key: keyof CairnState["loading"], value: boolean) =>
  set((s) => ({ loading: { ...s.loading, [key]: value } }));
```

  4. `runSearch` — set on entry, clear in a token-guarded `finally`:

```ts
async runSearch(query) {
  const token = ++seq.results;
  setLoading("search", true);
  try {
    const res = await client.runQuery({ type: "search", query });
    if (token !== seq.results) return;
    if (res.type === "search_results") {
      set({ /* unchanged body */ });
    }
  } catch (err) {
    if (token !== seq.results) return;
    set({ error: errMsg(err) });
  } finally {
    if (token === seq.results) setLoading("search", false);
  }
},
```

  5. `filterByTag` — identical wrapping (shares the `search` flag, since it shares `seq.results` and writes the same overlay): `setLoading("search", true)` on entry; `finally { if (token === seq.results) setLoading("search", false); }`.

  6. `loadGraph` — `setLoading("graph", true)` on entry; wrap the two try blocks so the `finally` clears once with a token guard:

```ts
async loadGraph() {
  const token = ++seq.graph;
  setLoading("graph", true);
  try {
    try {
      const res = await client.runQuery({ type: "get_graph" });
      if (token !== seq.graph) return;
      if (res.type === "graph") set({ graph: { nodes: res.nodes, edges: res.edges } });
    } catch (err) {
      if (token !== seq.graph) return;
      set({ error: errMsg(err) });
    }
    try {
      const tags = await client.noteTags();
      if (token !== seq.graph) return;
      set({ noteTags: tags });
    } catch {
      /* keep stale noteTags */
    }
  } finally {
    if (token === seq.graph) setLoading("graph", false);
  }
},
```

  7. `refreshBacklinks` — handle the no-path early return and the token-guarded `finally`:

```ts
async refreshBacklinks() {
  const path = get().activePath;
  if (!path) {
    setLoading("backlinks", false);
    return set({ backlinks: [] });
  }
  const token = ++seq.backlinks;
  setLoading("backlinks", true);
  try {
    const res = await client.runQuery({ type: "get_backlinks", path });
    if (token !== seq.backlinks) return;
    if (res.type === "paths") set({ backlinks: res.paths });
  } catch (err) {
    if (token !== seq.backlinks) return;
    set({ error: errMsg(err) });
  } finally {
    if (token === seq.backlinks) setLoading("backlinks", false);
  }
},
```

  8. `openNote` — scope `note` loading to the contents fetch only:

```ts
async openNote(path) {
  try {
    if (!get().openNotes[path]) {
      setLoading("note", true);
      try {
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type !== "note") return;
        set((s) => ({
          openNotes: {
            ...s.openNotes,
            [path]: { contents: res.contents, dirty: false, saving: false },
          },
        }));
      } finally {
        setLoading("note", false);
      }
    }
    applyTabs(openOrPreview(tabsState(), path));
    persist();
    await get().refreshBacklinks();
  } catch (err) {
    set({ error: errMsg(err) });
  }
},
```

  9. `openCairn` reset — add `loading: { search: false, graph: false, backlinks: false, note: false },` to its `set({...})`.

- [ ] **Step 4: Run** — `pnpm test -- store` → PASS (all, incl. existing).
- [ ] **Step 5: Commit** — `feat(store): per-area loading flags (U2)`.

---

## Task 6: Render loading in Backlinks (U2)

**Files:**
- Modify: `web/src/components/Backlinks.tsx`
- Test: `web/src/components/Backlinks.test.tsx` (append)

- [ ] **Step 1: Add failing test**

```tsx
it("shows a loading state distinct from the empty state", () => {
  render(<Backlinks paths={[]} loading onOpen={vi.fn()} />);
  expect(screen.getByRole("status", { name: /loading/i })).toBeInTheDocument();
  expect(screen.queryByText(/no backlinks/i)).toBeNull();
});
```

- [ ] **Step 2: Run** — FAIL.

- [ ] **Step 3: Implement** — add `loading?: boolean` to props, import `Spinner`, branch first on loading:

```tsx
import { SectionLabel } from "./ui/SectionLabel";
import { Spinner } from "./ui/Spinner";

export function Backlinks(props: {
  paths: string[];
  loading?: boolean;
  onOpen: (path: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <span className="mb-1">
        <SectionLabel>Backlinks</SectionLabel>
      </span>
      {props.loading ? (
        <span className="flex items-center gap-2 text-faint">
          <Spinner label="Loading backlinks" /> Loading…
        </span>
      ) : props.paths.length === 0 ? (
        <span className="text-faint">No backlinks</span>
      ) : (
        props.paths.map((path) => ( /* unchanged button */ ))
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `feat(a11y): loading state in Backlinks (U2)`.

---

## Task 7: Render loading in SearchResults (U2)

**Files:**
- Modify: `web/src/components/SearchResults.tsx`
- Test: `web/src/components/SearchResults.test.tsx` (append)

- [ ] **Step 1: Add failing tests**

```tsx
it("renders a searching state when loading with no results yet", () => {
  render(<SearchResults results={null} loading onOpen={vi.fn()} onClose={vi.fn()} />);
  expect(screen.getByRole("status", { name: /searching/i })).toBeInTheDocument();
});
it("still renders nothing when not loading and results is null", () => {
  const { container } = render(
    <SearchResults results={null} onOpen={vi.fn()} onClose={vi.fn()} />,
  );
  expect(container).toBeEmptyDOMElement();
});
```

- [ ] **Step 2: Run** — FAIL.

- [ ] **Step 3: Implement** — add `loading?: boolean`; show the overlay while loading even with `results === null`; show a searching spinner when there are no results yet.

```tsx
import { IconButton } from "./ui/IconButton";
import { SectionLabel } from "./ui/SectionLabel";
import { Spinner } from "./ui/Spinner";
import { splitSnippet, type SearchSnippet } from "./searchHighlight";

export function SearchResults(props: {
  results: string[] | null;
  loading?: boolean;
  onOpen: (path: string) => void;
  onClose: () => void;
  title?: string;
  snippets?: Record<string, SearchSnippet>;
}) {
  if (props.results === null && !props.loading) return null;
  const results = props.results ?? [];
  const showSpinner = props.loading && results.length === 0;
  return (
    <div
      data-testid="search-results"
      className="absolute left-2 top-12 z-10 flex max-h-[60vh] w-72 flex-col rounded border border-border bg-surface p-2 shadow-lg"
    >
      <div className="mb-1 flex items-center justify-between">
        <SectionLabel>
          {props.title ?? "Results"}
          {props.results ? ` (${results.length})` : ""}
        </SectionLabel>
        <IconButton label="close" onClick={props.onClose}>
          ✕
        </IconButton>
      </div>
      {showSpinner ? (
        <span className="flex items-center gap-2 text-sm text-faint">
          <Spinner label="Searching" /> Searching…
        </span>
      ) : results.length === 0 ? (
        <span className="text-sm text-faint">No matches</span>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          {results.map((path) => { /* unchanged */ })}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run** — `pnpm test -- SearchResults` → PASS (all, incl. existing).
- [ ] **Step 5: Commit** — `feat(a11y): searching state in SearchResults (U2)`.

---

## Task 8: Render loading in GraphView + editor-pane note overlay (U2)

**Files:**
- Modify: `web/src/components/GraphView.tsx`, `web/src/app/App.tsx`
- Test: covered by the store + consumer unit tests; GraphView overlay verified by build/lint (it embeds a canvas lib that is awkward to unit-test — note this).

- [ ] **Step 1: GraphView** — add `loading?: boolean` to props; render a centered overlay when loading. Inside the outer `containerRef` div, after the controls, add:

```tsx
{props.loading && (
  <div className="absolute inset-0 z-20 flex items-center justify-center bg-bg/50">
    <Spinner label="Loading graph" />
  </div>
)}
```

Add `import { Spinner } from "./ui/Spinner";` near the other imports.

- [ ] **Step 2: App wiring** — add a `loading` selector and pass it down:

```tsx
const loading = useCairn((s) => s.loading);
```

  - `<SearchResults ... loading={loading.search} />`
  - `<GraphView ... loading={loading.graph} />`
  - `<Backlinks paths={backlinks} loading={loading.backlinks} onOpen={...} />`
  - Wrap the editor `<Editor/>` container with a relative overlay:

```tsx
<div className="relative min-h-0 flex-1">
  {loading.note && (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-bg/50">
      <Spinner label="Loading note" />
    </div>
  )}
  <Editor ... />
</div>
```

  Add `import { Spinner } from "../components/ui/Spinner";`.

- [ ] **Step 3: Run** — `pnpm test` → all green; `pnpm build` → success.
- [ ] **Step 4: Commit** — `feat(a11y): loading overlays for graph + note (U2)`.

---

## Task 9: ESLint jsx-a11y + react rules (DX1)

**Files:**
- Modify: `web/eslint.config.js`, `web/package.json` (deps)

- [ ] **Step 1: Capture the rule firing (evidence for the lint test).** Temporarily revert the TabStrip keyboard fix is NOT needed — instead, before this task, run eslint after installing the plugins and capture the jsx-a11y error it raises on any remaining static-handler element. (If TabStrip is already fixed, demonstrate with `npx eslint` on a throwaway snippet, or rely on the captured diff.)

- [ ] **Step 2: Install plugins** — `pnpm add -D eslint-plugin-jsx-a11y eslint-plugin-react`.

- [ ] **Step 3: Edit `eslint.config.js`** — import and register:

```js
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import react from "eslint-plugin-react";
import jsxA11y from "eslint-plugin-jsx-a11y";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: [ /* unchanged */ ] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  // React + a11y recommended (scoped to source; tests/config excluded below).
  {
    files: ["**/*.{ts,tsx}"],
    ...react.configs.flat.recommended,
    settings: { react: { version: "detect" } },
  },
  react.configs.flat["jsx-runtime"], // new JSX transform: no React-in-scope
  jsxA11y.flatConfigs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks, "react-refresh": reactRefresh },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
      // TypeScript already enforces prop typing; the runtime prop-types rule is redundant.
      "react/prop-types": "off",
    },
  },
  { files: [ /* tests/config — unchanged */ ], rules: { "react-refresh/only-export-components": "off" } },
  prettier,
);
```

- [ ] **Step 4: Run eslint, triage** — `pnpm lint`. Fix violations **only in files this PR already touches**. For violations in untouched files, if the backlog is small fix them; if large, scope that specific rule down and record a follow-up in the PR body. Re-run until `pnpm lint` exits 0.

- [ ] **Step 5: Commit** — `chore(lint): enable jsx-a11y + react rules (DX1)`.

---

## Task 10: Verification & PR

- [ ] `pnpm test` → all green (capture).
- [ ] `pnpm lint` → exit 0 (capture).
- [ ] `pnpm build` → success (capture).
- [ ] `pnpm format:check` (or `prettier --check`) → clean (per ci-local-gates memory).
- [ ] Self-review the diff for scope creep (requesting-code-review).
- [ ] Commit any stragglers; push; `gh pr create -R tau-rs/cairn-web-ui --base main`, citing U1, U2, DX1. STOP — no merge.

---

## Self-Review (spec coverage)

- **U1 tabs keyboard** → Task 2. **U1 tree keyboard move** → Tasks 3–4 (F2 rename + rename-to-path note move; folder keyboard-move noted as follow-up).
- **U2 loading flags** → Task 5; **render distinct from empty** → Tasks 6–8 (search, backlinks, graph, note).
- **DX1 jsx-a11y + react** → Task 9.
- **Constraints:** no keydown changes in App (sessions 63/64); lint rollout scoped to touched files w/ follow-up note; U4 untouched.
