# Plugin host: `host/deleteNote` callback + capability constants

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation
**Builds on:** slices 3a/3b (host-callbacks + capability enforcement) and slice 2 (the SDK), [ADR-0008](../../decisions/0008-plugin-host.md)

## Goal

Two small, related changes to the plugin host:

1. **`host/deleteNote`** — the remaining note mutation callback (delete a note,
   emitting `NoteDeleted`), gated on `fs:write`. It mirrors `host/writeNote`
   end-to-end.
2. **Capability constants** — replace the `"fs:read"`/`"fs:write"` string literals
   in the host's capability gate with shared named constants in the protocol crate,
   so the cap names have one source of truth as the set grows.

## Why

Slice 3b added write/search/list callbacks but deferred `deleteNote` (and the
shared `CAP_*` constants) as polish. With write callbacks shipped, delete is the
natural completion of the mutation surface, and the literals are now duplicated
across the gate (`required_cap`) and several test manifests — a good moment to
name them.

## Decisions

- **`host/deleteNote` requires `fs:write`.** Delete is a mutation; the coarse cap
  model gates all mutations under `fs:write` (a plugin that can overwrite a note
  can already destroy its content, so a separate `fs:delete` adds little). The
  enforced cap set stays exactly `fs:read` / `fs:write`.
- **Capability names live in `cairn-plugin-protocol`** alongside the `METHOD_*`
  consts (capabilities are a protocol-level concept: declared in the manifest,
  enforced by the host).
- **Not idempotent.** Deleting a missing note surfaces whatever
  `Engine::delete_note` returns (a storage `NotFound`), mapped to `CALLBACK_FAILED`
  — consistent with the rest of the engine; no special "delete is a no-op if
  absent" behavior.

## Components

### 1. Capability constants — `crates/cairn-plugin-protocol/src/lib.rs`

```rust
/// Capability: read the cairn (read/search/list note content + metadata).
pub const CAP_FS_READ: &str = "fs:read";
/// Capability: mutate the cairn (create/overwrite/delete notes).
pub const CAP_FS_WRITE: &str = "fs:write";
```

These are the canonical capability strings. `required_cap`
(`crates/cairn-infra/src/plugin_host.rs`) switches its four literals to the consts:

```rust
fn required_cap(method: &str) -> Option<&'static str> {
    match method {
        METHOD_READ_NOTE => Some(CAP_FS_READ),
        METHOD_WRITE_NOTE => Some(CAP_FS_WRITE),
        METHOD_DELETE_NOTE => Some(CAP_FS_WRITE),
        METHOD_SEARCH => Some(CAP_FS_READ),
        METHOD_LIST_NOTES => Some(CAP_FS_READ),
        _ => None,
    }
}
```

The example `tests/host.rs` manifests reference `CAP_FS_READ`/`CAP_FS_WRITE` (via
`format!`) instead of bare `"fs:read"`/`"fs:write"` strings, so a future cap rename
is a single edit. (Doc comments that mention `fs:read`/`fs:write` in prose are left
as prose.)

### 2. `host/deleteNote` — protocol (`cairn-plugin-protocol`)

```rust
/// Plugin -> host: delete a note. Requires the `fs:write` capability.
pub const METHOD_DELETE_NOTE: &str = "host/deleteNote";

/// Params of the `host/deleteNote` callback. Success result is an empty object `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteNoteParams {
    pub path: String,
}
```

(Success body is `{}`, like `writeNote` — the plugin only checks for absence of
`error`.)

### 3. Callbacks port — `cairn-ports`

`PluginCallbacks` gains:

```rust
/// Delete a note. Gated on the `fs:write` capability. Emits a delete event
/// through the host's sink.
///
/// # Errors
/// [`PortError::NotFound`] if the note does not exist; [`PortError`] on a storage
/// failure.
fn delete_note(&mut self, path: &str) -> Result<(), PortError>;
```

`PluginHost::invoke`'s signature is unchanged (the sink rides inside the handler).

### 4. Engine — `cairn-app`

`EngineCallbacks::delete_note` routes to the existing `Engine::delete_note`, which
already emits `NoteDeleted` via `apply_change(FsChange::Removed)` and updates the
caches:

```rust
fn delete_note(&mut self, path: &str) -> Result<(), PortError> {
    let np = NotePath::new(path)
        .map_err(|e| PortError::NotFound(format!("invalid note path {path}: {e}")))?;
    self.engine.delete_note(&np, self.sink)
}
```

No new `Engine` method; `delete_note(&mut self, &NotePath, &mut dyn EventSink)`
already exists.

### 5. Host dispatch — `cairn-infra`

`required_cap` adds `METHOD_DELETE_NOTE => Some(CAP_FS_WRITE)` (shown above).
`service_callback` gains a `METHOD_DELETE_NOTE` arm (after the write arm), mirroring
write:

```rust
METHOD_DELETE_NOTE => match serde_json::from_value::<DeleteNoteParams>(cb.params.clone()) {
    Ok(p) => match callbacks.delete_note(&p.path) {
        Ok(()) => resp.result = Some(serde_json::json!({})),
        Err(e) => resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() }),
    },
    Err(e) => resp.error = Some(RpcError { code: CALLBACK_FAILED, message: e.to_string() }),
},
```

### 6. SDK — `cairn-plugin-sdk`

`Host::delete_note` mirrors `write_note`:

```rust
/// Delete a note (`host/deleteNote`, requires `fs:write`).
///
/// # Errors
/// [`PluginError`] if the host denies/fails the callback.
pub fn delete_note(&mut self, path: &str) -> Result<(), PluginError> {
    let params = serde_json::to_value(DeleteNoteParams { path: path.to_string() })?;
    // host/deleteNote returns an empty `{}` body on success; nothing to extract.
    self.call(METHOD_DELETE_NOTE, params)?;
    Ok(())
}
```

### 7. Example plugin — `cairn-plugin-example`

A `deleteNote` command: args `{ path }` → `host.delete_note(&path)` →
`{ "deleted": true }`. Declared at `initialize` (after `find`); the example
manifest already gets `["fs:read", "fs:write"]` in the relevant tests.

## Testing

| Test | Crate | Asserts |
|------|-------|---------|
| `delete_callback_emits_event` | cairn-app | stub host calls `callbacks.delete_note("x.md")` after seeding the note; sink contains `NoteDeleted("x.md")` and the note is gone (`read_note` errors / `list_notes` excludes it) |
| `delete_note via callback` | cairn-plugin-example | e2e: `deleteNote` with `fs:write` → `{"deleted": true}`, and the entry is removed from the `MapCallbacks` map |
| `delete denied without fs:write` | cairn-plugin-example | e2e: no `fs:write` → host denies `host/deleteNote` → `PortError::Adapter`, map unchanged |
| `delete_note_sends_request` | cairn-plugin-sdk | unit: `Host::delete_note` writes a `host/deleteNote` request and handles the `{}` response (mirrors the `read_note` test) |
| existing tests | all | unchanged and green (the caps-const swap is behavior-preserving) |

`MapCallbacks` (the example test double) gains `delete_note` = remove from the map.

## Out of scope

- The `net` / `agent` capabilities (later slices).
- A `CAP_NET`/`CAP_AGENT` until those land.
- UI plugins (the UI session's).

## Unchanged

`PluginHost` trait signature, `cairn-contract`, `cairn-cli`, the daemon. The
capability-const swap is a pure refactor (same string values), so all existing
capability tests pass unchanged.
