# Plugin host slice 3b: write + search + list callbacks (event-emitting writes)

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation
**Builds on:** slice 3a (host-callbacks + capability enforcement, [ADR-0008](../../decisions/0008-plugin-host.md), spec `2026-06-09-plugin-host-slice3a-design.md`)

## Goal

Add three more host-callbacks to the slice-3a machinery: one **mutation that emits
live events** (`host/writeNote`) and two **read-only queries** (`host/search`,
`host/listNotes`). The genuinely new mechanic is **threading the event sink through
the callback boundary** so a plugin's write produces `NoteChanged`/`Reindexed`
events — which the daemon already forwards to the UI over WebSocket.

## Why this slice

Slice 3a plugins can *read* one note. Useful plugins need to *write* (the new
capability) and to *discover* notes (search/list). The write path is the
interesting part: `Engine::write_note` emits domain events through an `EventSink`,
and a plugin-driven write must emit them too — so a plugin that, say, generates a
daily note triggers the same live-update flow as a user edit.

## Architecture context (recap from 3a)

Out-of-process plugins speak JSON-RPC 2.0 over NDJSON/stdio. During an
`invokeCommand`, the host's invoke is a full-duplex **dispatch loop** reading an
`Incoming` untagged enum (a callback `Request` or the invoke `Response`). A
callback is capability-gated by the host (`required_cap` → declared-check) and
serviced through a `PluginCallbacks` handler implemented by the engine. The
engine resolves the re-entrancy (its `&mut self` vs the borrowed host) by
`std::mem::replace`-ing the host out of `self` for the invoke's duration, so an
`EngineCallbacks` adapter can borrow the rest of `self`.

Capabilities are **coarse, namespaced strings**; the engine host enforces `fs:*`
at the callback boundary. This slice keeps the enforced set at exactly **`fs:read`**
and **`fs:write`**.

## Components

### 1. Capabilities

| Callback | Capability | Notes |
|----------|-----------|-------|
| `host/writeNote` | `fs:write` | New. Gates mutation. |
| `host/search` | `fs:read` | Searching reveals note content → a read. |
| `host/listNotes` | `fs:read` | Listing reads the cairn. |

No separate `search` capability — reading/searching/listing all "read the cairn",
so they share `fs:read`. This keeps the coarse set minimal (`fs:read` / `fs:write`);
`required_cap` simply maps three more method names.

### 2. Protocol — `crates/cairn-plugin-protocol/src/lib.rs`

New method constants and DTOs. The DTOs are **wire-decoupled** from the contract's
query types (`SearchResult`/`NoteSummary`) — the plugin protocol owns its own
minimal shapes (no UI-only fields like search highlights or note tags).

```rust
/// Plugin -> host: create/overwrite a note. Requires `fs:write`.
pub const METHOD_WRITE_NOTE: &str = "host/writeNote";
/// Plugin -> host: ranked full-text search. Requires `fs:read`.
pub const METHOD_SEARCH: &str = "host/search";
/// Plugin -> host: list all notes (path + title). Requires `fs:read`.
pub const METHOD_LIST_NOTES: &str = "host/listNotes";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteNoteParams {
    pub path: String,
    pub contents: String,
}
// host/writeNote success result is an empty object `{}` (no typed body).

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub path: String,
    pub score: f32,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub hits: Vec<SearchHitDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteSummaryDto {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummaryDto>,
}
```

`host/listNotes` takes no params (the plugin may send `params: null` or any value;
the host ignores them). `SearchHitDto`/`SearchResultDto` derive `PartialEq` but not
`Eq` (they hold `f32`).

### 3. Callbacks port — `crates/cairn-ports/src/lib.rs`

`PluginCallbacks` gains three methods. They return ports/domain types
(`SearchHit`, `Note` — `cairn-ports` already depends on `cairn-domain`); the host
adapter maps those to the wire DTOs.

```rust
pub trait PluginCallbacks {
    /// Read a note's raw contents by path. Gated on `fs:read`. (slice 3a)
    fn read_note(&mut self, path: &str) -> Result<String, PortError>;

    /// Create or overwrite a note. Gated on `fs:write`. Emits change events.
    ///
    /// # Errors
    /// [`PortError`] on an invalid path or a storage failure.
    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError>;

    /// Ranked full-text search. Gated on `fs:read`.
    ///
    /// # Errors
    /// [`PortError`] on an index failure.
    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError>;

    /// List all notes (for path + title). Gated on `fs:read`.
    ///
    /// # Errors
    /// [`PortError`] on a storage failure.
    fn list_notes(&mut self) -> Result<Vec<Note>, PortError>;
}
```

`PluginHost::invoke`'s signature is **unchanged** — the sink rides inside the
`&mut dyn PluginCallbacks` handler, not the host trait. `NoopPluginHost` is
unaffected. (`SearchHit` and `Note` are already imported in `cairn-ports`.)

### 4. Sink threading + re-entrancy — `crates/cairn-app/src/lib.rs`

The new mechanic. `EngineCallbacks` gains a sink field, and
`invoke_plugin_command` gains a `sink` parameter:

```rust
struct EngineCallbacks<'a, S, I, V> {
    engine: &'a mut Engine<S, I, V>,
    sink: &'a mut dyn EventSink,
}

pub fn invoke_plugin_command(
    &mut self,
    plugin: &str,
    command: &str,
    args: &serde_json::Value,
    sink: &mut dyn EventSink,
) -> Result<serde_json::Value, PortError> {
    let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
    let result = {
        let mut cb = EngineCallbacks { engine: self, sink };
        host.invoke(plugin, command, args, &mut cb)
    };
    self.plugins = host;
    result
}
```

`sink` is a parameter (a borrow disjoint from `self`), so `EngineCallbacks` holding
both `&mut self.engine` and `&mut *sink` is sound — no aliasing, same as 3a.

The `PluginCallbacks` impl:

```rust
impl<S: VaultStore, I: SearchIndex, V: Vcs> PluginCallbacks for EngineCallbacks<'_, S, I, V> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        self.engine.read_note(&np)
    }

    fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PortError> {
        let np = NotePath::new(path)
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        // Routes through the engine's write path: persists, updates the note
        // cache, and emits NoteChanged/Reindexed through the sink.
        self.engine.write_note(&np, contents, self.sink)
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        self.engine.search(query)
    }

    fn list_notes(&mut self) -> Result<Vec<Note>, PortError> {
        self.engine.list_notes()
    }
}
```

`Engine::write_note` is `&mut self` (callable through `&mut engine`);
`Engine::search`/`list_notes` are `&self`. `Engine::write_note(&np, contents,
self.sink)` reborrows the sink field.

**Ripple (minimal):** `dispatch_command`'s `InvokePluginCommand` arm already has a
`sink` in scope (it threads one to `write_note`/`delete_note`/etc.) — it passes it:
`engine.invoke_plugin_command(plugin, command, args, sink)`. The two `cairn-app`
plugin unit tests (`default_plugin_host_is_noop`, `invoke_services_read_callback`)
gain a `&mut sink` argument. `cairn-contract`, `cairn-cli`, and the daemon are
untouched (the contract `Command::InvokePluginCommand` is unchanged; the daemon
flows through `dispatch_command`).

### 5. Host dispatch + capability gate — `crates/cairn-infra/src/plugin_host.rs`

`required_cap` adds three mappings:

```rust
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some("fs:read"),
        METHOD_WRITE_NOTE => Some("fs:write"),
        METHOD_SEARCH => Some("fs:read"),
        METHOD_LIST_NOTES => Some("fs:read"),
        _ => None,
    }
}
```

`service_callback`'s allowed-method `match cb.method.as_str()` gains three arms
(the capability gate above already ran; these just dispatch + map results to DTOs):

- `METHOD_WRITE_NOTE`: parse `WriteNoteParams` → `callbacks.write_note(&p.path,
  &p.contents)` → on `Ok`, `resp.result = Some(serde_json::json!({}))`; on parse or
  op error, `CALLBACK_FAILED`.
- `METHOD_SEARCH`: parse `SearchParams` → `callbacks.search(&p.query)` → map each
  `SearchHit` to `SearchHitDto { path: hit.path.as_str().to_string(), score:
  hit.score, snippet: hit.snippet }` into `SearchResultDto { hits }` → `resp.result`.
- `METHOD_LIST_NOTES`: ignore params → `callbacks.list_notes()` → map each `Note`
  to `NoteSummaryDto { path: note.path.as_str().to_string(), title:
  note.display_title() }` into `ListNotesResult { notes }` → `resp.result`.

(The exact `NotePath`→`String` accessor mirrors what's already used elsewhere in
`cairn-infra`; `Note::display_title()` exists in `cairn-domain`.) The
`_ => CALLBACK_DENIED "unknown host method"` defensive arm and the "must stay in
sync with `required_cap`" comment remain. `PluginHost::invoke`'s signature is
unchanged.

### 6. Example plugin + tests — `crates/cairn-plugin-example/`

The example gains three commands (joining `echo` + `noteLen`), declared at
`initialize`, with the manifest declaring `capabilities = ["fs:read", "fs:write"]`:

- `writeNote`: args `{ path, contents }` → sends `host/writeNote` → returns
  `{ "written": true }`.
- `noteCount`: sends `host/listNotes` → returns `{ "count": <n> }`.
- `find`: args `{ query }` → sends `host/search` → returns `{ "hits": <n> }`.

Each follows the 3a `read_note_via_host` pattern: build the callback `Request`,
`write_message` to stdout, `read_message` the `Response`, propagate `error`, else
parse the typed result. (For `writeNote`, the success result is `{}` — the plugin
only checks for absence of `error`.)

`MapCallbacks` (the `tests/host.rs` test double) implements all four
`PluginCallbacks` methods over its `HashMap<String, String>`: `read_note` (get),
`write_note` (insert), `list_notes` (one `Note::parse` per entry), `search`
(substring match over the map → `SearchHit`s, or empty — kept simple since the
e2e only asserts counts).

**Tests:**

| Test | Crate | Asserts |
|------|-------|---------|
| `write_callback_emits_event` | cairn-app | **Key new test.** A stub host calls `callbacks.write_note("x.md","body")`; after `invoke_plugin_command(…, &mut sink)`, `sink` contains `NoteChanged("x.md")` **and** `eng.read_note(&"x.md")` returns `"body"` — proves the write routes through the real engine path and emits. |
| `write_note via callback` | cairn-plugin-example | e2e: `writeNote` with `fs:write` → `{ "written": true }`, and the note is present in the `MapCallbacks` map afterward |
| `write denied without fs:write` | cairn-plugin-example | e2e: no `fs:write` → host denies `host/writeNote` → `PortError::Adapter` |
| `note_count via callback` | cairn-plugin-example | e2e: `noteCount` with `fs:read` → `{ "count": <n> }` matching the seeded map |
| `find via callback` | cairn-plugin-example | e2e: `find` with `fs:read` → `{ "hits": <n> }` for a query matching the seeded map (`MapCallbacks::search` = substring over values) |
| `search denied without fs:read` | cairn-plugin-example | e2e: `find` without `fs:read` → `PortError::Adapter` |
| existing 3a/slice-1 tests | cairn-plugin-example | `echo`, `noteLen`, unknown-plugin/command still pass |

## Out of scope (deferred)

- `host/deleteNote` (the remaining mutation) and the `net` / `agent` capabilities.
- A shared `CAP_FS_READ`/`CAP_FS_WRITE` constants refactor (the literals appear in
  `required_cap` and tests) — worth doing once the cap set grows further.
- A full-stack integration test wiring a real subprocess to a real `Engine`
  (`EngineCallbacks` over a real store) in one test — the 3a/3b coverage gap; the
  natural home is once the example exercises a real on-disk cairn.
- **UI plugins** remain the UI session's (the shared manifest's `ui:*` axis).

## Unchanged this slice

`cairn-contract`, `cairn-cli`, and the daemon. `Command::InvokePluginCommand`
already carries arbitrary `args`/result JSON, and the daemon flows plugin invokes
through `dispatch_command` (which gains the one-line sink pass-through). The
`PluginHost` trait signature is unchanged.

## Risks

- **Sink-threading borrow:** `EngineCallbacks` now holds two `&mut` borrows
  (`engine`, `sink`); they're disjoint (one from `self`, one from the param), so
  it compiles — verified by the engine-level test. The `mem::replace` re-entrancy
  is unchanged from 3a.
- **3-OS CI:** e2e manifests keep the TOML *literal* (single-quote) `command='{bin}'`
  string (Windows-backslash lesson). Commit `Cargo.lock` if any dependency set
  changes (none expected — all new types are internal).
