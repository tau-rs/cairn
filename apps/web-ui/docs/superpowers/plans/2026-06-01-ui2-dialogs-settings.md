# Cairn UI‑2: In-App Dialogs + Settings Home Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the native `window.prompt` new-note and commit dialogs with styled in-app modals, and move auto-commit settings out of the Backlinks pane into a navbar gear → Settings modal.

**Architecture:** A `Modal` primitive wraps `@radix-ui/react-dialog`, skinned with UI‑1 tokens. `NewNoteDialog`/`CommitDialog`/`SettingsDialog` compose `Modal` + UI‑1 primitives. `NoteList`/`CommitBar` open their dialog from local state (callbacks unchanged); `App` adds a gear `IconButton` opening `SettingsDialog` and drops `Settings` from the right pane (Backlinks-only).

**Tech Stack:** `@radix-ui/react-dialog` + UI‑1 primitives (React 18 + TS + Tailwind). Vitest + Testing Library (Radix renders under jsdom with small polyfills); Playwright e2e.

**Reference:** Spec `docs/superpowers/specs/2026-06-01-ui2-dialogs-settings-design.md`. UI‑1 primitives exist: `ui/Button` (variant primary|secondary|ghost), `ui/IconButton` ({label, children}), `ui/Input`, `ui/SectionLabel`. `Settings.tsx` (auto-commit fields) is reused unchanged inside `SettingsDialog`. All work under `web/`; run from `web/`.

---

## File Structure

```
web/src/components/ui/Modal.tsx           NEW  Radix-backed modal (+ Modal.test.tsx)
web/src/components/NewNoteDialog.tsx      NEW  (+ test)
web/src/components/CommitDialog.tsx       NEW  (+ test)
web/src/components/SettingsDialog.tsx     NEW  (+ test)
web/src/components/NoteList.tsx           MOD  open NewNoteDialog instead of prompt (+ rewrite test)
web/src/components/CommitBar.tsx          MOD  open CommitDialog instead of prompt (+ rewrite test)
web/src/app/App.tsx                       MOD  navbar gear + SettingsDialog; Backlinks-only right pane
web/src/vitest.setup.ts                   MOD  jsdom polyfills for Radix
web/e2e/skeleton.spec.ts                  MOD  new-note + commit via modals
web/package.json                          MOD  + @radix-ui/react-dialog
```

---

## Task 1: @radix-ui/react-dialog + Modal primitive + jsdom polyfills

**Files:** Modify `web/package.json`, `web/src/vitest.setup.ts`; create `web/src/components/ui/Modal.tsx`, `web/src/components/ui/Modal.test.tsx`.

- [ ] **Step 1: Install Radix dialog**

From `web/`: `pnpm add @radix-ui/react-dialog`

- [ ] **Step 2: Add jsdom polyfills Radix needs**

Replace `web/src/vitest.setup.ts` with:
```ts
import "@testing-library/jest-dom/vitest";

// jsdom lacks these APIs that Radix's focus / dismissable-layer use in tests.
if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false;
}
if (!Element.prototype.releasePointerCapture) {
  Element.prototype.releasePointerCapture = () => {};
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}
```

- [ ] **Step 3: Write the failing test**

`web/src/components/ui/Modal.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Modal } from "./Modal";

describe("Modal", () => {
  it("renders title + children when open", () => {
    render(
      <Modal open onClose={vi.fn()} title="Hi">
        <div>body</div>
      </Modal>,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Hi")).toBeInTheDocument();
    expect(screen.getByText("body")).toBeInTheDocument();
  });

  it("renders nothing when closed", () => {
    render(
      <Modal open={false} onClose={vi.fn()} title="Hi">
        <div>body</div>
      </Modal>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("calls onClose on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Modal open onClose={onClose} title="Hi">
        <div>body</div>
      </Modal>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 4: Run test to verify it fails**

Run: `pnpm test -- "ui/Modal"`
Expected: FAIL — cannot find module `./Modal`.

- [ ] **Step 5: Implement the Modal**

`web/src/components/ui/Modal.tsx`:
```tsx
import * as Dialog from "@radix-ui/react-dialog";
import type { ReactNode } from "react";

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          // suppress Radix's missing-description warning when none is provided
          {...(description ? {} : { "aria-describedby": undefined })}
          className="fixed left-1/2 top-1/2 z-50 w-[min(92vw,360px)] -translate-x-1/2 -translate-y-1/2 rounded-xl border border-border bg-surface p-4 text-text shadow-2xl focus:outline-none"
        >
          <Dialog.Title className="text-sm font-semibold text-text">{title}</Dialog.Title>
          {description ? (
            <Dialog.Description className="mt-0.5 text-xs text-faint">
              {description}
            </Dialog.Description>
          ) : null}
          <div className="mt-3">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `pnpm test -- "ui/Modal"`
Expected: PASS (3 tests). If a Radix internal still throws under jsdom, add the missing API to `vitest.setup.ts` (same pattern as Step 2) — do not change the component.

- [ ] **Step 7: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml web/src/vitest.setup.ts web/src/components/ui/Modal.tsx web/src/components/ui/Modal.test.tsx
git commit -m "feat(ui): Modal primitive (Radix dialog) + jsdom polyfills"
```

---

## Task 2: NewNoteDialog

**Files:** Create `web/src/components/NewNoteDialog.tsx`, `web/src/components/NewNoteDialog.test.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/components/NewNoteDialog.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NewNoteDialog } from "./NewNoteDialog";

describe("NewNoteDialog", () => {
  it("creates from the typed path and closes", async () => {
    const onCreate = vi.fn();
    const onOpenChange = vi.fn();
    render(<NewNoteDialog open onOpenChange={onOpenChange} onCreate={onCreate} />);
    await userEvent.type(screen.getByPlaceholderText("notes/idea.md"), "a.md");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(onCreate).toHaveBeenCalledWith("a.md");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("disables Create when empty", () => {
    render(<NewNoteDialog open onOpenChange={vi.fn()} onCreate={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });

  it("Cancel closes without creating", async () => {
    const onCreate = vi.fn();
    const onOpenChange = vi.fn();
    render(<NewNoteDialog open onOpenChange={onOpenChange} onCreate={onCreate} />);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCreate).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- NewNoteDialog`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`web/src/components/NewNoteDialog.tsx`:
```tsx
import { useState } from "react";
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

export function NewNoteDialog({
  open,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (path: string) => void;
}) {
  const [path, setPath] = useState("");
  const submit = () => {
    const p = path.trim();
    if (!p) return;
    onCreate(p);
    setPath("");
    onOpenChange(false);
  };
  return (
    <Modal
      open={open}
      onClose={() => onOpenChange(false)}
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
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
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

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- NewNoteDialog`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/NewNoteDialog.tsx web/src/components/NewNoteDialog.test.tsx
git commit -m "feat: NewNoteDialog modal"
```

---

## Task 3: CommitDialog

**Files:** Create `web/src/components/CommitDialog.tsx`, `web/src/components/CommitDialog.test.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/components/CommitDialog.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CommitDialog } from "./CommitDialog";

describe("CommitDialog", () => {
  it("commits the typed message and closes", async () => {
    const onCommit = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <CommitDialog open committing={false} onOpenChange={onOpenChange} onCommit={onCommit} />,
    );
    await userEvent.type(screen.getByPlaceholderText("Describe this change"), "msg");
    await userEvent.click(screen.getByRole("button", { name: "Commit" }));
    expect(onCommit).toHaveBeenCalledWith("msg");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("disables Commit when empty or committing", () => {
    const { rerender } = render(
      <CommitDialog open committing={false} onOpenChange={vi.fn()} onCommit={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: "Commit" })).toBeDisabled(); // empty
    rerender(<CommitDialog open committing onOpenChange={vi.fn()} onCommit={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Commit" })).toBeDisabled(); // committing
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- CommitDialog`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`web/src/components/CommitDialog.tsx`:
```tsx
import { useState } from "react";
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

export function CommitDialog({
  open,
  onOpenChange,
  committing,
  onCommit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  committing: boolean;
  onCommit: (message: string) => void;
}) {
  const [msg, setMsg] = useState("");
  const submit = () => {
    const m = msg.trim();
    if (!m) return;
    onCommit(m);
    setMsg("");
    onOpenChange(false);
  };
  return (
    <Modal
      open={open}
      onClose={() => onOpenChange(false)}
      title="Commit"
      description="Describe this change"
    >
      <Input
        autoFocus
        placeholder="Describe this change"
        value={msg}
        onChange={(e) => setMsg(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!msg.trim() || committing} onClick={submit}>
          Commit
        </Button>
      </div>
    </Modal>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- CommitDialog`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/CommitDialog.tsx web/src/components/CommitDialog.test.tsx
git commit -m "feat: CommitDialog modal"
```

---

## Task 4: SettingsDialog

**Files:** Create `web/src/components/SettingsDialog.tsx`, `web/src/components/SettingsDialog.test.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/components/SettingsDialog.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsDialog } from "./SettingsDialog";
import { DEFAULT_SETTINGS } from "../store/store";

describe("SettingsDialog", () => {
  it("renders the auto-commit controls and Done closes", async () => {
    const onOpenChange = vi.fn();
    render(
      <SettingsDialog
        open
        onOpenChange={onOpenChange}
        settings={DEFAULT_SETTINGS}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText(/idle auto-commit/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- SettingsDialog`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`web/src/components/SettingsDialog.tsx`:
```tsx
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Settings } from "./Settings";
import type { Settings as SettingsType } from "../store/store";

export function SettingsDialog({
  open,
  onOpenChange,
  settings,
  onChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settings: SettingsType;
  onChange: (patch: Partial<SettingsType>) => void;
}) {
  return (
    <Modal open={open} onClose={() => onOpenChange(false)} title="Settings">
      <Settings settings={settings} onChange={onChange} />
      <div className="mt-3 flex justify-end">
        <Button variant="secondary" onClick={() => onOpenChange(false)}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- SettingsDialog`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/SettingsDialog.tsx web/src/components/SettingsDialog.test.tsx
git commit -m "feat: SettingsDialog modal wrapping auto-commit settings"
```

---

## Task 5: Wire NoteList + CommitBar to dialogs

**Files:** Modify `web/src/components/NoteList.tsx`, `web/src/components/NoteList.test.tsx`, `web/src/components/CommitBar.tsx`, `web/src/components/CommitBar.test.tsx`.

- [ ] **Step 1: Rewrite the NoteList new-note test**

In `web/src/components/NoteList.test.tsx`, replace the existing "+ New note"/`window.prompt` test (it currently spies on `window.prompt`) with the dialog flow; keep the open + delete tests unchanged:
```tsx
  it("opens the new-note dialog and creates a note", async () => {
    const onNew = vi.fn();
    render(
      <NoteList paths={[]} activePath={null} onOpen={vi.fn()} onNew={onNew} onDelete={vi.fn()} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /new note/i }));
    await userEvent.type(screen.getByPlaceholderText("notes/idea.md"), "new.md");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(onNew).toHaveBeenCalledWith("new.md");
  });
```
(Remove any `vi.spyOn(window, "prompt")` from this file.)

- [ ] **Step 2: Run to verify it fails**

Run: `pnpm test -- NoteList`
Expected: FAIL — no dialog yet (prompt still used).

- [ ] **Step 3: Wire NoteList**

In `web/src/components/NoteList.tsx`: add imports + state, and replace the "+ New note" `Button`'s `onClick` with opening the dialog; render `NewNoteDialog`:
```tsx
import { useState } from "react";
import { Button } from "./ui/Button";
import { SectionLabel } from "./ui/SectionLabel";
import { NewNoteDialog } from "./NewNoteDialog";
```
Add at the top of the component body: `const [newOpen, setNewOpen] = useState(false);`. Change the new-note `Button` to `onClick={() => setNewOpen(true)}` (drop the `window.prompt`). After the list (inside the root `<div>`), render:
```tsx
      <NewNoteDialog open={newOpen} onOpenChange={setNewOpen} onCreate={props.onNew} />
```

- [ ] **Step 4: Rewrite the CommitBar commit test**

In `web/src/components/CommitBar.test.tsx`, replace the `window.prompt` commit test with the dialog flow (keep the status-display tests). Note both the trigger and the dialog have a "Commit" button — scope the dialog one via `within`/role `dialog`:
```tsx
  it("opens the commit dialog and commits the message", async () => {
    const onCommit = vi.fn();
    render(
      <CommitBar
        saving={false}
        dirty={false}
        uncommitted
        lastCommit={null}
        committing={false}
        onCommit={onCommit}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Commit" })); // trigger
    await userEvent.type(screen.getByPlaceholderText("Describe this change"), "snapshot");
    const dialog = screen.getByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Commit" }));
    expect(onCommit).toHaveBeenCalledWith("snapshot");
  });
```
Add `within` to the testing-library import: `import { render, screen, within } from "@testing-library/react";`. Remove any `window.prompt` spy.

- [ ] **Step 5: Wire CommitBar**

In `web/src/components/CommitBar.tsx`: add `import { useState } from "react";` and `import { CommitDialog } from "./CommitDialog";`. Add `const [commitOpen, setCommitOpen] = useState(false);`. Change the Commit `Button` `onClick` to `() => setCommitOpen(true)` (drop `window.prompt`; keep `disabled={props.committing}`). Wrap the return in a fragment and render the dialog:
```tsx
  return (
    <>
      <div className="flex items-center gap-3 text-xs">
        {/* …status spans + the Commit Button (onClick=setCommitOpen(true))… */}
      </div>
      <CommitDialog
        open={commitOpen}
        onOpenChange={setCommitOpen}
        committing={props.committing}
        onCommit={props.onCommit}
      />
    </>
  );
```

- [ ] **Step 6: Run tests**

Run: `pnpm test -- NoteList` then `pnpm test -- CommitBar` then `pnpm test`
Expected: PASS (all). No `window.prompt` references remain: `grep -rn "window.prompt" web/src` → empty.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/NoteList.tsx web/src/components/NoteList.test.tsx web/src/components/CommitBar.tsx web/src/components/CommitBar.test.tsx
git commit -m "feat: NoteList + CommitBar use in-app dialogs (no window.prompt)"
```

---

## Task 6: App — settings gear + SettingsDialog + Backlinks-only pane

**Files:** Modify `web/src/app/App.tsx`.

- [ ] **Step 1: Wire the gear + SettingsDialog, slim the right pane**

In `web/src/app/App.tsx`:
- Imports: add
  ```tsx
  import { IconButton } from "../components/ui/IconButton";
  import { SettingsDialog } from "../components/SettingsDialog";
  ```
  and remove the `Settings` import (it's now only used by `SettingsDialog`).
- State: add `const [settingsOpen, setSettingsOpen] = useState(false);` (next to the other `useState`s).
- Navbar: in the top-bar right group, immediately BEFORE `<CommitBar … />`, add a gear button:
  ```tsx
          <IconButton label="Settings" onClick={() => setSettingsOpen(true)}>
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
  ```
- Right pane: change the `backlinks={…}` Shell prop from the `<div className="flex flex-col gap-4"><Backlinks…/><Settings…/></div>` stack to just:
  ```tsx
      backlinks={<Backlinks paths={backlinks} onOpen={actions.openNote} />}
  ```
- After the `<Shell … />` (inside the existing top-level fragment that also holds `<ErrorToast … />`), render:
  ```tsx
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        settings={settings}
        onChange={actions.setSettings}
      />
  ```
  (`settings` selector already exists in App.)

- [ ] **Step 2: Full gate**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: ALL PASS. If `format:check` fails, `pnpm format` and stage. Confirm `grep -rn "window.prompt" web/src` is empty.

- [ ] **Step 3: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat: navbar settings gear + SettingsDialog; Backlinks-only right pane"
```

---

## Task 7: Update e2e for the modal flows

**Files:** Modify `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: Replace the new-note + commit native-dialog steps**

In `web/e2e/skeleton.spec.ts` (the main loop test):
- The new-note step currently uses a native dialog handler, e.g.:
  ```ts
  page.once("dialog", (d) => d.accept("fresh.md"));
  await page.getByRole("button", { name: /new note/i }).click();
  ```
  Replace with the modal flow:
  ```ts
  await page.getByRole("button", { name: /new note/i }).click();
  await page.getByPlaceholder("notes/idea.md").fill("fresh.md");
  await page.getByRole("button", { name: "Create" }).click();
  ```
- The manual-commit step currently uses a native dialog handler, e.g.:
  ```ts
  page.once("dialog", (d) => d.accept("e2e snapshot"));
  await page.getByRole("button", { name: /^commit$/i }).click();
  ```
  Replace with (scope the dialog's Commit button — the navbar trigger is also "Commit"):
  ```ts
  await page.getByRole("button", { name: /^commit$/i }).click(); // opens the dialog
  await page.getByPlaceholder("Describe this change").fill("e2e snapshot");
  await page.getByRole("dialog").getByRole("button", { name: /^commit$/i }).click();
  ```
Remove any leftover `page.once("dialog", …)` handlers for these two flows. Keep every other step/assertion (autosave "Saved", search, backlink, commit `@c####`, the live-preview + graph tests) unchanged.

- [ ] **Step 2: Run the e2e**

Run (from `web/`): `pnpm e2e`
Expected: PASS (all tests). If port 5173 is held by tau-ui, `lsof -ti:5173 | xargs kill` and retry. The dialog inputs are reached by placeholder; the dialog's Commit button is scoped via `getByRole("dialog")` to avoid the navbar-trigger collision.

- [ ] **Step 3: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): new-note + commit via in-app modals"
```

---

## Task 8: Manual visual check (controller)

Not a subagent task.

- [ ] Start the dev server on a non-5173 port (5173 is tau-ui): `pnpm dev --port 5273 --strictPort`.
- [ ] Verify: **+ New note** opens the styled modal; **Commit** opens the commit modal; the navbar **gear** opens Settings with the auto-commit controls; the right pane shows **only Backlinks**. Esc/backdrop close; primary buttons indigo. Screenshot.

---

## Done criteria

- No `window.prompt` anywhere (`grep -rn "window.prompt" web/src` empty); new-note and commit are styled in-app modals; a navbar gear opens a Settings modal holding the auto-commit controls; the right pane is Backlinks-only.
- Full unit suite + both e2e tests green; `typecheck`/`lint`/`format:check`/`build` clean. Tauri/desktop unaffected. Live-preview internals + graph untouched (UI‑3/UI‑4).
```
