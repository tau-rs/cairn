# Cairn Web UI — UI‑2: In-App Dialogs + Settings Home Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑2 of the UI/UX overhaul (UI‑1 done). Phase 5-adjacent.
**Builds on:** UI‑1 design system (tokens + `Button`/`IconButton`/`Input`/`SectionLabel`).

---

## 1. Purpose

Replace the native `window.prompt` dialogs (new-note, commit) with styled in-app
modals, and move the auto-commit settings out of the Backlinks pane into a
dedicated **Settings** modal opened from a navbar gear. This addresses two audit
complaints: the off-style new-note popup and settings living under Backlinks.

### Non-goals (deferred)

- Live-preview document look (UI‑3) and the Obsidian graph (UI‑4).
- A full command palette / keyboard-shortcut system (Phase 5 proper).
- Settings beyond auto-commit (e.g. theme, editor default mode) — Settings holds
  just the existing auto-commit controls for now.
- Confirmation dialog for delete (note deletion stays a direct action this cycle).

---

## 2. Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Dialog primitive | **`@radix-ui/react-dialog`**, skinned with UI‑1 tokens (focus trap, Esc, backdrop, scroll-lock, ARIA, return-focus for free). |
| New-note | Modal: title + `Input` (path) + Cancel / **Create** (primary). Replaces `window.prompt`. |
| Commit | Modal: title + `Input` (message) + Cancel / **Commit** (primary). Replaces `window.prompt`. |
| Settings | Gear `IconButton` in the navbar opens a Settings modal containing the **auto-commit** controls; the right pane becomes **Backlinks only**. |
| Submit guards | Create/Commit are disabled when the field is empty/whitespace; Enter submits. |

---

## 3. Architecture

```
components/ui/Modal.tsx        Modal({ open, onClose, title, description?, children })
                               → Radix Dialog.Root/Portal/Overlay/Content, styled.
components/NewNoteDialog.tsx   ({ open, onOpenChange, onCreate }) Input(path) + Cancel/Create
components/CommitDialog.tsx    ({ open, onOpenChange, committing, onCommit }) Input(msg) + Cancel/Commit
components/SettingsDialog.tsx  ({ open, onOpenChange, settings, onChange }) wraps the existing <Settings/> fields
components/Settings.tsx        UNCHANGED fields component (auto-commit) — now rendered inside SettingsDialog only

NoteList.tsx   owns NewNoteDialog open-state; "+ New note" opens it; on create → props.onNew(path)
CommitBar.tsx  owns CommitDialog open-state; "Commit" opens it; on commit → props.onCommit(message)
App.tsx        navbar gains a gear IconButton → opens SettingsDialog (App owns open-state, passes settings/setSettings);
               right pane (`backlinks` Shell prop) becomes just <Backlinks/> (drop the stacked Settings)
package.json   + @radix-ui/react-dialog
```

- The `Modal` primitive is the only place Radix is touched; dialogs compose it +
  UI‑1 primitives. Each dialog owns its small form; the parent passes the
  callback (`onNew`/`onCommit`/`onChange`) — store wiring is unchanged.
- No store/contract changes. `createNote`, `commitManual`, `setSettings` already
  exist and keep their signatures.

---

## 4. Components

**`Modal`** — `props: { open: boolean; onClose: () => void; title: string; description?: string; children: ReactNode }`.
Renders Radix `Dialog.Root open onOpenChange` → `Dialog.Portal` → a styled
`Dialog.Overlay` (`fixed inset-0 bg-black/50`) + `Dialog.Content` (centered,
`bg-surface border border-border rounded-xl shadow-2xl w-[min(92vw,360px)]`),
with `Dialog.Title` (the title), optional `Dialog.Description`, and `children`
(body + footer). `onOpenChange(false)` calls `onClose`. Esc/backdrop close are
Radix defaults.

**`NewNoteDialog`** — `props: { open; onOpenChange: (b:boolean)=>void; onCreate: (path:string)=>void }`.
A `Modal` (title "New note", description "Path inside the cairn") containing an
`Input` (placeholder `notes/idea.md`, autofocus) + a footer with `Button` ghost
**Cancel** and `Button` primary **Create**. Local `path` state; **Create**
disabled when `path.trim()` is empty; submitting (Create or Enter) calls
`onCreate(path.trim())` then `onOpenChange(false)` and clears the field.

**`CommitDialog`** — `props: { open; onOpenChange; committing: boolean; onCommit: (msg:string)=>void }`.
Same shape with title "Commit", an `Input` (placeholder "Describe this change"),
**Commit** primary disabled when empty or `committing`; submit calls
`onCommit(msg.trim())` then closes + clears.

**`SettingsDialog`** — `props: { open; onOpenChange; settings; onChange }`.
A `Modal` (title "Settings") rendering the existing `<Settings settings onChange/>`
fields + a footer `Button` secondary **Done** (`onOpenChange(false)`).

---

## 5. Wiring changes

- **`NoteList.tsx`**: add `const [newOpen, setNewOpen] = useState(false)`. The
  "+ New note" `Button` `onClick` now `setNewOpen(true)` (drop `window.prompt`).
  Render `<NewNoteDialog open={newOpen} onOpenChange={setNewOpen}
  onCreate={props.onNew} />`. `props.onNew(path)` is unchanged (still
  `store.createNote`).
- **`CommitBar.tsx`**: add `const [commitOpen, setCommitOpen] = useState(false)`.
  The **Commit** `Button` `onClick` → `setCommitOpen(true)` (drop `window.prompt`).
  Render `<CommitDialog open={commitOpen} onOpenChange={setCommitOpen}
  committing={props.committing} onCommit={props.onCommit} />`.
- **`App.tsx`**: add `const [settingsOpen, setSettingsOpen] = useState(false)`;
  add a gear `IconButton label="Settings"` to the navbar (right group, before/near
  Commit) → `setSettingsOpen(true)`; render `<SettingsDialog open={settingsOpen}
  onOpenChange={setSettingsOpen} settings={settings} onChange={actions.setSettings} />`
  at App level. Change the `backlinks` Shell prop from the
  `<Backlinks/> + <Settings/>` stack to just `<Backlinks paths={backlinks}
  onOpen={actions.openNote} />`. (`settings` selector stays — now feeding the dialog.)

---

## 6. Testing

- **Unit (Vitest + Testing Library):**
  - `Modal`: with `open`, the `title` + children render (role `dialog`); a Close
    affordance / Esc triggers `onClose`. (Radix Dialog renders under jsdom; if a
    Radix internal needs a polyfill — e.g. `ResizeObserver`/`hasPointerCapture` —
    add a minimal mock in `vitest.setup.ts`.)
  - `NewNoteDialog`: open → typing a path + clicking **Create** calls
    `onCreate("the/path.md")` and closes; **Create** is disabled on empty;
    **Cancel** closes without calling `onCreate`.
  - `CommitDialog`: analogous (`onCommit`; disabled when empty or `committing`).
  - `SettingsDialog`: open → the auto-commit controls render; toggling fires
    `onChange` (reuse the existing `Settings` field behavior).
  - **Update** `NoteList.test.tsx` and `CommitBar.test.tsx`: they currently mock
    `window.prompt`; rewrite to the dialog flow (click the trigger → dialog → type
    → Create/Commit → callback fired). Remove the `window.prompt` spies.
- **e2e (Playwright):** the existing flow uses native dialogs
  (`page.once("dialog", …)`) for new-note and commit — **replace** with the modal
  flow: click **+ New note** → fill the dialog `Input` → click **Create**; and
  click **Commit** → fill the message → click **Commit**. Keep all downstream
  assertions (autosave, search, backlink, commit id, live-preview, graph).
- All green on the mock; Tauri/desktop unaffected.

---

## 7. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/ui/Modal.tsx` (+ `.test.tsx`) | **New.** Radix-backed modal. |
| `web/src/components/NewNoteDialog.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/CommitDialog.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/SettingsDialog.tsx` (+ `.test.tsx`) | **New.** |
| `web/src/components/NoteList.tsx` (+ `.test.tsx`) | **Modify.** dialog instead of prompt; tests rewritten. |
| `web/src/components/CommitBar.tsx` (+ `.test.tsx`) | **Modify.** dialog instead of prompt; tests rewritten. |
| `web/src/app/App.tsx` | **Modify.** navbar gear + SettingsDialog; Backlinks-only right pane. |
| `web/e2e/skeleton.spec.ts` | **Modify.** new-note + commit via modals. |
| `web/package.json` | **Modify.** add `@radix-ui/react-dialog`. |
| `web/src/components/Settings.tsx` | **Unchanged** (now used only inside SettingsDialog). |

---

## 8. Risks

- **Radix under jsdom:** Dialog renders, but some Radix primitives expect
  `ResizeObserver`, `DOMRect`, or `hasPointerCapture`/`scrollIntoView`. If a
  dialog test throws, add minimal mocks to `web/src/vitest.setup.ts` (a common,
  documented Radix-in-jsdom step). The interactive specifics are also e2e-covered.
- **e2e dialog rewrite:** the old `page.once("dialog", …)` handlers must be
  removed (no native dialog fires now) or the test will hang waiting — replace
  with input-fill + button-click. Don't weaken downstream assertions.
- **Focus/autofocus:** the dialog input should autofocus; Radix returns focus to
  the trigger on close. Verify Enter submits and Esc cancels.
- **Port 5173 / tau-ui:** e2e uses 5173; kill a stray tau-ui dev server if it
  collides.

---

## 9. Build order (for the plan)

1. Add `@radix-ui/react-dialog`; `Modal` primitive (+ test, + any jsdom polyfill in vitest.setup).
2. `NewNoteDialog` (TDD).
3. `CommitDialog` (TDD).
4. `SettingsDialog` (TDD).
5. Wire `NoteList` (dialog, rewrite its test), `CommitBar` (dialog, rewrite its test).
6. `App`: navbar gear + SettingsDialog + Backlinks-only right pane.
7. e2e: new-note + commit via modals.
8. Full gate: `pnpm test`/`typecheck`/`lint`/`format:check`/`build` + `pnpm e2e`; screenshot.
```
