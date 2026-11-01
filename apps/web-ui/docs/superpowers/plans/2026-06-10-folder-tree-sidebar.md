# Folder-Tree Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat note list with a collapsible folder tree (grouped on `/` in note paths), with per-folder "new note here", persisted collapse state, and reveal-on-open.

**Architecture:** A pure `folderTree` (paths → sorted tree + ancestor helper) + a pure `treePersistence` (collapsed-set localStorage) feed a recursive `FolderTree` component that replaces `NoteList`. `NewNoteDialog` gains an `initialPath` for the per-folder "+". No store/contract/backend changes.

**Tech Stack:** React 18 + TypeScript, Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-10-folder-tree-sidebar-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. Run `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if `format:check` flags files. Ignore stale LSP "cannot find module" noise — trust `pnpm typecheck`'s exit code.
- e2e on port 5273 (`--strictPort`). Current baseline: 208 unit, 11 e2e green.
- **Relevant existing code:**
  - `stem(path)` from `web/src/client/wikilink.ts` (`"notes/ideas.md"` → `"ideas"`).
  - Persistence pattern: `web/src/components/tabs/tabsPersistence.ts` (guarded localStorage; jsdom localStorage works via `web/src/vitest.setup.ts`).
  - Current `NoteList` (to be replaced) renders full paths; `App.tsx` passes `list={<NoteList paths={notePaths} activePath={activePath} onOpen={actions.openNote} onRequestNew={() => setNewNoteOpen(true)} onDelete={actions.deleteNote} />}`.
  - `NewNoteDialog` props today: `{ open, onOpenChange, onCreate }`.
  - **jsdom note:** Tailwind CSS is NOT applied in unit tests, so `hidden group-hover:block` elements ARE in the accessibility tree there (getByRole finds them). In the real browser (e2e) they're `display:none` until hover — so e2e must `hover()` the row before clicking a hover-revealed `+`/`✕`.
  - Tailwind tokens: `surface`, `surface-2`, `border`, `text`, `muted`, `faint`, `danger`, `accent`.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/tree/folderTree.ts` | Pure: `buildTree(paths)` + `ancestorFolders(path)`. |
| `web/src/components/tree/treePersistence.ts` | Pure: collapsed-folder set ↔ localStorage. |
| `web/src/components/tree/FolderTree.tsx` | Recursive tree component (replaces NoteList). |
| `web/src/components/NewNoteDialog.tsx` | Add `initialPath` to pre-fill the input. |
| `web/src/app/App.tsx` | Render FolderTree; new-note initial path. |
| `web/src/components/NoteList.tsx` (+ test) | Deleted. |
| `web/src/client/fixtures.ts` | Add one nested fixture note. |
| `web/e2e/skeleton.spec.ts` | Stem-selector migration + folder-tree test. |

---

## Task 1: folderTree (pure)

**Files:**
- Create: `web/src/components/tree/folderTree.ts`
- Create: `web/src/components/tree/folderTree.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tree/folderTree.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { buildTree, ancestorFolders } from "./folderTree";

describe("buildTree", () => {
  it("groups root notes and nested folders, folders-first then alpha", () => {
    const tree = buildTree([
      "index.md",
      "ideas.md",
      "notes/todo.md",
      "notes/sub/deep.md",
      "projects/cairn.md",
    ]);
    // top level: folders (notes, projects) alpha, then notes (ideas, index) alpha
    expect(tree.map((n) => [n.kind, n.name])).toEqual([
      ["folder", "notes"],
      ["folder", "projects"],
      ["note", "ideas"],
      ["note", "index"],
    ]);
  });

  it("nests children with full folder paths and stem leaf names", () => {
    const tree = buildTree(["notes/sub/deep.md", "notes/todo.md"]);
    const notes = tree[0];
    expect(notes).toMatchObject({ kind: "folder", name: "notes", path: "notes" });
    if (notes.kind !== "folder") throw new Error("expected folder");
    // folder "sub" before note "todo"
    expect(notes.children.map((n) => [n.kind, n.name, n.path])).toEqual([
      ["folder", "sub", "notes/sub"],
      ["note", "todo", "notes/todo.md"],
    ]);
    const sub = notes.children[0];
    if (sub.kind !== "folder") throw new Error("expected folder");
    expect(sub.children).toEqual([
      { kind: "note", name: "deep", path: "notes/sub/deep.md" },
    ]);
  });
});

describe("ancestorFolders", () => {
  it("returns enclosing folders outermost-first", () => {
    expect(ancestorFolders("a/b/c.md")).toEqual(["a", "a/b"]);
  });
  it("returns empty for a root note", () => {
    expect(ancestorFolders("index.md")).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- folderTree` — expect FAIL (module not found).

- [ ] **Step 3: Implement `folderTree.ts`**

Create `web/src/components/tree/folderTree.ts`:

```ts
import { stem } from "../../client/wikilink";

export type TreeNode =
  | { kind: "folder"; name: string; path: string; children: TreeNode[] }
  | { kind: "note"; name: string; path: string };

interface Acc {
  name: string;
  path: string;
  folders: Map<string, Acc>;
  notes: { name: string; path: string }[];
}

/** Group flat note paths into a tree (split on "/"), each level sorted
 *  folders-first then alphabetical (case-insensitive). Leaf `name` is the stem;
 *  folder `path` is the slash-joined prefix. */
export function buildTree(paths: string[]): TreeNode[] {
  const root: Acc = { name: "", path: "", folders: new Map(), notes: [] };
  for (const p of paths) {
    const segs = p.split("/");
    let cur = root;
    for (let i = 0; i < segs.length - 1; i++) {
      const seg = segs[i];
      const fpath = cur.path ? `${cur.path}/${seg}` : seg;
      let next = cur.folders.get(seg);
      if (!next) {
        next = { name: seg, path: fpath, folders: new Map(), notes: [] };
        cur.folders.set(seg, next);
      }
      cur = next;
    }
    cur.notes.push({ name: stem(segs[segs.length - 1]), path: p });
  }

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

  const toNodes = (acc: Acc): TreeNode[] => {
    const folders: TreeNode[] = [...acc.folders.values()]
      .map((f) => ({
        kind: "folder" as const,
        name: f.name,
        path: f.path,
        children: toNodes(f),
      }))
      .sort(byName);
    const notes: TreeNode[] = acc.notes
      .map((n) => ({ kind: "note" as const, name: n.name, path: n.path }))
      .sort(byName);
    return [...folders, ...notes];
  };
  return toNodes(root);
}

/** Folder paths enclosing a note path, outermost→innermost. "a/b/c.md" → ["a","a/b"]. */
export function ancestorFolders(path: string): string[] {
  const segs = path.split("/");
  segs.pop(); // drop the filename
  const out: string[] = [];
  let acc = "";
  for (const s of segs) {
    acc = acc ? `${acc}/${s}` : s;
    out.push(acc);
  }
  return out;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- folderTree` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tree/folderTree.ts web/src/components/tree/folderTree.test.ts
git commit -m "feat(tree): pure path→folder-tree builder + ancestorFolders"
```

---

## Task 2: treePersistence (pure)

**Files:**
- Create: `web/src/components/tree/treePersistence.ts`
- Create: `web/src/components/tree/treePersistence.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tree/treePersistence.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { loadCollapsed, saveCollapsed } from "./treePersistence";

beforeEach(() => localStorage.clear());

describe("treePersistence", () => {
  it("round-trips the collapsed folder set", () => {
    saveCollapsed(new Set(["notes", "notes/sub"]));
    expect([...loadCollapsed()].sort()).toEqual(["notes", "notes/sub"]);
  });
  it("returns an empty set when nothing is stored", () => {
    expect(loadCollapsed().size).toBe(0);
  });
  it("returns an empty set on malformed storage", () => {
    localStorage.setItem("cairn.folderTree", "{not json");
    expect(loadCollapsed().size).toBe(0);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- treePersistence` — expect FAIL (module not found).

- [ ] **Step 3: Implement `treePersistence.ts`**

Create `web/src/components/tree/treePersistence.ts`:

```ts
const STORAGE_KEY = "cairn.folderTree";

/** Load the set of collapsed folder paths (anything not in the set is expanded). */
export function loadCollapsed(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((x): x is string => typeof x === "string"));
  } catch {
    return new Set();
  }
}

export function saveCollapsed(collapsed: Set<string>): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify([...collapsed]));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- treePersistence` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tree/treePersistence.ts web/src/components/tree/treePersistence.test.ts
git commit -m "feat(tree): collapsed-folder localStorage persistence"
```

---

## Task 3: FolderTree component

**Files:**
- Create: `web/src/components/tree/FolderTree.tsx`
- Create: `web/src/components/tree/FolderTree.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/tree/FolderTree.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FolderTree } from "./FolderTree";

beforeEach(() => localStorage.clear());

function setup(over = {}) {
  const props = {
    paths: ["index.md", "notes/ideas.md", "notes/todo.md"],
    activePath: null as string | null,
    onOpen: vi.fn(),
    onDelete: vi.fn(),
    onRequestNew: vi.fn(),
    onRequestNewInFolder: vi.fn(),
    ...over,
  };
  render(<FolderTree {...props} />);
  return props;
}

describe("FolderTree", () => {
  it("renders folders with nested notes, and root notes", () => {
    setup();
    expect(screen.getByRole("button", { name: "notes" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "index" })).toBeInTheDocument();
  });
  it("collapses and re-expands a folder", () => {
    setup();
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "notes" }));
    expect(
      screen.queryByRole("button", { name: "ideas" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "notes" }));
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
  });
  it("opens a note on click (full path)", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "ideas" }));
    expect(props.onOpen).toHaveBeenCalledWith("notes/ideas.md");
  });
  it("requests a new note in a folder via the folder +", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "new note in notes" }));
    expect(props.onRequestNewInFolder).toHaveBeenCalledWith("notes");
  });
  it("deletes a note by full path without opening it", () => {
    const props = setup();
    fireEvent.click(
      screen.getByRole("button", { name: "delete notes/ideas.md" }),
    );
    expect(props.onDelete).toHaveBeenCalledWith("notes/ideas.md");
    expect(props.onOpen).not.toHaveBeenCalled();
  });
  it("requests a global new note", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "+ New note" }));
    expect(props.onRequestNew).toHaveBeenCalled();
  });
  it("reveals the active note by expanding its collapsed ancestors", () => {
    localStorage.setItem("cairn.folderTree", JSON.stringify(["notes"]));
    setup({ activePath: "notes/ideas.md" });
    expect(screen.getByRole("button", { name: "ideas" })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- FolderTree` — expect FAIL (module not found).

- [ ] **Step 3: Implement `FolderTree.tsx`**

Create `web/src/components/tree/FolderTree.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react";
import { Button } from "../ui/Button";
import { SectionLabel } from "../ui/SectionLabel";
import { buildTree, ancestorFolders, type TreeNode } from "./folderTree";
import { loadCollapsed, saveCollapsed } from "./treePersistence";

export function FolderTree(props: {
  paths: string[];
  activePath: string | null;
  onOpen: (path: string) => void;
  onDelete: (path: string) => void;
  onRequestNew: () => void;
  onRequestNewInFolder: (folderPath: string) => void;
}) {
  const tree = useMemo(() => buildTree(props.paths), [props.paths]);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => loadCollapsed());

  // Reveal-on-change: when the active note changes, expand its ancestor folders
  // (so a newly-opened or restored-on-load note is visible). Runs only on change,
  // so the user can re-collapse afterward.
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

  const renderNodes = (nodes: TreeNode[], depth: number): React.ReactNode =>
    nodes.map((node) => {
      const pad = { paddingLeft: depth * 12 + 8 };
      if (node.kind === "folder") {
        const isCollapsed = collapsed.has(node.path);
        return (
          <div key={node.path}>
            <div className="group flex items-center justify-between rounded pr-2 text-muted hover:bg-surface-2 hover:text-text">
              <button
                className="flex min-w-0 flex-1 items-center gap-1 py-1 text-left"
                style={pad}
                title={node.path}
                onClick={() => toggle(node.path)}
              >
                <span aria-hidden="true" className="text-faint">
                  {isCollapsed ? "▸" : "▾"}
                </span>
                <span className="truncate">{node.name}</span>
              </button>
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
          className={`group flex items-center justify-between rounded pr-2 ${
            active
              ? "bg-surface-2 text-text"
              : "text-muted hover:bg-surface-2 hover:text-text"
          }`}
        >
          <button
            className="min-w-0 flex-1 truncate py-1 text-left"
            style={pad}
            title={node.path}
            onClick={() => props.onOpen(node.path)}
          >
            {node.name}
          </button>
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
      <div className="mb-1 flex items-center justify-between">
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

Run: `pnpm test -- FolderTree` — expect PASS (7 tests).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/tree/FolderTree.tsx web/src/components/tree/FolderTree.test.tsx
git commit -m "feat(tree): recursive FolderTree component (collapse, reveal-on-open, new-in-folder)"
```

---

## Task 4: NewNoteDialog `initialPath`

**Files:**
- Modify: `web/src/components/NewNoteDialog.tsx`
- Create: `web/src/components/NewNoteDialog.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/NewNoteDialog.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { NewNoteDialog } from "./NewNoteDialog";

describe("NewNoteDialog", () => {
  it("seeds the input from initialPath when opened", () => {
    render(
      <NewNoteDialog
        open={true}
        initialPath="projects/"
        onOpenChange={vi.fn()}
        onCreate={vi.fn()}
      />,
    );
    expect(screen.getByPlaceholderText("notes/idea.md")).toHaveValue(
      "projects/",
    );
  });
  it("creates the typed path and closes", () => {
    const onCreate = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <NewNoteDialog
        open={true}
        onOpenChange={onOpenChange}
        onCreate={onCreate}
      />,
    );
    const input = screen.getByPlaceholderText("notes/idea.md");
    fireEvent.change(input, { target: { value: "fresh.md" } });
    fireEvent.click(screen.getByRole("button", { name: /^create$/i }));
    expect(onCreate).toHaveBeenCalledWith("fresh.md");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- NewNoteDialog` — expect FAIL (the seed test fails: input is empty, `initialPath` not supported yet).

- [ ] **Step 3: Implement the change**

Replace the ENTIRE contents of `web/src/components/NewNoteDialog.tsx` with:

```tsx
import { useEffect, useState } from "react";
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

export function NewNoteDialog({
  open,
  onOpenChange,
  onCreate,
  initialPath = "",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (path: string) => void;
  initialPath?: string;
}) {
  const [path, setPath] = useState(initialPath);
  // Re-seed the field each time the dialog opens (empty for the global button,
  // "<folder>/" for the per-folder +).
  useEffect(() => {
    if (open) setPath(initialPath);
  }, [open, initialPath]);
  const close = () => onOpenChange(false);
  const submit = () => {
    const p = path.trim();
    if (!p) return;
    onCreate(p);
    close();
  };
  return (
    <Modal
      open={open}
      onClose={close}
      title="New note"
      description="Path inside the cairn"
    >
      <Input
        autoFocus
        placeholder="notes/idea.md"
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!path.trim()} onClick={submit}>
          Create
        </Button>
      </div>
    </Modal>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- NewNoteDialog` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/NewNoteDialog.tsx web/src/components/NewNoteDialog.test.tsx
git commit -m "feat(tree): NewNoteDialog initialPath (pre-fill for new-note-in-folder)"
```

---

## Task 5: Swap FolderTree into App + fixture + e2e migration

**Files:**
- Modify: `web/src/app/App.tsx`
- Delete: `web/src/components/NoteList.tsx`, `web/src/components/NoteList.test.tsx`
- Modify: `web/src/client/fixtures.ts`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Wire FolderTree into App**

In `web/src/app/App.tsx`:

(a) Replace the import line `import { NoteList } from "../components/NoteList";` with:
```tsx
import { FolderTree } from "../components/tree/FolderTree";
```

(b) Add new-note initial-path state next to the other `useState`s (e.g. after `const [newNoteOpen, setNewNoteOpen] = useState(false);`):
```tsx
  const [newNoteInitial, setNewNoteInitial] = useState("");
```

(c) Replace the `list={ <NoteList ... /> }` block:
```tsx
        list={
          <NoteList
            paths={notePaths}
            activePath={activePath}
            onOpen={actions.openNote}
            onRequestNew={() => setNewNoteOpen(true)}
            onDelete={actions.deleteNote}
          />
        }
```
with:
```tsx
        list={
          <FolderTree
            paths={notePaths}
            activePath={activePath}
            onOpen={actions.openNote}
            onDelete={actions.deleteNote}
            onRequestNew={() => {
              setNewNoteInitial("");
              setNewNoteOpen(true);
            }}
            onRequestNewInFolder={(folder) => {
              setNewNoteInitial(folder + "/");
              setNewNoteOpen(true);
            }}
          />
        }
```

(d) Pass `initialPath` to the dialog — change:
```tsx
      <NewNoteDialog
        open={newNoteOpen}
        onOpenChange={setNewNoteOpen}
        onCreate={actions.createNote}
      />
```
to:
```tsx
      <NewNoteDialog
        open={newNoteOpen}
        onOpenChange={setNewNoteOpen}
        initialPath={newNoteInitial}
        onCreate={actions.createNote}
      />
```

(e) The palette's `runCommand` `new-note` case opens the dialog — make it reset the initial path so the palette always opens it empty. Change:
```tsx
      case "new-note":
        setNewNoteOpen(true);
        break;
```
to:
```tsx
      case "new-note":
        setNewNoteInitial("");
        setNewNoteOpen(true);
        break;
```

- [ ] **Step 2: Delete NoteList**

```bash
git rm web/src/components/NoteList.tsx web/src/components/NoteList.test.tsx
```

- [ ] **Step 3: Add a nested fixture note**

In `web/src/client/fixtures.ts`, add this entry to the `FIXTURE_NOTES` object (e.g. right after the `"todo.md"` line). It is self-contained (no wikilinks) so it doesn't affect existing backlink/search assertions:
```ts
  "projects/demo.md": "# Demo\n\nA standalone nested note.",
```

- [ ] **Step 4: Unit gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS. (The NoteList tests are gone; FolderTree + dialog tests cover the sidebar.)

- [ ] **Step 5: Migrate the e2e sidebar selectors**

The sidebar now shows **stems** (`ideas`) instead of full paths (`ideas.md`). In `web/e2e/skeleton.spec.ts`, apply EXACTLY these changes. **Only sidebar (note-list) selectors change — leave backlinks (`aside.last()`), search-results (`results.…`), and the NewNoteDialog placeholder/fill untouched.**

For each test that opens or asserts notes in the left sidebar, add a sidebar locator at its top and use it:
```ts
const sidebar = page.locator("aside").first();
```
Then apply this mapping (sidebar note-list usages only):

| Find (sidebar usage) | Replace with |
|---|---|
| `page.getByText("index.md")` (app-loaded checks) | `sidebar.getByText("index", { exact: true })` |
| `page.getByText("ideas.md")` (app-loaded checks) | `sidebar.getByText("ideas", { exact: true })` |
| `page.getByRole("button", { name: "ideas.md" }).click()` | `sidebar.getByRole("button", { name: "ideas", exact: true }).click()` |
| `page.getByRole("button", { name: "index.md" }).click()` | `sidebar.getByRole("button", { name: "index", exact: true }).click()` |
| `page.getByRole("button", { name: "kitchensink.md" }).click()` | `sidebar.getByRole("button", { name: "kitchensink", exact: true }).click()` |
| In the tabs test, the existing `noteList` locator's `{ name: "index.md" }` / `{ name: "ideas.md" }` / `{ name: "todo.md" }` | `{ name: "index", exact: true }` / `{ name: "ideas", exact: true }` / `{ name: "todo", exact: true }` |

**Do NOT change** (different components, still full paths):
- `page.locator("aside").last().getByRole("button", { name: "index.md" })` and `… "fresh.md"` — backlinks.
- `results.getByRole("button", { name: "fresh.md" })` — search results overlay.
- `newNoteDialog.getByPlaceholder("notes/idea.md").fill("fresh.md")` — create flow.

After editing, every previously-`page.getByRole("button", { name: "<x>.md" })` that referred to the LEFT sidebar must be `sidebar.getByRole("button", { name: "<stem>", exact: true })`. (The `fresh.md` note created mid-test is a root note → its sidebar stem is `fresh`, but the existing tests only click `fresh.md` in the search-results overlay / backlinks, which stay unchanged.)

- [ ] **Step 6: Add the folder-tree e2e**

Append this test to `web/e2e/skeleton.spec.ts`:
```ts
test("folder tree: nested note shows under its folder; collapse hides it; folder + pre-fills", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  // The nested fixture note appears under its folder.
  await expect(sidebar.getByRole("button", { name: "projects" })).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "demo", exact: true }),
  ).toBeVisible();

  // Collapsing the folder hides its child.
  await sidebar.getByRole("button", { name: "projects" }).click();
  await expect(
    sidebar.getByRole("button", { name: "demo", exact: true }),
  ).toHaveCount(0);

  // Re-expand, then the folder + opens the dialog pre-filled with "projects/".
  await sidebar.getByRole("button", { name: "projects" }).click();
  await sidebar.getByRole("button", { name: "projects" }).hover();
  await sidebar.getByRole("button", { name: "new note in projects" }).click();
  // The dialog's input (placeholder "notes/idea.md") is pre-filled with the folder.
  await expect(page.getByPlaceholder("notes/idea.md")).toHaveValue("projects/");
});
```

- [ ] **Step 7: Run e2e**

Run: `pnpm e2e` — expect 12 passed (11 existing, migrated + the new folder-tree test). If port 5273 is busy: `lsof -ti :5273 | xargs kill 2>/dev/null` then retry once.
- If a migrated test fails because a sidebar selector still uses `.md`, fix that selector (don't weaken it).
- If the folder `+` click fails as "not visible", ensure the `hover()` on the folder row precedes it (the `+` is `group-hover:block`).
- If a core assertion in the new test fails (folder shows / collapse hides / dialog pre-filled), STOP and report.

- [ ] **Step 8: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 9: Manual/visual check**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); confirm the dev log is error-free; stop it. Report the app loads. (Human confirms: the sidebar shows a `projects` folder containing `demo`; folders collapse/expand and persist on reload; clicking a note opens it; hovering a folder reveals `+` which pre-fills the dialog; opening a note inside a collapsed folder reveals it.)

- [ ] **Step 10: Commit**

```bash
git add web/src/app/App.tsx web/src/client/fixtures.ts web/e2e/skeleton.spec.ts
git commit -m "feat(tree): swap FolderTree into the sidebar + nested fixture + e2e migration"
```

---

## Notes for the executor

- **No store/contract/backend changes.** The tree is built from the existing `notePaths`; `createNote` already accepts nested paths; the backend makes folders implicitly.
- **jsdom vs browser for hover-revealed buttons.** Unit tests find the `+`/`✕` regardless (Tailwind not applied in jsdom). The e2e must `hover()` the folder row before clicking its `+`.
- **Accessible names = stems.** Note rows' accessible name is the stem (the disclosure ▸/▾ is `aria-hidden`), and the delete button keeps a full-path aria-label (`delete <path>`). This is why the e2e migrates open-clicks to stems but delete/backlinks selectors are unaffected.
- **Reveal-on-change** expands the active note's ancestors only when `activePath` changes (the `useEffect` dep), so the user can still collapse the active note's folder.
- **The e2e migration is the main risk** — run the full `pnpm e2e` and keep all pre-existing tests green; only the sidebar selectors should change.
```
