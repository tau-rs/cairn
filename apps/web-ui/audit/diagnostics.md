# Diagnostics findings — cairn-web-ui

How errors and backend failures are surfaced to users and developers, and the
observability of the system.

---

## DG1. No error boundary — a render throw blanks the entire app
**Severity: High**
**Location:** `web/src/main.tsx:9-15`

The app mounts `<App/>` with no React error boundary anywhere in the tree. A
throw during render — plausible in the dense `livePreview.ts` decoration builder
(it does a lot of index arithmetic against `state.doc`, e.g. `:162-166`,
`:241-249`), in the mutating `react-force-graph` integration
(`GraphView.tsx`), or in any widget's `toDOM` — unmounts to a blank window. The
user gets no message, no recovery, and (in the packaged app) no console they'd
think to open.

**Impact:** A single edge-case note (a malformed table, an unusual markdown
construct) can make the editor appear to "crash to white" with zero diagnostic.

**Recommendation:** Wrap `App` (and separately the editor + graph panes) in an
error boundary that renders a recoverable fallback and logs the error/stack.
Reset the boundary on note/view change.

---

## DG2. Backend failures surface only as a transient, lossy toast
**Severity: Medium**
**Location:** `web/src/store/store.ts:597-605` (`errMsg`) feeding the single
`error` slot consumed by `web/src/components/ErrorToast.tsx`

Every caught error from a command/query is funneled to `errMsg` and dropped into
the single `error: string | null`. There is:
- **no logging** — nothing is written to `console`, so a developer debugging a
  field report has no record of what failed or when (the only artifact is a
  toast the user already dismissed);
- **no context** — `errMsg` returns just `e.message`/`Not found: …`; the failing
  command/query, path, and error `type` are discarded;
- **loss under churn** — as noted in design.md U3/D2, a later `set` can
  overwrite the error before it's seen.

**Impact:** Backend/IPC failures are effectively unobservable after the fact for
both users (gone on next render) and developers (never logged).

**Recommendation:** `console.error` the structured error (command/query + typed
`ContractError`) at the client boundary in addition to the toast; keep a small
in-memory error log; include the operation in the surfaced message.

---

## DG3. Tauri event subscription has no disconnect/error handling
**Severity: Medium**
**Location:** `web/src/client/tauri.ts:22-34`; `web/src/store/store.ts:197-210`

`subscribe` registers a `listen("cairn://event", …)` and forwards payloads. There
is no handling for the channel failing to attach (the `pending.then` has no
`.catch`), no heartbeat, and no surfaced state if events stop arriving. Since the
entire reactive-refresh model (notes/tags/backlinks/graph updating after writes)
depends on these push events, a dropped event stream means the UI **silently
goes stale** — edits save but nothing else updates, with no indication anything
is wrong.

**Impact:** A failed/dead event channel degrades the app to "writes work,
everything else is frozen" with no diagnostic or reconnect.

**Recommendation:** `.catch` the listen registration and surface/log a
"live updates unavailable" state; consider a periodic reconciliation
(re-`refreshNotePaths`) as a backstop, and a manual refresh affordance.

---

## DG4. Floating promises swallow rejections outside the per-action try/catch
**Severity: Low**
**Location:** `web/src/store/store.ts:197-206` (`void get().refreshNotePaths()` etc.), `:282,348-349,389,396` and throughout; `web/src/app/App.tsx:43,106,137,177,250` (`void actions.x()`)

The codebase uses `void promise` liberally. Most store actions catch internally,
so this is usually fine — but it means any rejection that escapes an action's own
try/catch (e.g. a throw inside the subscribe callback before a try block, or a
future action that forgets its try/catch) becomes an unhandled rejection with no
boundary to catch it. There is no global `unhandledrejection` handler.

**Recommendation:** Add a `window.addEventListener("unhandledrejection", …)` that
logs (and optionally surfaces) escaped rejections, so the `void` pattern fails
loud instead of silent.

---

## DG5. No console/observability hooks for the index-refresh cascade
**Severity: Low**
**Location:** `web/src/store/store.ts:197-210`

The subscribe handler kicks off a fan-out of queries on every `note_changed`
(see design.md D2) with no logging, timing, or counters. When the storm causes
jank there is nothing to observe it by — no dev-mode trace of "save → N queries
→ graph rebuild". For an app whose whole value is a reactive index, the
refresh pipeline is a blind spot.

**Recommendation:** Behind a dev flag, log/trace each refresh trigger (event
type → actions dispatched) and time the backend calls; this also makes D1/D2
regressions visible in tests.

---

## Positive notes
- Errors from the contract are typed (`ContractError`) and `errMsg`
  (`store.ts:597-605`) handles the `not_found` variant specifically — the
  decoding is correct as far as it goes.
- Persistence loaders all fail safe (try/catch → sensible default), so corrupt
  `localStorage` never crashes startup.
- The mock client faithfully mirrors engine error semantics (e.g. NotFound on
  delete/rename, `mock.ts:114-124,145-156`), which makes error paths testable
  without the backend.
