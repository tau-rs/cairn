# Cairn Web UI — Walking-Skeleton Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** Phases 0+1 of [`docs/roadmap.md`](../../roadmap.md) (Scaffold +
Walking-skeleton UI)
**Engine handoff:** `tau-rs/cairn` —
`docs/superpowers/specs/2026-06-01-cairn-engine-design.md`

---

## 1. Purpose

Build the first vertical slice of the Cairn web UI: a working three-pane note
app that exercises the **entire** engine contract loop — open a cairn, list /
create / edit / delete notes, search, view backlinks, save, and commit — driven
by an async command/query/event contract, running against a **faithful mock**
of the engine.

The skeleton proves the transport-blind client architecture end-to-end so that
swapping the mock for a real transport (Tauri first) in Phase 2 touches a single
composition-root file and nothing else.

### Non-goals (deferred to later phases)

Real transport / Tauri shell (Phase 2), graph view (4), command palette /
panes / tabs / themes (5), UI-plugin host (6), tau actions (7), git push/sync,
multi-cairn management, conflict/merge UX.

---

## 2. Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Stack | React 18 + Vite + TS + Tailwind 3 + Zustand + react-router-dom; Vitest + Testing Library + Playwright; pnpm. (Mirrors `tau-web-ui`.) |
| Transport | **Tauri-first** at Phase 2, but the UI is written against a transport-abstracted `CairnClient`; skeleton runs on `MockClient`. |
| Layout | **Classic three-pane** (note list · editor · backlinks) + top bar. |
| Editor | **One `Editor` component, two modes:** rich CodeMirror 6 (default) ⇄ raw markdown `<textarea>` (toggle). |
| Save/commit | **Three layers:** debounced autosave (always) → auto-commit (idle **and** interval, both configurable) → manual commit (named). Push/sync deferred. |
| Mock fidelity | **Faithful** — really parses `[[wikilinks]]`, computes backlinks, substring search, emits correct events with realistic debounce timing. |
| Open a cairn | Skeleton **auto-loads a single in-memory fixture cairn**; the real picker (Tauri file dialog) lands in Phase 2. |

---

## 3. The contract (imported, not invented)

The source of truth is the generated TypeScript at
`tau-rs/cairn/crates/cairn-contract/bindings/` — `Command.ts`, `Query.ts`,
`Event.ts`. These are **vendored** into `web/src/contract/` (copied, with a sync
script and a recorded source commit) so the build is self-contained and the
pinned version is explicit, mirroring `tau-web-ui`'s pinned-contract approach.

```ts
// Command — mutations
type Command =
  | { type: "write_note";  path: string; contents: string }
  | { type: "delete_note"; path: string }
  | { type: "commit";      message: string };

// Query — reads
type Query =
  | { type: "get_note";      path: string }
  | { type: "search";        query: string }
  | { type: "get_backlinks"; path: string };

// Event — server → client push
type Event =
  | { type: "note_changed"; path: string }
  | { type: "note_deleted"; path: string }
  | { type: "committed";    commit: string }
  | { type: "reindexed";    count: number };
```

### Contract update (2026-06-01): the gap is closed

The "engine gap" this spec originally deferred — no dispatcher, no query
response DTOs, no event mapper — was **closed by the engine session** (ADR-0002,
`tau-rs/cairn` @ `079f9f9`). The engine now ships `cairn-service` (a
transport-blind dispatcher: `dispatch_command` / `dispatch_query` /
`app_event_to_wire`) and `cairn-daemon` (an HTTP transport). ADR-0002 explicitly
assigns Tauri to the UI session ("the UI session wires Tauri by calling
`cairn-service` in-process"), validating this project's roadmap.

We therefore vendor and build against the **real** response DTOs rather than
inventing our own. The full pinned surface (`079f9f9`):

```ts
// Command — mutations (unchanged)
type Command =
  | { type: "write_note"; path: string; contents: string }
  | { type: "delete_note"; path: string }
  | { type: "commit"; message: string };

// Query — reads (now includes list_notes + get_graph)
type Query =
  | { type: "get_note"; path: string }
  | { type: "search"; query: string }
  | { type: "get_backlinks"; path: string }
  | { type: "list_notes" }
  | { type: "get_graph" };

// CommandResponse — success of a command
type CommandResponse = { type: "done" } | { type: "committed"; commit: string };

// QueryResponse — success of a query
type QueryResponse =
  | { type: "note"; contents: string }              // get_note
  | { type: "paths"; paths: string[] }              // search, get_backlinks
  | { type: "notes"; notes: NoteSummary[] }         // list_notes
  | { type: "graph"; nodes: string[]; edges: GraphEdge[] };  // get_graph

type NoteSummary = { path: string; title: string };
type GraphEdge = { from: string; to: string };

// ContractError — failure of any command/query (rejection)
type ContractError =
  | { type: "not_found"; what: string }
  | { type: "invalid_request"; message: string }
  | { type: "internal"; message: string };
```

**Behavioral notes from the real dispatcher (`cairn-service`):**
- `get_note` on a **missing** note rejects with `not_found` (it does *not* return
  a null note). The UI/mock must treat a missing note as an error.
- `search` and `get_backlinks` both return the `paths` variant.
- `list_notes` returns `notes` (one `NoteSummary` each); `title` =
  `display_title` (frontmatter `title:`, else first `# ` heading, else path
  stem). This replaces the earlier `search("")` note-list hack.
- `get_graph` returns `graph` (note paths + directed link edges). Available for
  the Phase-4 graph view; the skeleton does not render it yet.
- Invalid note paths reject with `invalid_request`; adapter failures with
  `internal`.

---

## 4. The `CairnClient` seam

The single abstraction the entire UI is written against. The app imports only
this interface; concrete implementations are selected once, in the composition
root.

```ts
interface CairnClient {
  sendCommand(c: Command): Promise<CommandResponse>;  // rejects with ContractError
  runQuery(q: Query): Promise<QueryResponse>;         // rejects with ContractError
  subscribe(cb: (e: Event) => void): Unsubscribe;     // () => void
}
```

`sendCommand`/`runQuery` reject with a `ContractError` on failure (the same
typed error the daemon and `cairn-service` produce), so the store's error
handling is identical across mock and real transports.

Implementations:

| Impl | Phase | Transport |
|---|---|---|
| `MockClient` | 0–1 (this spec) | in-memory fixture cairn |
| `TauriClient` | 2 | Tauri IPC + event channel |
| `DaemonClient` | later | `fetch` + WebSocket |

Design constraints that keep the seam transport-neutral:

- `subscribe` returns an unsubscribe function (works for both a Tauri event
  listener and a WebSocket).
- No method assumes synchronous/in-process behavior; everything is `Promise`.
- The client never exposes filesystem or engine internals — only contract types.

---

## 5. The faithful mock

`MockClient` owns an in-memory model of a cairn and behaves like the real
engine so the UI can be exercised and e2e-tested before any transport exists.

**State:** `Map<path, contents>` seeded from `client/fixtures/` (a handful of
interlinked markdown notes, e.g. `index.md`, `ideas.md`, `todo.md` with
`[[wikilinks]]` between them).

**Behavior (mirrors `cairn-app` semantics):**

- `write_note` → upsert contents; debounce-free at the client layer (debounce
  lives in the UI), then **emit `note_changed`**, then recompute the index and
  **emit `reindexed`** (matching the engine's write→reindex event order).
- `delete_note` → remove; **emit `note_deleted`** then **`reindexed`**.
- `commit` → return a fake short id (e.g. monotonic `c0001`); **emit
  `committed`**. No real git in the mock.
- `get_note` → `{ type: "note", contents }`; a **missing** note **rejects** with
  `{ type: "not_found", what: path }` (mirrors `dispatch_query`).
- `search` → `{ type: "paths", paths }`: case-insensitive substring over **body
  OR path**, sorted by path (matches `InMemoryIndex`).
- `get_backlinks` → `{ type: "paths", paths }`: parse every note's
  `[[wikilinks]]`, return paths whose notes link to the target, resolved by
  **stem**, sorted + deduped (matches `Graph::backlinks`); links to nonexistent
  notes resolve to nothing.
- `list_notes` → `{ type: "notes", notes }`: one `NoteSummary` per note, sorted
  by path; `title` = `display_title` (frontmatter `title:`, else first `# `
  heading, else stem).
- `get_graph` → `{ type: "graph", nodes, edges }`: all note paths + resolved
  directed link edges. (Implemented for fidelity; the skeleton UI does not render
  it.)
- An invalid note path rejects with `{ type: "invalid_request", message }`.

**`[[wikilink]]` parsing** must match the engine's extraction rules; the parser
is its own small, unit-tested module so it can be reused by the editor
(autocomplete later) and verified against the engine's behavior.

Event emission is asynchronous (microtask/`queueMicrotask`) so subscribers
observe the same "push after the fact" timing a real transport gives.

---

## 6. State (Zustand)

A single store (sliced) holds UI-facing state; the `CairnClient` is the only
way it talks to the engine.

- **notes slice** — known note paths (from `list_notes`, mapping `NoteSummary`
  → path), the active note path + its loaded contents, dirty/saving status.
- **search slice** — current query, results, open/closed.
- **backlinks slice** — backlinks for the active note.
- **commit slice** — last commit id, autosave/auto-commit status, pending state.
- **settings slice** — auto-commit config (idle on/off + delay ms; interval
  on/off + minutes), editor mode (rich/raw).

The store subscribes to `client.subscribe` once at startup and reacts to events:
`note_changed`/`note_deleted` → refresh the note list and, if it's the active
note, reconcile; `committed` → update commit status; `reindexed` → refresh
search/backlinks if visible. This is the same reactive path real engine events
will drive.

---

## 7. Save & commit model

Three independent layers, each tested in isolation:

1. **Autosave** — editor edits mark the buffer dirty and schedule a debounced
   `write_note` (~1s idle). Always on. Status shown in the CommitBar
   ("saving…" / "saved").
2. **Auto-commit** — configurable, both triggers available:
   - *idle*: after ~N seconds of no edits (debounced longer than autosave),
   - *interval*: every ~N minutes,
   fire `commit` with an auto-generated message (e.g. `cairn: update ideas.md`
   or `cairn: 3 notes updated`). Only commits when there is uncommitted change.
   Avoids commit bloat (only on idle/interval, never per keystroke) and the
   quit-before-interval gap (idle trigger covers it).
3. **Manual commit** — a CommitBar button opens a message input and fires
   `commit` with the user's message; always available regardless of auto-commit
   settings.

A small **debounce/timer utility** module backs autosave + idle/interval
auto-commit and is unit-tested with fake timers.

---

## 8. UI components

| Component | Responsibility |
|---|---|
| `Shell` | three-pane layout + top bar; owns nothing but composition |
| `TopBar` | `SearchBar` + `CommitBar` |
| `SearchBar` | query input → `search`; opens the results overlay |
| `SearchResults` | overlay/panel over the note list; click → open note |
| `CommitBar` | autosave/auto-commit status indicator + manual commit button/input |
| `NoteList` | list known notes; new-note (path prompt → `write_note`) + delete (`delete_note`) actions; select → load active note |
| `Editor` | rich (CodeMirror 6) ⇄ raw (`<textarea>`) toggle over one buffer; drives autosave |
| `Backlinks` | backlinks of the active note; click → open |
| `Settings` | minimal panel for auto-commit knobs + editor default mode |
| `ErrorToast` | non-blocking surface for rejected commands/queries |

---

## 9. Error handling

`sendCommand`/`runQuery` reject with a `ContractError`. The store catches,
formats it to a human string (`not_found` → "Not found: …"; `invalid_request` /
`internal` → their message), surfaces a non-blocking `ErrorToast`, and
**preserves the editor buffer on write failure** (never lose the user's text).
Failed autosave retries on the next edit/idle; failed commit leaves state
uncommitted and is reported. The mock can be told to inject failures for tests.

---

## 10. Testing

Mirrors `tau-web-ui`:

- **Unit (Vitest):** wikilink parser, mock semantics (events, search,
  backlinks), debounce/timer utility, store reducers/effects, settings.
- **Component (Testing Library):** each component against the store + mock.
- **e2e (Playwright):** the full loop on the faithful mock — create a note,
  link to another, edit, observe autosave, search and find it, open via
  backlink, trigger auto-commit (fake timers / forced), manual commit. Because
  the mock is faithful, these tests stay valid after the Phase-2 transport swap.

CI (GitHub Actions, mirroring tau-rs conventions): typecheck, lint, unit, e2e,
build.

---

## 11. Build order within the sub-project

**Phase 0 — Scaffold**
1. Vite + React + TS + Tailwind + Zustand + router; pnpm; eslint/prettier; CI.
2. Vendor contract types (incl. `CommandResponse`/`QueryResponse`/
   `ContractError`/`NoteSummary`/`GraphEdge`) into `web/src/contract/` + sync
   script + recorded source commit (`079f9f9`).
3. Define `CairnClient` over the real `CommandResponse`/`QueryResponse`/
   `ContractError` types.
4. Wikilink parser (TDD).
5. `MockClient` + fixture cairn (TDD against engine semantics).
6. Composition root selecting `MockClient`; startup event subscription.

**Phase 1 — Walking-skeleton UI** (each step TDD)
7. Store slices + event reactions.
8. `Shell`/`TopBar` three-pane layout.
9. `NoteList` (+ new/delete).
10. `Editor` (rich + raw toggle) + buffer.
11. Autosave (debounced `write_note`) + status.
12. `SearchBar`/`SearchResults`.
13. `Backlinks`.
14. Auto-commit (idle + interval) + `CommitBar` + manual commit.
15. `Settings`.
16. `ErrorToast`.
17. Playwright e2e of the full loop.

---

## 12. Phase-2 handshake (what this spec hands forward)

- The engine gap is already closed: `cairn-service` (`dispatch_command` /
  `dispatch_query` / `app_event_to_wire`) and `cairn-daemon` exist. Phase 2 is
  pure UI-side wiring, no engine work required.
- `TauriClient` wraps `cairn-service` in-process via Tauri IPC + an event
  channel; `DaemonClient` (later) wraps `cairn-daemon` over HTTP/WS. Both
  satisfy the same `CairnClient` interface the mock implements, so swapping the
  composition root is the only intended UI change.
- The faithful mock's behavior is the conformance target for both real clients.
- Open-a-cairn picker + Tauri file dialog land here.
