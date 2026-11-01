# Ask chat — tablet & mobile surface

**Date:** 2026-06-14
**Status:** Approved (design)
**Builds on:** `2026-06-14-ask-ui-chat-panel-design.md` (PR #62, branch `ask-ui-track`)

## Problem

The "cairn ask" chat works on every breakpoint as a prompt **bar** (`mode: "bar"`),
but the full conversation surface is desktop-only. `AskPanelHost` plugs into the
desktop `Shell`'s `ask` region; `MobileShell`/`TabletShell` ignore it, and the
bar's `⤢` promote button is gated off non-desktop (`canPromote={breakpoint === "desktop"}`
in `DialogHost.tsx`). On tablet/mobile a user can ask one question in the bar but
cannot expand into the scrollable, multi-turn conversation.

## Goal

Render the conversation as a slide-in surface on tablet/mobile, reusing the
existing presentational pieces. No store/reducer changes — the conversation model
(`askSlice`, `mode: "closed" | "bar" | "panel"`) is untouched; `"panel"` simply
gains a second renderer on small screens.

## Prior art (surface choice)

Material 3 recommends **swapping a bottom sheet for a side sheet on expanded
(tablet) window sizes** to use horizontal space; NN/g and LogRocket note bottom
sheets suit brief interruptions, not interactive dwell-on-it surfaces like a chat.
So the surface splits by breakpoint:

- **Mobile** → bottom sheet (`Drawer side="bottom"`)
- **Tablet** → right side sheet (`Drawer side="right"`, matching the existing
  backlinks drawer)

The existing `components/ui/Drawer.tsx` already supports both sides, so this is a
single `side` prop, not a second layout.

## Open mechanism

No new trigger. The model mirrors desktop exactly:

1. The **prompt bar** is the single entry on every breakpoint, opened by the
   *Ask* command (⌘K palette / keybinding, surfaced in the mobile *More* menu).
   The bar already renders on all breakpoints (`DialogHost`).
2. The bar's **`⤢` "Continue in panel"** button promotes `bar → panel` (pure mode
   flip, `askPromote`). Today gated to desktop; **un-gating it is the open
   mechanism** for tablet/mobile.
3. Close (✕ / Escape / scrim tap) → `mode: "closed"`. Re-opening goes through the
   bar again, then `⤢` — same as desktop.

Adding a dedicated nav entry (BottomNav/MoreMenu) is explicitly out of scope (those
files are owned by other streams).

## Architecture

Two new files in `components/ask/`, plus edits to the two shells, `DialogHost`,
and `AskPanel`. Ownership stays within `components/ask/*` + `shells/MobileShell.tsx`
+ `shells/TabletShell.tsx` (disjoint from other streams).

### `components/ask/AskSheet.tsx` (new — presentational)

- Props: `AskSurfaceProps` (imported from `./AskBar`) **+** `side: "bottom" | "right"`.
- Wraps the conversation in the existing `Drawer` (`open`, `onClose`, `side`,
  `label="Ask"`). `Drawer` (Radix `Dialog.Root`, modal) provides the scrim,
  portal, and **Escape/scrim close for free** — closed state mounts no portal, so
  the surface is absent from the DOM exactly like `AskPanel` returning `null`.
- Body mirrors `AskPanel`'s internals (the layout differs from `AskPanel`'s fixed
  `w-[340px]` `<aside>`, so it is a sibling component, not a reuse of `AskPanel`):
  - Header: `Ask ✦` + a ✕ button (`aria-label="Close ask"`) calling `onClose`.
  - Scrollable turn list: `turns.map` → `AnswerView`, with `streaming` true only
    on the last assistant turn (same predicate as `AskPanel`).
  - Error block (`data-testid="ask-error"`) when `error`.
  - Composer: text input (`placeholder="Ask a follow-up…"`, Enter submits, trims,
    clears) + send button (`aria-label="Send"`, disabled while `streaming`).
- Root carries `data-testid="ask-sheet"`.

### `components/ask/AskSheetHost.tsx` (new — store/router wiring)

- Mirrors `AskPanelHost`: reads `ask` slice + `notePaths`, wires `askSubmit` /
  `askClose` / citation navigation (`resolveStem` + `noteUrl`).
- Props: `{ side: "bottom" | "right" }` — the shell supplies the side (the shell
  already *is* the breakpoint; no `useBreakpoint` needed in the host).
- Renders `<AskSheet open={ask.mode === "panel"} side={side} … />`.

### `shells/MobileShell.tsx` / `shells/TabletShell.tsx` (edit)

- Render `<AskSheetHost side="bottom" />` (mobile) / `<AskSheetHost side="right" />`
  (tablet) directly inside the shell, alongside the existing backlinks `Drawer`.
  Not via the `ask` shell-region prop — that prop carries the desktop
  `AskPanelHost`; each shell owns its own ask surface.

### `DialogHost.tsx` (edit)

- Remove `canPromote={breakpoint === "desktop"}` so `⤢` works on all breakpoints
  (`AskBar`'s `canPromote` defaults to `true`). Remove the now-unused
  `useBreakpoint` import and `breakpoint` local (else lint fails).

### `AskPanel.tsx` (edit — desktop)

- Add Escape-to-close. `AskPanel` is a plain `<aside>` (no Radix), so it has no
  native Escape. Add a `useEffect` that, while `open`, listens for `keydown` on
  `window` and calls `onClose()` on `Escape`; clean up on unmount/close.
- The sheet does **not** need this — `Drawer`'s Radix dialog already handles Escape.

## Data flow

```
Ask command ──▶ askOpen() ──▶ mode "bar" ──▶ AskBar (all breakpoints, DialogHost)
                                              │  tap ⤢ → askPromote()
                                              ▼
                                          mode "panel"
        desktop ──▶ Shell ask region ──▶ AskPanelHost ──▶ AskPanel (aside)
        tablet  ──▶ TabletShell      ──▶ AskSheetHost side="right" ─┐
        mobile  ──▶ MobileShell      ──▶ AskSheetHost side="bottom" ┴▶ AskSheet ─▶ Drawer
                                          ✕ / Esc / scrim → askClose() → mode "closed"
```

Only one shell mounts per viewport, so `"panel"` renders exactly one surface. The
bar is closed (`open=false`) while in `"panel"`. No double-render.

## Testing (TDD, per `components/ask/*.test.tsx`)

- **`AskSheet.test.tsx`** (mirror `AskPanel.test.tsx`): renders nothing when
  `open=false`; renders all turns when open; submits a follow-up via Enter; ✕
  button calls `onClose`. Exercise both `side` values render (`data-testid="ask-sheet"`).
- **`AskSheetHost.test.tsx`** (mirror `AskPanelHost.test.tsx`): sheet absent until
  `mode === "panel"` (`askOpen` → `askPromote`); follow-up submit appends a turn;
  `askClose` tears it down. Render inside `MemoryRouter`.
- **`AskPanel.test.tsx`**: add a test that pressing `Escape` while open calls
  `onClose`.
- **`DialogHost`**: no `DialogHost.test.tsx` exists, and the un-gate is just
  dropping a prop. `AskBar.test.tsx` already covers `⤢` firing `onPromote` with
  `canPromote` defaulting to `true`, so no new test is added.

## Out of scope (YAGNI)

- Drag-to-resize / snap points on the sheet (Drawer is fixed-height).
- A demote (`panel → bar`) control — desktop has none; parity kept.
- New nav entries / FAB to open the sheet directly.
- Any store, reducer, or contract change.

## Gate before claiming green

`cd web && pnpm format:check && pnpm typecheck && pnpm lint && pnpm test`
(run single files with `pnpm exec vitest run <file>`).
