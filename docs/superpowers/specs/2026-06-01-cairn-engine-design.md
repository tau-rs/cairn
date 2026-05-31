# Cairn Engine — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Scope of this document:** the Cairn *engine* (`cairn-core`), its CLI, and
the contract surfaces that a separate web-UI session will consume. The web UI
itself is out of scope here and is the subject of a later handoff.

---

## 1. Vision

Cairn is an open-source, git-backed, Obsidian-class note-management app. The
base is intentionally minimal and is augmented through plugins. Two pillars:

1. An **engine built in Rust** that is usable as a CLI and embeddable in a
   front-end app.
2. **First-class tau integration** — Cairn is built for tau (Titouan's
   terminal-native Rust agent runtime), which is still in active build.

Cairn lives in the **`tau-rs`** GitHub org and mirrors tau's engineering
conventions (hexagonal architecture, `forbid(unsafe_code)`, ADR + Diátaxis
docs, dual MIT/Apache license, lefthook hooks, GitHub Actions CI).

### What this session produces

- A **walking skeleton**: a working git-backed engine + CLI.
- The **full contract surface** (commands / queries / events) plus generated
  TypeScript bindings — the artifact the UI session imports.
- Every other capability (engine-plugin host, tau adapter, daemon transport,
  CRDT collab, UI-plugin host) present as a **proven seam** (a port/trait with
  a `Null`/stub adapter), to be filled in by later sub-projects.
- A **handoff document** for the UI session.

---

## 2. Vocabulary

Cairn keeps standard, generic software vocabulary wherever it is clear and not
distinctive to Obsidian: *note, link, backlink, tag, embed, command, index,
graph, plugin, frontmatter, attachment, search, workspace.* These are
industry-standard and are not renamed.

The **one** renamed term is Obsidian's signature "Vault":

| Concept | Cairn term |
|---|---|
| The whole note collection (Obsidian: "Vault") | **a cairn** |

The product is **Cairn**; one collection is **"a cairn"** — analogous to Git →
"a repo" or Docker → "an image". CLI: `cairn open <path>`, "my work cairn".

> Naming principle: only rename where there is a real point (IP
> distinctiveness or genuine clarity). Do not over-theme.

---

## 3. Integration model: one core, one contract, two transports

No single shell serves every target, so Cairn commits to a transport-blind
core and layers shells on top.

```
                 cairn-core  (Rust engine — all real logic, transport-blind)
                      |
        ONE contract: Commands / Queries / Event-stream
        (async request -> response + push events; serde types)
                      |
        +-------------+--------------+
        |                            |
  in-process transport         network transport
  (Tauri IPC)                  (daemon: HTTP/WS or JSON-RPC)
        |                            |
   Desktop + Mobile             Pure browser + Remote/multi-device
   offline-first, efficient     hosted or LAN access
        |                            |
        +------------+---------------+
                     |
        ONE web UI, written against a transport-abstracted client
        (same generated TypeScript contract; swaps IPC <-> network at runtime)
```

| Surface | How it is served | Status |
|---|---|---|
| Desktop, offline-first | Tauri shell, core in-process, local git cairn | UI session |
| Mobile (iOS/Android) | same Tauri v2 shell + same web UI | UI session / later |
| Pure browser (no install) | daemon transport, UI served as a web app | later sub-project |
| Remote / multi-device | same daemon, reachable over the network | later sub-project |

**Discipline this imposes (good hygiene regardless):** the contract is
designed as **async request→response + an event stream** from day one and never
assumes the engine is in the same process. This is what makes all four surfaces
reachable without a rewrite.

**Build order:** the in-process transport is validated first (the CLI is its
in-process consumer this session; the Tauri shell is the UI session). The
daemon transport is a later additive sub-project.

---

## 4. Architecture — the hexagon

`cairn-core` holds pure domain + application logic and depends only on trait
**ports**. Every concrete strategy is an **adapter** chosen at startup. This is
tau's `domain / ports / infra` split.

### Crate layout (proposed)

```
cairn-domain     pure model: cairn, note, link, graph, tag, thread — no I/O
cairn-ports      the traits (see Port Catalog)
cairn-app        use-cases / command+query handlers, event emission
cairn-infra/*    adapters: localfs-store, git-vcs, tantivy-index,
                 notify-watcher, native-executor, ... (one module/crate each)
cairn-contract   typed Commands/Queries/Events + codegen -> TypeScript
cairn-plugin-protocol   wire types + framing for engine plugins (tau-style)
cairn-plugin-sdk        SDK for writing engine plugins
cairn-cli        in-process consumer of the contract; validates it end-to-end
xtask            build/codegen/dev tasks
```

`#![forbid(unsafe_code)]` workspace-wide. Adapters never leak into the domain.

### Port catalog & default adapters

| Port (trait) | Abstracts | Adapters (strategies) | Default |
|---|---|---|---|
| `VaultStore` | read/write/list note content | `LocalFsStore` · `RemoteStore` · `MemoryStore` | **LocalFs** |
| `Vcs` (history) | commit / log / diff / branch / merge | `GitVcs` (gix/libgit2) · `NullVcs` | **GitVcs** |
| `MergePolicy` (consistency) | reconcile concurrent edits | `GitThreeWayMerge` · `SingleWriterLock` · *future* `CrdtMerge` | **GitThreeWay** |
| `SearchIndex` | query notes / links / tags | `TantivyIndex` · `InMemoryIndex` · `SqliteFtsIndex` | **Tantivy** |
| `Watcher` | detect external changes | `NotifyWatcher` · `PollingWatcher` · `RemotePushWatcher` · `NoopWatcher` | **Notify** |
| `Executor` (concurrency) | background / parallel work | `NativeExecutor` (tokio + rayon) · `WasmExecutor` | **Native** |
| `Transport` (client boundary) | how clients reach core | `InProcess` · `Daemon` | InProcess first |
| `AuthPolicy` (daemon only) | who may connect | `LoopbackTrust` · `TokenAuth` · `MutualTls` | **LoopbackTrust** |
| `CollabSession` | live CRDT co-editing | `NoCollab` · `LocalCrdt` · `RelayCrdt` | **NoCollab** |
| `AgentRuntime` (tau) | agent execution | `NullRuntime` · `TauRuntime` (loosely versioned) | **NullRuntime** |

### Concurrency model

- Async I/O on **tokio**; CPU-bound work (indexing, graph) on **rayon**.
- **Blocking git** runs on a dedicated blocking pool.
- The engine never calls `thread::spawn` directly — all concurrency goes
  through the `Executor` port, so a future WASM build swaps in a
  web-worker/single-thread executor without touching core logic.
- Long jobs carry **cancellation tokens**; the event stream uses **bounded
  channels** so a slow UI applies backpressure instead of exhausting memory.
- **Browser threading is a non-issue:** the browser surface is served by the
  daemon, where the engine runs natively with full parallelism; the browser
  runs only the thin UI client. WASM-in-tab (the only threading-constrained
  case) is deprioritized.

---

## 5. Domain model

- A **cairn** is a directory of plain markdown notes under a git repository.
- A **note** = a markdown file with optional YAML frontmatter, `[[links]]`,
  tags, embeds, and attachments.
- **Links** are resolved into a **backlink graph**; the **graph** (notes +
  links) is derived and indexed, not stored separately as source of truth.
- **Agent threads** are persisted conversations (stored as note sidecars,
  versioned in git), attachable to a note or to the whole cairn.

Plain markdown files in git are **always the canonical form** — the cairn is
"just files": diffable, portable, CLI-readable, and syncable/backupable via
git, on every surface.

---

## 6. The contract (the UI handoff artifact)

Async **request→response + event stream**, transport-blind, serde types, with
generated **TypeScript** bindings (specta / ts-rs).

- **Commands** (mutate): `create_note`, `update_note`, `rename_note`,
  `delete_note`, `commit`, `run_agent_action`, `post_thread_message`,
  `invoke_plugin_command`, …
- **Queries** (read): `get_note`, `list_notes`, `search`, `get_backlinks`,
  `get_graph`, `get_thread`, `list_plugins`, …
- **Events** (push): `note_changed`, `index_progress`, `graph_updated`,
  `git_state_changed`, `thread_message`, `agent_progress`, `plugin_event`, …

**Contract versioning** is explicit and additive (matters doubly because tau is
still in build and its surface shifts). New capabilities are additive contract
versions; existing shapes are not broken.

---

## 7. Plugin system

One **manifest** per plugin, declaring an optional `engine` part, an optional
`ui` part, capabilities, and dependencies. **Distribution:** git URLs, built
from source, **no marketplace** (mirrors tau's NG4).

### Engine plugins (designed + hosted by the engine)

- Run **out-of-process**, over **MessagePack-RPC**, **sandboxed**, and must
  **declare capabilities** (read cairn / write cairn / network / invoke-agent /
  …) which the core enforces. This mirrors tau's plugin host, capability
  filtering, and sandbox model.
- **Extension points (v1):**
  - **Commands** — register invocable actions (id, args, handler).
  - **Vault events** — subscribe to note created/modified/deleted/renamed,
    indexed, git commit.
  - **Content processors** — custom frontmatter extractors, markdown
    post-processors, custom link/embed resolvers feeding graph + index.
  - **Import / export + port backends** — importers/exporters (Notion, Roam,
    …) and plugin-supplied port adapters (e.g. a custom `VaultStore` /
    remote backend).

### UI plugins (contract designed now, host built in UI session)

- JS/TS in the webview (custom views, editor extensions, ribbon items, themes).
- This session defines the **API surface UI plugins call**; the UI-plugin host
  is built next session.

---

## 8. Tau integration

Via the `AgentRuntime` port. `NullRuntime` is the default so Cairn runs fully
before tau is ready; `TauRuntime` is a thin, **loosely-versioned** adapter.

Roles in scope (v1 design):

1. **Agent actions on notes** — Cairn invokes tau agents over content
   (summarize, find-related, draft-from-outline, rewrite, extract tasks),
   surfaced as commands. *(cairn → tau)*
2. **Cairn as agent context/tool** — expose search/read/write/link-traversal of
   the cairn to tau agents as a tool/context source (MCP-style), so an agent can
   ground itself in the notes. *(tau → cairn)*
3. **Agent threads attached to notes** — persistent agent conversations stored
   in the cairn (note sidecars, versioned in git), tied to a note or the whole
   cairn.

*Deferred:* plugins exposing tools/skills to tau and tau-packaged tools usable
from cairn (the plugin↔tau-tool bridge) — most coupled, revisit once tau firms
up.

---

## 9. Collaboration (two-tier; seams now, build later)

Git is the canonical at-rest store; a CRDT layer is an additive live tier.

```
LIVE TIER (only while 2+ clients edit the same note, online)
  client A <-> CRDT doc (yrs) <-> relay <-> client B   char-level, conflict-free
        | materialize (debounced / on idle / on save)
        v
DURABLE TIER (always on — source of truth)
  plain markdown file --commit--> git repo            diffable, offline, CLI
```

- Single user / offline / no relay → **no CRDT at all**, pure markdown→git.
- Concurrent live session + relay available → CRDT handles the live merge;
  result materializes back to markdown + git on a sensible cadence.
- Cross-session / offline / device-to-device → **git 3-way merge** (the
  `MergePolicy` port); true conflicts surfaced to the user.
- Optional later refinement: persist CRDT state as a sidecar for
  conflict-free *offline* merges.

This session wires the `CollabSession` port through the contract with the
`NoCollab` adapter; the CRDT live layer + relay is its own later sub-project.

---

## 10. CLI (`cairn`)

Same contract as the UI, consumed in-process. Surface (indicative):

```
cairn init | open <path>
cairn note new | edit | rm | mv
cairn search <query>
cairn links <note> | backlinks <note>
cairn graph
cairn commit | sync
cairn plugin add <git-url> | list
cairn agent <action> [<note>]
cairn serve            # daemon transport (later sub-project)
```

---

## 11. Repository & CI conventions (mirror tau)

- `#![forbid(unsafe_code)]` workspace-wide.
- Dual-licensed **MIT OR Apache-2.0**.
- **ADRs** in `docs/decisions/`; **Diátaxis** docs structure.
- A `CONSTITUTION.md` / guidelines document adapted for Cairn.
- **lefthook** pre-commit/pre-push hooks.
- GitHub Actions CI: `cargo build`, `cargo test`, `cargo clippy`,
  `cargo deny`, fmt check.
- Repo in the **`tau-rs`** GitHub org.

---

## 12. Decomposition into sub-projects

Each gets its own spec → plan → implementation cycle. The contract is designed
across all of them **now** so nothing is retrofitted.

| # | Sub-project | This session? |
|---|---|---|
| ① | Core + CLI git engine (domain, app, localfs, git, tantivy, watcher, contract, TS codegen) | **Build (walking skeleton)** |
| ② | Engine-plugin host (MessagePack-RPC, sandbox, capabilities) | Seam now; build later |
| ③ | Tau / `AgentRuntime` adapter | Seam now; build later |
| ④ | Daemon transport (+ `AuthPolicy`) | Seam now; build later |
| ⑤ | CRDT collaboration (`CollabSession`, relay) | Seam now; build later |
| ⑥ | UI-plugin host | Contract now; build in UI session |

### This session's build target (walking skeleton)

- `cairn-domain`, `cairn-ports`, `cairn-app`.
- Adapters: `LocalFsStore`, `GitVcs`, `TantivyIndex`, `NotifyWatcher`,
  `NativeExecutor`.
- `cairn-contract` with full Commands/Queries/Events + TypeScript codegen.
- `cairn-cli` exercising the contract in-process.
- Ports for plugin host, tau, collab, daemon present as traits with
  `Null`/stub adapters (seams proven, not filled).
- Repo scaffolding + CI + initial ADRs.

---

## 13. Open questions deferred to later specs/ADRs

- `gix` vs `libgit2` for `GitVcs`.
- Exact contract/RPC tooling (`rspc` vs `tauri-specta` vs hand-rolled
  JSON-RPC) and the codegen pipeline.
- Tantivy schema + incremental reindex strategy details.
- Target cairn scale (note count) for index tuning.
- Materialization cadence + offline CRDT sidecar (collab sub-project).
- Daemon auth defaults for network exposure (TokenAuth vs mTLS).
- The exact tau contract once tau's plugin/agent surface stabilizes.
```
