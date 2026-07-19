# Plugin content processors — design

**Date:** 2026-07-19
**Status:** Approved (design); implementation pending
**Slice:** Stream 5 (roadmap). Shares plugin crates with Stream 6 — sequence 5→6.
**Base:** `main`. MSRV 1.88, `forbid(unsafe_code)`.

## Goal

Let a plugin register a **content processor** that transforms note content on the
read/render path. This is a new *host→plugin* invocation direction on the existing
plugin machinery (like `cairn/event`, not a plugin→host callback). A processor
receives a note's raw content and returns transformed content; while processing it
may make capability-gated, **read-only** host callbacks.

Non-goals: write-time or index-time processing; MIME-based routing; author-tunable
processor ordering. Each is a clean, non-breaking future addition (see
[Deferred](#deferred-non-breaking-later-additions)).

## Context (verified machinery)

- **Protocol** (`crates/cairn-plugin-protocol/src/lib.rs`): `METHOD_INITIALIZE` /
  `METHOD_INVOKE`; host-callback methods `host/{readNote,writeNote,deleteNote}` +
  `host/search` + `host/listNotes`; host→plugin `cairn/event`. Capabilities
  `fs:read` / `fs:write` / `events` / `net`. `InitializeResult` already carries
  `commands` + `contributions`.
- **Host** (`crates/cairn-infra/src/plugin_host.rs`): `required_cap(method)` gates
  *plugin→host* callbacks; `service_callback` dispatches them per-method;
  `call_with_callbacks` sends one host→plugin request and services callbacks until
  the response arrives; `dispatch_event` delivers `cairn/event` **only to plugins
  that declared `CAP_EVENTS`** (a host→plugin push gated by a declared cap — the
  precedent this feature follows). A plugin silent past `DEFAULT_PLUGIN_TIMEOUT` is
  killed.
- **SDK** (`crates/cairn-plugin-sdk/src/lib.rs`): `Plugin::{command,on_event,
  contribution}`; `Host::{read_note,write_note,delete_note,search,list_notes}`;
  `run_io` dispatches `initialize` / `invokeCommand` / `cairn/event`.
- **Engine** (`crates/cairn-app/src/lib.rs`): `Engine::read_note(&self)` = raw
  `store.read`. `invoke_plugin_command(&mut self, …, sink)` moves the host out via
  `mem::replace` (to break the self-alias), builds `EngineCallbacks{engine, sink}`,
  and catches panics so a bad plugin can't poison the daemon's engine mutex.
  `EngineCallbacks` bridges callbacks to engine ops.
- **Read path** (`crates/cairn-service/src/lib.rs`): `Query::GetNote →
  dispatch_query(engine: &Engine) → engine.read_note`. `dispatch_query` is
  `&Engine` (type-proves queries don't mutate) and takes no `EventSink`.
- **Contract** (`crates/cairn-contract/src/lib.rs`): `Command` (mutating) vs
  `Query` (read); `AskRequest` was carved out as its own shape when it fit neither.
- **Daemon** (`crates/cairn-daemon/src/lib.rs`): `Arc<Mutex<Engine>>`; all engine
  access is serialized behind the lock. `run_query_blocking` holds the guard and
  calls `dispatch_query`.

## Decisions

Seven decisions, each resolved during design.

### D1 — Invocation point: a dedicated `RenderNote` query (not on `GetNote`)

Raw `GetNote` stays immutable and unprocessed (editors, diffs, rename link-rewrite,
and plugin callbacks all keep getting the true on-disk bytes). Rendered/display
reads use a **new** `Query::RenderNote { path }` routed through a mutating engine
method. Raw vs rendered become two honestly-typed operations.

**Recursion floor is structural:** processors run only inside `render_note`; a
processor's own `host/readNote` callback routes to `EngineCallbacks::read_note →
engine.read_note` (raw `store.read`), which sits *below* the processor layer and
cannot re-enter it. No convention required.

### D2 — Registration: `content:process` cap in the manifest; matcher at `initialize`

- **Gate** — `CAP_CONTENT_PROCESS = "content:process"` declared in
  `manifest.toml`'s `[engine].capabilities`, exactly like `CAP_EVENTS`: the host
  invokes `content/process` only on plugins that declared it. The
  security-relevant fact ("this plugin can rewrite what you read") lives in the
  static, content-hash-pinned manifest.
- **Matcher** — declared at `initialize` via a new
  `InitializeResult.processors: Vec<ProcessorDecl>`, parallel to `commands` and
  `contributions`. This follows the codebase rule *trust+caps → manifest;
  behavior → initialize*, reuses existing plumbing, and adds no TOML parsing.

### D3 — No `stage` field (read/render is the whole scope)

The server holds only markdown *text* (rendering to HTML is client-side), so the
only processing point that exists is "transform text on read." `stage` is omitted.
A future write- or index-stage is a non-breaking `#[serde(default)] stage` addition
when such a processor is actually built.

### D4 — Read-only callback surface during processing (side-effect-free)

For the duration of a `content/process` call, `read_note` / `search` /
`list_notes` pass through; `write_note` / `delete_note` are **denied regardless of
declared caps**, via a `ReadOnlyCallbacks` decorator at the host boundary.

Rationale — capabilities are *not* a security boundary (a loaded plugin is
fully-trusted code that can write to disk directly). The decorator's value is
narrower and real: it makes `render_note` honestly read-only and closes the
accidental loop where a write callback emits `NoteChanged → sink → re-render`.
Because nothing can mutate, **`render_note` needs no `EventSink`**.

### D5 — Fail-soft: log + keep last-good content

A processor that errors, times out (the host kills it), or returns garbage does
**not** fail the read. The failure is a `tracing::warn!(plugin, error, …)` and the
chain keeps the last-good content (raw if the first processor fails; the previous
processor's output otherwise). Mirrors the existing best-effort stance of
`dispatch_event` (audit G4). Processors *enrich* plaintext markdown; degrading to
un-processed content is safe, not corruption.

### D6 — Multiple matching processors chain deterministically by plugin id

Two plugins can match the same note; they chain (each sees the prior output), so
order must be deterministic and reproducible. Sort matching processors by their
plugin `id`. Author-tunable ordering (`order: Option<i32>`, mirroring
`PluginContribution.order`) is deferred as a non-breaking `#[serde(default)]`
addition for when two real processors conflict.

### D7 — `RenderNote` is a `Query` variant; widen query dispatch to `&mut Engine`

`RenderNote` needs `&mut Engine` (the `mem::replace` host-alias break, unavoidable
even with read-only callbacks). The daemon already serializes engine access behind
`Mutex<Engine>`, so `&mut` costs no concurrency. The wire API models the client's
mental model — rendering *is* a read — so `RenderNote` joins the `Query` enum.

**Implementation refinement (discovered during planning):** rather than widen
`dispatch_query` itself to `&mut Engine` — which would cascade `&mut` into ~30
read-only call sites, including `gather_answer_context` (deliberately `&Engine`,
the read-only half of the Ask streaming split) and the entire CLI — keep
`dispatch_query(&Engine)` pristine and add a thin wrapper the daemon calls:

```rust
pub fn dispatch_query_mut(engine: &mut Engine, q: &Query) -> Result<QueryResponse, ServiceError> {
    match q {
        Query::RenderNote { path } => { let p = parse_path(path)?;
            Ok(QueryResponse::Note { contents: engine.render_note(&p)? }) }
        other => dispatch_query(engine, other),   // &mut auto-reborrows as &Engine
    }
}
```

`dispatch_query` gains a defensive `Query::RenderNote => Err(InvalidRequest(...))`
guard arm (exhaustiveness; unreachable via the daemon, which routes through
`dispatch_query_mut`). `GetNote`'s arm is unchanged (still raw). The lost "queries
can't mutate" type-proof is mild and now localized: only `dispatch_query_mut` takes
`&mut`; under D4 a render mutates nothing observable, so the `&mut` is mechanical.

## Data flow

```
 client ── Query::RenderNote{path} ──► daemon (mut guard) ──► dispatch_query(&mut Engine)
                                                                     │
                                                          Engine::render_note(&mut self)
                                                                     │ raw = read_note (store.read)   ← recursion floor
                                                                     │ mem::replace(host); EngineCallbacks + read-only wrap
                                                                     ▼
                                              ProcessPluginHost::process_content(path, raw, cb)
                                                     │  for each loaded plugin, sorted by id:
                                                     │    declared CAP_CONTENT_PROCESS?  and  ProcessorDecl matches ext?
                                                     │       └─ call_with_callbacks(content/process, {path, content})
                                                     │            plugin may call host/readNote|search|listNotes (read-only)
                                                     │            write/delete callbacks → DENIED
                                                     │       ok → content = result.content ; err/timeout → warn!, keep last-good
                                                     ▼
                                              processed content ──► QueryResponse::Note ──► client
```

## Component changes

### Protocol (`cairn-plugin-protocol`)

```rust
pub const METHOD_PROCESS_CONTENT: &str = "content/process";   // host→plugin
pub const CAP_CONTENT_PROCESS:   &str = "content:process";    // gate (like CAP_EVENTS)

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContentParams { pub path: String, pub content: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContentResult { pub content: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessorDecl { pub extensions: Vec<String> }      // empty = all note types

// InitializeResult gains, alongside commands/contributions:
#[serde(default)] pub processors: Vec<ProcessorDecl>,
```

Tests: DTO + `ProcessorDecl` + `InitializeResult.processors` roundtrips;
`CAP_CONTENT_PROCESS == "content:process"`.

### Host dispatch (`cairn-infra/plugin_host.rs`)

- `LoadedPlugin` gains `processors: Vec<ProcessorDecl>`, populated from
  `init.processors` in `spawn_plugin`.
- `ReadOnlyCallbacks<'a>(&'a mut dyn PluginCallbacks)`: forwards
  `read_note`/`search`/`list_notes`; `write_note`/`delete_note` →
  `Err(PortError::Adapter("… not permitted during content processing"))`.
- New `PluginHost` trait method (default no-op returning `content` unchanged, like
  `dispatch_event`'s default):
  ```rust
  fn process_content(&mut self, path: &str, content: &str,
                     callbacks: &mut dyn PluginCallbacks) -> Result<String, PortError>;
  ```
  Impl on `ProcessPluginHost`: sort `loaded` by `info.id`; for each plugin that
  declared `CAP_CONTENT_PROCESS` **and** has a `ProcessorDecl` matching `path`'s
  extension (`decl.extensions.is_empty() || any(ext matches)`), wrap `callbacks`
  read-only and `call_with_callbacks(METHOD_PROCESS_CONTENT, {path, content})`.
  Thread `content` through the chain; per-plugin `Err` → `tracing::warn!` and keep
  last-good.
- `CAP_CONTENT_PROCESS` is **not** added to `required_cap` (that gates plugin→host
  callbacks); it is checked at the invoke site, like `CAP_EVENTS` in
  `dispatch_event`.

Tests: chains two plugins in id order; a processor without the cap is skipped; a
write callback is denied mid-process; a hung/erroring processor yields last-good.

### Ports (`cairn-ports`)

`PluginHost` trait: add `process_content` with a default no-op body (returns
`content` unchanged) so `NoopPluginHost` and other seams need no change.

### Engine (`cairn-app`)

```rust
pub fn render_note(&mut self, path: &NotePath) -> Result<String, PortError> {
    let raw = self.read_note(path)?;                               // raw floor
    let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
    let out = std::panic::catch_unwind(AssertUnwindSafe(|| {       // panic → PortError, as invoke_plugin_command
        let mut cb = EngineCallbacks { engine: self, sink: &mut NullSink };  // read-only ⇒ sink unused
        host.process_content(path.as_str(), &raw, &mut cb)
    }));
    self.plugins = host;
    out.unwrap_or_else(|_| Err(PortError::Adapter("plugin host panicked".into())))
}
```

`NullSink` — a discarding `EventSink`; never touched because writes are denied.

### Contract + service + daemon

- Contract: `Query::RenderNote { path }` (+ `TS` export; `query_kind` arm
  `"render_note"`).
- Service: keep `dispatch_query(engine: &Engine, …)` unchanged except a defensive
  `RenderNote` guard arm; add `dispatch_query_mut(engine: &mut Engine, …)` handling
  `RenderNote → engine.render_note(&p)? → QueryResponse::Note` and delegating all
  other variants to `dispatch_query`. `GetNote` arm unchanged (raw). This avoids
  cascading `&mut` into the read-only callers (CLI, `gather_answer_context`).
- Daemon: `run_query_blocking` → `let mut guard; dispatch_query_mut(&mut guard, query)`.

### SDK (`cairn-plugin-sdk`)

- `Plugin::processor(exts, handler)` where
  `handler: FnMut(ProcessContentParams, &mut Host) -> Result<ProcessContentResult, PluginError>`.
  Stored and emitted in `InitializeResult.processors`.
- `run_io`: new match arm on `METHOD_PROCESS_CONTENT` — builds a `Host` for
  callbacks (like invoke), runs the handler, replies with `ProcessContentResult`.

Tests: `processor` appears in `initialize`; `content/process` invokes the handler;
a handler callback round-trips.

### Example (`cairn-plugin-example`)

A demo processor declaring `content:process` (+ `fs:read` for a transclusion-style
variant). e2e tests: happy path (raw `GetNote` vs `RenderNote` differ) and denied
(no cap ⇒ `RenderNote` equals raw).

## Deferred (non-breaking, later additions)

- `stage` field (write/index-time processing) — `#[serde(default)]`.
- `order: Option<i32>` on `ProcessorDecl` for author-tunable chain order —
  `#[serde(default)]`, sort by `(order, id)`.
- MIME-based matching — add when a non-extension note distinction exists.

## Definition of done

`cargo test --workspace` + `clippy --locked` + `cargo fmt --check` green. TDD per
crate. `thiserror` at boundaries. New deps ⇒ `git add Cargo.lock`. Merge queue:
branch off `main` → PR → `gh pr merge --auto --squash`; no manual rebase.
Protocol/SDK additions are additive (`#[serde(default)]`) to stay compatible with
Stream 6's concurrent work on the plugin crates.
