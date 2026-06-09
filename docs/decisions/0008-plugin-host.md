# ADR-0008: Plugin host (slice 1: out-of-process command host)

**Status:** Accepted
**Date:** 2026-06-09

## Context

The §7 engine design promised out-of-process, capability-declared plugins; only the
`Executor`/`AgentRuntime` *traits* existed. This builds the walking-skeleton slice
that proves the architecture (manifest → spawn → handshake → invoke), deferring
everything heavier.

## Decision

Own a standalone `cairn-plugin-protocol` crate — **JSON-RPC 2.0 over NDJSON
(line-delimited) on stdio, MCP-style** — with **no tau dependency** (industry
pattern: don't couple to a sister app's in-flight protocol; JSON-RPC rather than
MessagePack for debuggability + standards alignment, future MCP-compatible for the
agent-tool role). A `ProcessPluginHost` adapter (`cairn-infra`) spawns each
`<cairn>/.cairn/plugins/<id>/manifest.toml` binary and speaks the protocol
(synchronous, one in-flight request per plugin). It sits behind a `PluginHost` port
(`cairn-ports`, default `NoopPluginHost`), injected into the engine via
`Engine::set_plugin_host` (a `Box<dyn PluginHost>` — no 4th generic, no ripple).
The contract gains `Query::ListPlugins` and `Command::InvokePluginCommand`
(args/result are arbitrary JSON → ts-rs `JsonValue`). Capabilities are declared in
the manifest but **not enforced** this slice (no host-callbacks yet, so plugins
can't touch the cairn).

## Consequences

The full out-of-process path is proven end-to-end (an example plugin spawned via the
host, handshake, command invoke). The daemon loads plugins on startup
(absent/broken → graceful). Plugins exit on stdin EOF; `Drop` also kills them (no
orphans). Deferred to later slices: plugin SDK (slice 2), vault events (4), content
processors / port backends (5), OS sandbox (6), git-URL distribution (7); UI plugins
are the UI session's. JSON-RPC id correlation is unchecked (safe under one-in-flight;
revisit if concurrency is added).

**Slice 3a (done):** bidirectional RPC — a plugin command can call back to the host
mid-invoke (the host's invoke is now a full-duplex dispatch loop over an `Incoming`
message: a callback request or the invoke response). Capabilities are now *enforced*
at the callback boundary (the host gates each `host/*` method on a manifest-declared,
namespaced capability string). Scope is one read-only callback, `host/readNote`
(requires `fs:read`); the re-entrancy (engine `&mut self` vs the borrowed host) is
resolved by `mem::replace`-ing the host out of the engine for the invoke's duration.
Deferred to **slice 3b:** write callbacks (`host/writeNote`) + event emission, plus
`search`/`listNotes` and the `net`/`agent` capabilities. See
`docs/superpowers/specs/2026-06-09-plugin-host-slice3a-design.md`.
