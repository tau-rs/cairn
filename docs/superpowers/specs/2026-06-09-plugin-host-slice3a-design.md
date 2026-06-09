# Plugin host slice 3a: host-callbacks + capability enforcement (read-only)

**Date:** 2026-06-09
**Status:** Design — approved, pre-implementation
**Builds on:** slice 1 (out-of-process command host, [ADR-0008](../../decisions/0008-plugin-host.md))

## Goal

Let a plugin command call **back** to the host mid-invoke — gated by a
capability its manifest declares (now **enforced**, not just declared) — proven
with a single read-only callback (`host/readNote`). This isolates the three hard
mechanics (bidirectional RPC, re-entrancy, capability enforcement) without write
or event complexity. Writes/events/search land in slice 3b.

## Why this is the meaty slice

Slice 1 plugins are inert: they receive `invokeCommand` and return a result, but
cannot touch the cairn. Useful plugins need to *read/search/write* notes. That
requires the host's invoke to become **full-duplex**: while a plugin is handling
an `invokeCommand`, it may send host-callback **requests** back, which the host
services and answers before the plugin produces its final invoke **response**.

## Architecture context

cairn's plugin host is **out-of-process and headless** (a separate binary
speaking JSON-RPC over NDJSON on stdio, hosted inside the engine/daemon). The
right precedent is **VS Code** (out-of-process extension host, UI via declarative
contributions + webviews), **not Obsidian** (in-process JS with direct DOM).
Consequently:

- This slice's capabilities gate the **functional/headless** surface only
  (read/write/search the cairn).
- **UI plugins are a separate axis**, owned by the UI session (the Tauri app),
  which reads the *same* one manifest and wires up declared UI contributions.
  They are explicitly **out of scope** here (and for the engine plugin host
  generally), per ADR-0008.

### Capability model: coarse, namespaced strings

The manifest's `capabilities` is a flat list of namespaced strings. The engine
host enforces the namespaces it understands (`fs:*`, later `net`/`agent`) at the
callback boundary, and ignores `ui:*` (the UI session's concern). This extends to
both axes with zero new machinery and no path/glob scoping (which would
over-engineer the read axis and help nothing on the UI axis).

```
capabilities = ["fs:read", "fs:write", "search"]   # this + slice 3b (engine)
            ... "net", "agent"                       # later engine slices
            ... "ui:command", "ui:panel"             # future UI session
```

Slice 3a defines and enforces exactly one: **`fs:read`** (required by
`host/readNote`).

## Components

### 1. Protocol — `crates/cairn-plugin-protocol/src/lib.rs`

Slice 1's `LoadedPlugin::call` does *write request → read one `Response`*. Slice
3a makes the read a **dispatch loop**: after the host sends `invokeCommand`, the
plugin's stdout may carry **either** a host-callback *request* **or** the final
invoke *response*. They are distinguishable on the wire — a `Request` has
`method`; a `Response` has `result`/`error` and no `method`.

New items:

```rust
/// Plugin → host: read a note's raw contents. Requires the `fs:read` capability.
pub const METHOD_READ_NOTE: &str = "host/readNote";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadNoteParams {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadNoteResult {
    pub contents: String,
}

/// A message the host reads from a plugin *during* an invoke: either a callback
/// request from the plugin, or the response to the host's invoke. Distinguished
/// untagged by the presence of `method` (Request) vs `result`/`error` (Response).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Request(Request),
    Response(Response),
}

/// JSON-RPC error codes the host emits when answering a plugin's callback.
pub const CALLBACK_DENIED: i64 = -32001; // capability not declared / unknown method
pub const CALLBACK_FAILED: i64 = -32002; // host op errored (e.g. note not found)
```

`Incoming` is `#[serde(untagged)]`: serde tries `Request` (requires `method`)
first, falls back to `Response`. `Request`/`Response`/`RpcError` from slice 1 are
unchanged. The plugin remains a *server* for `initialize`/`invokeCommand` and
additionally becomes a *client* for `host/*`. Capability strings are plain
manifest entries — no protocol type.

**Edge:** the untagged decode must round-trip the existing `Response` shape
(`{jsonrpc,id,result,error}`) and the `Request` shape (`{jsonrpc,id,method,params}`)
unambiguously. A `Request` always has `method`; a `Response` never does — so the
`method`-bearing variant must be listed first. A unit test asserts both directions.

### 2. Callbacks port — `crates/cairn-ports/src/lib.rs`

A new trait the host calls to service a callback, implemented by the engine so the
host stays engine-agnostic:

```rust
/// Operations a plugin may request of the host during an invoke. The host gates
/// each on a declared capability before calling through to the implementation.
pub trait PluginCallbacks {
    /// Read a note's raw contents by path. Gated on `fs:read`.
    ///
    /// # Errors
    /// [`PortError::NotFound`] if the note does not exist; [`PortError::Adapter`]
    /// on a storage failure.
    fn read_note(&mut self, path: &str) -> Result<String, PortError>;
}
```

`PluginHost::invoke` gains the handler argument:

```rust
fn invoke(
    &mut self,
    plugin: &str,
    command: &str,
    args: &serde_json::Value,
    callbacks: &mut dyn PluginCallbacks,
) -> Result<serde_json::Value, PortError>;
```

`NoopPluginHost::invoke` ignores `callbacks` and still returns
`PortError::NotFound`. `read_note` takes `&mut self` for forward-compatibility
with slice-3b write callbacks, even though a read does not mutate.

### 3. Capability enforcement + dispatch loop — `crates/cairn-infra/src/plugin_host.rs`

`LoadedPlugin` gains `capabilities: Vec<String>`, populated from the manifest's
`engine.capabilities` (already parsed in slice 1, currently unused).

`LoadedPlugin::call` is replaced by a callback-aware invoke loop. (The
`initialize` handshake still uses a one-shot request/response — only
`invokeCommand` needs the loop.) Pseudocode:

```
write Request{ id: N, method: "invokeCommand", params }
loop {
    msg: Incoming = read_message(stdout)?            // EOF → Adapter("plugin closed…")
    match msg {
        Incoming::Response(r) if r.id == N => {
            return r.error → Err(Adapter) | r.result → Ok | None → Err(Adapter)
        }
        Incoming::Response(_)  => continue,          // stray id (one-in-flight; ignore)
        Incoming::Request(cb)  => {
            let cap = required_cap(&cb.method);      // "host/readNote" → Some("fs:read")
            match cap {
                None => write Response{ id: cb.id, error: CALLBACK_DENIED "unknown method" },
                Some(c) if !self.capabilities.contains(c) =>
                    write Response{ id: cb.id, error: CALLBACK_DENIED "capability <c> not declared" },
                Some(_) => {
                    // dispatch the specific callback
                    let params: ReadNoteParams = parse(cb.params)?;
                    match callbacks.read_note(&params.path) {
                        Ok(contents) => write Response{ id: cb.id, result: ReadNoteResult{contents} },
                        Err(e)       => write Response{ id: cb.id, error: CALLBACK_FAILED e.to_string() },
                    }
                }
            }
            continue
        }
    }
}
```

The **host** owns the capability gate (it holds the manifest); the engine merely
performs the op. `required_cap` is a small `match` on the method name. Writing a
callback response reuses `write_message`. A malformed callback `params` →
`CALLBACK_FAILED` response (don't kill the plugin over one bad request).

`ProcessPluginHost::invoke` threads `callbacks: &mut dyn PluginCallbacks` to the
matched `LoadedPlugin`. The slice-1 plugin/command-existence checks (`NotFound`)
are unchanged.

### 4. Re-entrancy — `crates/cairn-app/src/lib.rs`

The crux. `invoke_plugin_command(&mut self)` must call `self.plugins.invoke(…,
&mut cb)` where `cb` borrows `self` to reach the store — but
`self.plugins.invoke` already reborrows `&mut self.plugins` (part of `self`), so
the borrow checker rejects holding `&mut self.plugins` and a `self` borrow at
once, even though the fields are disjoint. Resolve by moving the host *out* of
`self` into a local for the duration, so the callbacks can borrow everything else
freely:

```rust
pub fn invoke_plugin_command(
    &mut self,
    plugin: &str,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PortError> {
    // Move the real host into a local so `self.plugins` no longer aliases it;
    // the callbacks handler can then borrow the rest of `self` freely.
    let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
    let mut cb = EngineCallbacks { engine: self };
    let result = host.invoke(plugin, command, args, &mut cb);
    drop(cb); // release the &mut self borrow before restoring the host
    self.plugins = host;
    result
}
```

`EngineCallbacks` is a small struct wrapping `&mut Engine` and implementing
`PluginCallbacks`:

```rust
struct EngineCallbacks<'a, S, I, V> {
    engine: &'a mut Engine<S, I, V>,
}

impl<S: ..., I: ..., V: ...> PluginCallbacks for EngineCallbacks<'_, S, I, V> {
    fn read_note(&mut self, path: &str) -> Result<String, PortError> {
        let np = NotePath::new(path)            // -> Result<_, NotePathError>
            .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
        self.engine.read_note(&np)              // existing &self accessor: store.read(np)
    }
}
```

The Engine already exposes `pub fn read_note(&self, path: &NotePath) ->
Result<String, PortError>` (`crates/cairn-app/src/lib.rs:332`, body `store.read`)
and `NotePath::new(raw: &str) -> Result<Self, NotePathError>`
(`crates/cairn-domain/src/note.rs:15`) — the callback just adapts the `&str` path
to a `NotePath` and delegates. The generic bounds on `EngineCallbacks` mirror the
`impl` block on `Engine`. `read_note` is `&self`, so 3a never mutates through the
handler; the `&mut Engine` field and `&mut self` trait method are forward-compat
for slice-3b writes.

While `cb` lives, `self.plugins` is `NoopPluginHost`, so there is no aliasing —
the real host sits in the `host` local and the callbacks borrow everything else.
The contract-layer signature is **unchanged**; reads emit no events, so no
`EventSink` is threaded this slice (that arrives in 3b with writes).

**Panic note:** if `host.invoke` panics, `self.plugins` is left as
`NoopPluginHost` rather than restored. A panicking host is already a bug and the
engine is likely poisoned regardless; we accept this rather than add
`catch_unwind`. Documented, not guarded.

### 5. Example plugin + tests — `crates/cairn-plugin-example/`

The example becomes bidirectional. Its invoke handler gains a command
**`noteLen`**: given `{ "path": "<note>" }`, it sends a `host/readNote` request
back to the host, reads the `ReadNoteResult`, and returns `{ "len": <n> }`. The
plugin's stdin/stdout loop must now *interleave* — while handling an
`invokeCommand` it writes a `host/readNote` request and blocks reading its
response before replying to the invoke. Its manifest declares
`capabilities = ["fs:read"]`.

`tests/host.rs` (real-process e2e, the slice-1 pattern) adds:

- **Happy path:** seed a real note via the engine/store, load the example with
  `fs:read`, `invoke("example", "noteLen", {path})` → `{ "len": <byte/char len> }`
  matching the note.
- **Capability denied:** load the example with `capabilities = []` (or omitting
  `fs:read`), invoke `noteLen` → the host denies the `host/readNote` callback →
  the plugin surfaces the error → `invoke` returns `PortError::Adapter` (the
  plugin-reported error path).
- **Protocol unit test** (in the protocol crate): `Incoming` round-trips a
  `Request` and a `Response` to the correct variant.
- Existing echo / unknown-plugin / unknown-command assertions still pass.

Because the e2e test invokes through `ProcessPluginHost::invoke`, it must pass a
`PluginCallbacks` impl. A tiny test double (e.g. a `HashMap<String, String>` of
path→contents) implementing `PluginCallbacks` keeps the host test independent of
the full Engine; the engine re-entrancy is covered by a `cairn-app` unit test
that drives `invoke_plugin_command` against a stub `PluginHost` which calls
`callbacks.read_note`.

## Out of scope (deferred)

- **Slice 3b:** write callbacks (`host/writeNote`) + event emission through an
  `EventSink`; `host/search` / `host/listNotes`; the `net` / `agent` caps.
- **UI plugins:** the UI session's, via the shared manifest's `ui:*` contributions.
- **Concurrency:** still one in-flight request per plugin; stray-id responses are
  ignored, not correlated. Revisit if concurrency is added.

## Unchanged this slice

`cairn-contract`, `cairn-service`, `cairn-cli`, and the daemon wiring. The
contract's `Command::InvokePluginCommand` already carries arbitrary `args`/result
JSON, so a callback-driven command needs no contract change.

## Testing summary

| Test | Crate | Asserts |
|------|-------|---------|
| `incoming_roundtrips` | cairn-plugin-protocol | `Incoming` decodes Request vs Response correctly |
| `invoke_services_read_callback` | cairn-app | `invoke_plugin_command` services a `read_note` callback via stub host |
| `noteLen reads via callback` | cairn-plugin-example | e2e: callback returns correct length |
| `noteLen denied without fs:read` | cairn-plugin-example | e2e: missing cap → `Adapter` error |
| slice-1 assertions | cairn-plugin-example | echo / unknown still pass |

## Risks

- **Untagged `Incoming` ambiguity** — mitigated by ordering (Request first) +
  round-trip test.
- **Re-entrancy borrow** — the `mem::replace` pattern is the load-bearing trick;
  the `cairn-app` unit test exercises it directly.
- **3-OS CI** — the e2e test writes the binary path into a TOML **literal**
  string (single quotes), per the slice-1 Windows-backslash lesson. Commit
  `Cargo.lock` if any dependency set changes.
