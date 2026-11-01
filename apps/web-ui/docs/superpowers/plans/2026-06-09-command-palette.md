# Command Palette Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A ⌘K/Ctrl-K command palette: an overlay with a search input and a unified, fuzzy-filtered list of commands + notes (run an action or jump to a note, keyboard-first).

**Architecture:** A pure `fuzzy.ts` (subsequence score + filter); a `CommandPalette` Radix overlay (input + grouped list + keyboard nav); App owns the open-state + a global ⌘K listener + the command list. The New-note/Commit dialogs are lifted from NoteList/CommitBar to App so both the toolbar buttons and the palette share them.

**Tech Stack:** React 18 + TypeScript, `@radix-ui/react-dialog`, Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-09-command-palette-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if needed.
- e2e on port 5273. Current: 168 unit, 9 e2e, all green.
- **Relevant existing code:**
  - `stem(path)` from `web/src/client/wikilink.ts` (note path → display label).
  - `Modal`/`SettingsDialog` use `@radix-ui/react-dialog`; `vitest.setup.ts` has the Radix jsdom polyfills (pointer capture / scrollIntoView / ResizeObserver) already.
  - `NoteList` owns `NewNoteDialog` (local `newOpen` state; "+ New note" button); `CommitBar` owns `CommitDialog` (local `commitOpen`; "Commit" button). These get lifted to App in Task 3.
  - `App.tsx` already selects `notePaths`, `committing`, `editorMode` (via settings), `view`/`setView`, and `actions` (createNote, commitManual, openNote, setSettings, loadGraph). It already renders `SettingsDialog` (App-owned `settingsOpen`).
  - `NewNoteDialog` props `{open, onOpenChange, onCreate}`; `CommitDialog` props `{open, onOpenChange, committing, onCommit}`.
- The palette is plain DOM (Radix) — fully unit-testable (unlike the canvas graph).

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/command-palette/fuzzy.ts` | Pure `fuzzyScore` + `filterItems`. |
| `web/src/components/command-palette/CommandPalette.tsx` | Radix overlay: input + grouped list + keyboard nav. |
| `web/src/components/NoteList.tsx` | "+ New note" → `onRequestNew` (dialog lifted to App). |
| `web/src/components/CommitBar.tsx` | "Commit" → `onRequestCommit` (dialog lifted to App). |
| `web/src/app/App.tsx` | Palette open-state + ⌘K + command list + lifted dialogs. |
| `web/e2e/skeleton.spec.ts` | ⌘K palette flows. |

---

## Task 1: fuzzy (pure)

**Files:**
- Create: `web/src/components/command-palette/fuzzy.ts`
- Create: `web/src/components/command-palette/fuzzy.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/command-palette/fuzzy.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { fuzzyScore, filterItems } from "./fuzzy";

describe("fuzzyScore", () => {
  it("matches a subsequence (case-insensitive)", () => {
    expect(fuzzyScore("comm", "Commit changes")).not.toBeNull();
    expect(fuzzyScore("ide", "ideas")).not.toBeNull();
  });
  it("returns null when not a subsequence", () => {
    expect(fuzzyScore("xyz", "ideas")).toBeNull();
    expect(fuzzyScore("idx", "ideas")).toBeNull();
  });
  it("ranks a contiguous prefix match above a scattered one", () => {
    const contiguous = fuzzyScore("co", "Commit changes")!;
    const scattered = fuzzyScore("cc", "Commit changes")!; // c…c
    expect(contiguous).toBeGreaterThan(scattered);
  });
  it("an empty query matches everything with a neutral score", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });
});

describe("filterItems", () => {
  const items = ["Commit changes", "New note", "Open Settings"];
  it("drops non-matches and sorts by score", () => {
    expect(filterItems(items, "note", (s) => s)).toEqual(["New note"]);
  });
  it("returns all items (original order) for an empty query", () => {
    expect(filterItems(items, "", (s) => s)).toEqual(items);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- fuzzy`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `fuzzy.ts`**

Create `web/src/components/command-palette/fuzzy.ts`:

```ts
/** Case-insensitive subsequence match. Returns a score (higher = better; rewards
 *  contiguous runs and word-start/prefix matches) or null if `query` is not a
 *  subsequence of `text`. An empty query scores a neutral 0 (matches all). */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  if (q === "") return 0;
  const t = text.toLowerCase();

  let qi = 0;
  let score = 0;
  let run = 0;
  let prev = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      run = ti === prev + 1 ? run + 1 : 0;
      let bonus = 1 + run; // contiguous-run bonus
      if (ti === 0 || /[^a-z0-9]/.test(t[ti - 1])) bonus += 3; // word start
      if (ti === 0) bonus += 2; // prefix
      score += bonus;
      prev = ti;
      qi++;
    }
  }
  return qi === q.length ? score : null;
}

/** Score each item by its searchable text, drop non-matches, sort by score desc
 *  then text asc. Empty query → all items in their original order. */
export function filterItems<T>(
  items: T[],
  query: string,
  text: (item: T) => string,
): T[] {
  if (query.trim() === "") return items;
  return items
    .map((item) => ({ item, score: fuzzyScore(query, text(item)) }))
    .filter((x): x is { item: T; score: number } => x.score !== null)
    .sort(
      (a, b) => b.score - a.score || text(a.item).localeCompare(text(b.item)),
    )
    .map((x) => x.item);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- fuzzy`
Expected: PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (168 + new fuzzy tests).

- [ ] **Step 6: Commit**

```bash
git add web/src/components/command-palette/fuzzy.ts web/src/components/command-palette/fuzzy.test.ts
git commit -m "feat(palette): fuzzy subsequence match + filter"
```

---

## Task 2: CommandPalette component

**Files:**
- Create: `web/src/components/command-palette/CommandPalette.tsx`
- Create: `web/src/components/command-palette/CommandPalette.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/command-palette/CommandPalette.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CommandPalette } from "./CommandPalette";

const commands = [
  { id: "new-note", label: "New note" },
  { id: "commit", label: "Commit changes" },
];
const notes = ["index.md", "ideas.md"];

function setup(over: Partial<React.ComponentProps<typeof CommandPalette>> = {}) {
  const props = {
    open: true,
    onClose: vi.fn(),
    commands,
    notes,
    onRunCommand: vi.fn(),
    onOpenNote: vi.fn(),
    ...over,
  };
  render(<CommandPalette {...props} />);
  return props;
}

describe("CommandPalette", () => {
  it("shows commands and notes when open", () => {
    setup();
    expect(screen.getByText("New note")).toBeInTheDocument();
    expect(screen.getByText("Commit changes")).toBeInTheDocument();
    expect(screen.getByText("ideas")).toBeInTheDocument(); // stem label
  });
  it("filters by the query and runs the matching command on Enter", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.change(input, { target: { value: "commit" } });
    expect(screen.queryByText("New note")).not.toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onRunCommand).toHaveBeenCalledWith("commit");
    expect(props.onClose).toHaveBeenCalled();
  });
  it("opens a note on Enter when a note is highlighted", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.change(input, { target: { value: "ideas" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onOpenNote).toHaveBeenCalledWith("ideas.md");
  });
  it("ArrowDown moves the highlight before running", () => {
    const props = setup();
    const input = screen.getByPlaceholderText(/type a command/i);
    fireEvent.keyDown(input, { key: "ArrowDown" }); // 1st → 2nd item
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onRunCommand).toHaveBeenCalledWith("commit");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- CommandPalette`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `CommandPalette.tsx`**

Create `web/src/components/command-palette/CommandPalette.tsx`:

```tsx
import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useMemo, useState } from "react";
import { stem } from "../../client/wikilink";
import { filterItems } from "./fuzzy";

export interface PaletteCommand {
  id: string;
  label: string;
}

type Item =
  | { kind: "command"; id: string; label: string }
  | { kind: "note"; id: string; label: string; path: string };

const EMPTY_NOTE_CAP = 6;

export function CommandPalette(props: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
  notes: string[];
  onRunCommand: (id: string) => void;
  onOpenNote: (path: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  // Reset query + highlight whenever the palette opens.
  useEffect(() => {
    if (props.open) {
      setQuery("");
      setIndex(0);
    }
  }, [props.open]);

  const results = useMemo<Item[]>(() => {
    const cmdItems: Item[] = props.commands.map((c) => ({
      kind: "command",
      id: c.id,
      label: c.label,
    }));
    const noteItems: Item[] = props.notes.map((p) => ({
      kind: "note",
      id: p,
      label: stem(p),
      path: p,
    }));
    const text = (i: Item) =>
      i.kind === "note" ? `${i.label} ${i.path}` : i.label;
    if (query.trim() === "") {
      return [...cmdItems, ...noteItems.slice(0, EMPTY_NOTE_CAP)];
    }
    return filterItems([...cmdItems, ...noteItems], query, text);
  }, [props.commands, props.notes, query]);

  // Keep the highlight in range when results shrink.
  useEffect(() => {
    setIndex((i) => Math.min(i, Math.max(0, results.length - 1)));
  }, [results.length]);

  const run = (item: Item | undefined) => {
    if (!item) return;
    if (item.kind === "command") props.onRunCommand(item.id);
    else props.onOpenNote(item.path);
    props.onClose();
  };

  const cmds = results.filter((r) => r.kind === "command");
  const noteRes = results.filter((r) => r.kind === "note");

  return (
    <Dialog.Root
      open={props.open}
      onOpenChange={(o) => {
        if (!o) props.onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed left-1/2 top-[15%] z-50 w-[min(92vw,520px)] -translate-x-1/2 overflow-hidden rounded-xl border border-border bg-surface text-text shadow-2xl focus:outline-none"
        >
          <Dialog.Title className="sr-only">Command palette</Dialog.Title>
          <input
            autoFocus
            className="w-full border-b border-border bg-transparent px-4 py-3 text-sm text-text placeholder:text-faint focus:outline-none"
            placeholder="Type a command or search notes…"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(i + 1, results.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                run(results[index]);
              }
            }}
          />
          <div className="max-h-80 overflow-y-auto py-1">
            {results.length === 0 && (
              <div className="px-4 py-3 text-xs text-faint">No matches</div>
            )}
            {cmds.length > 0 && (
              <Group label="Commands">
                {cmds.map((item) => (
                  <Row
                    key={item.id}
                    selected={results[index] === item}
                    onClick={() => run(item)}
                  >
                    {item.label}
                  </Row>
                ))}
              </Group>
            )}
            {noteRes.length > 0 && (
              <Group label="Notes">
                {noteRes.map((item) => (
                  <Row
                    key={item.id}
                    selected={results[index] === item}
                    onClick={() => run(item)}
                  >
                    {item.label}
                    <span className="ml-auto text-[11px] text-faint">
                      {item.kind === "note" ? item.path : ""}
                    </span>
                  </Row>
                ))}
              </Group>
            )}
          </div>
          <div className="flex gap-4 border-t border-border px-4 py-2 text-[10px] text-faint">
            <span>↑↓ navigate</span>
            <span>↵ run / open</span>
            <span>esc close</span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Group(props: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="px-4 pb-1 pt-2 text-[9px] uppercase tracking-wide text-faint">
        {props.label}
      </div>
      {props.children}
    </div>
  );
}

function Row(props: {
  selected: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={
        "flex w-full items-center gap-2 px-4 py-1.5 text-left text-[13px] " +
        (props.selected ? "bg-surface-2 text-text" : "text-muted hover:bg-surface-2")
      }
      onMouseDown={(e) => {
        // run on mousedown so the input doesn't blur-close first
        e.preventDefault();
        props.onClick();
      }}
    >
      {props.children}
    </button>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- CommandPalette`
Expected: PASS (4 tests). If the Radix dialog needs a jsdom polyfill not already present, add it to `vitest.setup.ts` (the existing Radix polyfills should cover it).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/command-palette/CommandPalette.tsx web/src/components/command-palette/CommandPalette.test.tsx
git commit -m "feat(palette): CommandPalette overlay (input + list + keyboard nav)"
```

---

## Task 3: Lift the New-note + Commit dialogs to App

**Files:**
- Modify: `web/src/components/NoteList.tsx` (+ `NoteList.test.tsx`)
- Modify: `web/src/components/CommitBar.tsx` (+ `CommitBar.test.tsx`)
- Modify: `web/src/app/App.tsx`

This is a behavior-preserving refactor (the toolbar buttons still open the dialogs — now rendered by App). Do it before adding the palette so it's verifiable on its own.

- [ ] **Step 1: NoteList — call a request callback instead of owning the dialog**

Replace `web/src/components/NoteList.tsx` with:

```tsx
import { Button } from "./ui/Button";
import { SectionLabel } from "./ui/SectionLabel";

export function NoteList(props: {
  paths: string[];
  activePath: string | null;
  onOpen: (path: string) => void;
  onRequestNew: () => void;
  onDelete: (path: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <div className="mb-1 flex items-center justify-between">
        <SectionLabel>Notes</SectionLabel>
        <Button variant="ghost" onClick={props.onRequestNew}>
          + New note
        </Button>
      </div>
      {props.paths.map((path) => (
        <div
          key={path}
          className={`group flex items-center justify-between rounded px-2 py-1 ${
            path === props.activePath
              ? "bg-surface-2 text-text"
              : "text-muted hover:bg-surface-2 hover:text-text"
          }`}
        >
          <button
            className="min-w-0 flex-1 truncate text-left"
            onClick={() => props.onOpen(path)}
          >
            {path}
          </button>
          <button
            className="ml-1 hidden text-faint hover:text-danger group-hover:block"
            aria-label={`delete ${path}`}
            onClick={() => props.onDelete(path)}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Update `NoteList.test.tsx`**

The new-note flow now lives in App. Update the test so the "+ New note" button fires `onRequestNew` (remove any dialog-interaction assertions). Open `web/src/components/NoteList.test.tsx`, replace the `onNew` mock with `onRequestNew`, and the new-note test with:

```tsx
it("requests a new note when '+ New note' is clicked", () => {
  const onRequestNew = vi.fn();
  render(
    <NoteList
      paths={["a.md"]}
      activePath={null}
      onOpen={vi.fn()}
      onRequestNew={onRequestNew}
      onDelete={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /new note/i }));
  expect(onRequestNew).toHaveBeenCalled();
});
```

Keep the other NoteList tests (list rendering, open, delete) but change their props to the new shape (`onRequestNew` instead of `onNew`). Remove any test that drove the NewNoteDialog through NoteList.

- [ ] **Step 3: CommitBar — call a request callback instead of owning the dialog**

Replace `web/src/components/CommitBar.tsx` with:

```tsx
import { Button } from "./ui/Button";

export function CommitBar(props: {
  saving: boolean;
  dirty: boolean;
  uncommitted: boolean;
  lastCommit: string | null;
  committing: boolean;
  onRequestCommit: () => void;
}) {
  const status = props.saving
    ? "Saving…"
    : props.dirty
      ? "Unsaved"
      : props.uncommitted
        ? "Saved · uncommitted"
        : "Saved";
  return (
    <div className="flex items-center gap-3 text-xs">
      <span className="text-muted">{status}</span>
      {props.lastCommit && (
        <span className="text-faint">@{props.lastCommit}</span>
      )}
      <Button
        variant="primary"
        disabled={props.committing}
        onClick={props.onRequestCommit}
      >
        Commit
      </Button>
    </div>
  );
}
```

- [ ] **Step 4: Update `CommitBar.test.tsx`**

Replace the `onCommit`/dialog-flow assertions with a request-callback test; keep the status-display tests. Open `web/src/components/CommitBar.test.tsx`; for the commit action test use:

```tsx
it("requests a commit when 'Commit' is clicked", () => {
  const onRequestCommit = vi.fn();
  render(
    <CommitBar
      saving={false}
      dirty={false}
      uncommitted={true}
      lastCommit={null}
      committing={false}
      onRequestCommit={onRequestCommit}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^commit$/i }));
  expect(onRequestCommit).toHaveBeenCalled();
});
```

Change any other CommitBar test props to the new shape (`onRequestCommit` instead of `onCommit`). Remove any test that drove CommitDialog through CommitBar.

- [ ] **Step 5: App — own the dialogs + wire the request callbacks**

In `web/src/app/App.tsx`:

Add imports:

```tsx
import { NewNoteDialog } from "../components/NewNoteDialog";
import { CommitDialog } from "../components/CommitDialog";
```

Add state (with the other `useState`s):

```tsx
  const [newNoteOpen, setNewNoteOpen] = useState(false);
  const [commitOpen, setCommitOpen] = useState(false);
```

Change the `<NoteList … />` props: replace `onNew={actions.createNote}` with `onRequestNew={() => setNewNoteOpen(true)}`.

Change the `<CommitBar … />` props: replace `onCommit={actions.commitManual}` with `onRequestCommit={() => setCommitOpen(true)}`. (Keep `saving`/`dirty`/`uncommitted`/`lastCommit`/`committing`.)

Render the lifted dialogs in the top-level fragment, next to `<SettingsDialog/>` and `<ErrorToast/>`:

```tsx
      <NewNoteDialog
        open={newNoteOpen}
        onOpenChange={setNewNoteOpen}
        onCreate={actions.createNote}
      />
      <CommitDialog
        open={commitOpen}
        onOpenChange={setCommitOpen}
        committing={committing}
        onCommit={actions.commitManual}
      />
```

(`committing` is already selected in App for `<CommitBar/>`. If it isn't a standalone variable, add `const committing = useCairn((s) => s.committing);`.)

- [ ] **Step 6: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Then: `pnpm e2e`
Expected: all PASS — the existing new-note + commit e2e flows (open the dialog from the toolbar, fill, submit) still work because the dialogs now render at App and the toolbar buttons open them. 9/9 e2e. If a dialog no longer opens from the toolbar, fix the wiring before moving on.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/NoteList.tsx web/src/components/NoteList.test.tsx web/src/components/CommitBar.tsx web/src/components/CommitBar.test.tsx web/src/app/App.tsx
git commit -m "refactor: lift New-note + Commit dialogs to App (shareable by the palette)"
```

---

## Task 4: Wire the palette into App + ⌘K + e2e

**Files:**
- Modify: `web/src/app/App.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Add the palette state, ⌘K listener, command list, and render**

In `web/src/app/App.tsx`:

Add the import:

```tsx
import {
  CommandPalette,
  type PaletteCommand,
} from "../components/command-palette/CommandPalette";
```

Add state:

```tsx
  const [paletteOpen, setPaletteOpen] = useState(false);
```

Add a global ⌘K / Ctrl-K listener (with the other effects):

```tsx
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
```

Define the command list + dispatcher (place near the actions; `editorMode` and `view` are already in scope):

```tsx
  const COMMANDS: PaletteCommand[] = [
    { id: "new-note", label: "New note" },
    { id: "commit", label: "Commit changes…" },
    { id: "toggle-view", label: "Toggle Graph / Editor" },
    { id: "open-settings", label: "Open Settings" },
    { id: "toggle-editor-mode", label: "Toggle Source / Live preview" },
  ];
  const runCommand = (id: string) => {
    switch (id) {
      case "new-note":
        setNewNoteOpen(true);
        break;
      case "commit":
        setCommitOpen(true);
        break;
      case "toggle-view":
        setView((v) => {
          const next = v === "graph" ? "editor" : "graph";
          if (next === "graph") void actions.loadGraph();
          return next;
        });
        break;
      case "open-settings":
        setSettingsOpen(true);
        break;
      case "toggle-editor-mode":
        actions.setSettings({
          editorMode: editorMode === "livepreview" ? "source" : "livepreview",
        });
        break;
    }
    setPaletteOpen(false);
  };
```

Render the palette in the top-level fragment (next to the dialogs):

```tsx
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={COMMANDS}
        notes={notePaths}
        onRunCommand={runCommand}
        onOpenNote={(p) => {
          void actions.openNote(p);
          setView("editor");
          setPaletteOpen(false);
        }}
      />
```

(`setView` here is the state setter; if `setView` is currently used only as `setView("editor")` elsewhere, the functional form in `toggle-view` is still valid. `settingsOpen`/`setSettingsOpen` already exist from the SettingsDialog wiring.)

- [ ] **Step 2: Gate (palette is plain DOM but the wiring is App-level — build confirms)**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 3: Add the e2e**

In `web/e2e/skeleton.spec.ts`, add a new test:

```ts
test("command palette: ⌘K quick-opens a note and runs a command", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("ideas.md")).toBeVisible(); // app loaded

  // ⌘K (Meta on mac, Control elsewhere — send both is harmless; use Control for CI).
  await page.keyboard.press("Control+k");
  const input = page.getByPlaceholder(/type a command/i);
  await expect(input).toBeVisible();

  // Quick-open a note.
  await input.fill("ideas");
  await page.keyboard.press("Enter");
  await expect(page.locator(".cm-content")).toContainText("Ideas"); // ideas.md opened

  // Re-open, run the Commit command → the commit dialog appears.
  await page.keyboard.press("Control+k");
  await page.getByPlaceholder(/type a command/i).fill("commit");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("button", { name: /^commit$/i }).last(),
  ).toBeVisible(); // commit dialog's submit button
});
```

(If `Control+k` doesn't trigger on the test platform, use `Meta+k`; the App listener accepts either. The note-opened assertion mirrors the existing live-preview e2e — `ideas.md` renders "Ideas". The commit assertion checks the dialog opened.)

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e`
Expected: 10/10 (9 existing + the new palette test). If ⌘K doesn't open the palette (listener not firing / key combo), debug the listener; if a note doesn't open or the commit dialog doesn't appear, STOP and report.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 6: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop it. Report the app loads. (The human confirms: ⌘K opens the palette; typing filters commands + notes; ↑/↓ + Enter run/open; each command works; Esc/backdrop close.)

- [ ] **Step 7: Commit**

```bash
git add web/src/app/App.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(palette): ⌘K command palette wired into the app"
```

---

## Notes for the executor

- **Dialog-lift is behavior-preserving** — the toolbar "+ New note" / "Commit" buttons must still open the dialogs (now App-owned). The existing dialog e2e flows are the regression guard; keep them green in Task 3 BEFORE adding the palette.
- **Global ⌘K** uses a `window` keydown listener with `preventDefault` so the browser/OS and CodeMirror don't swallow it; clean it up on unmount. It toggles the palette.
- **Run-on-mousedown** in the palette rows (not click) so selecting a row doesn't blur the input and close the dialog before the action fires.
- **Reset on open** — the palette clears its query + highlight each time it opens (the `useEffect` on `open`).
- **No new deps** — reuse `@radix-ui/react-dialog` (Radix jsdom polyfills already in `vitest.setup.ts`).
- **Commands are label-only in v1** (no icons) — `PaletteCommand = {id,label}`. Icons are optional polish, not in scope.
- **Empty-state note cap** = first 6 notes (no recency model). Typing fuzzy-filters all notes + commands.
