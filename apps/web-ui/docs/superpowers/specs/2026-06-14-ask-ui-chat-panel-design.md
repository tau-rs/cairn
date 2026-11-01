# `cairn ask` UI chat panel — design

Date: 2026-06-14 · Track 04 · Branch: `ask-ui-track`

## Goal

A chat/"ask" surface where the user asks a question and a grounded answer streams
in token-by-token, with citations linking to the notes it drew from. Built now
against a **mock streaming agent** so it does not wait on the engine track
(Track 03). Wave 2 swaps in the real transport behind the same client seam.

## Scope decision — hand-off model (prompt bar → docked panel)

Prior-art research (Setproduct AI-chat anatomy, UX Collective "where AI sits",
Cursor / Perplexity / Claude) converged on two findings for an assistant
**embedded next to an editor**:

1. The home base should be a **resizable docked side panel** (Cursor Chat /
   Copilot Chat pattern), not a centered modal. A **blocking modal during
   streaming is an anti-pattern** — the user must be able to keep reading/editing
   while the answer streams.
2. The lightweight summon should be a **slim prompt bar** (Raycast / Cursor ⌘K),
   not a heavy modal chat.

So the feature is **one conversation surfaced at two moments**:

- **Prompt bar** — the fast door in. A slim, **non-modal** centered overlay
  (does not trap focus or block the editor). User types a question; the answer
  streams inline beneath it. `esc` dismisses (nothing persists); `⤢ Continue in
  panel` promotes.
- **Docked panel** — where a conversation lives. A resizable right-docked strip
  with the full turn list, a docked composer, and a numbered "Sources" footer.

They are **never on screen simultaneously**. Promotion is a presentation
`mode` flip over unchanged conversation state — no data migration, the stream is
not interrupted.

**Out of scope (deliberate):** the three-zone full-route view (history rail +
stream + sources panel); multi-conversation history/persistence across reload;
real transport wiring (Wave 2). A single in-memory conversation is enough for
this track.

## Architecture

The decision that keeps the hand-off clean: **conversation state and the stream
subscription live in the store slice, not in either component.** The bar and the
panel are thin views subscribed to the same state. This follows the existing
hexagonal seams in the repo (client port → store domain → component adapters).

### 1. Client seam (port) — `web/src/client/agent.ts` + `CairnClient`

The vendored contract has **no `AgentEvent` yet** (Track 03 adds it engine-side),
so it is defined locally now, treated as `#[non_exhaustive]`:

```ts
// web/src/client/agent.ts
export type AgentEvent =
  | { type: "text_delta"; text: string }
  | { type: "tool_started"; tool: string }
  | { type: "tool_completed"; tool: string; ok: boolean }
  | { type: "turn_completed" }
  | { type: "completed" }
  | { type: "failed"; message: string };
```

`CairnClient` (in `web/src/client/types.ts`) gains one method mirroring the
existing `subscribe` shape:

```ts
ask(
  question: string,
  onEvent: (e: AgentEvent) => void,
  onError?: (err: unknown) => void,
): Unsubscribe;
```

Wave 2 swaps the real transport behind this exact signature — one line.
**Event names must be coordinated with Track 03**; if the engine names differ,
this local type and the mock are the only places to change.

### 2. Citations — embedded `[[wikilinks]]`, no new event

The brief's event list has no citation variant. Citations ride **inside the
`text_delta` stream as `[[note-stem]]` wikilinks** (cairn already has a linkifier:
`web/src/client/wikilink.ts`, `extractLinks`/`stem`). The reducer collects the
distinct cited stems into the turn's `citations` list, rendered as a numbered
"Sources" footer and as inline clickable links that open the note. This needs no
new event and degrades gracefully if absent. If Track 03 later emits a dedicated
citation event, the reducer gains one `case`.

### 3. Store (domain) — `web/src/store/askSlice.ts`

Single source of truth. Wired into `store.ts` with **one import + one spread**,
plus `& AskState` on the `CairnState` type (the parallel-track conflict rule).

```ts
export interface AskTurn {
  role: "user" | "assistant";
  text: string;
  citations: string[];          // note stems, distinct, in first-seen order
  tools: { tool: string; ok: boolean | null }[]; // null = in progress
}

export interface AskState {
  ask: {
    mode: "closed" | "bar" | "panel";
    turns: AskTurn[];
    streaming: boolean;
    error: string | null;
  };
  askOpen(): void;       // mode -> "bar"
  askSubmit(q: string): void;  // push user turn + assistant turn; owns client.ask()
  askPromote(): void;    // mode "bar" -> "panel" (pure flag flip)
  askClose(): void;      // mode -> "closed"; cancel any in-flight stream
}
```

Two isolated, separately-testable pieces:

- **`web/src/store/askReducer.ts`** — a pure function
  `applyAgentEvent(turn: AskTurn, e: AgentEvent): AskTurn`. Appends `text_delta`
  to `text`, recomputes `citations` from embedded wikilinks, tracks tool
  start/complete, and has a `default:` case that **ignores unknown event kinds**
  (non-exhaustive safety). This is the primary unit-test target.
- **The stream subscription lives in the slice's closure**, exactly like the
  existing `connectEvents`/`eventUnsub` pattern in `store.ts`. Because the store
  (not the bar component) owns the `Unsubscribe`, unmounting the bar on promote
  cannot tear down the stream. A monotonic run token guards against a stale run
  applying after a newer `askSubmit` or `askClose` (mirrors the existing `seq`
  pattern for searches).

### 4. Views (adapters) — `web/src/components/ask/*`

Three dumb components, all rendering from slice state:

- **`AnswerView.tsx`** (shared) — renders one turn: markdown-ish text + streaming
  caret while `streaming` + linkified `[[citations]]` + (assistant) a numbered
  Sources strip. Used by **both** surfaces, so "renders in two places" is one
  component with zero duplicated logic.
- **`AskBar.tsx`** — mounts when `ask.mode === "bar"`. **Non-modal** centered
  overlay (Radix `Dialog` with `modal={false}`, or a positioned popover): input +
  latest `AnswerView` + `⤢ Continue in panel`. Does not block the editor.
- **`AskPanel.tsx`** — mounts when `ask.mode === "panel"`. Resizable right-docked
  strip: full turn list via `AnswerView`, docked composer at the bottom, Sources
  footer.

**Mount points:** `AskBar` renders in `DialogHost.tsx` (alongside
`CommandPalette`). `AskPanel` slots into the desktop layout shell. Tablet/mobile
get a drawer/sheet reusing the same components (matches the existing responsive
pattern); MVP focus is the desktop shell, with the bar available everywhere.

### 5. Wiring (conflict-minimal edits)

- `web/src/components/shortcuts/commands.ts`: one `CommandDef`
  `{ id: "open-ask", label: "Ask…", defaultBinding: "Mod+Shift+A" }`.
- `web/src/app/useCommands.ts`: one `case "open-ask": st.askOpen()`.
- `web/src/store/store.ts`: one import + one spread (+ `& AskState` on type).

### Data flow

```
Mod+Shift+A ─▶ askOpen()  ─▶ mode:"bar"  ─▶ AskBar mounts
AskBar submit ─▶ askSubmit(q) ─▶ client.ask() subscription (owned by slice)
   stream ─▶ applyAgentEvent ─▶ ask.turns grows ─▶ AnswerView re-renders (bar)
⤢ ─▶ askPromote() ─▶ mode:"panel" ─▶ AskBar unmounts, AskPanel mounts
   …same subscription, same ask.turns, AnswerView continues. No migration.
```

## Streaming UX (per research)

- Token-by-token accumulation of `text_delta`.
- Blinking caret at the stream tail while `streaming === true`.
- Tool affordance: `tool_started` shows a pulsing "running {tool}…"; on
  `tool_completed` it resolves to a ✓/✗.
- `failed` renders the message as an inline error on the assistant turn (with a
  retry affordance); the stream finalizes.
- Finalize the turn on `completed`.
- Auto-scroll the panel to the tail unless the user has scrolled up (lightweight;
  full "jump to latest" button is a nicety, not required for done).

## Mock streaming agent

`MockClient.ask()` (`web/src/client/mock.ts`) emits the event sequence on a timer:
a short `tool_started`("search_notes") → `tool_completed` → several `text_delta`
chunks whose text embeds `[[stem]]` citations drawn from real seeded note stems →
`turn_completed` → `completed`. A question containing a trigger token (e.g.
"fail") emits `failed` instead, to exercise the error path in tests. Returns an
`Unsubscribe` that stops the timer.

## Testing (part of done)

- **`askReducer.test.ts`** — pure reducer: text accumulation, citation
  extraction/dedup, tool start→complete, unknown-event ignore, failed handling.
- **`mock.test.ts`** (extend) — `ask()` emits a well-formed sequence and the
  error path; unsubscribe stops emission.
- **`askSlice`** coverage via the store test harness — `askOpen`/`askSubmit`/
  `askPromote`/`askClose`, run-token guarding (stale run does not apply),
  subscription survives promote.
- **Component tests** — `AskBar` and `AskPanel` render turns, show the streaming
  caret/tool indicator, render citations as links, and the error state.
- Full local gate green (incl. `prettier --check`/format, which eslint misses).

## Definition of done

Panel streams a mock answer end-to-end with citations + tool indicator + error
path; bar→panel hand-off works without interrupting the stream; the reducer is
unit-tested; full local gate green. Real transport untouched (Wave 2).
