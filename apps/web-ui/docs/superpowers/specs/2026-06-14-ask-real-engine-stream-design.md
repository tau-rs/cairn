# Wire `cairn ask` to the real engine stream — design

**Date:** 2026-06-14
**Track:** 04 — Wave 2
**Branch:** `wire-ask-real-engine-stream`

## Goal

Replace the mocked `cairn ask` agent stream in the UI with the real engine
stream, through the transport-abstracted `CairnClient`, so the docked chat panel
and the mobile/tablet sheet answer from actual notes: citations from the engine's
`sources` frame, live text from `text_delta`, graceful `failed` rendering.

## Context (what already shipped)

- **Engine seam — merged.** `tau-rs/cairn` PR #67 (engine main `eafc876`): wire
  types `AskRequest { query, top_k? }` + closed `AnswerEvent`
  (`sources{paths}` → `text_delta{text}` → `tool_started{tool}` /
  `tool_completed{tool,ok}` / `turn_completed` → `completed` | `failed{message}`).
  Daemon `POST /ask` streams SSE (`data: {AnswerEvent json}\n\n` per frame),
  token + origin gated. Pre-stream failure = HTTP error with `ContractError`;
  in-run failure = `failed` frame. Consume via `fetch` + `ReadableStream`, not
  `EventSource` (can't send the bearer token). In-process:
  `cairn_service::augmented_answer(...)` + `agent_event_to_wire(...)` exist for a
  future Tauri command.
- **Contract vendored — merged.** cairn-web-ui PR #70:
  `web/src/contract/{AskRequest,AnswerEvent}.ts` + barrel exports (`@/contract`).
  Raw ts-rs, in `.prettierignore` — do not hand-edit.
- **Ask UI — on `ask-ui-track` (not main).** `askSlice` owns the stream;
  `AskPanel` (desktop docked) + `AskSheet`/`AskSheetHost` (tablet/mobile, from the
  prior `wire-real-agent-stream` attempt). Driven today by a MOCK agent shaped
  like the *old* `AgentEvent` (no `sources` frame; citations scraped from
  `[[wikilink]]` prose).
- **DaemonClient — merged (this branch / main).** `daemon.ts`: HTTP `/command`
  `/query` + WS `/events`, bearer-token + base-URL config via `DaemonSettings`.
  Has `headers()` and `errorFor(res)` helpers but no `ask` method yet.
- **Prior attempt — `wire-real-agent-stream`.** Wired only the *TS* side
  `invoke("ask", …)` over a Tauri `Channel`; no Rust command was ever written, and
  it used the in-process `AgentEvent`, not the wire `AnswerEvent`. Treated as a
  reference for the `AskSheet`/`AskSheetHost` files and the `errMsg` unify, not as
  finished work.

## Decisions

1. **Transport scope for this PR:** DaemonClient (real) + MockClient (tests) now.
   TauriClient deferred to a follow-up PR — the daemon path is real,
   browser-working, fully unit-testable, with zero Rust and zero engine rev-bump.
   The Tauri path needs a 5-crate rev-bump (`079f9f9f` → `eafc876`, fallout noted
   in memory) + an unwritten async Rust command running `augmented_answer` with a
   `TAU_BIN` runtime that is effectively untestable in CI; isolating it keeps a
   rev-bump regression from blocking the daemon win.
2. **Signature:** `ask(req: AskRequest, onEvent, onError): Unsubscribe` —
   callback+`Unsubscribe` (mirrors `subscribe`, matches the slice's existing
   async-delivery + cancel assumptions), carrying the contract `AskRequest`.
   `top_k` stays `null` (engine defaults to 5); no UI control yet.
3. **Citations:** the `sources` frame is authoritative; drop wikilink-scraping for
   citations. The engine reports the grounding notes in rank order — strictly
   better than inferring from prose links. The inline `[[wikilink]]` buttons that
   `AnswerView` renders from prose text are a *separate* feature and stay.

## Transport interface

`web/src/client/types.ts` — add to `CairnClient`:

```ts
ask(
  req: AskRequest,
  onEvent: (e: AnswerEvent) => void,
  onError?: (err: unknown) => void,
): Unsubscribe;
```

Unify the whole ask path on the vendored `AnswerEvent` from `@/contract`. Delete
the local `web/src/client/agent.ts` `AgentEvent` type.

All impls satisfy the interface. This PR ships Daemon + Mock; `TauriClient` (and
`HostClient` if it implements `CairnClient`) keep an honest typed stub:
`onError(new Error("desktop ask not wired yet — use daemon mode"))` + no-op unsub,
flagged for the follow-up PR.

## Components

### DaemonClient.ask (`daemon.ts`) — the real work

- `POST ${url}/ask`, reuse `headers()` (bearer + content-type), body
  `JSON.stringify(req)`.
- `!res.ok` → `onError(await this.errorFor(res))` (existing helper: typed
  `ContractError` body, else generic; 401 → unauthorized). Pre-stream failure.
- Else read `res.body` via `getReader()` + `TextDecoder`: accumulate decoded text
  in a buffer, split completed frames on `\n\n`, per frame take `data:`-prefixed
  lines, strip the prefix (+ optional space), `JSON.parse` →
  `assertAnswerEvent` → `onEvent`. Ignore comment (`:`) and empty lines. Flush a
  trailing buffered frame on stream end.
- **Cancellation:** `Unsubscribe` sets `cancelled = true` and calls
  `reader.cancel()`. No `onEvent`/`onError` fires after unsub. Server run finishes
  harmlessly (no v1 cancel endpoint).
- The read loop runs in an async IIFE so `unsub` is assigned before the first
  `onEvent` (preserves the slice's async-delivery assumption).
- Injectable `fetch` (already on `DaemonClientOptions`) → unit-testable with a
  fake `ReadableStream`.

### MockClient.ask reshape (`mock.ts`)

Emit real `AnswerEvent` frames. Success: `sources{paths:[firstPath]}` →
`tool_started` → `tool_completed` → `text_delta`×N (text may embed a `[[stem]]` to
exercise inline links) → `turn_completed` → `completed`. Fail (question contains
"fail"): `sources` → tool → `failed{message}`. Same `queueMicrotask` chaining +
cancel as today.

### State: reducer + slice

- `askReducer.ts`: retype to `AnswerEvent`; add
  `case "sources": return { ...turn, citations: e.paths }`; `text_delta` now only
  appends text (drop `extractLinks`/`distinct` citation-scraping). Rename
  `applyAgentEvent` → `applyAnswerEvent`.
- `askSlice.ts`: `askSubmit` builds `AskRequest { query: q, top_k: null }` and
  calls `client.ask(req, onEvent, onError)`. `onEvent` typed `AnswerEvent`;
  lifecycle (`failed`/`completed`/`turn_completed`) unchanged; `sources` /
  `text_delta` / `tool_*` → `applyAnswerEvent`. Unify `errMsg` — use the shared
  `store/errMsg.ts`, delete the slice-local copy.

### UI (minimal)

- `AnswerView` / `citation.ts`: `turn.citations` are now note **paths**. The
  "Sources:" footer renders them, label = `stem(path)` for readability, click
  passes the full path to `onOpenNote`. `AskPanelHost.resolveStem(notePaths, path)`
  still resolves (it stems both sides). Inline `renderText` wikilink buttons
  untouched. `AskSheet`/`AskSheetHost` get the same treatment.

### Contract guard

Add `assertAnswerEvent` to `client/contractGuards.ts` (validate the `type`
discriminant + per-variant payload), used by DaemonClient — mirrors `assertEvent`.

## Data flow

1. User submits in `AskBar`/`AskPanel`/`AskSheet` → `askSubmit(question)`.
2. Slice appends a user turn + an empty assistant turn, sets `streaming`, builds
   `AskRequest { query, top_k: null }`, calls `client.ask(req, onEvent, onError)`.
3. Daemon `POST /ask` → SSE stream. First frame `sources` → reducer sets
   `citations = paths`. `text_delta` frames append text (live caret). `tool_*`
   frames update the tool list. `completed` → `streaming = false`, stop. `failed`
   → `error = message`, `streaming = false`, stop.
4. User closes the surface → `askClose` → `Unsubscribe` cancels the reader; a
   monotonic run token drops any late events from a superseded run.

## Error handling

- **Pre-stream** (auth/origin/engine): HTTP non-2xx → `onError` with the typed
  `ContractError` (401 → unauthorized) → slice sets `error`, `streaming = false`.
- **In-run**: `failed{message}` frame → slice sets `error = message`.
- **Parse/transport drift**: `assertAnswerEvent` throws → caught in the read loop
  → `onError`.
- Slice `errMsg` formats both `ContractError` bodies and `Error`s uniformly.

## Testing (TDD — test first)

- `daemon.test.ts`: success sequence (sources→deltas→completed); HTTP-error →
  `onError`; in-run `failed` delivered as an event; cancel stops further events +
  cancels the reader; multi-frame-in-one-chunk; frame-split-across-chunks.
- `mock.test.ts`: `AnswerEvent` shapes (sources first, completed last; fail path).
- `askReducer.test.ts`: `sources` → citations; `text_delta` appends text only.
- `askSlice.test.ts`: builds `AskRequest`; sources→citations; failed→error;
  cancel-on-close drops late events.

## Integration / git

- Develop on `wire-ask-real-engine-stream` (current = main: has contract +
  daemon) with `ask-ui-track` merged in → reproduces the eventual main state (ask
  UI + contract + daemon). Reconcile conflicts: `mock.ts` (main's plugin-broker
  mock vs ask-ui-track's ask method), `store.ts` (askSlice registration), `errMsg`
  unify, delete `agent.ts`. Fold in the `AskSheet`/`AskSheetHost` mobile/tablet
  files from `wire-real-agent-stream`.
- PR: target `main`, merge queue, "Merge when ready". Stacked on #62 (ask-ui) —
  flag in the PR body that it should land after #62, then rebase onto main so the
  final diff is just the ask-wiring delta.

## Done

A real engine (daemon) streams a note-grounded answer into the docked panel and
the mobile/tablet sheet: citations from `sources`, live text from `text_delta`,
graceful `failed` rendering. Full web gate green (lint, format:check, typecheck,
test, build) + `contract-drift` passing. Tauri desktop path explicitly deferred to
a follow-up PR (honest stub until then).
