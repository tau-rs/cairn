# Cairn Web UI — Roadmap

**Date:** 2026-06-01 · **Last status refresh:** 2026-08-27
**Status:** living document — see [Live status](#live-status--2026-08-27) for the
current board; the phase narrative below is kept as design history.

This is the decomposition and ordering plan for `tau-rs/cairn-web-ui`, the
web-tech UI for [Cairn](https://github.com/tau-rs/cairn). Each phase gets its
own `spec → plan → build` cycle, mirroring how the engine repo was built
(walking skeleton first, everything else a proven seam).

## Context

Cairn's Rust engine is a transport-blind hexagon exposing one async contract:
**commands** (mutations), **queries** (reads), and a push **event stream**.
The generated TypeScript bindings at `tau-rs/cairn/crates/cairn-contract/bindings/`
are the source of truth this UI imports.

**The engine "gap" — now closed (update 2026-06-01):** the engine session
shipped ADR-0002 (`tau-rs/cairn` @ `079f9f9`): `cairn-service` (a transport-blind
dispatcher — `dispatch_command` / `dispatch_query` / `app_event_to_wire`) and
`cairn-daemon` (HTTP transport). The contract now includes the response DTOs
(`CommandResponse`, `QueryResponse`, `ContractError`, `NoteSummary`, `GraphEdge`)
and `list_notes` / `get_graph` queries. ADR-0002 explicitly assigns Tauri to the
UI session ("the UI session wires Tauri by calling `cairn-service` in-process"),
validating this roadmap. Phases 0–1 still build against a `MockClient` (no
running engine needed); Phase 2 is now pure UI-side wiring, no engine work.

## Locked decisions

- **Transport target: Tauri-first.** The engine runs in-process behind Tauri
  IPC — the spec's *primary shell*: offline-first desktop, the everyday personal
  note-taking experience. The daemon (HTTP/WS, for browser/remote/multi-device)
  is a genuinely-secondary later add-on.
- **Design for both modes.** The UI is written against a single
  transport-abstracted `CairnClient` interface with swappable implementations
  (`MockClient`, `TauriClient`, `DaemonClient`). The React app never knows which
  transport it is on; transport is a single composition-root choice. Supporting
  both Tauri and daemon is therefore a design property, not extra work.
- **Engine-gap ownership: resolved — the engine already closed it** (ADR-0002:
  `cairn-service` + `cairn-daemon`). No UI-side engine work remains for Phase 2.

## Proposed stack (mirrors `tau-rs`/tau-web-ui)

React 18 + Vite + TypeScript + Tailwind 3 + Zustand + react-router-dom,
`@xyflow/react` for the graph view, Vitest + Testing Library for unit/component
tests, Playwright for e2e. pnpm. Tauri v2 added at Phase 2.

## The `CairnClient` seam

```ts
interface CairnClient {
  sendCommand(c: Command): Promise<CommandResponse>;  // rejects with ContractError
  runQuery(q: Query): Promise<QueryResponse>;         // rejects with ContractError
  subscribe(cb: (e: Event) => void): Unsubscribe;
}
```

(`CommandResponse` / `QueryResponse` / `ContractError` are the real vendored
contract DTOs from `tau-rs/cairn`, not invented here.)

Implementations:
- `MockClient` — in-browser fake over a fixture cairn (Phases 0–1, dev/test).
- `TauriClient` — Tauri IPC + event channel (Phase 2, wired first).
- `DaemonClient` — `fetch` + WebSocket (later, zero UI changes).

## Phases

| Phase | What | Depends on | Where |
|---|---|---|---|
| **0 — Scaffold** ✅ done | Vite + React + TS + Tailwind + Zustand app. Vendored contract TS types. `CairnClient` interface + `MockClient` + fixture cairn. CI. | — | this repo |
| **1 — Walking-skeleton UI** ✅ done | Vertical slice on the mock: open a cairn, note list, open/edit a markdown note, search, backlinks panel, commit button, live refresh from the event stream. | 0 | this repo |
| **2 — Real transport** ✅ done | `TauriClient` (wraps `cairn-service` in-process) + Tauri v2 desktop shell + open-a-cairn picker (persist + auto-reopen); mock↔real switch isolated to `makeBackend`. Mobile target scaffolded (open-on-mobile deferred). | 1 | this repo |
| **3 — Editor depth** ✅ done | Rendered GitHub-README-style markdown (GFM + Tailwind Typography + code highlighting) is the DEFAULT view; CodeMirror source is the secondary "Edit source" mode; clickable `[[wikilinks]]` (resolved-by-stem). `editorMode` = `rendered`\|`source`. `[[wikilink]]` autocomplete in source and frontmatter rendering shipped in PR #50. | 1 | this repo |
| **4 — Graph view** ✅ done | `@xyflow/react` force-directed (`d3-force`) whole-cairn graph in a center-pane toggle; click node → opens note; live-refreshes on note events. *Deferred:* local/neighborhood graph. | 1 | this repo |
| **5 — Shell polish** ✅ done | Command palette, keyboard shortcuts, dialog host, `Settings`/`SettingsDialog`/`DaemonSettings`. | 1 | this repo |
| **6 — UI-plugin host** ✅ done | Iframe-sandboxed plugin host: broker (`pluginBroker`/`pluginBrokerHost`), permission prompts, slot/widget rendering, tier-3 contributions (`IframeHost`, `PermissionPrompt`, `SlotRenderer`, `WidgetView`, `PluginCapability`). | 3–5 | this repo |
| **7 — Tau actions** ⏸ gated | Surface `AgentRuntime` actions (summarize, find-related, …) once tau firms up — engine seam is still `NullRuntime`. External dependency. | 2 + tau | this repo + engine |
| **8 — Live-collab + recovery UI** 🔵 in progress | Surface engine epic A: CRDT collab sessions, the recovery surface (`Recover`/`Recoverable`), restore-to-new-block. Recovery/restore shipped (#146 ✅); live-collab presence (corner pill + non-destructive reload) on `feat/live-collab-presence`. | pin-resync ✅ + engine A ✅ | this repo |

**Critical path:** 0 → 1 → 2. Phases 3–8 fan out from the skeleton and can
reorder freely.

## Live status — 2026-08-27

Reconciled against live repo state (merged PRs + open issues), not carried
forward from the previous snapshot.
Engine `tau-rs/cairn` @ `main` #184 (`c147547`) · UI `tau-rs/cairn-web-ui` @
`main` #166 (`b2d68e8`).

**Two engine pins, and they disagree** — `web/src/contract/source.ts` is at
`ad97b05` (engine #179, C0), while all six `cairn-*` revs in
`src-tauri/Cargo.toml` are still `96a4ac5` (engine #168). Tracked as issue #172;
see the Tauri re-pin lane below. Nothing currently enforces that these two agree.

Combined `Lane | Deps | Status | Unblocks` board.
Legend: ⬜ blocked · 🟡 ready · 🔵 in progress · 🟣 in review · ✅ done.

**Engine (`tau-rs/cairn`)**

| Lane | Deps | Status | Unblocks |
|---|---|---|---|
| A — Live collaboration (CRDT fold-back, watcher arbitration, late-joiner snapshot, recovery surface, restore, e2e wire tests) | — | ✅ #144–#163 | UI-8 |
| B — Graph visualization (temporal contract + standalone viz) | — | ✅ | folded into UI-4 |
| C — Plugin trust & reach (capability vocab, net/agent cap enforcement, integration test) | — | ✅ | — |
| C· #40 plugin-trust hardening follow-ups | C ✅ | 🟡 open | — |
| D — Engine-owned auto-commit + versioning (C0 contract, diff summaries, message generator, `cairn/*` tags, idle/backstop sealing, `auto_commit` on) | — | ✅ engine #179 (`ad97b05`) | UI Phase 8 versions |
| E — Maintenance | — | ✅ dependabot backlog cleared 2026-08-10 (6 PRs enqueued, dirs-major #124 closed); RUSTSEC #131/#100 were already closed 2026-07-24 | — |

**UI (`tau-rs/cairn-web-ui`)**

| Lane | Deps | Status | Unblocks |
|---|---|---|---|
| Phases 0–6 (scaffold → Tauri transport → editor → graph → shell polish → plugin host) | — | ✅ | — |
| pin-resync: bump engine `8abc0ef` → `ed037d9` (`main` #163; pull recovery/restore/collab DTOs) | engine A ✅ | ✅ done | UI-8 |
| Phase 8 — Live-collab + recovery UI | pin-resync ✅ | ✅ recovery #146/#149; live-collab presence #153/#156/#157/#158; presence + Versions #159; C0 reconcile #160 | — |
| Tauri re-pin to C0 — `src-tauri` still links engine `96a4ac5`, which cannot produce `Revision.summary`/`.name`, `Command::name_version` or `CommandResponse::nothing_to_commit` | engine D ✅ | 🔵 in progress — issue #172, branch `repin-tauri-engine-c0` | desktop Versions |
| Phase 7 — Tau actions | tau firms up (external) | ⬜ gated | — |
| Phase 3 polish — `[[wikilink]]` autocomplete, frontmatter rendering | — | ✅ #50 | — |
| Maintenance — dependency flywheel | — | 🟡 ongoing; was **deadlocked** 08-24→08-27 (five green PRs never merged, three independent causes) — fixed in #166 + #171 | — |

**The Versions feature is done on the daemon transport and NOT on Tauri** until
#172 lands: the TypeScript types promise C0 shapes the desktop-linked engine
never sends, so named milestones and word deltas degrade silently there. That is
the one real dependency edge left. Everything else
is independent or done. There is no multi-wave DAG here, which is *why* this
board is a plain status doc and not a self-perpetuating rolling-handoffs pipeline
(status board + handoff protocol + auto-emitting handoff template): that
machinery earns its keep on a wide graph of blocked lanes feeding each other, and
would be pure ceremony over the ~4 flat, mostly-independent lanes above. When a
future frontier *does* fan out into many dependency-ordered lanes across parallel
workspaces, reach for that pattern then; until then, keep this board honest and
verify live repo state (both repos) before trusting any older roadmap snapshot.

## Deferred decisions

- ~~Engine-gap ownership~~ — resolved: the engine closed it (ADR-0002).
- Daemon transport + `AuthPolicy` defaults (`cairn-daemon` exists; wiring a
  `DaemonClient` + auth is its own later sub-project).
- Whether mobile (Tauri v2) is in scope and when.
- Codegen tooling for the real transport (`ts-rs` as-is vs `tauri-specta`/`rspc`
  to generate command wiring too).
