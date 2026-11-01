# Wire `cairn ask` to the real engine stream — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the mocked `cairn ask` agent stream with the real engine stream over the transport-abstracted `CairnClient`, so the docked panel + mobile/tablet sheet answer from actual notes (daemon transport; Tauri deferred).

**Architecture:** Add `ask(req, onEvent, onError): Unsubscribe` to `CairnClient`, unified on the vendored `AnswerEvent` (`@/contract`). `DaemonClient` does the real work: `POST /ask` → SSE read via `fetch` + `ReadableStream`. `MockClient` emits real `AnswerEvent` frames for tests. The `askSlice`/`askReducer` consume `AnswerEvent`; the `sources` frame is the authoritative citation source. `TauriClient`/`HostClient` keep an honest stub (follow-up PR).

**Tech Stack:** TypeScript, React 19, Zustand, Vitest, Tauri (stub only), `fetch`/`ReadableStream`/`TextDecoder`.

---

## File structure

| File | Responsibility | Change |
|---|---|---|
| `web/src/client/types.ts` | `CairnClient` interface | add `ask` |
| `web/src/client/contractGuards.ts` | boundary tag-checks | add `assertAnswerEvent` |
| `web/src/client/daemon.ts` | daemon transport | add real `ask` (SSE) |
| `web/src/client/mock.ts` | test transport | reshape `ask` → `AnswerEvent` |
| `web/src/client/tauri.ts` | desktop transport | `ask` stub (deferred) |
| `web/src/client/host.ts` | host transport | `ask` stub if it implements `CairnClient` |
| `web/src/client/agent.ts` | old `AgentEvent` type | **delete** |
| `web/src/store/askReducer.ts` | per-turn event reducer | `sources`→citations; retype |
| `web/src/store/askSlice.ts` | ask conversation slice | `AskRequest`; `errMsg` unify |
| `web/src/store/errMsg.ts` | shared error formatter | create (extract from store) |
| `web/src/components/ask/AnswerView.tsx` | render a turn | path-based citations |
| `web/src/components/ask/AskSheet*.tsx` | mobile/tablet sheet | retype to `AnswerEvent` |

Plus matching `*.test.ts(x)` for each touched module.

---

## Task 1: Establish the integration baseline (merge)

**Why:** The current branch (`wire-ask-real-engine-stream` = main) has the vendored contract + `DaemonClient` but **no ask UI**. `wire-real-agent-stream` has the ask UI + `AskSheet`/`AskSheetHost` + the `errMsg.ts` extraction (but predates the contract/daemon and uses the old `AgentEvent`). Merging it onto the current branch yields the union: contract + daemon + ask UI + sheets.

**Files:** many (merge). No feature code yet.

- [ ] **Step 1: Confirm clean tree + branch**

Run: `git -C /Users/titouanlebocq/conductor/workspaces/cairn-ui/kelowna status -sb`
Expected: on `wire-ask-real-engine-stream`, clean.

- [ ] **Step 2: Merge `wire-real-agent-stream`**

```bash
git merge --no-ff wire-real-agent-stream -m "merge(ask): bring ask UI + sheets onto contract/daemon baseline"
```

Conflicts are expected in cross-cut files. Resolve with this rule of thumb:
- **`web/src/store/store.ts`** — keep main's structure (note-history/tier3/daemon slices) AND keep the ask wiring from the other side: `import { createAskSlice, type AskState }`, `CairnState extends AskState`, and `...createAskSlice(set, get, client)` in the store factory.
- **`web/src/client/mock.ts`** — keep main's plugin-broker mock additions AND the `ask(...)` method + `AgentEvent` import from the other side (it will be reshaped in Task 5).
- **`web/src/client/tauri.ts`** — keep main's version; **discard** the other side's `Channel`-based `ask` wiring (it's the wrong, pre-contract approach). The proper stub is added in Task 4.
- **`web/src/app/*`, shells, `makeBackend.ts`** — keep both: main's infra + the other side's ask region wiring (`AskPanelHost`, `AskSheetHost`).
- Keep new ask files verbatim: `web/src/components/ask/*`, `web/src/store/ask*.ts`, `web/src/store/errMsg.ts`, `web/src/client/agent.ts`.

- [ ] **Step 3: Install + run the full gate to confirm a green baseline**

Run (from `web/`):
```bash
pnpm install
pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build
```
Expected: all pass. The mock-driven ask still works (old `AgentEvent`); this proves the merge is sound before any reshaping.

- [ ] **Step 4: Commit the resolved merge** (if `--no-ff` left it staged, finalize)

```bash
git commit --no-edit || true
```

---

## Task 2: `assertAnswerEvent` contract guard

**Files:**
- Modify: `web/src/client/contractGuards.ts`
- Test: `web/src/client/contractGuards.test.ts` (create if absent)

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { assertAnswerEvent, ContractShapeError } from "./contractGuards";

describe("assertAnswerEvent", () => {
  it("accepts each known AnswerEvent tag", () => {
    for (const e of [
      { type: "sources", paths: ["a.md"] },
      { type: "text_delta", text: "hi" },
      { type: "tool_started", tool: "search" },
      { type: "tool_completed", tool: "search", ok: true },
      { type: "turn_completed" },
      { type: "completed" },
      { type: "failed", message: "boom" },
    ]) {
      expect(assertAnswerEvent(e)).toBe(e);
    }
  });

  it("rejects an unknown tag", () => {
    expect(() => assertAnswerEvent({ type: "nope" })).toThrow(ContractShapeError);
  });

  it("rejects a non-object", () => {
    expect(() => assertAnswerEvent(null)).toThrow(ContractShapeError);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm test -- contractGuards`
Expected: FAIL — `assertAnswerEvent` is not exported.

- [ ] **Step 3: Implement**

In `web/src/client/contractGuards.ts`, add `AnswerEvent` to the type import and add the guard:

```ts
// add to the existing type import on line 1:
import type { Event, CommandResponse, QueryResponse, AnswerEvent } from "../contract";

// add alongside the other *_TYPES consts:
const ANSWER_EVENT_TYPES = [
  "sources",
  "text_delta",
  "tool_started",
  "tool_completed",
  "turn_completed",
  "completed",
  "failed",
] as const;

// add alongside the other exported guards:
export const assertAnswerEvent = (x: unknown): AnswerEvent =>
  assertTagged(x, ANSWER_EVENT_TYPES, "answer event");
```

- [ ] **Step 4: Run it to verify it passes**

Run: `pnpm test -- contractGuards`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/contractGuards.ts web/src/client/contractGuards.test.ts
git commit -m "feat(client): add assertAnswerEvent contract guard"
```

---

## Task 3: `DaemonClient.ask` — real SSE stream

Built as a standalone method first (not yet on the interface), tested directly. Uses a fake `fetch` returning a `ReadableStream` of SSE bytes.

**Files:**
- Modify: `web/src/client/daemon.ts`
- Test: `web/src/client/daemon.test.ts`

- [ ] **Step 1: Write the failing tests**

Add to `web/src/client/daemon.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { DaemonClient } from "./daemon";
import type { AnswerEvent } from "../contract";

/** Build a Response whose body streams the given UTF-8 chunks in order. */
function sseResponse(chunks: string[], init?: ResponseInit): Response {
  const enc = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc.encode(c));
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "content-type": "text/event-stream" },
    ...init,
  });
}

function frame(e: AnswerEvent): string {
  return `data: ${JSON.stringify(e)}\n\n`;
}

async function collect(
  client: DaemonClient,
  onError?: (e: unknown) => void,
): Promise<AnswerEvent[]> {
  return new Promise((resolve) => {
    const got: AnswerEvent[] = [];
    client.ask(
      { query: "q", top_k: null },
      (e) => {
        got.push(e);
        if (e.type === "completed" || e.type === "failed") resolve(got);
      },
      (err) => {
        onError?.(err);
        resolve(got);
      },
    );
  });
}

describe("DaemonClient.ask", () => {
  it("yields sources → deltas → completed in order", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: ["a.md", "b.md"] }),
        frame({ type: "text_delta", text: "Hello " }),
        frame({ type: "text_delta", text: "world" }),
        frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const client = new DaemonClient({ url: "http://x", token: "t", fetch });
    const got = await collect(client);
    expect(got.map((e) => e.type)).toEqual([
      "sources",
      "text_delta",
      "text_delta",
      "completed",
    ]);
    expect(got[0]).toEqual({ type: "sources", paths: ["a.md", "b.md"] });
    // POSTs to /ask with the request body + bearer token.
    const [url, opts] = (fetch as unknown as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe("http://x/ask");
    expect(opts.method).toBe("POST");
    expect(JSON.parse(opts.body)).toEqual({ query: "q", top_k: null });
    expect(opts.headers.Authorization).toBe("Bearer t");
  });

  it("parses multiple frames in one chunk", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }) +
          frame({ type: "text_delta", text: "x" }) +
          frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const got = await collect(new DaemonClient({ url: "http://x", fetch }));
    expect(got.map((e) => e.type)).toEqual([
      "sources",
      "text_delta",
      "completed",
    ]);
  });

  it("parses a frame split across chunks", async () => {
    const full = frame({ type: "text_delta", text: "hi" });
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }),
        full.slice(0, 6),
        full.slice(6),
        frame({ type: "completed" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const got = await collect(new DaemonClient({ url: "http://x", fetch }));
    expect(got).toContainEqual({ type: "text_delta", text: "hi" });
  });

  it("delivers an in-run failed frame as an event (not onError)", async () => {
    const fetch = vi.fn(async () =>
      sseResponse([
        frame({ type: "sources", paths: [] }),
        frame({ type: "failed", message: "boom" }),
      ]),
    ) as unknown as typeof globalThis.fetch;
    const onError = vi.fn();
    const got = await collect(new DaemonClient({ url: "http://x", fetch }), onError);
    expect(got[got.length - 1]).toEqual({ type: "failed", message: "boom" });
    expect(onError).not.toHaveBeenCalled();
  });

  it("routes a pre-stream HTTP error to onError", async () => {
    const fetch = vi.fn(async () =>
      new Response("nope", { status: 401 }),
    ) as unknown as typeof globalThis.fetch;
    const onError = vi.fn();
    await collect(new DaemonClient({ url: "http://x", fetch }), onError);
    expect(onError).toHaveBeenCalledOnce();
    expect(String((onError.mock.calls[0][0] as Error).message)).toMatch(
      /unauthorized/,
    );
  });

  it("stops emitting after unsubscribe and cancels the reader", async () => {
    let cancelled = false;
    const enc = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(enc.encode(frame({ type: "sources", paths: [] })));
        // never closes; the test cancels.
      },
      cancel() {
        cancelled = true;
      },
    });
    const fetch = vi.fn(async () =>
      new Response(stream, {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      }),
    ) as unknown as typeof globalThis.fetch;
    const seen: AnswerEvent[] = [];
    const unsub = new DaemonClient({ url: "http://x", fetch }).ask(
      { query: "q", top_k: null },
      (e) => seen.push(e),
    );
    // let the first frame flush, then cancel.
    await new Promise((r) => setTimeout(r, 10));
    unsub();
    await new Promise((r) => setTimeout(r, 10));
    expect(cancelled).toBe(true);
    expect(seen.every((e) => e.type === "sources")).toBe(true);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- daemon`
Expected: FAIL — `ask` does not exist on `DaemonClient`.

- [ ] **Step 3: Implement `ask` in `daemon.ts`**

Add the import and method. Insert near the top with the other imports:

```ts
import { assertAnswerEvent } from "./contractGuards";
import type { AskRequest, AnswerEvent } from "../contract";
```

Add inside the `DaemonClient` class (e.g. after `runQuery`):

```ts
  /** Stream a note-grounded answer from `POST /ask` (SSE over POST). The first
   *  frame is `sources`, then `text_delta`/`tool_*`, then a terminal
   *  `completed`/`failed`. A pre-stream failure is an HTTP error → `onError`
   *  (the typed `ContractError`/401); an in-run failure arrives as a `failed`
   *  event. `EventSource` can't carry the bearer token, so we read the body via
   *  `fetch` + `ReadableStream`. Unsubscribe drops further events and cancels
   *  the reader; the server run finishes harmlessly (no v1 cancel endpoint). */
  ask(
    req: AskRequest,
    onEvent: (e: AnswerEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe {
    let cancelled = false;
    let reader: ReadableStreamDefaultReader<Uint8Array> | null = null;

    // Async IIFE so the returned `unsub` is assigned before any onEvent fires
    // (the ask slice relies on this).
    void (async () => {
      let res: Response;
      try {
        res = await this.fetch(`${this.url}/ask`, {
          method: "POST",
          headers: this.headers(),
          body: JSON.stringify(req),
        });
      } catch (err) {
        if (!cancelled) onError?.(err);
        return;
      }
      if (cancelled) return;
      if (!res.ok || !res.body) {
        if (!cancelled) onError?.(await this.errorFor(res));
        return;
      }
      reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (cancelled) return;
          if (done) break;
          buf += decoder.decode(value, { stream: true });
          let sep: number;
          // SSE frames are separated by a blank line.
          while ((sep = buf.indexOf("\n\n")) !== -1) {
            const raw = buf.slice(0, sep);
            buf = buf.slice(sep + 2);
            const e = parseSseFrame(raw);
            if (e && !cancelled) onEvent(assertAnswerEvent(e));
          }
        }
        // Flush a trailing frame with no blank-line terminator.
        const tail = parseSseFrame(buf);
        if (tail && !cancelled) onEvent(assertAnswerEvent(tail));
      } catch (err) {
        if (!cancelled) onError?.(err);
      }
    })();

    return () => {
      cancelled = true;
      void reader?.cancel().catch(() => {});
    };
  }
```

Add this module-level helper at the bottom of `daemon.ts` (above `DaemonHost`):

```ts
/** Extract the JSON payload from one SSE frame: concatenate its `data:` lines
 *  (ignoring comments/blank/`event:` lines), or null if the frame has no data. */
function parseSseFrame(raw: string): unknown {
  const data = raw
    .split("\n")
    .filter((l) => l.startsWith("data:"))
    .map((l) => l.slice(5).replace(/^ /, ""))
    .join("\n");
  if (data === "") return null;
  return JSON.parse(data);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- daemon`
Expected: PASS (all `DaemonClient.ask` tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/daemon.ts web/src/client/daemon.test.ts
git commit -m "feat(client): stream real answers from daemon POST /ask (SSE)"
```

---

## Task 4: Add `ask` to the interface + Tauri/Host stubs

This is the type seam. Adding `ask` to `CairnClient` forces every impl to have it; `DaemonClient` already does (Task 3), so only the stubs are needed here. `MockClient`/`askSlice`/`askReducer` are reshaped in Tasks 5–6.

**Files:**
- Modify: `web/src/client/types.ts`
- Modify: `web/src/client/tauri.ts`
- Modify: `web/src/client/host.ts`
- Test: `web/src/client/tauri.test.ts` (if present) / add a focused stub test

- [ ] **Step 1: Add to the interface**

In `web/src/client/types.ts`, add the import and the method:

```ts
import type {
  Command,
  Query,
  Event,
  CommandResponse,
  QueryResponse,
  AskRequest,
  AnswerEvent,
} from "../contract";

// inside interface CairnClient, after noteTags():
  /** Stream a note-grounded answer. `onEvent` receives `AnswerEvent` frames
   *  (`sources` first, then deltas/tool events, then `completed`/`failed`).
   *  `onError` fires only on a pre-stream/transport failure; an in-run failure
   *  is a `failed` event. The returned `Unsubscribe` cancels the stream. */
  ask(
    req: AskRequest,
    onEvent: (e: AnswerEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe;
```

- [ ] **Step 2: Write the failing stub test**

Add to `web/src/client/tauri.test.ts` (create if absent, mirroring its style):

```ts
import { describe, it, expect, vi } from "vitest";
import { TauriClient } from "./tauri";

describe("TauriClient.ask (deferred)", () => {
  it("reports a degraded state via onError and returns a no-op unsub", async () => {
    const onError = vi.fn();
    const unsub = new TauriClient().ask(
      { query: "q", top_k: null },
      () => {},
      onError,
    );
    await new Promise((r) => queueMicrotask(r));
    expect(onError).toHaveBeenCalledOnce();
    expect(() => unsub()).not.toThrow();
  });
});
```

- [ ] **Step 3: Run to verify failure**

Run: `pnpm test -- tauri`
Expected: FAIL — `ask` not on `TauriClient` (or typecheck error).

- [ ] **Step 4: Implement the stubs**

In `web/src/client/tauri.ts`, add the imports `AskRequest, AnswerEvent` from `../contract`, and the method:

```ts
  /** Desktop ask is deferred to a follow-up PR (it needs an in-process Tauri
   *  command running `cairn_service::augmented_answer` + an engine rev-bump).
   *  Until then, report a degraded state so the UI can prompt for daemon mode.
   *  Deferred to a microtask so `unsub` is assigned before this fires. */
  ask(
    _req: AskRequest,
    _onEvent: (e: AnswerEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe {
    let cancelled = false;
    queueMicrotask(() => {
      if (!cancelled)
        onError?.(new Error("desktop ask not wired yet — use daemon mode"));
    });
    return () => {
      cancelled = true;
    };
  }
```

If `web/src/client/host.ts` implements `CairnClient`, add the identical stub there (same body, message can be the same). If it only wraps another client, delegate: `ask(...args) { return this.inner.ask(...args); }`.

- [ ] **Step 5: Run to verify pass + typecheck**

Run: `pnpm test -- tauri && pnpm typecheck`
Expected: stub test PASS. Typecheck will still FAIL on `MockClient`/`askSlice` (old `AgentEvent`) — that's expected; Tasks 5–6 fix it. (If your executor requires green between tasks, fold Tasks 4–6 into one commit.)

- [ ] **Step 6: Commit**

```bash
git add web/src/client/types.ts web/src/client/tauri.ts web/src/client/host.ts web/src/client/tauri.test.ts
git commit -m "feat(client): add ask to CairnClient + deferred Tauri/Host stub"
```

---

## Task 5: Reshape `MockClient.ask` to `AnswerEvent`

**Files:**
- Modify: `web/src/client/mock.ts`
- Modify: `web/src/client/mock.test.ts`

- [ ] **Step 1: Update the failing test**

Replace the `MockClient.ask` block in `web/src/client/mock.test.ts`. Change the import `import type { AgentEvent } from "./agent";` → `import type { AnswerEvent } from "../contract";`, retype `collect`/arrays to `AnswerEvent`, and assert the real shape:

```ts
describe("MockClient.ask", () => {
  function collect(client: MockClient, q: string): Promise<AnswerEvent[]> {
    return new Promise((resolve) => {
      const events: AnswerEvent[] = [];
      client.ask({ query: q, top_k: null }, (e) => {
        events.push(e);
        if (e.type === "completed" || e.type === "failed") resolve(events);
      });
    });
  }

  it("emits sources first and completed last on success", async () => {
    const client = makeMock(); // however other tests build a MockClient with notes
    const events = await collect(client, "hello");
    const types = events.map((e) => e.type);
    expect(types[0]).toBe("sources");
    expect(types).toContain("text_delta");
    expect(types[types.length - 1]).toBe("completed");
  });

  it("emits a failed frame for a 'fail' question", async () => {
    const client = makeMock();
    const events = await collect(client, "please fail");
    expect(events[events.length - 1]).toEqual({
      type: "failed",
      message: "stream interrupted (mock)",
    });
  });

  it("stops after unsubscribe", async () => {
    const seen: AnswerEvent[] = [];
    const unsub = makeMock().ask({ query: "hello", top_k: null }, (e) =>
      seen.push(e),
    );
    unsub();
    await new Promise<void>((r) => queueMicrotask(() => queueMicrotask(r)));
    expect(seen).toHaveLength(0);
  });
});
```
(Use whatever existing helper the test file uses to build a `MockClient` with seeded notes; if it inlines `new MockClient(...)`, keep that.)

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- mock`
Expected: FAIL — mock still emits `AgentEvent` / signature is `(question, ...)`.

- [ ] **Step 3: Reshape the implementation**

In `web/src/client/mock.ts`: change `import type { AgentEvent } from "./agent";` → `import type { AskRequest, AnswerEvent } from "../contract";`, and rewrite `ask`:

```ts
  ask(req: AskRequest, onEvent: (e: AnswerEvent) => void): Unsubscribe {
    let cancelled = false;
    const fail = req.query.toLowerCase().includes("fail");
    const firstPath = [...this.notes.keys()][0];
    const firstStem = firstPath ? stem(firstPath) : undefined;
    const cite = firstStem ? ` [[${firstStem}]]` : "";
    const sources: AnswerEvent = {
      type: "sources",
      paths: firstPath ? [firstPath] : [],
    };
    const seq: AnswerEvent[] = fail
      ? [
          sources,
          { type: "tool_started", tool: "search_notes" },
          { type: "tool_completed", tool: "search_notes", ok: true },
          { type: "failed", message: "stream interrupted (mock)" },
        ]
      : [
          sources,
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

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- mock`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(client): mock emits real AnswerEvent frames (sources→deltas→completed)"
```

---

## Task 6: Reshape `askReducer` + `askSlice` to `AnswerEvent`; unify `errMsg`; delete `agent.ts`

**Files:**
- Modify: `web/src/store/askReducer.ts`
- Modify: `web/src/store/askReducer.test.ts`
- Modify: `web/src/store/askSlice.ts`
- Modify: `web/src/store/askSlice.test.ts`
- Create (if absent): `web/src/store/errMsg.ts`
- Modify: `web/src/store/store.ts` (use shared `errMsg`)
- Delete: `web/src/client/agent.ts`

- [ ] **Step 1: Update the reducer test**

In `web/src/store/askReducer.test.ts`, change the import to `AnswerEvent` and assert `sources` sets citations while `text_delta` only appends text:

```ts
import { applyAnswerEvent, emptyAssistantTurn } from "./askReducer";
import type { AnswerEvent } from "../contract";

it("sets citations from a sources frame", () => {
  const t = applyAnswerEvent(emptyAssistantTurn(), {
    type: "sources",
    paths: ["a.md", "b.md"],
  } satisfies AnswerEvent);
  expect(t.citations).toEqual(["a.md", "b.md"]);
});

it("appends text only on text_delta (no citation scraping)", () => {
  let t = emptyAssistantTurn();
  t = applyAnswerEvent(t, { type: "text_delta", text: "see [[a]] " });
  t = applyAnswerEvent(t, { type: "text_delta", text: "and [[b]]" });
  expect(t.text).toBe("see [[a]] and [[b]]");
  expect(t.citations).toEqual([]);
});
```
Keep/adjust the existing `tool_started`/`tool_completed` cases (rename the call to `applyAnswerEvent`).

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- askReducer`
Expected: FAIL — `applyAnswerEvent` undefined / old citation behavior.

- [ ] **Step 3: Reshape the reducer**

In `web/src/store/askReducer.ts`: drop the `AgentEvent`/`extractLinks` imports, import `AnswerEvent`, rename `applyAgentEvent` → `applyAnswerEvent`, add the `sources` case, and make `text_delta` text-only:

```ts
import type { AnswerEvent } from "../contract";

// ...AskTurn + emptyAssistantTurn unchanged...

export function applyAnswerEvent(turn: AskTurn, e: AnswerEvent): AskTurn {
  switch (e.type) {
    case "sources":
      return { ...turn, citations: e.paths };
    case "text_delta":
      return { ...turn, text: turn.text + e.text };
    case "tool_started":
      return { ...turn, tools: [...turn.tools, { tool: e.tool, ok: null }] };
    case "tool_completed":
      return { ...turn, tools: markDone(turn.tools, e.tool, e.ok) };
    default:
      return turn;
  }
}
```
Delete the now-unused `distinct` helper if nothing else uses it. Keep `markDone`.

- [ ] **Step 4: Create the shared `errMsg`**

Create `web/src/store/errMsg.ts` (copy the formatter currently inline in `store.ts`, which formats `ContractError` bodies + `Error`s):

```ts
import type { ContractError } from "../contract";

/** Format any backend error uniformly: a typed `ContractError` body's message,
 *  else an `Error`'s message, else `String(err)`. */
export function errMsg(err: unknown): string {
  if (err && typeof err === "object" && "message" in err)
    return String((err as { message: unknown }).message);
  return err instanceof Error ? err.message : String(err);
}
```
(If `store.ts`'s inline `errMsg` does richer `ContractError` formatting, copy that body verbatim instead, then have `store.ts` import from here and delete its local copy.)

- [ ] **Step 5: Reshape the slice**

In `web/src/store/askSlice.ts`:
- Replace `import type { AgentEvent } from "../client/agent";` with `import type { AskRequest, AnswerEvent } from "../contract";`.
- Replace the import of `applyAgentEvent` with `applyAnswerEvent`.
- Add `import { errMsg } from "./errMsg";` and delete the slice-local `errMsg`.
- In `askSubmit`, build the request and pass it; retype `onEvent`:

```ts
      const onEvent = (e: AnswerEvent) => {
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
          turns[i] = applyAnswerEvent(turns[i], e);
          return { ask: { ...s.ask, turns } };
        });
      };

      const req: AskRequest = { query: q, top_k: null };
      unsub = client.ask(req, onEvent, (err) => {
        if (token !== runToken) return;
        stop();
        set((s) => ({ ask: { ...s.ask, streaming: false, error: errMsg(err) } }));
      });
```

- [ ] **Step 6: Update the slice test**

In `web/src/store/askSlice.test.ts`, make the fake client's `ask` accept `(req, onEvent, onError)` and emit `AnswerEvent` (a `sources` then `completed`, plus a `failed` case). Assert the slice forwards `{ query, top_k: null }`, sets `citations` from `sources`, and sets `error` from `failed`. Mirror existing test scaffolding; key assertion:

```ts
expect(askArg).toEqual({ query: "what is x", top_k: null });
```

- [ ] **Step 7: Delete `agent.ts` + fix `store.ts`**

```bash
git rm web/src/client/agent.ts
```
In `web/src/store/store.ts`, import `errMsg` from `./errMsg` and remove the inline copy. Grep for any other `from "../client/agent"` / `from "./agent"` importers and repoint them (there should be none after Tasks 5–6).

Run: `grep -rn "client/agent\|from \"./agent\"" web/src` — Expected: no results.

- [ ] **Step 8: Run reducer + slice tests + typecheck**

Run: `pnpm test -- askReducer askSlice && pnpm typecheck`
Expected: PASS, and typecheck now clean (the seam from Task 4 is closed).

- [ ] **Step 9: Commit**

```bash
git add web/src/store/ web/src/client/agent.ts
git commit -m "feat(ask): consume AnswerEvent; sources-frame citations; unify errMsg"
```

---

## Task 7: Path-based citations in the UI

`turn.citations` are now note **paths** (from `sources`), not stems. Render the source list with a readable stem label but resolve via the full path.

**Files:**
- Modify: `web/src/components/ask/AnswerView.tsx`
- Modify: `web/src/components/ask/AnswerView.test.tsx`
- Check: `web/src/components/ask/AskSheet.tsx` / `AskSheetHost.tsx` (retype to `AnswerEvent` if they reference the old type)

- [ ] **Step 1: Update the test**

In `web/src/components/ask/AnswerView.test.tsx`, set a turn with path citations and assert the "Sources:" footer renders a clickable label and calls `onOpenNote` with the **path**:

```ts
const turn = {
  role: "assistant" as const,
  text: "answer",
  citations: ["notes/alpha.md"],
  tools: [],
};
const onOpenNote = vi.fn();
render(<AnswerView turn={turn} streaming={false} onOpenNote={onOpenNote} />);
const btn = screen.getByText(/alpha/);
fireEvent.click(btn);
expect(onOpenNote).toHaveBeenCalledWith("notes/alpha.md");
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- AnswerView`
Expected: FAIL (label currently shows the full path / index format).

- [ ] **Step 3: Implement**

In `AnswerView.tsx`, import `stem` and update the citations footer to label by stem but pass the path:

```ts
import { stem } from "../../client/wikilink";

// in the citations footer map:
{turn.citations.map((c, i) => (
  <button
    key={c}
    className="mr-2 text-accent underline"
    onClick={() => onOpenNote(c)}
  >
    {i + 1} {stem(c)}
  </button>
))}
```
The inline `renderText` wikilink buttons are unchanged.

- [ ] **Step 4: Verify sheets compile against `AnswerEvent`**

Run: `grep -rn "AgentEvent\|client/agent" web/src/components/ask`
Expected: no results. If `AskSheet*`/`AskPanel*` import the old type, repoint to `AnswerEvent` (they consume `AskTurn`, so likely no change needed).

- [ ] **Step 5: Run to verify pass**

Run: `pnpm test -- AnswerView`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/ask/
git commit -m "feat(ask): render path-based source citations"
```

---

## Task 8: Full gate + contract-drift

**Files:** none (verification).

- [ ] **Step 1: Run the full web gate**

Run (from `web/`):
```bash
pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build
```
Expected: all PASS. If `format:check` fails, run `pnpm format` and re-commit.

- [ ] **Step 2: Run the contract-drift check**

Run: `just contract-check` (or the repo's documented contract-drift command — check `justfile`/CI). Expected: PASS (we only added a guard + consumed vendored types; no contract edit).

- [ ] **Step 3: Manual smoke (optional, daemon)**

With a `cairn-daemon` running and the URL/token set in `DaemonSettings`, ask a question; confirm sources render, text streams, and a deliberately failing query surfaces the `failed` message.

- [ ] **Step 4: Commit any formatting fixups**

```bash
git add -A && git commit -m "chore(ask): gate fixups" || true
```

---

## Self-review notes (spec coverage)

- Signature `ask(req: AskRequest, onEvent, onError): Unsubscribe` — Tasks 4, 6. ✓
- `AnswerEvent` unification + delete `agent.ts` — Tasks 4–7. ✓
- DaemonClient SSE (`POST /ask`, fetch+ReadableStream, cancel) — Task 3. ✓
- Mock emits real `AnswerEvent` — Task 5. ✓
- `sources`-frame authoritative citations; drop wikilink-scraping — Task 6. ✓
- Tauri/Host honest stub (deferred) — Task 4. ✓
- `assertAnswerEvent` guard — Task 2. ✓
- `errMsg` unify — Task 6. ✓
- Path-based citation UI — Task 7. ✓
- Full gate + contract-drift — Task 8. ✓
- Integration baseline (ask UI + sheets onto contract/daemon) — Task 1. ✓

**Open risk:** Task 1 (merge) conflict resolution can't be fully scripted; the gate in Task 1 Step 3 is the proof the baseline is sound before any reshaping. If green-between-tasks is required by the executor, fold Tasks 4–6 into one commit (the `AgentEvent`→`AnswerEvent` flip is a single type seam).
