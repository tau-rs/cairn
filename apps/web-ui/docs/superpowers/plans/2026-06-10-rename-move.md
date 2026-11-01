# Rename / Move Notes & Folders Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename and move notes — and folders (bulk) — in the folder tree via inline edit (double-click) and drag-and-drop, backed by the engine's `RenameNote`.

**Architecture:** A pure `treeMoves` planner turns a gesture into a `{from,to}[]` list; the store's `applyRenames` runs them sequentially via `rename_note` and remaps the path-keyed `openNotes`/`tabs`/`activePath`; `FolderTreeView` adds inline edit + native HTML5 drag/drop. The mock implements `rename_note` (move + wikilink rewrite). No engine/contract change.

**Tech Stack:** React 18 + TypeScript, Zustand, Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-10-rename-move-design.md`

**Working conventions (read before starting):**
- Run `pnpm` from `web/`. Git from repo root.
- Per-task gate: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. `pnpm format` + re-stage if needed. **Ignore stale LSP diagnostics — trust `pnpm typecheck`'s exit code** (the harness LSP lags behind generated/edited files in this repo).
- e2e on port 5273. Baseline (post-search merge on `main`): 266 unit, 15 e2e green.
- **Relevant existing code:**
  - Contract: `Command` has `{type:"rename_note", from: string, to: string}`. `ContractError` variants: `{type:"not_found", what}`, `{type:"invalid_request", message}`, `{type:"internal", message}`.
  - `MockClient.sendCommand` (`src/client/mock.ts`) has `write_note`/`delete_note`/`commit` cases + a `default` throw. Imports `{ extractLinks, stem }` from `./wikilink`, and `ContractError` from `../contract`. `this.notes` is a `Map<path, body>`; `emit(e)` broadcasts an `Event`.
  - Store (`src/store/store.ts`): `openNotes: Record<path, NoteBuffer>`, `tabs: {path; preview}[]`, `activePath`. A `persist()` closure (saveTabs) + `refreshBacklinks()` + `errMsg(err)` exist. `deleteNote` precedent: `sendCommand(delete_note)` then `closeTab(path)`.
  - `FolderTree` (`src/components/tree/FolderTreeView.tsx`) props: `{ paths, activePath, onOpen, onDelete, onRequestNew, onRequestNewInFolder }`; folder rows have a name `<button onClick={toggle}>` + hover `+`; note rows have a name `<button onClick={onOpen}>` + hover `✕`. `buildTree` gives `TreeNode = {kind:"folder", name, path, children} | {kind:"note", name, path}` where note `name` is the stem.
  - App renders `<FolderTree …/>` in the `list` slot.
  - Tailwind tokens: `surface`, `surface-2`, `border`, `text`, `muted`, `faint`, `accent`, `danger`.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/tree/treeMoves.ts` | Pure: plan rename/move → `Rename[]`; `canDrop`. |
| `web/src/client/mock.ts` | `rename_note`: move + rewrite `[[wikilinks]]` + emit events. |
| `web/src/store/store.ts` | `applyRenames(ops)`: sequential rename + path-keyed remap. |
| `web/src/components/tree/FolderTreeView.tsx` | Inline rename (dbl-click) + HTML5 drag/drop → `onApplyRenames`. |
| `web/src/app/App.tsx` | Wire `onApplyRenames`. |
| `web/e2e/skeleton.spec.ts` | Inline-rename e2e. |

---

## Task 1: treeMoves (pure)

**Files:**
- Create: `web/src/components/tree/treeMoves.ts`
- Create: `web/src/components/tree/treeMoves.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tree/treeMoves.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import {
  planRenameNote,
  planRenameFolder,
  planMoveNote,
  planMoveFolder,
  canDrop,
} from "./treeMoves";

describe("planRenameNote", () => {
  it("renames within the same folder", () => {
    expect(planRenameNote("projects/ideas.md", "plan")).toEqual([
      { from: "projects/ideas.md", to: "projects/plan.md" },
    ]);
  });
  it("strips a typed .md and works at root", () => {
    expect(planRenameNote("a.md", "b.md")).toEqual([
      { from: "a.md", to: "b.md" },
    ]);
  });
  it("no-ops on an unchanged or invalid name", () => {
    expect(planRenameNote("a.md", "a")).toEqual([]);
    expect(planRenameNote("a.md", "")).toEqual([]);
    expect(planRenameNote("a.md", "x/y")).toEqual([]);
  });
});

describe("planRenameFolder", () => {
  it("bulk-renames every descendant note, preserving the parent + nesting", () => {
    expect(
      planRenameFolder("projects", "work", [
        "projects/ideas.md",
        "projects/sub/b.md",
        "index.md",
      ]),
    ).toEqual([
      { from: "projects/ideas.md", to: "work/ideas.md" },
      { from: "projects/sub/b.md", to: "work/sub/b.md" },
    ]);
  });
  it("preserves the parent of a nested folder", () => {
    expect(planRenameFolder("a/b", "c", ["a/b/x.md"])).toEqual([
      { from: "a/b/x.md", to: "a/c/x.md" },
    ]);
  });
});

describe("planMoveNote", () => {
  it("moves a note into a folder", () => {
    expect(planMoveNote("ideas.md", "archive")).toEqual([
      { from: "ideas.md", to: "archive/ideas.md" },
    ]);
  });
  it("moves a note to root and no-ops when already there", () => {
    expect(planMoveNote("a/x.md", "")).toEqual([
      { from: "a/x.md", to: "x.md" },
    ]);
    expect(planMoveNote("archive/x.md", "archive")).toEqual([]);
  });
});

describe("planMoveFolder", () => {
  it("moves a subtree under the destination (basename preserved)", () => {
    expect(
      planMoveFolder("projects", "archive", [
        "projects/a.md",
        "projects/sub/b.md",
      ]),
    ).toEqual([
      { from: "projects/a.md", to: "archive/projects/a.md" },
      { from: "projects/sub/b.md", to: "archive/projects/sub/b.md" },
    ]);
  });
  it("no-ops into itself or its own descendant", () => {
    expect(planMoveFolder("a", "a", ["a/x.md"])).toEqual([]);
    expect(planMoveFolder("a", "a/b", ["a/x.md"])).toEqual([]);
  });
});

describe("canDrop", () => {
  it("allows a real note/folder move and blocks no-ops/self/descendant", () => {
    expect(canDrop("ideas.md", false, "archive")).toBe(true);
    expect(canDrop("archive/x.md", false, "archive")).toBe(false); // already there
    expect(canDrop("a", true, "b")).toBe(true);
    expect(canDrop("a", true, "a")).toBe(false); // itself
    expect(canDrop("a", true, "a/b")).toBe(false); // own descendant
    expect(canDrop("a/b", true, "a")).toBe(false); // current parent
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- treeMoves` — expect FAIL (module not found).

- [ ] **Step 3: Implement `treeMoves.ts`**

Create `web/src/components/tree/treeMoves.ts`:
```ts
export interface Rename {
  from: string;
  to: string;
}

function dirOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? "" : path.slice(0, i);
}
function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}
const join = (dir: string, name: string): string => (dir ? `${dir}/${name}` : name);

/** Rename a note's filename within its folder. `newName` is a bare stem
 *  (slashes rejected; a typed `.md` is stripped). [] if empty/invalid/unchanged. */
export function planRenameNote(notePath: string, newName: string): Rename[] {
  const name = newName.trim().replace(/\.md$/, "");
  if (!name || name.includes("/")) return [];
  const to = join(dirOf(notePath), `${name}.md`);
  return to === notePath ? [] : [{ from: notePath, to }];
}

/** Replace a folder's last segment → one Rename per descendant note. */
export function planRenameFolder(
  folderPath: string,
  newName: string,
  allPaths: string[],
): Rename[] {
  const name = newName.trim();
  if (!name || name.includes("/")) return [];
  const newFolder = join(dirOf(folderPath), name);
  if (newFolder === folderPath) return [];
  const prefix = `${folderPath}/`;
  return allPaths
    .filter((p) => p.startsWith(prefix))
    .map((p) => ({ from: p, to: newFolder + p.slice(folderPath.length) }));
}

/** Move a note into `destFolder` ("" = root). [] if already there. */
export function planMoveNote(notePath: string, destFolder: string): Rename[] {
  if (dirOf(notePath) === destFolder) return [];
  return [{ from: notePath, to: join(destFolder, baseName(notePath)) }];
}

/** Move a folder's subtree under `destFolder`. [] when the drop isn't allowed. */
export function planMoveFolder(
  folderPath: string,
  destFolder: string,
  allPaths: string[],
): Rename[] {
  if (!canDrop(folderPath, true, destFolder)) return [];
  const newFolder = join(destFolder, baseName(folderPath));
  const prefix = `${folderPath}/`;
  return allPaths
    .filter((p) => p.startsWith(prefix))
    .map((p) => ({ from: p, to: newFolder + p.slice(folderPath.length) }));
}

/** Whether dropping `draggedPath` into `destFolder` ("" = root) is a real move:
 *  not the current parent, not itself, not a folder into its own subtree. */
export function canDrop(
  draggedPath: string,
  isFolder: boolean,
  destFolder: string,
): boolean {
  if (isFolder) {
    if (destFolder === draggedPath) return false;
    if (destFolder.startsWith(`${draggedPath}/`)) return false;
    return dirOf(draggedPath) !== destFolder;
  }
  return dirOf(draggedPath) !== destFolder;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- treeMoves` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tree/treeMoves.ts web/src/components/tree/treeMoves.test.ts
git commit -m "feat(rename): pure rename/move planners + canDrop"
```

---

## Task 2: Mock `rename_note`

**Files:** modify `web/src/client/mock.ts`, `web/src/client/mock.test.ts`.

- [ ] **Step 1: Write failing tests**

Append inside `describe("MockClient", …)` in `web/src/client/mock.test.ts`:
```ts
  it("rename_note moves the note and rewrites [[wikilinks]] when the stem changes", async () => {
    const c = new MockClient({
      "a.md": "# A",
      "x.md": "see [[a]] and [[a|alias]]",
    });
    await c.sendCommand({ type: "rename_note", from: "a.md", to: "c.md" });
    expect(await c.runQuery({ type: "get_note", path: "c.md" })).toEqual({
      type: "note",
      contents: "# A",
    });
    await expect(
      c.runQuery({ type: "get_note", path: "a.md" }),
    ).rejects.toEqual({ type: "not_found", what: "a.md" });
    expect(await c.runQuery({ type: "get_note", path: "x.md" })).toEqual({
      type: "note",
      contents: "see [[c]] and [[c|alias]]",
    });
  });
  it("rename_note keeps links when only the folder changes (stem unchanged)", async () => {
    const c = new MockClient({ "a.md": "# A", "x.md": "see [[a]]" });
    await c.sendCommand({ type: "rename_note", from: "a.md", to: "sub/a.md" });
    expect(await c.runQuery({ type: "get_note", path: "x.md" })).toEqual({
      type: "note",
      contents: "see [[a]]",
    });
  });
  it("rename_note errors on a missing source and an existing target", async () => {
    const c = new MockClient({ "a.md": "x", "b.md": "y" });
    await expect(
      c.sendCommand({ type: "rename_note", from: "missing.md", to: "z.md" }),
    ).rejects.toEqual({ type: "not_found", what: "missing.md" });
    await expect(
      c.sendCommand({ type: "rename_note", from: "a.md", to: "b.md" }),
    ).rejects.toMatchObject({ type: "invalid_request" });
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- mock` — expect FAIL (`rename_note` hits the `default` throw).

- [ ] **Step 3: Implement**

In `web/src/client/mock.ts`, add a module-level helper (near `splitFrontmatter`):
```ts
/** Rewrite `[[oldStem]]` / `[[oldStem|alias]]` → newStem (link target only). */
function rewriteWikilinks(raw: string, oldStem: string, newStem: string): string {
  const esc = oldStem.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return raw.replace(new RegExp(`\\[\\[${esc}(\\]\\]|\\|)`, "g"), `[[${newStem}$1`);
}
```
Then add a `rename_note` case to `sendCommand` (before the `default`):
```ts
      case "rename_note": {
        if (!this.notes.has(c.from)) {
          const err: ContractError = { type: "not_found", what: c.from };
          throw err;
        }
        if (this.notes.has(c.to)) {
          const err: ContractError = {
            type: "invalid_request",
            message: `already exists: ${c.to}`,
          };
          throw err;
        }
        const body = this.notes.get(c.from) as string;
        this.notes.delete(c.from);
        this.notes.set(c.to, body);
        const oldStem = stem(c.from);
        const newStem = stem(c.to);
        if (oldStem !== newStem) {
          for (const [p, raw] of [...this.notes]) {
            if (p === c.to) continue;
            const rewritten = rewriteWikilinks(raw, oldStem, newStem);
            if (rewritten !== raw) this.notes.set(p, rewritten);
          }
        }
        this.emit({ type: "note_deleted", path: c.from });
        this.emit({ type: "note_changed", path: c.to });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
```
(`stem` and `ContractError` are already imported in `mock.ts`.)

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- mock` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(rename): mock rename_note (move + wikilink rewrite + events)"
```

---

## Task 3: Store `applyRenames`

**Files:** modify `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Write failing tests**

Append inside `describe("cairn store", …)` in `web/src/store/store.test.ts`:
```ts
  it("applyRenames moves an open note's tab + activePath to the new path", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().applyRenames([{ from: "a.md", to: "c.md" }]);
    expect(store.getState().activePath).toBe("c.md");
    expect(store.getState().tabs.map((t) => t.path)).toContain("c.md");
    expect(store.getState().tabs.map((t) => t.path)).not.toContain("a.md");
    expect(store.getState().openNotes["c.md"]).toBeDefined();
    expect(store.getState().openNotes["a.md"]).toBeUndefined();
  });
  it("applyRenames stops on the first error (no further commands)", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A" });
    const spy = vi.spyOn(client, "sendCommand");
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().applyRenames([
      { from: "missing.md", to: "z.md" }, // not_found → stop
      { from: "a.md", to: "y.md" },
    ]);
    expect(store.getState().error).toBeTruthy();
    const renameCalls = spy.mock.calls.filter(
      ([cmd]) => cmd.type === "rename_note",
    );
    expect(renameCalls.length).toBe(1); // stopped before the second
    expect(store.getState().notePaths).toContain("a.md"); // a.md untouched
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- store` — expect FAIL (`applyRenames` undefined).

- [ ] **Step 3: Implement**

In `web/src/store/store.ts`:

(a) Import the type near the top:
```ts
import type { Rename } from "../components/tree/treeMoves";
```
(b) In `CairnState`, add to the actions section (near `deleteNote`):
```ts
  applyRenames(ops: Rename[]): Promise<void>;
```
(c) Add the action (next to `deleteNote`):
```ts
      async applyRenames(ops) {
        for (const { from, to } of ops) {
          try {
            await client.sendCommand({ type: "rename_note", from, to });
          } catch (err) {
            set({ error: errMsg(err) });
            break;
          }
          set((s) => {
            const openNotes = { ...s.openNotes };
            if (from in openNotes) {
              openNotes[to] = openNotes[from];
              delete openNotes[from];
            }
            return {
              openNotes,
              tabs: s.tabs.map((t) =>
                t.path === from ? { ...t, path: to } : t,
              ),
              activePath: s.activePath === from ? to : s.activePath,
            };
          });
        }
        persist();
        if (get().activePath) void get().refreshBacklinks();
      },
```
(`persist`, `get`, `errMsg`, `refreshBacklinks` are all in scope.)

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- store` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(rename): store applyRenames (sequential rename + path-keyed remap)"
```

---

## Task 4: FolderTreeView — inline rename + drag-to-move

One cohesive task (one file) to avoid fragile re-edits.

**Files:** modify `web/src/components/tree/FolderTreeView.tsx`, create `web/src/components/tree/FolderTreeView.dnd.test.tsx` (a new test file alongside the existing `FolderTreeView.test.tsx`, which stays unchanged).

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tree/FolderTreeView.dnd.test.tsx`:
```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FolderTree } from "./FolderTreeView";

beforeEach(() => localStorage.clear());

function setup(over = {}) {
  const props = {
    paths: ["archive/keep.md", "notes/ideas.md", "notes/todo.md"],
    activePath: null as string | null,
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

describe("FolderTree rename/move", () => {
  it("double-click a note → input → Enter renames within its folder", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "ideas" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "renamed" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "notes/renamed.md" },
    ]);
  });
  it("double-click a folder → input → Enter bulk-renames its notes", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "notes" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "work" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "work/ideas.md" },
      { from: "notes/todo.md", to: "work/todo.md" },
    ]);
  });
  it("Escape cancels an inline rename", () => {
    const props = setup();
    fireEvent.doubleClick(screen.getByRole("button", { name: "ideas" }));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(props.onApplyRenames).not.toHaveBeenCalled();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
  it("dragging a note onto a folder moves it", () => {
    const props = setup();
    fireEvent.dragStart(screen.getByRole("button", { name: "ideas" }));
    fireEvent.drop(screen.getByRole("button", { name: "archive" }));
    expect(props.onApplyRenames).toHaveBeenCalledWith([
      { from: "notes/ideas.md", to: "archive/ideas.md" },
    ]);
  });
  it("dropping a folder onto itself does nothing", () => {
    const props = setup();
    fireEvent.dragStart(screen.getByRole("button", { name: "notes" }));
    fireEvent.drop(screen.getByRole("button", { name: "notes" }));
    expect(props.onApplyRenames).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- FolderTreeView` — expect FAIL (`onApplyRenames` not a prop; no inline edit / drag handlers).

- [ ] **Step 3: Implement — replace `web/src/components/tree/FolderTreeView.tsx` entirely**

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Button } from "../ui/Button";
import { SectionLabel } from "../ui/SectionLabel";
import { buildTree, ancestorFolders, type TreeNode } from "./folderTree";
import { loadCollapsed, saveCollapsed } from "./treePersistence";
import {
  planRenameNote,
  planRenameFolder,
  planMoveNote,
  planMoveFolder,
  canDrop,
  type Rename,
} from "./treeMoves";

/** Inline rename input: autofocuses, selects all, commits on Enter/blur, cancels on Esc. */
function RenameInput(props: {
  initial: string;
  onCommit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(props.initial);
  return (
    <input
      autoFocus
      className="min-w-0 flex-1 rounded border border-accent bg-surface-2 px-1 py-0.5 text-sm text-text outline-none"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onFocus={(e) => e.target.select()}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation(); // don't trigger global shortcuts / tree toggles
        if (e.key === "Enter") props.onCommit(value);
        else if (e.key === "Escape") props.onCancel();
      }}
      onBlur={() => props.onCommit(value)}
    />
  );
}

export function FolderTree(props: {
  paths: string[];
  activePath: string | null;
  onOpen: (path: string) => void;
  onDelete: (path: string) => void;
  onRequestNew: () => void;
  onRequestNewInFolder: (folderPath: string) => void;
  onApplyRenames: (ops: Rename[]) => void;
}) {
  const tree = useMemo(() => buildTree(props.paths), [props.paths]);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => loadCollapsed());
  const [editingPath, setEditingPath] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const dragged = useRef<{ path: string; isFolder: boolean } | null>(null);

  const activePath = props.activePath;
  useEffect(() => {
    if (!activePath) return;
    const anc = ancestorFolders(activePath);
    if (anc.length === 0) return;
    setCollapsed((prev) => {
      if (!anc.some((f) => prev.has(f))) return prev;
      const next = new Set(prev);
      for (const f of anc) next.delete(f);
      saveCollapsed(next);
      return next;
    });
  }, [activePath]);

  const toggle = (folderPath: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(folderPath)) next.delete(folderPath);
      else next.add(folderPath);
      saveCollapsed(next);
      return next;
    });
  };

  const commitRename = (node: TreeNode, newName: string) => {
    setEditingPath(null);
    const ops =
      node.kind === "folder"
        ? planRenameFolder(node.path, newName, props.paths)
        : planRenameNote(node.path, newName);
    if (ops.length) props.onApplyRenames(ops);
  };

  const onDropInto = (destFolder: string) => {
    const d = dragged.current;
    dragged.current = null;
    setDropTarget(null);
    if (!d) return;
    const ops = d.isFolder
      ? planMoveFolder(d.path, destFolder, props.paths)
      : planMoveNote(d.path, destFolder);
    if (ops.length) props.onApplyRenames(ops);
  };

  const startDrag = (e: React.DragEvent, path: string, isFolder: boolean) => {
    dragged.current = { path, isFolder };
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  };

  // Drop-target props for a folder path ("" = root).
  const dropProps = (destFolder: string) => ({
    onDragOver: (e: React.DragEvent) => {
      const d = dragged.current;
      if (d && canDrop(d.path, d.isFolder, destFolder)) {
        e.preventDefault();
        setDropTarget(destFolder);
      }
    },
    onDragLeave: () =>
      setDropTarget((t) => (t === destFolder ? null : t)),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      onDropInto(destFolder);
    },
  });

  const renderNodes = (nodes: TreeNode[], depth: number): ReactNode =>
    nodes.map((node) => {
      const pad = { paddingLeft: depth * 12 + 8 };
      const editing = editingPath === node.path;
      if (node.kind === "folder") {
        const isCollapsed = collapsed.has(node.path);
        const isDrop = dropTarget === node.path;
        return (
          <div key={node.path}>
            <div
              draggable={!editing}
              onDragStart={(e) => startDrag(e, node.path, true)}
              {...dropProps(node.path)}
              className={
                "group flex items-center justify-between rounded pr-2 text-muted hover:bg-surface-2 hover:text-text " +
                (isDrop ? "ring-1 ring-accent" : "")
              }
            >
              {editing ? (
                <span className="flex-1" style={pad}>
                  <RenameInput
                    initial={node.name}
                    onCommit={(v) => commitRename(node, v)}
                    onCancel={() => setEditingPath(null)}
                  />
                </span>
              ) : (
                <button
                  className="flex min-w-0 flex-1 items-center gap-1 py-1 text-left"
                  style={pad}
                  title={node.path}
                  onClick={() => toggle(node.path)}
                  onDoubleClick={() => setEditingPath(node.path)}
                >
                  <span aria-hidden="true" className="text-faint">
                    {isCollapsed ? "▸" : "▾"}
                  </span>
                  <span className="truncate">{node.name}</span>
                </button>
              )}
              <button
                className="ml-1 hidden text-faint hover:text-text group-hover:block"
                aria-label={`new note in ${node.path}`}
                onClick={() => props.onRequestNewInFolder(node.path)}
              >
                +
              </button>
            </div>
            {!isCollapsed && renderNodes(node.children, depth + 1)}
          </div>
        );
      }
      const active = node.path === props.activePath;
      return (
        <div
          key={node.path}
          draggable={!editing}
          onDragStart={(e) => startDrag(e, node.path, false)}
          className={`group flex items-center justify-between rounded pr-2 ${
            active
              ? "bg-surface-2 text-text"
              : "text-muted hover:bg-surface-2 hover:text-text"
          }`}
        >
          {editing ? (
            <span className="flex-1" style={pad}>
              <RenameInput
                initial={node.name}
                onCommit={(v) => commitRename(node, v)}
                onCancel={() => setEditingPath(null)}
              />
            </span>
          ) : (
            <button
              className="min-w-0 flex-1 truncate py-1 text-left"
              style={pad}
              title={node.path}
              onClick={() => props.onOpen(node.path)}
              onDoubleClick={() => setEditingPath(node.path)}
            >
              {node.name}
            </button>
          )}
          <button
            className="ml-1 hidden text-faint hover:text-danger group-hover:block"
            aria-label={`delete ${node.path}`}
            onClick={() => props.onDelete(node.path)}
          >
            ✕
          </button>
        </div>
      );
    });

  return (
    <div className="flex flex-col gap-0.5 text-sm">
      <div
        {...dropProps("")}
        className={
          "mb-1 flex items-center justify-between rounded " +
          (dropTarget === "" ? "ring-1 ring-accent" : "")
        }
      >
        <SectionLabel>Notes</SectionLabel>
        <Button variant="ghost" onClick={props.onRequestNew}>
          + New note
        </Button>
      </div>
      {renderNodes(tree, 0)}
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- FolderTreeView` — expect PASS (the existing `FolderTreeView.test.tsx` + the new `FolderTreeView.dnd.test.tsx`). The existing tests must stay green — open/delete/collapse/new-in-folder behavior is unchanged. But `onApplyRenames` is now a **required** prop, so update `FolderTreeView.test.tsx`: add `onApplyRenames: vi.fn(),` to its `setup()` props-default object (that single change covers all its renders). If any test there renders `<FolderTree>` directly without the helper, add `onApplyRenames={vi.fn()}` to that JSX too.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS. Run `pnpm format` + re-stage if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tree/FolderTreeView.tsx web/src/components/tree/FolderTreeView.dnd.test.tsx web/src/components/tree/FolderTreeView.test.tsx
git commit -m "feat(rename): FolderTree inline rename + drag-to-move"
```

---

## Task 5: App wiring + e2e

**Files:** modify `web/src/app/App.tsx`, `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: App — pass onApplyRenames**

In `web/src/app/App.tsx`, add the prop to the `<FolderTree …/>` element (it already receives `onOpen`/`onDelete`/`onRequestNew`/`onRequestNewInFolder`):
```tsx
            onApplyRenames={actions.applyRenames}
```

- [ ] **Step 2: Gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 3: Add the e2e (inline rename)**

Append to `web/e2e/skeleton.spec.ts`:
```ts
test("rename: inline-rename a note in the tree", async ({ page }) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  // index.md is a root note shown as "index".
  const idx = sidebar.getByRole("button", { name: "index", exact: true });
  await expect(idx).toBeVisible();
  await idx.dblclick();
  const input = sidebar.getByRole("textbox");
  await input.fill("home");
  await input.press("Enter");
  // The note now appears under its new name; the old name is gone.
  await expect(
    sidebar.getByRole("button", { name: "home", exact: true }),
  ).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "index", exact: true }),
  ).toHaveCount(0);
});
```
(Drag-to-move is not e2e-tested — native HTML5 DnD is unreliable in Playwright; it's covered by the `treeMoves` unit tests + the `FolderTreeView` drop-handler test. Verify drag manually.)

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e` — expect 16 passed (15 existing + this). If port 5273 busy: `lsof -ti :5273 | xargs kill 2>/dev/null` then retry once.
- The existing tests must stay green. Note: other tests open notes by clicking tree buttons by stem; renaming `index`→`home` happens only inside this new test's page, so it doesn't affect others (each test gets a fresh page).
- If the rename doesn't take, confirm `applyRenames` is wired and the mock `rename_note` emits `note_changed`/`note_deleted` (the tree refreshes from `notePaths`). STOP and report if a core assertion fails.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 6: Manual/visual check**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); confirm the dev log is error-free; stop it. (Human confirms: double-click a note/folder name renames it; dragging a note onto a folder moves it; dragging a folder onto a folder moves the subtree; dropping on "Notes" moves to root; an open note's tab follows the rename; `[[wikilinks]]` stay valid.)

- [ ] **Step 7: Commit**

```bash
git add web/src/app/App.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(rename): wire FolderTree onApplyRenames + inline-rename e2e"
```

---

## Notes for the executor

- **The crux is `applyRenames`'s path-keyed remap** — `openNotes[from]→[to]`, `tabs` path, `activePath`. The engine's `note_deleted`/`note_changed` events refresh notePaths/backlinks/graph but never touch tabs/openNotes, so the explicit remap is what keeps an open tab attached. Renames run sequentially; stop on the first error.
- **Folders are bulk.** A folder rename/move is one `RenameNote` per descendant note (the engine has no folder command); non-atomic by nature.
- **`canDrop` gates both the highlight and the drop**; the planners also re-check (return `[]`), so a stray drop can't produce a bad path.
- **Wikilink rewrite only on stem change** (moving between folders keeps the stem → links stay valid); the mock mirrors this.
- **Inline edit `stopPropagation`** on keydown/click so typing/Esc doesn't fire global shortcuts or toggle a folder.
- **DnD isn't e2e-tested** (flaky in Playwright); its correctness is the pure `treeMoves` planners + the component drop-handler test. The e2e covers inline rename.
- **No engine/contract change** — `rename_note` + `RenameNote` were already synced.
```
