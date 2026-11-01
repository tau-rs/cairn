# `cairn ask` UI Chat Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a grounded "ask" chat surface to cairn — a slim prompt bar that promotes into a resizable docked panel — that streams a mock agent's answer token-by-token with citations and a tool indicator, built entirely against a mock streaming agent (real transport is Wave 2).

**Architecture:** Conversation state and the agent-stream subscription live in a new `askSlice` (store), so the bar and the panel are thin prop-driven views of one state and the bar→panel hand-off is a `mode` flag flip with no data migration and no stream interruption. A new `client.ask()` seam (mirroring `subscribe`) is implemented by the mock now and stubbed in the Tauri client; the swap to the real transport is one method later. Follows the repo's hexagonal seams: client port → store domain → dumb component adapters wired by host components.

**Tech Stack:** TypeScript, React, Zustand (vanilla store + `useStore` hooks), Radix Dialog, Tailwind (theme tokens: `bg`, `surface`, `surface-2`, `border`, `text`, `faint`, `accent`, `accent-fg`, `danger`), Vitest + Testing Library.

---

## File Structure

**Create:**
- `web/src/client/agent.ts` — local `AgentEvent` type (mirrors engine, `#[non_exhaustive]`).
- `web/src/store/askReducer.ts` — pure `applyAgentEvent` + `AskTurn` type.
- `web/src/store/askSlice.ts` — `AskState` + `createAskSlice(set, get, client)` owning the stream subscription.
- `web/src/components/ask/AnswerView.tsx` — renders one turn (text + caret + linkified citations + sources).
- `web/src/components/ask/AskBar.tsx` — non-modal prompt-bar surface (+ exports shared `AskSurfaceProps`).
- `web/src/components/ask/AskPanel.tsx` — docked panel surface.
- `web/src/components/ask/AskPanelHost.tsx` — wires `AskPanel` to the store + navigation.
- `web/src/components/ask/citation.ts` — `resolveStem(notePaths, target)` helper.
- Tests: `askReducer.test.ts`, `AnswerView.test.tsx`, `AskBar.test.tsx`, `AskPanel.test.tsx`, `citation.test.ts`.

**Modify:**
- `web/src/client/types.ts` — add `ask()` to `CairnClient`.
- `web/src/client/mock.ts` — implement `MockClient.ask()`.
- `web/src/client/tauri.ts` — stub `TauriClient.ask()` (Wave 2 real impl).
- `web/src/store/store.ts` — 3 edits: import, `extends AskState`, spread.
- `web/src/components/shortcuts/commands.ts` — one `open-ask` command def.
- `web/src/app/useCommands.ts` — one `case "open-ask"`.
- `web/src/components/shells/regions.ts` — add optional `ask?: ReactNode`.
- `web/src/components/Shell.tsx` — render the `ask` region.
- `web/src/components/DialogHost.tsx` — mount + wire `AskBar`.
- `web/src/app/App.tsx` — pass `ask={<AskPanelHost />}` region.
- `web/src/client/mock.test.ts` — cover `ask()`.

---

## Task 1: Client seam — `AgentEvent` + `CairnClient.ask()` + mock

**Files:**
- Create: `web/src/client/agent.ts`
- Modify: `web/src/client/types.ts`, `web/src/client/mock.ts`, `web/src/client/tauri.ts`
- Test: `web/src/client/mock.test.ts`

- [ ] **Step 1: Create the AgentEvent type**

Create `web/src/client/agent.ts`:

```ts
/** A streaming agent event. Mirrors the engine's `AgentEvent` (Track 03), which
 *  is `#[non_exhaustive]` — consumers MUST ignore unknown `type` values rather
 *  than crash. Citations are not a separate variant: the engine embeds cited
 *  notes as `[[stem]]` wikilinks inside `text_delta` text. */
export type AgentEvent =
  | { type: "text_delta"; text: string }
  | { type: "tool_started"; tool: string }
  | { type: "tool_completed"; tool: string; ok: boolean }
  | { type: "turn_completed" }
  | { type: "completed" }
  | { type: "failed"; message: string };
```

- [ ] **Step 2: Extend the CairnClient interface**

In `web/src/client/types.ts`, add the import and the method. Add to the imports at top:

```ts
import type { AgentEvent } from "./agent";
```

Add this method inside `interface CairnClient`, after `subscribe(...)`:

```ts
  /** Ask the grounded agent a question; `onEvent` receives the stream (see
   *  AgentEvent). Mirrors `subscribe`'s shape: `onError` fires if the stream
   *  fails to attach. Returns an Unsubscribe that cancels the in-flight run.
   *  The real transport is wired in Wave 2 (Track 03); the mock streams now. */
  ask(
    question: string,
    onEvent: (e: AgentEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe;
```

- [ ] **Step 3: Write the failing mock test**

In `web/src/client/mock.test.ts`, add at the top of the file's imports if not present:

```ts
import type { AgentEvent } from "./agent";
```

Add this `describe` block:

```ts
describe("MockClient.ask", () => {
  function collect(client: MockClient, q: string): Promise<AgentEvent[]> {
    return new Promise((resolve) => {
      const events: AgentEvent[] = [];
      client.ask(q, (e) => {
        events.push(e);
        if (e.type === "completed" || e.type === "failed") resolve(events);
      });
    });
  }

  it("streams a tool round, text deltas with a citation, then completes", async () => {
    const client = new MockClient({ "store.ts": "# Store\n" });
    const events = await collect(client, "how does it work?");
    const types = events.map((e) => e.type);
    expect(types[0]).toBe("tool_started");
    expect(types).toContain("tool_completed");
    expect(types).toContain("text_delta");
    expect(types.at(-1)).toBe("completed");
    const text = events
      .filter((e): e is { type: "text_delta"; text: string } => e.type === "text_delta")
      .map((e) => e.text)
      .join("");
    expect(text).toContain("[[store]]");
  });

  it("emits the failed path when the question contains 'fail'", async () => {
    const client = new MockClient({ "store.ts": "x" });
    const events = await collect(client, "please fail");
    expect(events.at(-1)).toEqual({ type: "failed", message: expect.any(String) });
  });

  it("unsubscribe stops further events", async () => {
    const client = new MockClient({ "store.ts": "x" });
    const seen: AgentEvent[] = [];
    const unsub = client.ask("hello", (e) => seen.push(e));
    unsub();
    await new Promise((r) => queueMicrotask(() => queueMicrotask(r)));
    await new Promise((r) => queueMicrotask(() => queueMicrotask(r)));
    expect(seen.length).toBe(0);
  });
});
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cd web && pnpm test -- src/client/mock.test.ts`
Expected: FAIL — `client.ask is not a function`.

- [ ] **Step 5: Implement MockClient.ask()**

In `web/src/client/mock.ts`, add the import near the other type imports at the top:

```ts
import type { AgentEvent } from "./agent";
```

Add this method to the `MockClient` class (e.g. right after `noteTags()`):

```ts
  /** Mock streaming agent: emits a tool round, then text deltas that embed a
   *  `[[stem]]` citation drawn from a real seeded note, then completes. A
   *  question containing "fail" emits the failed path instead. Events fire on
   *  chained microtasks (ordered + async); unsubscribe cancels mid-stream. */
  ask(question: string, onEvent: (e: AgentEvent) => void): Unsubscribe {
    let cancelled = false;
    const fail = question.toLowerCase().includes("fail");
    const firstStem = [...this.notes.keys()].map(stem)[0];
    const cite = firstStem ? ` [[${firstStem}]]` : "";
    const seq: AgentEvent[] = fail
      ? [
          { type: "tool_started", tool: "search_notes" },
          { type: "tool_completed", tool: "search_notes", ok: true },
          { type: "failed", message: "stream interrupted (mock)" },
        ]
      : [
          { type: "tool_started", tool: "search_notes" },
          { type: "tool_completed", tool: "search_notes", ok: true },
          { type: "text_delta", text: "Based on your notes, " },
          { type: "text_delta", text: "the answer is grounded in" },
          { type: "text_delta", text: cite },
          { type: "text_delta", text: " and complete." },
          { type: "turn_completed" },
          { type: "completed" },
        ];
    const step = (i: number) => {
      if (cancelled || i >= seq.length) return;
      onEvent(seq[i]);
      queueMicrotask(() => step(i + 1));
    };
    queueMicrotask(() => step(0));
    return () => {
      cancelled = true;
    };
  }
```

- [ ] **Step 6: Stub TauriClient.ask() (Wave 2 does the real impl)**

In `web/src/client/tauri.ts`, add the import:

```ts
import type { AgentEvent } from "./agent";
```

Add this method to `TauriClient` (after `noteTags()`). It satisfies the interface and degrades gracefully; it does NOT touch any existing transport wiring:

```ts
  /** Wave 2 (Track 03) wires the real agent stream over Tauri IPC. Until then
   *  the channel is unavailable: report via onError so the UI can show a
   *  degraded state, and return a no-op unsubscribe. */
  ask(
    _question: string,
    _onEvent: (e: AgentEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe {
    onError?.(new Error("agent stream not available yet"));
    return () => {};
  }
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cd web && pnpm test -- src/client/mock.test.ts`
Expected: PASS (all three new tests + existing mock tests).

- [ ] **Step 8: Commit**

```bash
git add web/src/client/agent.ts web/src/client/types.ts web/src/client/mock.ts web/src/client/tauri.ts web/src/client/mock.test.ts
git commit -m "feat(ask): add AgentEvent seam + mock streaming agent"
```

---

## Task 2: Pure streaming reducer

**Files:**
- Create: `web/src/store/askReducer.ts`
- Test: `web/src/store/askReducer.test.ts`

- [ ] **Step 1: Write the failing reducer test**

Create `web/src/store/askReducer.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { applyAgentEvent, emptyAssistantTurn } from "./askReducer";
import type { AgentEvent } from "../client/agent";

const reduce = (events: AgentEvent[]) =>
  events.reduce(applyAgentEvent, emptyAssistantTurn());

describe("applyAgentEvent", () => {
  it("accumulates text deltas in order", () => {
    const t = reduce([
      { type: "text_delta", text: "Hello " },
      { type: "text_delta", text: "world" },
    ]);
    expect(t.text).toBe("Hello world");
  });

  it("extracts distinct citations from embedded wikilinks", () => {
    const t = reduce([
      { type: "text_delta", text: "see [[store]] and " },
      { type: "text_delta", text: "[[timer]] and again [[store]]" },
    ]);
    expect(t.citations).toEqual(["store", "timer"]);
  });

  it("tracks tool start then completion", () => {
    const t = reduce([
      { type: "tool_started", tool: "search_notes" },
      { type: "tool_completed", tool: "search_notes", ok: true },
    ]);
    expect(t.tools).toEqual([{ tool: "search_notes", ok: true }]);
  });

  it("ignores unknown event kinds (non-exhaustive safety)", () => {
    const before = emptyAssistantTurn();
    const after = applyAgentEvent(before, { type: "mystery" } as unknown as AgentEvent);
    expect(after).toEqual(before);
  });

  it("does not mutate the input turn", () => {
    const before = emptyAssistantTurn();
    applyAgentEvent(before, { type: "text_delta", text: "x" });
    expect(before.text).toBe("");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && pnpm test -- src/store/askReducer.test.ts`
Expected: FAIL — cannot resolve `./askReducer`.

- [ ] **Step 3: Implement the reducer**

Create `web/src/store/askReducer.ts`:

```ts
import type { AgentEvent } from "../client/agent";
import { extractLinks } from "../client/wikilink";

/** One message in the ask conversation. */
export interface AskTurn {
  role: "user" | "assistant";
  text: string;
  /** Distinct cited note stems, in first-seen order (assistant turns). */
  citations: string[];
  /** Tool activity; `ok === null` means still running. */
  tools: { tool: string; ok: boolean | null }[];
}

export function emptyAssistantTurn(): AskTurn {
  return { role: "assistant", text: "", citations: [], tools: [] };
}

/** Apply one content event to the in-flight assistant turn. Pure. Lifecycle
 *  events (turn_completed/completed/failed) and unknown kinds are no-ops here —
 *  the slice owns lifecycle (streaming flag, error). */
export function applyAgentEvent(turn: AskTurn, e: AgentEvent): AskTurn {
  switch (e.type) {
    case "text_delta": {
      const text = turn.text + e.text;
      return { ...turn, text, citations: distinct(extractLinks(text)) };
    }
    case "tool_started":
      return { ...turn, tools: [...turn.tools, { tool: e.tool, ok: null }] };
    case "tool_completed":
      return { ...turn, tools: markDone(turn.tools, e.tool, e.ok) };
    default:
      return turn;
  }
}

function distinct(xs: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const x of xs)
    if (!seen.has(x)) {
      seen.add(x);
      out.push(x);
    }
  return out;
}

/** Mark the most recent still-running tool with this name as done. */
function markDone(
  tools: AskTurn["tools"],
  tool: string,
  ok: boolean,
): AskTurn["tools"] {
  for (let i = tools.length - 1; i >= 0; i--) {
    if (tools[i].tool === tool && tools[i].ok === null) {
      const next = tools.slice();
      next[i] = { tool, ok };
      return next;
    }
  }
  return tools;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd web && pnpm test -- src/store/askReducer.test.ts`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/askReducer.ts web/src/store/askReducer.test.ts
git commit -m "feat(ask): pure agent-event reducer + AskTurn"
```

---

## Task 3: Ask slice + store wiring

**Files:**
- Create: `web/src/store/askSlice.ts`
- Modify: `web/src/store/store.ts` (3 edits)
- Test: `web/src/store/askSlice.test.ts`

- [ ] **Step 1: Write the failing slice test**

Create `web/src/store/askSlice.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";

const make = () => createCairnStore(new MockClient({ "store.ts": "# Store\n" }));

describe("ask slice", () => {
  it("opens the bar", () => {
    const s = make();
    expect(s.getState().ask.mode).toBe("closed");
    s.getState().askOpen();
    expect(s.getState().ask.mode).toBe("bar");
  });

  it("submit pushes a user turn + assistant turn and streams to completion", async () => {
    const s = make();
    s.getState().askSubmit("how does it work?");
    expect(s.getState().ask.streaming).toBe(true);
    expect(s.getState().ask.turns.map((t) => t.role)).toEqual([
      "user",
      "assistant",
    ]);
    await vi.waitFor(() => expect(s.getState().ask.streaming).toBe(false));
    const ai = s.getState().ask.turns[1];
    expect(ai.text).toContain("grounded");
    expect(ai.citations).toContain("store");
  });

  it("promote flips bar -> panel without touching turns", async () => {
    const s = make();
    s.getState().askSubmit("hi");
    const before = s.getState().ask.turns;
    s.getState().askPromote();
    expect(s.getState().ask.mode).toBe("panel");
    expect(s.getState().ask.turns).toBe(before);
    await vi.waitFor(() => expect(s.getState().ask.streaming).toBe(false));
  });

  it("captures the failed path as an error", async () => {
    const s = make();
    s.getState().askSubmit("please fail");
    await vi.waitFor(() => expect(s.getState().ask.error).not.toBeNull());
    expect(s.getState().ask.streaming).toBe(false);
  });

  it("close cancels an in-flight run (stale events do not apply)", async () => {
    const s = make();
    s.getState().askSubmit("how does it work?");
    s.getState().askClose();
    expect(s.getState().ask.mode).toBe("closed");
    const turnsAtClose = s.getState().ask.turns;
    await new Promise((r) => setTimeout(r, 20));
    expect(s.getState().ask.turns).toBe(turnsAtClose);
    expect(s.getState().ask.streaming).toBe(false);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd web && pnpm test -- src/store/askSlice.test.ts`
Expected: FAIL — `askOpen is not a function`.

- [ ] **Step 3: Implement the slice**

Create `web/src/store/askSlice.ts`:

```ts
import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, Unsubscribe } from "../client/types";
import type { AgentEvent } from "../client/agent";
import type { CairnState } from "./store";
import { applyAgentEvent, emptyAssistantTurn, type AskTurn } from "./askReducer";

export type AskMode = "closed" | "bar" | "panel";

export interface AskConversation {
  mode: AskMode;
  turns: AskTurn[];
  streaming: boolean;
  error: string | null;
}

export interface AskState {
  ask: AskConversation;
  /** Open the prompt bar (no-op if already open in some mode). */
  askOpen(): void;
  /** Submit a question: append turns + start the stream. Ignored while streaming. */
  askSubmit(question: string): void;
  /** Promote the bar into the docked panel (pure mode flip). */
  askPromote(): void;
  /** Close the surface and cancel any in-flight run. */
  askClose(): void;
}

export const DEFAULT_ASK: AskConversation = {
  mode: "closed",
  turns: [],
  streaming: false,
  error: null,
};

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

function errMsg(err: unknown): string {
  if (err && typeof err === "object" && "message" in err)
    return String((err as { message: unknown }).message);
  return err instanceof Error ? err.message : String(err);
}

/** Ask conversation slice. Owns the agent-stream subscription in its closure
 *  (like store.ts's eventUnsub), so unmounting the bar on promote can't tear
 *  the stream down. A monotonic run token drops events from a superseded run. */
export function createAskSlice(
  set: Set,
  get: Get,
  client: CairnClient,
): AskState {
  let unsub: Unsubscribe | null = null;
  let runToken = 0;

  const stop = () => {
    unsub?.();
    unsub = null;
  };

  return {
    ask: DEFAULT_ASK,

    askOpen() {
      set((s) => ({
        ask: { ...s.ask, mode: s.ask.mode === "closed" ? "bar" : s.ask.mode },
      }));
    },

    askSubmit(question) {
      const q = question.trim();
      if (!q || get().ask.streaming) return;
      stop();
      const token = ++runToken;
      set((s) => ({
        ask: {
          ...s.ask,
          mode: s.ask.mode === "closed" ? "bar" : s.ask.mode,
          streaming: true,
          error: null,
          turns: [
            ...s.ask.turns,
            { role: "user", text: q, citations: [], tools: [] },
            emptyAssistantTurn(),
          ],
        },
      }));

      const onEvent = (e: AgentEvent) => {
        if (token !== runToken) return;
        if (e.type === "failed") {
          stop();
          set((s) => ({ ask: { ...s.ask, streaming: false, error: e.message } }));
          return;
        }
        if (e.type === "completed") {
          stop();
          set((s) => ({ ask: { ...s.ask, streaming: false } }));
          return;
        }
        if (e.type === "turn_completed") return;
        set((s) => {
          const turns = s.ask.turns.slice();
          const i = turns.length - 1;
          turns[i] = applyAgentEvent(turns[i], e);
          return { ask: { ...s.ask, turns } };
        });
      };

      unsub = client.ask(q, onEvent, (err) => {
        if (token !== runToken) return;
        stop();
        set((s) => ({ ask: { ...s.ask, streaming: false, error: errMsg(err) } }));
      });
    },

    askPromote() {
      set((s) => ({
        ask: { ...s.ask, mode: s.ask.mode === "bar" ? "panel" : s.ask.mode },
      }));
    },

    askClose() {
      stop();
      runToken++;
      set((s) => ({ ask: { ...s.ask, mode: "closed", streaming: false } }));
    },
  };
}
```

- [ ] **Step 4: Wire the slice into the store (3 edits)**

In `web/src/store/store.ts`:

Edit A — add the import near the other store-local imports (e.g. after the `./trace` import on line 44):

```ts
import { createAskSlice, type AskState } from "./askSlice";
```

Edit B — make `CairnState` extend `AskState`. Change the interface declaration (line 111):

```ts
export interface CairnState extends AskState {
```

Edit C — spread the slice into the returned state object. In the big `return {` object (line 480), add this line immediately after `liveUpdates: "ok",` (line 513):

```ts
      ...createAskSlice(set, get, client),
```

- [ ] **Step 5: Run the slice + store tests to verify they pass**

Run: `cd web && pnpm test -- src/store/askSlice.test.ts src/store/store.test.ts`
Expected: PASS (all new slice tests + existing store tests still green).

- [ ] **Step 6: Commit**

```bash
git add web/src/store/askSlice.ts web/src/store/askSlice.test.ts web/src/store/store.ts
git commit -m "feat(ask): conversation slice owning the agent stream"
```

---

## Task 4: AnswerView component + citation helper

**Files:**
- Create: `web/src/components/ask/citation.ts`, `web/src/components/ask/AnswerView.tsx`
- Test: `web/src/components/ask/citation.test.ts`, `web/src/components/ask/AnswerView.test.tsx`

- [ ] **Step 1: Write the failing citation-helper test**

Create `web/src/components/ask/citation.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { resolveStem } from "./citation";

describe("resolveStem", () => {
  it("finds the note path whose stem matches", () => {
    expect(resolveStem(["a/store.ts", "timer.md"], "store")).toBe("a/store.ts");
  });
  it("accepts a target with an alias or path form", () => {
    expect(resolveStem(["notes/timer.md"], "notes/timer")).toBe("notes/timer.md");
  });
  it("returns null when nothing matches", () => {
    expect(resolveStem(["a.md"], "missing")).toBeNull();
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && pnpm test -- src/components/ask/citation.test.ts`
Expected: FAIL — cannot resolve `./citation`.

- [ ] **Step 3: Implement the helper**

Create `web/src/components/ask/citation.ts`:

```ts
import { stem } from "../../client/wikilink";

/** Resolve a citation target (a stem, possibly path-like) to a real note path
 *  by stem match, or null if none of `notePaths` matches. */
export function resolveStem(notePaths: string[], target: string): string | null {
  const t = stem(target);
  return notePaths.find((p) => stem(p) === t) ?? null;
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cd web && pnpm test -- src/components/ask/citation.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Write the failing AnswerView test**

Create `web/src/components/ask/AnswerView.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AnswerView } from "./AnswerView";
import type { AskTurn } from "../../store/askReducer";

const turn = (over: Partial<AskTurn> = {}): AskTurn => ({
  role: "assistant",
  text: "",
  citations: [],
  tools: [],
  ...over,
});

describe("AnswerView", () => {
  it("renders text and linkifies inline citations", () => {
    const onOpenNote = vi.fn();
    render(
      <AnswerView turn={turn({ text: "see [[store]] now" })} streaming={false} onOpenNote={onOpenNote} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "store" }));
    expect(onOpenNote).toHaveBeenCalledWith("store");
  });

  it("shows a sources footer for assistant turns with citations", () => {
    render(
      <AnswerView turn={turn({ text: "x [[store]]", citations: ["store"] })} streaming={false} onOpenNote={vi.fn()} />,
    );
    expect(screen.getByTestId("sources")).toHaveTextContent("store");
  });

  it("shows a caret while streaming and a running tool indicator", () => {
    render(
      <AnswerView
        turn={turn({ text: "partial", tools: [{ tool: "search_notes", ok: null }] })}
        streaming
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByTestId("caret")).toBeInTheDocument();
    expect(screen.getByTestId("tool")).toHaveTextContent("search_notes");
  });

  it("renders a user turn as plain text with no sources", () => {
    render(<AnswerView turn={turn({ role: "user", text: "my question" })} streaming={false} onOpenNote={vi.fn()} />);
    expect(screen.getByText("my question")).toBeInTheDocument();
    expect(screen.queryByTestId("sources")).toBeNull();
  });
});
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cd web && pnpm test -- src/components/ask/AnswerView.test.tsx`
Expected: FAIL — cannot resolve `./AnswerView`.

- [ ] **Step 7: Implement AnswerView**

Create `web/src/components/ask/AnswerView.tsx`:

```tsx
import type { ReactNode } from "react";
import type { AskTurn } from "../../store/askReducer";

const CITE = /\[\[([^\]]+)\]\]/g;

/** Render plain text, turning `[[target]]` wikilinks into clickable buttons. */
function renderText(text: string, onOpenNote: (target: string) => void): ReactNode[] {
  const parts: ReactNode[] = [];
  let last = 0;
  let key = 0;
  let m: RegExpExecArray | null;
  CITE.lastIndex = 0;
  while ((m = CITE.exec(text)) !== null) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const target = m[1].split("|")[0].trim();
    parts.push(
      <button
        key={`c${key++}`}
        className="text-accent underline underline-offset-2"
        onClick={() => onOpenNote(target)}
      >
        {target}
      </button>,
    );
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

export function AnswerView(props: {
  turn: AskTurn;
  /** Is this the live, streaming turn? Controls the caret. */
  streaming: boolean;
  onOpenNote: (target: string) => void;
}) {
  const { turn, streaming, onOpenNote } = props;
  const isUser = turn.role === "user";
  return (
    <div
      className={
        isUser
          ? "ml-8 rounded-lg bg-accent/15 px-3 py-2 text-sm text-text"
          : "rounded-lg bg-surface-2 px-3 py-2 text-sm text-text"
      }
    >
      {!isUser &&
        turn.tools.map((t, i) => (
          <div key={i} data-testid="tool" className="mb-1 flex items-center gap-2 text-xs text-accent">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-current" />
            {t.ok === null ? `running ${t.tool}…` : t.ok ? `${t.tool} ✓` : `${t.tool} ✗`}
          </div>
        ))}
      <span className="whitespace-pre-wrap">
        {renderText(turn.text, onOpenNote)}
        {streaming && (
          <span
            data-testid="caret"
            className="ml-0.5 inline-block h-3.5 w-1 animate-pulse bg-accent align-text-bottom"
          />
        )}
      </span>
      {!isUser && turn.citations.length > 0 && (
        <div data-testid="sources" className="mt-1 border-t border-border pt-1 text-xs text-faint">
          Sources:{" "}
          {turn.citations.map((c, i) => (
            <button key={c} className="mr-2 text-accent underline" onClick={() => onOpenNote(c)}>
              {i + 1} {c}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 8: Run it to verify it passes**

Run: `cd web && pnpm test -- src/components/ask/AnswerView.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 9: Commit**

```bash
git add web/src/components/ask/citation.ts web/src/components/ask/citation.test.ts web/src/components/ask/AnswerView.tsx web/src/components/ask/AnswerView.test.tsx
git commit -m "feat(ask): AnswerView + citation resolver"
```

---

## Task 5: AskBar (non-modal prompt bar)

**Files:**
- Create: `web/src/components/ask/AskBar.tsx`
- Test: `web/src/components/ask/AskBar.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/ask/AskBar.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskBar } from "./AskBar";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onPromote: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskBar", () => {
  it("submits the typed question on Enter and clears the input", () => {
    const onSubmit = vi.fn();
    render(<AskBar {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask about your notes…");
    fireEvent.change(input, { target: { value: "how?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("how?");
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("renders the latest assistant turn", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q", citations: [], tools: [] },
      { role: "assistant", text: "the answer", citations: [], tools: [] },
    ];
    render(<AskBar {...base} turns={turns} />);
    expect(screen.getByText("the answer")).toBeInTheDocument();
  });

  it("promotes to the panel", () => {
    const onPromote = vi.fn();
    render(<AskBar {...base} onPromote={onPromote} />);
    fireEvent.click(screen.getByRole("button", { name: /continue in panel/i }));
    expect(onPromote).toHaveBeenCalled();
  });

  it("shows the error state", () => {
    render(<AskBar {...base} error="boom" />);
    expect(screen.getByTestId("ask-error")).toHaveTextContent("boom");
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && pnpm test -- src/components/ask/AskBar.test.tsx`
Expected: FAIL — cannot resolve `./AskBar`.

- [ ] **Step 3: Implement AskBar**

Create `web/src/components/ask/AskBar.tsx`:

```tsx
import * as Dialog from "@radix-ui/react-dialog";
import { useState } from "react";
import { AnswerView } from "./AnswerView";
import type { AskTurn } from "../../store/askReducer";

/** Props shared by both ask surfaces (bar + panel). */
export interface AskSurfaceProps {
  open: boolean;
  turns: AskTurn[];
  streaming: boolean;
  error: string | null;
  onSubmit: (q: string) => void;
  onClose: () => void;
  onOpenNote: (target: string) => void;
}

/** Slim, NON-modal prompt bar (modal={false}: does not block the editor).
 *  Shows the latest assistant turn inline; ⤢ promotes into the docked panel. */
export function AskBar(props: AskSurfaceProps & { onPromote: () => void }) {
  const { open, turns, streaming, error, onSubmit, onClose, onPromote, onOpenNote } = props;
  const [value, setValue] = useState("");
  const last = turns[turns.length - 1];
  const answer = last && last.role === "assistant" ? last : null;

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };

  return (
    <Dialog.Root open={open} onOpenChange={(o) => { if (!o) onClose(); }} modal={false}>
      <Dialog.Portal>
        <Dialog.Content
          aria-describedby={undefined}
          onInteractOutside={() => onClose()}
          className="fixed left-1/2 top-[15%] z-50 w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden rounded-xl border border-accent bg-surface-2 text-text shadow-2xl focus:outline-none"
        >
          <Dialog.Title className="sr-only">Ask</Dialog.Title>
          <div className="flex items-center gap-2 border-b border-border px-3 py-3">
            <span className="text-accent">✦</span>
            <input
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") submit(); }}
              placeholder="Ask about your notes…"
              className="w-full bg-transparent text-sm text-text placeholder:text-faint focus:outline-none"
            />
          </div>
          {(answer || error) && (
            <div className="max-h-72 overflow-y-auto px-3 py-2">
              {answer && <AnswerView turn={answer} streaming={streaming} onOpenNote={onOpenNote} />}
              {error && (
                <div data-testid="ask-error" className="mt-1 text-sm text-danger">
                  ⚠ {error}
                </div>
              )}
            </div>
          )}
          <div className="flex items-center justify-between border-t border-border px-3 py-2 text-xs text-faint">
            <button className="rounded border border-accent px-2 py-0.5 text-accent" onClick={onPromote}>
              ⤢ Continue in panel
            </button>
            <span>esc to close</span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cd web && pnpm test -- src/components/ask/AskBar.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ask/AskBar.tsx web/src/components/ask/AskBar.test.tsx
git commit -m "feat(ask): non-modal prompt bar surface"
```

---

## Task 6: AskPanel (docked panel)

**Files:**
- Create: `web/src/components/ask/AskPanel.tsx`
- Test: `web/src/components/ask/AskPanel.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/ask/AskPanel.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AskPanel } from "./AskPanel";
import type { AskTurn } from "../../store/askReducer";

const base = {
  open: true,
  turns: [] as AskTurn[],
  streaming: false,
  error: null as string | null,
  onSubmit: vi.fn(),
  onClose: vi.fn(),
  onOpenNote: vi.fn(),
};

describe("AskPanel", () => {
  it("renders nothing when closed", () => {
    render(<AskPanel {...base} open={false} />);
    expect(screen.queryByTestId("ask-panel")).toBeNull();
  });

  it("renders all turns when open", () => {
    const turns: AskTurn[] = [
      { role: "user", text: "q1", citations: [], tools: [] },
      { role: "assistant", text: "a1", citations: [], tools: [] },
    ];
    render(<AskPanel {...base} turns={turns} />);
    expect(screen.getByText("q1")).toBeInTheDocument();
    expect(screen.getByText("a1")).toBeInTheDocument();
  });

  it("submits a follow-up", () => {
    const onSubmit = vi.fn();
    render(<AskPanel {...base} onSubmit={onSubmit} />);
    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "more?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("more?");
  });

  it("closes via the close button", () => {
    const onClose = vi.fn();
    render(<AskPanel {...base} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /close ask panel/i }));
    expect(onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && pnpm test -- src/components/ask/AskPanel.test.tsx`
Expected: FAIL — cannot resolve `./AskPanel`.

- [ ] **Step 3: Implement AskPanel**

Create `web/src/components/ask/AskPanel.tsx`:

```tsx
import { useState } from "react";
import { AnswerView } from "./AnswerView";
import type { AskSurfaceProps } from "./AskBar";

/** Docked right-side panel: full turn list + docked composer. Renders null when
 *  closed so the shell region collapses. */
export function AskPanel(props: AskSurfaceProps) {
  const { open, turns, streaming, error, onSubmit, onClose, onOpenNote } = props;
  const [value, setValue] = useState("");
  if (!open) return null;

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };
  const lastIdx = turns.length - 1;

  return (
    <aside
      data-testid="ask-panel"
      className="flex w-[340px] shrink-0 flex-col border-l border-border bg-surface"
    >
      <div className="flex items-center justify-between border-b border-border px-3 py-2 text-sm font-semibold text-accent">
        <span>Ask ✦</span>
        <button aria-label="Close ask panel" className="text-faint" onClick={onClose}>
          ✕
        </button>
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {turns.map((t, i) => (
          <div key={i} className="my-1.5">
            <AnswerView
              turn={t}
              streaming={streaming && i === lastIdx && t.role === "assistant"}
              onOpenNote={onOpenNote}
            />
          </div>
        ))}
        {error && (
          <div data-testid="ask-error" className="m-1.5 text-sm text-danger">
            ⚠ {error}
          </div>
        )}
      </div>
      <div className="flex gap-2 border-t border-border p-2">
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") submit(); }}
          placeholder="Ask a follow-up…"
          className="flex-1 rounded-md border border-border bg-bg px-2 py-1.5 text-sm text-text focus:border-accent focus:outline-none"
        />
        <button
          aria-label="Send"
          className="rounded-md bg-accent px-3 text-accent-fg disabled:opacity-40"
          onClick={submit}
          disabled={streaming}
        >
          ↑
        </button>
      </div>
    </aside>
  );
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cd web && pnpm test -- src/components/ask/AskPanel.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ask/AskPanel.tsx web/src/components/ask/AskPanel.test.tsx
git commit -m "feat(ask): docked panel surface"
```

---

## Task 7: Wiring — command, keybinding, shell region, hosts

**Files:**
- Create: `web/src/components/ask/AskPanelHost.tsx`
- Modify: `web/src/components/shortcuts/commands.ts`, `web/src/app/useCommands.ts`, `web/src/components/shells/regions.ts`, `web/src/components/Shell.tsx`, `web/src/components/DialogHost.tsx`, `web/src/app/App.tsx`
- Test: `web/src/app/useCommands.test.ts` (extend if present; otherwise add the assertion to an existing dispatch test), `web/src/components/ask/AskPanelHost.test.tsx`

- [ ] **Step 1: Add the command definition**

In `web/src/components/shortcuts/commands.ts`, add this entry to the `COMMAND_DEFS` array (after `open-settings`):

```ts
  { id: "open-ask", label: "Ask…", defaultBinding: "Mod+Shift+A" },
```

- [ ] **Step 2: Dispatch the command**

In `web/src/app/useCommands.ts`, add this case inside the `switch (id)` in `runCommand` (e.g. after the `open-settings` case):

```ts
      case "open-ask":
        st.askOpen();
        break;
```

- [ ] **Step 3: Add the optional shell region**

In `web/src/components/shells/regions.ts`, add the field to `ShellRegions`:

```ts
  /** Optional docked ask panel (desktop shell). */
  ask?: ReactNode;
```

- [ ] **Step 4: Render the region in the desktop Shell**

In `web/src/components/Shell.tsx`, add `{props.ask}` as the last child inside the inner flex row (after the backlinks `<aside>`, before the closing `</div>` on line 17):

```tsx
        <aside className="w-56 shrink-0 overflow-auto border-l border-border bg-surface p-2">
          {props.backlinks}
        </aside>
        {props.ask}
```

- [ ] **Step 5: Wire AskBar into DialogHost**

In `web/src/components/DialogHost.tsx`:

Add imports:

```tsx
import { AskBar } from "./ask/AskBar";
import { resolveStem } from "./ask/citation";
import { noteUrl } from "../app/routes";
```

(`useNavigate`, `useCairn`, `useActions` are already imported; `noteUrl` may already be imported — if so, skip it.)

Inside the component, read ask state (after the existing `useCairn` calls):

```tsx
  const ask = useCairn((s) => s.ask);
```

Add `AskBar` inside the returned fragment (after `<CommandPalette ... />`):

```tsx
      <AskBar
        open={ask.mode === "bar"}
        turns={ask.turns}
        streaming={ask.streaming}
        error={ask.error}
        onSubmit={actions.askSubmit}
        onPromote={actions.askPromote}
        onClose={actions.askClose}
        onOpenNote={(target) => {
          const path = resolveStem(notePaths, target);
          if (path) {
            navigate(noteUrl(path));
            actions.askClose();
          }
        }}
      />
```

- [ ] **Step 6: Write the failing AskPanelHost test**

Create `web/src/components/ask/AskPanelHost.test.tsx`:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { AskPanelHost } from "./AskPanelHost";
import { cairnStore } from "../../app/cairnStore";

describe("AskPanelHost", () => {
  it("shows the panel only when mode is 'panel' and submits follow-ups", () => {
    render(
      <MemoryRouter>
        <AskPanelHost />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId("ask-panel")).toBeNull();

    cairnStore.getState().askOpen();
    cairnStore.getState().askPromote();
    expect(screen.getByTestId("ask-panel")).toBeInTheDocument();

    const input = screen.getByPlaceholderText("Ask a follow-up…");
    fireEvent.change(input, { target: { value: "hi" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(cairnStore.getState().ask.turns.length).toBeGreaterThan(0);

    cairnStore.getState().askClose();
  });
});
```

- [ ] **Step 7: Run it to verify it fails**

Run: `cd web && pnpm test -- src/components/ask/AskPanelHost.test.tsx`
Expected: FAIL — cannot resolve `./AskPanelHost`.

- [ ] **Step 8: Implement AskPanelHost**

Create `web/src/components/ask/AskPanelHost.tsx`:

```tsx
import { useNavigate } from "react-router-dom";
import { useCairn, useActions } from "../../app/cairnStore";
import { noteUrl } from "../../app/routes";
import { AskPanel } from "./AskPanel";
import { resolveStem } from "./citation";

/** Wires the docked AskPanel to the store + router. Passed as the `ask` shell
 *  region; renders null unless the conversation is in panel mode. */
export function AskPanelHost() {
  const navigate = useNavigate();
  const actions = useActions();
  const ask = useCairn((s) => s.ask);
  const notePaths = useCairn((s) => s.notePaths);

  return (
    <AskPanel
      open={ask.mode === "panel"}
      turns={ask.turns}
      streaming={ask.streaming}
      error={ask.error}
      onSubmit={actions.askSubmit}
      onClose={actions.askClose}
      onOpenNote={(target) => {
        const path = resolveStem(notePaths, target);
        if (path) navigate(noteUrl(path));
      }}
    />
  );
}
```

- [ ] **Step 9: Mount the panel region in App**

In `web/src/app/App.tsx`, add the import:

```tsx
import { AskPanelHost } from "../components/ask/AskPanelHost";
```

Add the `ask` prop to the `<AppShell ... />` element:

```tsx
        ask={<AskPanelHost />}
```

- [ ] **Step 10: Add the command-dispatch assertion**

Confirm whether `web/src/app/useCommands.test.ts` exists:

Run: `ls web/src/app/useCommands.test.ts 2>/dev/null && echo EXISTS || echo MISSING`

If it EXISTS, add this test alongside the others (it renders the hook and dispatches; match the file's existing harness for `runCommand`):

```ts
  it("open-ask opens the bar", () => {
    cairnStore.getState().askClose();
    runCommandFor("open-ask"); // use the file's existing dispatch helper
    expect(cairnStore.getState().ask.mode).toBe("bar");
  });
```

If it is MISSING, skip adding a new test file here — Task 3's slice test already covers `askOpen`, and Step 11's full run verifies wiring compiles. Note this in the commit body.

- [ ] **Step 11: Run the ask + wiring tests**

Run: `cd web && pnpm test -- src/components/ask src/store/askSlice.test.ts src/app`
Expected: PASS.

- [ ] **Step 12: Commit**

```bash
git add web/src/components/ask/AskPanelHost.tsx web/src/components/ask/AskPanelHost.test.tsx web/src/components/shortcuts/commands.ts web/src/app/useCommands.ts web/src/components/shells/regions.ts web/src/components/Shell.tsx web/src/components/DialogHost.tsx web/src/app/App.tsx
git commit -m "feat(ask): wire command, keybinding, docked panel region + hosts"
```

---

## Task 8: Full local gate

**Files:** none (verification only).

- [ ] **Step 1: Typecheck, lint, format, and full test run**

Run the full local gate (the same checks CI runs). From the repo root:

```bash
just check
```

If a `just check` recipe is not available, run the web gate explicitly:

```bash
cd web && pnpm typecheck && pnpm lint && pnpm prettier --check . && pnpm test
```

Expected: all green. The `prettier --check` step is easy to miss and eslint will NOT catch formatting — do not skip it.

- [ ] **Step 2: Fix any failures**

If `prettier --check` reports files, run `pnpm prettier --write <files>` and re-run the gate. If typecheck flags the `set`/`get` types passed to `createAskSlice`, ensure `store.ts` passes the callback's own `set, get` (they are assignable to `StoreApi<CairnState>["setState"]`/`["getState"]`).

- [ ] **Step 3: Commit any fixups**

```bash
git add -A
git commit -m "chore(ask): satisfy full local gate (format/lint/types)"
```

---

## Self-Review

**Spec coverage:**
- Hand-off model (bar → panel, one state, mode flip) → Tasks 3 (slice), 5 (bar), 6 (panel), 7 (wiring). ✓
- Client seam `ask()` + local `AgentEvent`, non-exhaustive → Task 1. ✓
- Citations via embedded `[[wikilinks]]` → Tasks 2 (reducer extract), 4 (AnswerView render + resolver). ✓
- Pure reducer, unit-tested (incl. unknown-event ignore) → Task 2. ✓
- Subscription owned by slice, survives promote, run-token guard → Task 3 (impl + tests). ✓
- Streaming UX (caret, tool indicator, error, finalize on completed) → Tasks 4 (caret/tool), 5/6 (error), 3 (lifecycle). ✓
- Mock streaming agent + error path → Task 1. ✓
- Command + keybinding `Mod+Shift+A` → Task 7. ✓
- Desktop docked panel mount; bar available everywhere → Task 7 (Shell region + DialogHost). Tablet/mobile sheet is explicitly out of MVP scope per spec. ✓
- Conflict rule (one import + extends + spread in store.ts) → Task 3 Step 4. ✓
- Full gate incl. prettier → Task 8. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. The only conditional is Task 7 Step 10 (test file may or may not exist) — both branches are spelled out. ✓

**Type consistency:** `AskTurn` (role/text/citations/tools) is used identically across reducer, AnswerView, AskBar, AskPanel. `AskSurfaceProps` defined in AskBar and reused by AskPanel/AskPanelHost. `applyAgentEvent`/`emptyAssistantTurn`/`createAskSlice`/`resolveStem`/`AskState`/`AgentEvent` names match every call site. `askOpen`/`askSubmit`/`askPromote`/`askClose` consistent across slice, useCommands, DialogHost, AskPanelHost. ✓
