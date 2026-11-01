# Cairn Phase 2 — Tauri Desktop Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wrap the existing React UI in a Tauri v2 desktop shell that runs the real Cairn engine in-process, so the app opens a real markdown-under-git folder and operates on it — swapping the in-browser `MockClient` for a `TauriClient` at a single composition root.

**Architecture:** A new `src-tauri/` Rust crate (engine git-pinned to `tau-rs/cairn@079f9f9`) mirrors `cairn-daemon`: an `Arc<Mutex<Option<Engine<LocalFsStore, InMemoryIndex, GitVcs>>>>` plus a `TauriSink` that emits engine events to the webview. Tauri commands (`send_command`, `run_query`, `pick_and_open_cairn`, `current_cairn`) run engine work under `spawn_blocking`. The frontend gains a `CairnHost` abstraction (open/current cairn) beside the unchanged `CairnClient`; `makeBackend()` returns the mock pair in a browser and the Tauri pair under Tauri.

**Tech Stack:** Tauri v2 (Rust), `tauri-plugin-dialog`; `@tauri-apps/api` v2; the existing React 18 + TS + Vite + Zustand app under `web/`; cargo-tauri CLI. Engine: `tau-rs/cairn@079f9f9` via git deps.

**Reference:** Spec `docs/superpowers/specs/2026-06-01-phase2-tauri-desktop-design.md`. Engine daemon reference: `tau-rs/cairn` `crates/cairn-daemon/src/lib.rs`. The walking-skeleton lives under `web/`.

---

## Preflight (one-time, environment)

Before Task 1, confirm the toolchain (these are environment prerequisites, not committed work):

- `rustc --version` (Tauri v2 needs Rust ≥ 1.77; the engine builds on current stable).
- `cargo install tauri-cli --version "^2.0"` → `cargo tauri --version` prints a 2.x version.
- `cmake --version` and a C compiler are available (the engine's `git2`/libgit2 builds vendored; on macOS `brew install cmake` if missing).
- macOS uses the system WebView (no extra install). The desktop app is **run manually** at the end (a GUI window); automated steps use `cargo build`/`cargo test`, never a blocking `tauri dev`.

If `cargo tauri` or `cmake` is unavailable and cannot be installed, STOP and report BLOCKED.

---

## File Structure

```
src-tauri/                      (NEW — Tauri desktop crate, repo root)
  Cargo.toml                    engine git-deps + tauri + dialog
  build.rs                      tauri-build (generated)
  tauri.conf.json               points at ../web; identifier rs.tau.cairn
  capabilities/default.json     core:default for the main window (generated)
  icons/                        app icons (generated)
  src/main.rs                   thin entry → cairn_lib::run()
  src/lib.rs                    state, TauriSink, open_engine, commands, builder, setup
  .gitignore                    target/, gen/
web/src/
  client/host.ts                NEW: CairnHost, alwaysOpenHost, MockHost
  client/host.test.ts           NEW
  client/tauri.ts               NEW: TauriClient + TauriHost
  client/tauri.test.ts          NEW
  app/makeBackend.ts            NEW (replaces app/makeClient.ts)
  app/cairnStore.ts             MODIFY: use makeBackend; expose host
  store/store.ts                MODIFY: createCairnStore(client, host?), cairnPath, openCairn
  store/store.test.ts           MODIFY: add cairnPath/openCairn tests
  components/OpenCairn.tsx       NEW
  components/OpenCairn.test.tsx  NEW
  app/App.tsx                   MODIFY: cairnPath ? Shell : OpenCairn
.github/workflows/ci.yml        MODIFY: add a Rust (src-tauri) job
README.md                       MODIFY: note desktop run + mobile deferral
```

Repo-root commands assume cwd `/Users/titouanlebocq/code/cairn-ui`. `cargo` commands for the Tauri crate run from `src-tauri/`. Web commands run from `web/`.

---

## Task 1: Scaffold the Tauri desktop crate

**Files:** Create `src-tauri/**` (via CLI), then edit `src-tauri/Cargo.toml`, `src-tauri/.gitignore`.

- [ ] **Step 1: Scaffold with the Tauri CLI (non-interactive)**

From the repo root:
```bash
cargo tauri init \
  --ci \
  --app-name cairn \
  --window-title Cairn \
  --frontend-dist ../web/dist \
  --dev-url http://localhost:5173 \
  --before-dev-command "pnpm --dir web dev --port 5173 --strictPort" \
  --before-build-command "pnpm --dir web build"
```
This creates `src-tauri/` with `Cargo.toml`, `build.rs`, `tauri.conf.json`, `capabilities/default.json`, `icons/`, and `src/{main.rs,lib.rs}`.

- [ ] **Step 2: Add the dialog plugin**

From the repo root: `cargo tauri add dialog`
This adds `tauri-plugin-dialog` to `src-tauri/Cargo.toml`, registers the plugin permission in `capabilities/default.json`, and adds the JS package to `web/` (harmless; we use the Rust API).

- [ ] **Step 3: Pin the engine git dependencies**

Edit `src-tauri/Cargo.toml` `[dependencies]` to add (keep the generated `tauri`, `serde`, `serde_json`, `tauri-plugin-dialog` entries):
```toml
cairn-domain   = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9f5178d357f3df743fc077ffd727cffe89" }
cairn-app      = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9f5178d357f3df743fc077ffd727cffe89" }
cairn-infra    = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9f5178d357f3df743fc077ffd727cffe89" }
cairn-contract = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9f5178d357f3df743fc077ffd727cffe89" }
cairn-service  = { git = "https://github.com/tau-rs/cairn", rev = "079f9f9f5178d357f3df743fc077ffd727cffe89" }
```

- [ ] **Step 4: Ensure generated artifacts are git-ignored**

Create/confirm `src-tauri/.gitignore`:
```
/target
/gen
```
(The `tauri init` may already add `/target`; ensure `/gen` — mobile output — is also ignored.)

- [ ] **Step 5: Build-check (fetches + compiles the engine)**

From `src-tauri/`: `cargo check`
Expected: fetches `tau-rs/cairn@079f9f9`, compiles the engine (incl. `git2`) and the default Tauri app; finishes with no errors. (First build is slow.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/ web/package.json && git commit -m "feat(tauri): scaffold desktop crate, engine git-pinned to 079f9f9"
```
(Do NOT commit `src-tauri/target` or `src-tauri/gen`.)

---

## Task 2: Engine state, event sink, and the open-engine helper

**Files:** Modify `src-tauri/src/lib.rs`. Test: inline `#[cfg(test)]` in `lib.rs`.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/lib.rs` (bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cairn_contract::{Command, Query, QueryResponse};

    #[test]
    fn open_engine_then_write_and_query() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = open_engine(tmp.path()).expect("open");
        // No-op sink: we only assert state here.
        let mut sink: Vec<cairn_app::Event> = Vec::new();
        cairn_service::dispatch_command(
            &mut engine,
            &Command::WriteNote { path: "a.md".into(), contents: "hello [[b]]".into() },
            &mut sink,
        )
        .unwrap();
        let got = cairn_service::dispatch_query(&engine, &Query::GetNote { path: "a.md".into() }).unwrap();
        assert_eq!(got, QueryResponse::Note { contents: "hello [[b]]".into() });
    }
}
```
Add `tempfile` as a dev-dependency in `src-tauri/Cargo.toml`:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Run test to verify it fails**

From `src-tauri/`: `cargo test open_engine_then_write_and_query`
Expected: FAIL — `open_engine` not found.

- [ ] **Step 3: Implement state, sink, and `open_engine`**

At the top of `src-tauri/src/lib.rs` (above the generated `run`), add:
```rust
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cairn_app::{Engine, Event as AppEvent, EventSink};
use cairn_contract::{Command, CommandResponse, ContractError, Event as WireEvent, Query, QueryResponse};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, ServiceError};
use tauri::{AppHandle, Emitter, Manager, State};

/// The concrete engine the desktop app runs.
type CairnEngine = Engine<LocalFsStore, InMemoryIndex, GitVcs>;

/// Shared app state: the engine (None until a cairn is opened) + its path.
#[derive(Default)]
struct CairnState {
    engine: Mutex<Option<CairnEngine>>,
    path: Mutex<Option<PathBuf>>,
}

/// An `EventSink` that forwards engine events to the webview as wire events.
struct TauriSink(AppHandle);
impl EventSink for TauriSink {
    fn emit(&mut self, event: AppEvent) {
        // A failed emit (no window yet) is not fatal.
        let _ = self.0.emit("cairn://event", app_event_to_wire(event));
    }
}

/// Build (or open) an engine rooted at `dir`, creating the git repo if needed.
fn open_engine(dir: &Path) -> Result<CairnEngine, ServiceError> {
    let store = LocalFsStore::open(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let vcs = GitVcs::open_or_init(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(Engine::new(store, InMemoryIndex::default(), vcs))
}
```
Note: confirm `LocalFsStore::open` / `GitVcs::open_or_init` signatures against the engine (the CLI uses exactly these). If a constructor returns `PortError`, the `.map_err(|e| Internal(e.to_string()))` still compiles.

- [ ] **Step 4: Run test to verify it passes**

From `src-tauri/`: `cargo test open_engine_then_write_and_query`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/ && git commit -m "feat(tauri): CairnState, TauriSink, open_engine helper"
```

---

## Task 3: send_command / run_query commands with the no-cairn guard

**Files:** Modify `src-tauri/src/lib.rs`. Test: `#[cfg(test)]` using Tauri's mock runtime.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/lib.rs`:
```rust
    use tauri::Manager;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(CairnState::default())
            .invoke_handler(tauri::generate_handler![send_command, run_query])
            .build(tauri::generate_context!())
            .expect("build mock app")
    }

    #[test]
    fn command_without_open_cairn_errors() {
        let app = test_app();
        let state: tauri::State<CairnState> = app.state();
        let res = run_command_blocking(&state, &app.handle().clone(),
            &Command::Commit { message: "x".into() });
        assert!(matches!(res, Err(ServiceError::InvalidRequest(_))));
    }

    #[test]
    fn query_after_open_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state: tauri::State<CairnState> = app.state();
        *state.engine.lock().unwrap() = Some(open_engine(tmp.path()).unwrap());
        run_command_blocking(&state, &app.handle().clone(),
            &Command::WriteNote { path: "n.md".into(), contents: "body".into() }).unwrap();
        let r = run_query_blocking(&state, &Query::Search { query: "body".into() }).unwrap();
        assert_eq!(r, QueryResponse::Paths { paths: vec!["n.md".into()] });
    }
```
(If `tauri::test` requires a cargo feature, add `tauri = { version = "2", features = ["test"] }` or a `[dev-dependencies] tauri = { version = "2", features = ["test"] }` entry as the Tauri docs specify.)

- [ ] **Step 2: Run test to verify it fails**

From `src-tauri/`: `cargo test command_without_open_cairn_errors query_after_open_succeeds`
Expected: FAIL — `send_command`/`run_query`/`run_command_blocking`/`run_query_blocking` not found.

- [ ] **Step 3: Implement the blocking helpers and commands**

Add to `src-tauri/src/lib.rs`:
```rust
fn run_command_blocking(
    state: &State<CairnState>,
    app: &AppHandle,
    command: &Command,
) -> Result<CommandResponse, ServiceError> {
    let mut guard = state.engine.lock().expect("engine mutex poisoned");
    let engine = guard
        .as_mut()
        .ok_or_else(|| ServiceError::InvalidRequest("no cairn open".into()))?;
    let mut sink = TauriSink(app.clone());
    dispatch_command(engine, command, &mut sink)
}

fn run_query_blocking(
    state: &State<CairnState>,
    query: &Query,
) -> Result<QueryResponse, ServiceError> {
    let guard = state.engine.lock().expect("engine mutex poisoned");
    let engine = guard
        .as_ref()
        .ok_or_else(|| ServiceError::InvalidRequest("no cairn open".into()))?;
    dispatch_query(engine, query)
}

#[tauri::command]
async fn send_command(
    state: State<'_, CairnState>,
    app: AppHandle,
    command: Command,
) -> Result<CommandResponse, ContractError> {
    run_command_blocking(&state, &app, &command).map_err(ContractError::from)
}

#[tauri::command]
async fn run_query(
    state: State<'_, CairnState>,
    query: Query,
) -> Result<QueryResponse, ContractError> {
    run_query_blocking(&state, &query).map_err(ContractError::from)
}
```
Note: `async` commands let Tauri run them off the main thread; the engine work itself is synchronous under the mutex. (If profiling later shows main-thread contention, wrap the body in `tauri::async_runtime::spawn_blocking` as the daemon does — not needed for correctness here.)

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test`
Expected: PASS (all three tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/ && git commit -m "feat(tauri): send_command/run_query with no-cairn guard"
```

---

## Task 4: pick_and_open_cairn, current_cairn, persistence, auto-reopen

**Files:** Modify `src-tauri/src/lib.rs`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module (a unit test for the persistence path helpers, which are pure):
```rust
    #[test]
    fn open_at_sets_state_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        let app = test_app();
        let state: tauri::State<CairnState> = app.state();
        open_at(&state, &app.handle().clone(), tmp.path()).expect("open_at");
        assert!(state.engine.lock().unwrap().is_some());
        assert_eq!(
            state.path.lock().unwrap().as_deref(),
            Some(tmp.path())
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

From `src-tauri/`: `cargo test open_at_sets_state_and_path`
Expected: FAIL — `open_at` not found.

- [ ] **Step 3: Implement open_at, persistence, commands, and setup wiring**

Add to `src-tauri/src/lib.rs`:
```rust
/// Open a cairn at `dir`: build the engine, reindex (emitting events), and
/// record it in state. Persists the path for next launch.
fn open_at(state: &State<CairnState>, app: &AppHandle, dir: &Path) -> Result<(), ServiceError> {
    let mut engine = open_engine(dir)?;
    let mut sink = TauriSink(app.clone());
    engine.reindex(&mut sink).map_err(|e| ServiceError::Internal(e.to_string()))?;
    *state.engine.lock().expect("poisoned") = Some(engine);
    *state.path.lock().expect("poisoned") = Some(dir.to_path_buf());
    let _ = persist_path(app, dir); // persistence failure is non-fatal
    Ok(())
}

fn config_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("last-cairn.txt"))
}

fn persist_path(app: &AppHandle, dir: &Path) -> std::io::Result<()> {
    if let Some(f) = config_file(app) {
        if let Some(parent) = f.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(f, dir.to_string_lossy().as_bytes())?;
    }
    Ok(())
}

fn last_path(app: &AppHandle) -> Option<PathBuf> {
    let f = config_file(app)?;
    let s = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(s.trim());
    p.is_dir().then_some(p)
}

#[tauri::command]
async fn pick_and_open_cairn(
    state: State<'_, CairnState>,
    app: AppHandle,
) -> Result<Option<String>, ContractError> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder();
    let Some(folder) = picked else { return Ok(None) };
    let dir = folder
        .into_path()
        .map_err(|e| ContractError::Internal { message: e.to_string() })?;
    open_at(&state, &app, &dir).map_err(ContractError::from)?;
    Ok(Some(dir.to_string_lossy().into_owned()))
}

#[tauri::command]
fn current_cairn(state: State<'_, CairnState>) -> Option<String> {
    state
        .path
        .lock()
        .expect("poisoned")
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
}
```
Then update the generated `run()` builder to register state, the dialog plugin, all four commands, and auto-reopen in `setup`:
```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(CairnState::default())
        .setup(|app| {
            if let Some(dir) = last_path(&app.handle().clone()) {
                let state = app.state::<CairnState>();
                // Best-effort: ignore reopen failure, fall back to empty state.
                let _ = open_at(&state, &app.handle().clone(), &dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_command,
            run_query,
            pick_and_open_cairn,
            current_cairn
        ])
        .run(tauri::generate_context!())
        .expect("error while running cairn");
}
```
Confirm `tauri-plugin-dialog`'s folder-pick API name against the installed version (`blocking_pick_folder` is the v2 blocking API; the returned type's path accessor may be `.into_path()` or `.as_path()` — adjust to compile). Also confirm the mock-runtime test app registers the new commands if you extend tests.

- [ ] **Step 4: Run tests to verify they pass**

From `src-tauri/`: `cargo test` then `cargo build`
Expected: tests PASS; `cargo build` succeeds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/ && git commit -m "feat(tauri): pick/open cairn, persistence, auto-reopen on launch"
```

---

## Task 5: Frontend CairnHost (interface + alwaysOpenHost + MockHost)

**Files:** Create `web/src/client/host.ts`, `web/src/client/host.test.ts`. Run from `web/`.

- [ ] **Step 1: Write the failing test**

`web/src/client/host.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { alwaysOpenHost, MockHost } from "./host";

describe("alwaysOpenHost", () => {
  it("reports a cairn always open", async () => {
    expect(await alwaysOpenHost.currentCairn()).toBe("(fixture)");
    expect(await alwaysOpenHost.openCairn()).toBe("(fixture)");
  });
});

describe("MockHost", () => {
  it("behaves like the always-open host", async () => {
    const h = new MockHost();
    expect(await h.currentCairn()).toBe("(fixture)");
    expect(await h.openCairn()).toBe("(fixture)");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- host`
Expected: FAIL — cannot find module `./host`.

- [ ] **Step 3: Implement**

`web/src/client/host.ts`:
```ts
/** App-level cairn lifecycle, separate from the engine contract (CairnClient). */
export interface CairnHost {
  /** The currently-open cairn's path, or null if none is open. */
  currentCairn(): Promise<string | null>;
  /** Pick + open a cairn; resolves the path, or null if cancelled. */
  openCairn(): Promise<string | null>;
}

const FIXTURE = "(fixture)";

/** A host where a cairn is always open — used under the mock so the UI never
 *  shows the empty state and existing tests are unaffected. */
export const alwaysOpenHost: CairnHost = {
  currentCairn: () => Promise.resolve(FIXTURE),
  openCairn: () => Promise.resolve(FIXTURE),
};

/** Class form for parity with MockClient construction. */
export class MockHost implements CairnHost {
  currentCairn() {
    return Promise.resolve<string | null>(FIXTURE);
  }
  openCairn() {
    return Promise.resolve<string | null>(FIXTURE);
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- host`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/host.ts web/src/client/host.test.ts && git commit -m "feat: CairnHost interface + alwaysOpenHost/MockHost"
```

---

## Task 6: Store — cairnPath + openCairn, host-aware init

**Files:** Modify `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Write the failing test**

Add to `web/src/store/store.test.ts` (inside `describe("cairn store", ...)`):
```ts
  it("defaults to an always-open cairn (mock) and sets cairnPath on init", async () => {
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().cairnPath).toBe("(fixture)");
    expect(store.getState().notePaths.length).toBeGreaterThan(0);
  });

  it("openCairn sets cairnPath and loads notes via the host", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "x.md": "hi" });
    const host = {
      currentCairn: () => Promise.resolve<string | null>(null),
      openCairn: () => Promise.resolve<string | null>("/tmp/mycairn"),
    };
    const store = createCairnStore(client, host);
    await store.getState().init();
    expect(store.getState().cairnPath).toBeNull(); // nothing open yet
    await store.getState().openCairn();
    expect(store.getState().cairnPath).toBe("/tmp/mycairn");
    expect(store.getState().notePaths).toContain("x.md");
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- store`
Expected: FAIL — `cairnPath`/`openCairn` undefined; `createCairnStore` arity.

- [ ] **Step 3: Implement**

In `web/src/store/store.ts`:

(a) Add the import and a default host:
```ts
import { alwaysOpenHost, type CairnHost } from "../client/host";
```

(b) Add `cairnPath: string | null;` to the `CairnState` interface and `openCairn(): Promise<void>;`.

(c) Change the signature and thread the host:
```ts
export function createCairnStore(
  client: CairnClient,
  host: CairnHost = alwaysOpenHost,
): StoreApi<CairnState> {
```

(d) Initialize `cairnPath: null,` in the state object.

(e) In `init()`, gate the existing setup on an open cairn. Replace the body of `init` so it reads:
```ts
    async init() {
      if (started) return;
      started = true;
      const path = await host.currentCairn();
      set({ cairnPath: path });
      client.subscribe((e) => {
        if (e.type === "note_changed" || e.type === "note_deleted") {
          void get().refreshNotePaths();
          if (get().searchResults !== null) void get().runSearch(get().query);
          if (get().activePath) void get().refreshBacklinks();
        } else if (e.type === "committed") {
          set({ lastCommit: e.commit, uncommitted: false });
        }
      });
      if (path !== null) {
        await get().refreshNotePaths();
        get().rearmInterval();
      }
    },
```

(f) Add the `openCairn` action (place near `setSettings`):
```ts
    async openCairn() {
      try {
        const path = await host.openCairn();
        if (path === null) return; // cancelled
        set({ cairnPath: path, activePath: null, activeContents: "", backlinks: [] });
        await get().refreshNotePaths();
        get().rearmInterval();
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- store` then `pnpm test`
Expected: PASS — including all pre-existing store tests (they call `createCairnStore(client)`, so `host` defaults to `alwaysOpenHost`, `cairnPath` becomes `"(fixture)"`, and behavior is unchanged).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/ && git commit -m "feat: store cairnPath + openCairn, host-aware init"
```

---

## Task 7: OpenCairn empty-state component

**Files:** Create `web/src/components/OpenCairn.tsx`, `web/src/components/OpenCairn.test.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/components/OpenCairn.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OpenCairn } from "./OpenCairn";

describe("OpenCairn", () => {
  it("calls onOpen when the button is clicked", async () => {
    const onOpen = vi.fn();
    render(<OpenCairn onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: /open a cairn/i }));
    expect(onOpen).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- OpenCairn`
Expected: FAIL — cannot find module `./OpenCairn`.

- [ ] **Step 3: Implement**

`web/src/components/OpenCairn.tsx`:
```tsx
export function OpenCairn(props: { onOpen: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-neutral-900 text-neutral-100">
      <h1 className="text-lg font-semibold">Cairn</h1>
      <p className="text-sm text-neutral-400">No cairn open.</p>
      <button
        className="rounded border border-neutral-600 px-4 py-2 text-sm hover:bg-neutral-800"
        onClick={props.onOpen}
      >
        Open a cairn…
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- OpenCairn`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/OpenCairn.tsx web/src/components/OpenCairn.test.tsx && git commit -m "feat: OpenCairn empty-state component"
```

---

## Task 8: TauriClient + TauriHost

**Files:** Create `web/src/client/tauri.ts`, `web/src/client/tauri.test.ts`. Add `@tauri-apps/api` to `web/package.json`.

- [ ] **Step 1: Add the Tauri API dependency**

From `web/`: `pnpm add @tauri-apps/api@^2`

- [ ] **Step 2: Write the failing test**

`web/src/client/tauri.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
const listen = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: unknown[]) => listen(...a) }));

import { TauriClient, TauriHost } from "./tauri";

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
});

describe("TauriClient", () => {
  it("sendCommand invokes send_command and returns the response", async () => {
    invoke.mockResolvedValueOnce({ type: "done" });
    const c = new TauriClient();
    const res = await c.sendCommand({ type: "write_note", path: "a.md", contents: "x" });
    expect(invoke).toHaveBeenCalledWith("send_command", {
      command: { type: "write_note", path: "a.md", contents: "x" },
    });
    expect(res).toEqual({ type: "done" });
  });

  it("runQuery invokes run_query", async () => {
    invoke.mockResolvedValueOnce({ type: "paths", paths: ["a.md"] });
    const c = new TauriClient();
    const res = await c.runQuery({ type: "search", query: "x" });
    expect(invoke).toHaveBeenCalledWith("run_query", { query: { type: "search", query: "x" } });
    expect(res).toEqual({ type: "paths", paths: ["a.md"] });
  });

  it("subscribe forwards event payloads and returns an unlisten", async () => {
    const unlisten = vi.fn();
    let handler: (e: { payload: unknown }) => void = () => {};
    listen.mockImplementationOnce((_name: string, h: (e: { payload: unknown }) => void) => {
      handler = h;
      return Promise.resolve(unlisten);
    });
    const c = new TauriClient();
    const cb = vi.fn();
    const unsub = c.subscribe(cb);
    handler({ payload: { type: "committed", commit: "c1" } });
    expect(cb).toHaveBeenCalledWith({ type: "committed", commit: "c1" });
    unsub();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalled();
  });
});

describe("TauriHost", () => {
  it("openCairn invokes pick_and_open_cairn", async () => {
    invoke.mockResolvedValueOnce("/tmp/c");
    expect(await new TauriHost().openCairn()).toBe("/tmp/c");
    expect(invoke).toHaveBeenCalledWith("pick_and_open_cairn");
  });

  it("currentCairn invokes current_cairn", async () => {
    invoke.mockResolvedValueOnce(null);
    expect(await new TauriHost().currentCairn()).toBeNull();
    expect(invoke).toHaveBeenCalledWith("current_cairn");
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm test -- tauri`
Expected: FAIL — cannot find module `./tauri`.

- [ ] **Step 4: Implement**

`web/src/client/tauri.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Command, Query, Event, CommandResponse, QueryResponse } from "../contract";
import type { CairnClient, Unsubscribe } from "./types";
import type { CairnHost } from "./host";

/** Talks to the Rust backend over Tauri IPC. Rejections are ContractError
 *  (the Err payload of the Rust command), matching MockClient. */
export class TauriClient implements CairnClient {
  sendCommand(command: Command): Promise<CommandResponse> {
    return invoke<CommandResponse>("send_command", { command });
  }
  runQuery(query: Query): Promise<QueryResponse> {
    return invoke<QueryResponse>("run_query", { query });
  }
  subscribe(cb: (e: Event) => void): Unsubscribe {
    const pending = listen<Event>("cairn://event", (e) => cb(e.payload));
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void pending.then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }
}

/** App-level cairn lifecycle over Tauri commands. */
export class TauriHost implements CairnHost {
  currentCairn(): Promise<string | null> {
    return invoke<string | null>("current_cairn");
  }
  openCairn(): Promise<string | null> {
    return invoke<string | null>("pick_and_open_cairn");
  }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- tauri`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/client/tauri.ts web/src/client/tauri.test.ts web/package.json web/pnpm-lock.yaml && git commit -m "feat: TauriClient + TauriHost over Tauri IPC"
```

---

## Task 9: makeBackend composition root + App empty-state branch

**Files:** Create `web/src/app/makeBackend.ts`; delete `web/src/app/makeClient.ts`; modify `web/src/app/cairnStore.ts`, `web/src/app/App.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/app/makeBackend.test.ts`:
```ts
import { describe, it, expect, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));

import { makeBackend } from "./makeBackend";
import { MockClient } from "../client/mock";
import { MockHost } from "../client/host";

describe("makeBackend", () => {
  it("returns the mock backend when not under Tauri", () => {
    const { client, host } = makeBackend();
    expect(client).toBeInstanceOf(MockClient);
    expect(host).toBeInstanceOf(MockHost);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- makeBackend`
Expected: FAIL — cannot find module `./makeBackend`.

- [ ] **Step 3: Implement makeBackend and rewire the store instance**

`web/src/app/makeBackend.ts`:
```ts
import { isTauri } from "@tauri-apps/api/core";
import type { CairnClient } from "../client/types";
import type { CairnHost } from "../client/host";
import { MockClient } from "../client/mock";
import { MockHost } from "../client/host";
import { FIXTURE_NOTES } from "../client/fixtures";
import { TauriClient, TauriHost } from "../client/tauri";

export interface Backend {
  client: CairnClient;
  host: CairnHost;
}

/** The single place the transport is chosen. Tauri in the app, mock in a browser. */
export function makeBackend(): Backend {
  if (isTauri()) {
    return { client: new TauriClient(), host: new TauriHost() };
  }
  return { client: new MockClient(FIXTURE_NOTES), host: new MockHost() };
}
```
Replace `web/src/app/cairnStore.ts` body:
```ts
import { useStore } from "zustand";
import { createCairnStore, type CairnState } from "../store/store";
import { makeBackend } from "./makeBackend";

const { client, host } = makeBackend();
export const cairnStore = createCairnStore(client, host);

export function useCairn<T>(selector: (s: CairnState) => T): T {
  return useStore(cairnStore, selector);
}
```
Delete `web/src/app/makeClient.ts` (`git rm web/src/app/makeClient.ts`).

- [ ] **Step 4: Add the empty-state branch to App**

In `web/src/app/App.tsx`: add a `cairnPath` selector and an `OpenCairn` import, and branch the render. Add:
```tsx
  const cairnPath = useCairn((s) => s.cairnPath);
```
Wrap the existing returned `<>…</>` so that when no cairn is open the empty state shows instead of the Shell:
```tsx
  if (cairnPath === null) {
    return <OpenCairn onOpen={() => void actions.openCairn()} />;
  }
  return (
    <>
      {/* …existing Shell + ErrorToast unchanged… */}
    </>
  );
```
Add the import `import { OpenCairn } from "../components/OpenCairn";`. (Under the mock, `cairnPath` is `"(fixture)"`, so the Shell branch renders exactly as before — the e2e is unaffected.)

- [ ] **Step 5: Run the full suite + typecheck + lint + build**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm build`
Expected: all PASS. `makeClient.ts` is gone with no dangling imports.

- [ ] **Step 6: Run the e2e to confirm the mock path is intact**

Run: `pnpm e2e`
Expected: PASS (browser → `isTauri()` false → mock backend → fixture cairn always open → same loop as before).

- [ ] **Step 7: Commit**

```bash
git add web/src/app/ web/src/components/ && git commit -m "feat: makeBackend (Tauri vs mock) + App open-cairn empty state"
```

---

## Task 10: CI Rust job

**Files:** Modify `.github/workflows/ci.yml`.

- [ ] **Step 1: Add a `tauri` job**

Append a job to `.github/workflows/ci.yml`:
```yaml
  tauri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install WebKitGTK + build deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
            libayatana-appindicator3-dev librsvg2-dev build-essential cmake
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: cargo fmt --manifest-path src-tauri/Cargo.toml --check
      - run: cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
      - run: cargo test --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 2: Verify locally**

From `src-tauri/`: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all PASS (fix any fmt/clippy findings the job would catch).

- [ ] **Step 3: Commit and push**

```bash
git add .github/workflows/ci.yml && git commit -m "ci: build/test the src-tauri desktop crate"
git push
```

---

## Task 11: Mobile target scaffold (best-effort) + README note

**Files:** Possibly `src-tauri/gen/**` (git-ignored), `src-tauri/Cargo.toml` (lib targets), `README.md`.

- [ ] **Step 1: Initialize mobile targets (best-effort)**

From the repo root, attempt:
```bash
cargo tauri android init
cargo tauri ios init   # macOS + Xcode only
```
These generate `src-tauri/gen/` (git-ignored). If a toolchain is missing (no Android SDK/NDK, or no Xcode), record that the target was not initialized in this environment and continue — do NOT block.

- [ ] **Step 2: Confirm the shell builds for an available mobile target (if any)**

If `android init` succeeded and an emulator/SDK is present: `cargo tauri android build --debug` (or note it requires a device/emulator). If neither mobile toolchain is available, skip — the desktop build remains the verified artifact.

- [ ] **Step 3: Document the deferral in README**

Add to `README.md` a short "Platforms" section:
```markdown
## Platforms

Desktop (macOS/Linux/Windows) is the supported target. The Tauri **mobile**
targets (iOS/Android) are scaffolded so the shell builds and launches, but
opening a cairn on mobile (document picker / SAF + git-on-device) is **not yet
wired** — it is a follow-up phase. On mobile the app currently shows the
"Open a cairn…" empty state only.
```

- [ ] **Step 4: Commit**

```bash
git add README.md src-tauri/Cargo.toml 2>/dev/null; git commit -m "docs: mobile targets scaffolded; mobile open-cairn deferred" || echo "nothing to commit"
```

---

## Task 12: Manual desktop verification (controller)

Not a subagent task — the controller runs the GUI app and screenshots it.

- [ ] **Step 1: Build the web assets**

From `web/`: `pnpm build`

- [ ] **Step 2: Launch the desktop app**

From the repo root: `cargo tauri dev` (opens a window). Or build a bundle: `cargo tauri build` and launch the produced app.

- [ ] **Step 3: Verify the loop on a real folder**

Click **Open a cairn…**, pick a temp folder, then: create a note, edit it (rich + raw), search, follow a backlink, commit. Confirm autosave/commit status and that files + git history appear on disk. Quit and relaunch → the same cairn auto-reopens.

- [ ] **Step 4: Screenshot** the running desktop app for the record.

---

## Done criteria

- `src-tauri` builds, `cargo test`/`clippy`/`fmt` clean; CI Rust job green.
- The desktop app opens a real cairn (folder picker), persists + auto-reopens it, and runs the full loop against real markdown-under-git via `cairn-service` in-process.
- All web unit tests + the browser e2e remain green on the mock (`isTauri()` false → mock backend, fixture always open).
- Swapping mock↔real is isolated to `makeBackend.ts`; the engine is pinned to `079f9f9` (same as the TS contract).
- Mobile targets scaffolded; mobile open-cairn/git documented as deferred.
```
