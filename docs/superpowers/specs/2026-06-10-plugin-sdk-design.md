# Plugin SDK (slice 2): `cairn-plugin-sdk`

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation
**Builds on:** slices 1/3a/3b (the plugin command host + host-callbacks, [ADR-0008](../../decisions/0008-plugin-host.md))

## Goal

A `cairn-plugin-sdk` crate so a plugin author writes *only* command declarations
and typed handlers — the SDK owns every line of protocol plumbing (the stdio
NDJSON loop, the `initialize` handshake, the `invokeCommand` dispatch + JSON-RPC
error codes, and the host-callback round-trip). Validation: rewrite the example
plugin onto the SDK (266 lines → ~40) with the existing `tests/host.rs` e2e suite
passing **byte-for-byte unchanged** (identical wire behavior).

## Why

Slices 1/3a/3b made plugins capable, but every author would re-hand-roll the same
~90% of `crates/cairn-plugin-example/src/main.rs`: the `main` read/dispatch/write
loop, the `Response` construction, the `initialize` reply, the unknown-command/
bad-params error codes, and the `call_host` + four `*_via_host` callback wrappers.
The author's actual code is tiny — command ids/titles and per-command logic. The
SDK closes that gap.

## Design decisions (resolved during brainstorming)

- **Typed handlers, not untyped `Value`.** A handler is generic over its argument
  and output types (`A: DeserializeOwned`, `O: Serialize`); the SDK deserializes
  args and serializes results at the boundary. This catches field-name typos and
  wrong result shapes at **compile time** instead of at runtime inside a spawned
  subprocess. (Raw JSON is still available per-command by using `serde_json::Value`
  as `A`/`O` — `echo` does exactly that.)
- **Generic `command()` with internal erasure.** Each typed handler is wrapped into
  a uniform `Box<dyn FnMut(Value, &mut Host) -> Result<Value, PluginError>>`, so the
  registry is homogeneous while the author stays fully typed.
- **Non-generic `Plugin`/`Host` via trait objects.** `Host` borrows
  `&mut dyn BufRead` + `&mut dyn Write` rather than `<R, W>` generics, keeping the
  public API free of generic noise (negligible dynamic-dispatch cost: one call per
  message).
- **Ergonomic `Host` return types.** `search`/`list_notes` unwrap the DTO envelope
  and return `Vec<SearchHitDto>` / `Vec<NoteSummaryDto>`; the SDK re-exports those
  so authors don't depend on `cairn-plugin-protocol` directly.
- **Manifest stays hand-authored.** The SDK is runtime-only; it does not generate or
  own the `manifest.toml` (that is the host's contract, declaring the command binary
  and capabilities). No async. No in-code capability declarations (enforcement is
  host-side; `host.read_note` simply returns an error if the capability is missing).
  UI plugins remain the UI session's.

## Author-facing surface

```rust
use cairn_plugin_sdk::{Plugin, Host, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
struct NoteLenArgs { path: String }
#[derive(Serialize)]
struct NoteLen { len: usize }

fn main() {
    let mut plugin = Plugin::new("example", env!("CARGO_PKG_VERSION"));

    // Raw JSON when you want it (Value: Deserialize + Serialize):
    plugin.command("echo", "Echo", |args: Value, _host| Ok(args));

    // Typed when you want safety:
    plugin.command("noteLen", "Note length", |a: NoteLenArgs, host: &mut Host| {
        let contents = host.read_note(&a.path)?;
        Ok(NoteLen { len: contents.len() })
    });

    plugin.run(); // owns the stdio loop; returns only on stdin EOF
}
```

## Components

New crate `crates/cairn-plugin-sdk` (deps: `cairn-plugin-protocol`, `serde`,
`serde_json`). Single `src/lib.rs` for this slice (small, focused).

### 1. `Plugin` — builder + registry

```rust
pub struct Plugin {
    name: String,
    version: String,
    commands: Vec<RegisteredCommand>, // preserves declaration order for `initialize`
}

struct RegisteredCommand {
    id: String,
    title: String,
    // Higher-ranked over the Host borrow: the handler is stored independently of
    // any live IO and is called with a freshly-built Host each invoke.
    handler: Box<dyn FnMut(Value, &mut Host<'_>) -> Result<Value, PluginError>>,
}

impl Plugin {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self;

    /// Register a typed command. `A` is deserialized from the invoke args, `O` is
    /// serialized into the result.
    pub fn command<A, O, F>(&mut self, id: impl Into<String>, title: impl Into<String>, mut handler: F)
    where
        A: serde::de::DeserializeOwned,
        O: serde::Serialize,
        F: FnMut(A, &mut Host) -> Result<O, PluginError> + 'static,
    {
        let boxed: Box<dyn FnMut(Value, &mut Host<'_>) -> Result<Value, PluginError>> =
            Box::new(move |raw: Value, host: &mut Host<'_>| {
                let args: A = serde_json::from_value(raw)
                    .map_err(|e| PluginError::bad_args(e.to_string()))?;
                let out: O = handler(args, host)?;
                serde_json::to_value(out).map_err(|e| PluginError::internal(e.to_string()))
            });
        self.commands.push(RegisteredCommand { id: id.into(), title: title.into(), handler: boxed });
    }

    /// Run the stdio loop until stdin EOF. Uses real stdin/stdout.
    pub fn run(self);
}
```

`run()` delegates to an internal `run_io(self, reader: &mut dyn BufRead, stdout:
&mut dyn Write)` so unit tests can drive it with cursors. The loop mirrors the
current example's `main`:

- read a `Request` (EOF → return);
- `initialize` → reply `InitializeResult { name, version, commands: [(id,title)] }`;
- `invokeCommand` → parse `InvokeParams`; find the command by id (not found →
  error `-32601`); call its boxed handler with `(params.args, &mut host)`; the
  handler's `Ok(Value)`/`Err(PluginError)` becomes the response `result`/`error`;
- any other method → error `-32601`;
- write the `Response`; on write failure, break.

The capability gate, dispatch-loop interleaving, and id correlation all live
**host-side** (unchanged); the plugin remains a synchronous one-in-flight server +
callback client.

### 2. `Host` — typed callback handle

```rust
pub struct Host<'a> {
    reader: &'a mut dyn std::io::BufRead,
    stdout: &'a mut dyn std::io::Write,
    next_cb_id: &'a mut u64,
}

impl Host<'_> {
    pub fn read_note(&mut self, path: &str) -> Result<String, PluginError>;
    pub fn write_note(&mut self, path: &str, contents: &str) -> Result<(), PluginError>;
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchHitDto>, PluginError>;
    pub fn list_notes(&mut self) -> Result<Vec<NoteSummaryDto>, PluginError>;
}
```

Each method is the `call_host` round-trip from the example, lifted into the SDK: a
private `call(&mut self, method, params) -> Result<Value, PluginError>` increments
`next_cb_id`, `write_message`s the `Request`, `read_message`s the `Response`,
returns `Err(PluginError)` on the host's `error` (preserving its code+message,
e.g. `CALLBACK_DENIED`), else returns the `result` Value. The four public methods
serialize their params (`ReadNoteParams`/`WriteNoteParams`/`SearchParams`/null) and
deserialize the typed result (`ReadNoteResult.contents`, `()`, `SearchResultDto.hits`,
`ListNotesResult.notes`). `run_io` constructs the `Host` fresh per invoke, borrowing
the loop's reader/stdout and a persistent `next_cb_id` counter (so callback ids stay
unique across commands, starting at 1000 as today).

### 3. `PluginError` — error model

```rust
#[derive(Debug, Clone)]
pub struct PluginError {
    pub code: i64,
    pub message: String,
}

impl PluginError {
    pub fn new(message: impl Into<String>) -> Self;       // code = -32603 (internal)
    fn bad_args(message: impl Into<String>) -> Self;       // code = -32602
    fn internal(message: impl Into<String>) -> Self;       // code = -32603
}

impl From<&str> for PluginError { /* internal */ }
impl From<String> for PluginError { /* internal */ }
impl From<cairn_plugin_protocol::RpcError> for PluginError { /* preserve code + message */ }
```

A handler returns `Result<O, PluginError>`; `?`-ing a `Host` call bubbles the host's
error verbatim. `run_io` maps a `PluginError` to the response `RpcError { code,
message }`. (Default error code is the JSON-RPC internal `-32603`; bad-args is
`-32602`; unknown command/method is `-32601`, produced by the loop itself.)

### 4. Re-exports

The SDK re-exports the protocol types an author touches in handler return positions:
`pub use cairn_plugin_protocol::{SearchHitDto, NoteSummaryDto};`. Authors depend
only on `cairn-plugin-sdk` (+ serde/serde_json for their own arg/result derives).

### 5. Example rewrite + testing

`crates/cairn-plugin-example/Cargo.toml`: drop the direct `cairn-plugin-protocol`
dependency, add `cairn-plugin-sdk` and `serde` (it already has `serde_json`).
`src/main.rs` becomes `Plugin::new(...)` + five `command(...)` registrations
(`echo`, `noteLen`, `writeNote`, `noteCount`, `find`) + `run()`, with small
`#[derive(Deserialize)]`/`Serialize` arg/result structs — preserving the exact same
command ids, titles, and result shapes (`{"len":n}`, `{"written":true}`,
`{"count":n}`, `{"hits":n}`).

**Cross-check (the load-bearing test):** the existing `crates/cairn-plugin-example/
tests/host.rs` e2e suite — which spawns the real example binary through
`ProcessPluginHost` and exercises echo/noteLen/writeNote/noteCount/find plus the
capability-denied paths — must pass **unchanged**. That proves the SDK reproduces
the hand-rolled wire behavior exactly.

**SDK unit tests** (`crates/cairn-plugin-sdk/src/lib.rs`, driving `run_io` with
`std::io::Cursor`):
- `initialize` → response lists the registered commands in declaration order.
- unknown command → `RpcError` code `-32601`.
- malformed args for a typed command → code `-32602`.
- a roundtrip: feed `initialize` then `invokeCommand{echo}` → assert the echoed result.
- `Host::read_note`: pre-load the reader with a canned `host/readNote` response,
  assert (a) the SDK wrote a `host/readNote` request with the right params and (b)
  the returned `String` matches; and a denied response → `Err` preserving the code.

## File structure

| File | Responsibility |
|------|----------------|
| `crates/cairn-plugin-sdk/Cargo.toml` | new crate manifest (protocol + serde + serde_json deps) |
| `crates/cairn-plugin-sdk/src/lib.rs` | `Plugin`, `Host`, `PluginError`, `run_io`, re-exports, unit tests |
| `crates/cairn-plugin-example/Cargo.toml` | swap protocol dep → sdk dep (+ serde) |
| `crates/cairn-plugin-example/src/main.rs` | rewritten onto the SDK |
| `Cargo.toml` (workspace) | add `"crates/cairn-plugin-sdk"` to the explicit `members` list (no glob) |

## Unchanged

`cairn-plugin-protocol` (the SDK consumes it), `cairn-ports`, `cairn-app`,
`cairn-infra` (the host), `cairn-service`, `cairn-contract`, `cairn-cli`, the
daemon, and `tests/host.rs` (the cross-check). The SDK is a new, additive,
author-side crate.

## Risks

- **Borrow shape in `run_io`:** the command registry and the IO are separate locals,
  and `Host` is built per invoke borrowing the IO — so looking up a handler (mut
  borrow of the `commands` vec/map) and calling it with `&mut host` (borrow of the
  IO locals) are disjoint. The handler being `FnMut` is fine since each is called
  one at a time. Verified by the unit tests + the example compiling.
- **Higher-ranked lifetime on the boxed handler:** because `Host<'a>` borrows the
  IO, the stored closure type is `Box<dyn FnMut(Value, &mut Host<'_>) -> ...>` —
  the `'_` elides to a higher-ranked `for<'h>` bound so one stored handler accepts a
  Host of any lifetime. Writing the closure args as `|raw: Value, host: &mut Host<'_>|`
  lets the compiler infer this; no explicit `for<…>` syntax is needed.
- **Behavior parity:** the e2e `tests/host.rs` suite is the guardrail — any wire
  drift (error codes, result shapes, callback framing) fails it.
- **3-OS CI / lock:** new crate + the example's dependency swap change `Cargo.lock`
  — commit it (CI runs `--locked`). No Windows-specific paths here, but the example's
  manifest tests already use TOML literal strings.
