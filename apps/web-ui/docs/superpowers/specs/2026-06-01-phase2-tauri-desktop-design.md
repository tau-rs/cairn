# Cairn Web UI — Phase 2: Tauri Desktop Shell Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 2 of [`docs/roadmap.md`](../../roadmap.md) (Real transport)
**Builds on:** the walking-skeleton UI
([`2026-06-01-walking-skeleton-ui-design.md`](2026-06-01-walking-skeleton-ui-design.md))
**Engine:** `tau-rs/cairn` @ `079f9f9` (`cairn-service` dispatcher + `cairn-daemon`
reference; ADR-0002 assigns Tauri wiring to the UI session)

---

## 1. Purpose

Replace the in-browser `MockClient` with a real, in-process engine behind a
Tauri v2 desktop shell, so Cairn becomes an installable offline desktop app
operating on real markdown-under-git on disk. The UI is unchanged except for an
"open a cairn" entry point; the swap is isolated to the composition root.

The engine gap is already closed upstream (`cairn-service`), so this is **pure
UI-side wiring** — no engine-repo work.

### Non-goals (deferred)

- Real mobile behavior: opening a cairn on iOS/Android (document picker /
  security-scoped bookmarks / Android SAF) and `git2` cross-compilation. The
  mobile **build target** is scaffolded here; mobile file-access + git are a
  follow-up phase.
- Daemon/`DaemonClient` (separate later sub-project), auth, multi-cairn windows,
  Tauri-driver e2e, app-store packaging/signing pipelines.

---

## 2. Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| TS↔Rust bindings | **Hand-rolled `invoke`/`listen`**, reusing the vendored ts-rs contract types. Rust commands take/return the real DTOs; errors reject with `ContractError` (identical to `MockClient`). No `tauri-specta`. |
| Opening a cairn | **Folder picker + persist last path + auto-reopen on launch.** |
| Platform | **Desktop fully working now**; mobile **target scaffolded** (builds/launches) but mobile file-access + git **deferred**. |
| Engine dependency | Git deps on `tau-rs/cairn` **pinned to `079f9f9`** (same commit as the vendored TS contract). |

---

## 3. Architecture

Mirrors `cairn-daemon` (the proven reference), swapping HTTP/WS for Tauri IPC +
events.

```
Webview (React, unchanged)            Tauri (Rust, new: src-tauri/)
  components → store                    #[command] send_command / run_query
       │                                #[command] pick_and_open_cairn / current_cairn
  CairnClient + CairnHost  ──invoke──▶  TauriSink (EventSink) ──▶ cairn-service
       ▲                                        │                 dispatch_*
       └────────── listen("cairn://event") ◀─── emit              │
                                                                  ▼
                                          Engine<LocalFsStore, InMemoryIndex, GitVcs>
                                                                  ▼
                                                       markdown + git on disk
```

- The engine runs in-process, synchronous, behind `Arc<Mutex<Option<CairnEngine>>>`,
  on a blocking thread (`spawn_blocking`) — exactly as the daemon does.
- `Option` models "no cairn open yet".

---

## 4. Rust backend (`src-tauri/`)

New crate at the repo root. `Cargo.toml` git-pins the engine:

```toml
[dependencies]
cairn-domain   = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9" }
cairn-app      = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9" }
cairn-infra    = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9" }
cairn-contract = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9" }
cairn-service  = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9" }
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```
(Exact crate-version pins resolved at implementation time; engine `rev` is fixed.)

### State & event sink

```rust
type CairnEngine = Engine<LocalFsStore, InMemoryIndex, GitVcs>;

struct CairnState {
    engine: Mutex<Option<CairnEngine>>,
    path: Mutex<Option<PathBuf>>, // currently-open cairn path
}

struct TauriSink(AppHandle); // emit -> app.emit("cairn://event", app_event_to_wire(e))
```

### Commands

All engine work runs under `spawn_blocking`; `ServiceError` maps to
`ContractError` for the `Err` arm (Tauri serializes it as the rejection value).

- `send_command(command: Command) -> Result<CommandResponse, ContractError>` —
  errors `invalid_request("no cairn open")` if none open; else `dispatch_command`
  with a `TauriSink`.
- `run_query(query: Query) -> Result<QueryResponse, ContractError>` — same guard;
  `dispatch_query` (no events).
- `pick_and_open_cairn() -> Result<Option<String>, ContractError>` — Rust folder
  picker (`tauri-plugin-dialog`); `None` if cancelled. On pick: build the engine
  (`LocalFsStore::open`, `GitVcs::open_or_init`, `InMemoryIndex::default`),
  `reindex` (emitting `reindexed` via a `TauriSink`), store it + the path, and
  **persist the path**. Returns the chosen path.
- `current_cairn() -> Option<String>` — the open path, if any.

### Persistence & auto-reopen

The open path is written to a file in `app_config_dir()` (e.g.
`last-cairn.txt`). In Tauri `setup`, read it; if present and openable, build the
engine and set state before the window loads. Failures to reopen are
non-fatal — the app falls back to the empty state.

### `tauri.conf.json`

`identifier: rs.tau.cairn`; `build.beforeDevCommand: "pnpm --dir web dev"`,
`build.devUrl: "http://localhost:5173"`, `build.beforeBuildCommand:
"pnpm --dir web build"`, `build.frontendDist: "../web/dist"`; `tauri-plugin-dialog`
registered; one main window.

---

## 5. Frontend seam

The composition root becomes a `{ client, host }` pair.

```ts
// CairnClient is unchanged (engine contract). New, separate:
interface CairnHost {
  currentCairn(): Promise<string | null>; // open path, or null
  openCairn(): Promise<string | null>;    // pick + open; null if cancelled
}

interface Backend { client: CairnClient; host: CairnHost; }
function makeBackend(): Backend; // isTauri() ? Tauri : Mock
```

- `TauriClient` (`web/src/client/tauri.ts`): `sendCommand` → `invoke("send_command", { command })`;
  `runQuery` → `invoke("run_query", { query })`; `subscribe` →
  `listen("cairn://event", (e) => cb(e.payload as Event))` returning the unlisten
  fn. `invoke` rejection is re-thrown as-is (the payload is a `ContractError`),
  so the store's `errMsg` handles it identically to the mock.
- `TauriHost`: `currentCairn` → `invoke("current_cairn")`; `openCairn` →
  `invoke("pick_and_open_cairn")`.
- `MockHost` (`web/src/client/mock.ts` or a sibling): `currentCairn()` resolves a
  constant sentinel (e.g. `"(fixture)"`) so a cairn is **always open** under the
  mock; `openCairn()` resolves the same sentinel (no-op). This keeps every
  existing unit test and the browser e2e behaving as today.
- `makeBackend` (`web/src/app/makeBackend.ts`, replacing `makeClient.ts`): detect
  Tauri via `isTauri()` (presence of `window.__TAURI_INTERNALS__` /
  `@tauri-apps/api` `isTauri()`); return the matching pair.

---

## 6. Store + UI changes

- Store: add `cairnPath: string | null`. The store creator becomes
  `createCairnStore(client, host?)` where `host` **defaults to an always-open
  host** (`currentCairn`/`openCairn` resolve a constant sentinel). This keeps the
  existing `createCairnStore(client)` call sites and all current store tests
  working unchanged (a cairn is always "open"); only the app's composition root
  passes a real `host`.
  `init()` first calls `host.currentCairn()` → sets `cairnPath`; if non-null,
  proceeds with `refreshNotePaths()` + event subscription + `rearmInterval()`.
  New action `openCairn()` → `host.openCairn()`; on a non-null result, set
  `cairnPath`, clear active note, `refreshNotePaths()`. (Cancel = no change, no
  error.)
- `App.tsx`: `cairnPath ? <Shell …/> : <OpenCairn onOpen={openCairn} />`.
- New `OpenCairn` component: centered empty state with title + **"Open a cairn…"**
  button calling `openCairn`. Never rendered under the mock (cairn always open),
  so existing component tests and e2e are unaffected.

---

## 7. Testing

- **Rust (`src-tauri`):** Tauri mock-runtime tests of the command layer:
  `send_command`/`run_query` with no cairn open → `ContractError`; opening a
  `tempfile::tempdir` cairn then `get_note`/`search`/`list_notes` succeed and
  events emit; persisted-path round-trip. (Dispatch correctness itself is already
  covered in `cairn-service`.) `cargo fmt`/`clippy` clean.
- **TS:** unit-test `TauriClient` and `TauriHost` by mocking `@tauri-apps/api`
  `invoke`/`listen`; `MockHost`; store `openCairn`/`cairnPath` with a fake host.
  All pre-existing tests remain green on the mock.
- **e2e:** the browser+mock Playwright e2e stays the integration gate (it runs
  with no Tauri present → mock backend). Real Tauri-driver e2e deferred.
- **Manual:** run the desktop app (`pnpm --dir web build` + `cargo tauri dev` /
  `tauri build`) and verify open→edit→search→backlink→commit on a real folder;
  capture a screenshot.

---

## 8. CI + mobile

- CI: add a Rust job — `cargo fmt --check`, `cargo clippy`, `cargo check`, and
  `cargo test` for `src-tauri` on Ubuntu with the WebKitGTK dev libs
  (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`,
  `librsvg2-dev`, `build-essential`). The existing `web` job is unchanged. Full
  desktop bundling and mobile builds stay out of CI.
- Mobile: run `cargo tauri ios init` and `cargo tauri android init` so the
  project builds and launches the shell on a simulator/emulator. The committed
  result documents (in this spec and a README note) that **mobile open-a-cairn +
  git-on-device are deferred** — the mobile shell will show the empty state but
  picking a cairn is not yet wired for mobile file systems.

---

## 9. Build order (for the plan)

1. Scaffold `src-tauri/` (Tauri v2, dialog plugin, conf pointing at `web/`),
   engine git-pinned to `079f9f9`; app boots showing the existing UI (mock still
   active because the frontend seam isn't switched yet).
2. Rust: `CairnState`, `TauriSink`, `open_engine` helper (+ tempdir test).
3. Rust commands: `send_command`/`run_query` with the no-cairn guard (+ mock-runtime tests).
4. Rust: `pick_and_open_cairn` + `current_cairn` + persistence + auto-reopen in `setup`.
5. TS: `CairnHost` interface; `MockHost`; make the store take `{client, host}`,
   add `cairnPath`/`openCairn`, wire `init` (+ store tests with a fake host).
6. TS: `OpenCairn` component + `App` empty-state branch (+ component test).
7. TS: `TauriClient` + `TauriHost` (+ unit tests mocking `@tauri-apps/api`);
   `makeBackend` with `isTauri()` detection (replaces `makeClient`).
8. CI Rust job; verify full unit/e2e suite still green on the mock.
9. Mobile: `tauri ios/android init`; confirm the shell builds/launches; document
   the deferral.
10. Manual desktop run + screenshot.

---

## 10. Risks

- **Engine git-dep build:** first build fetches `tau-rs/cairn` @ `079f9f9` and
  compiles `git2`/libgit2 — slow first build; needs network. Acceptable.
- **Tauri command threading:** engine is sync + `&mut`; must run under
  `spawn_blocking` under the mutex (as the daemon does) to avoid blocking the
  async runtime.
- **`isTauri()` detection** must be robust so the browser dev server / tests
  never accidentally select the Tauri backend.
- **Mobile init** may alter project layout (`gen/`); keep generated mobile
  artifacts out of git except the checked-in config, and don't let it touch the
  desktop build.
