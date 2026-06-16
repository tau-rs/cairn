# ADR-0011: Cairn-as-MCP-server — note operations as agent tools

**Status:** Accepted
**Date:** 2026-06-16

## Context

v1 (`cairn ask`) drives `tau serve` and injects a fixed top-K of pre-retrieved
notes into the prompt. The agent cannot fetch a note it wasn't handed, nor mutate
notes. The named follow-up is to expose cairn's note operations as MCP tools so an
agent fetches and mutates notes autonomously.

Reading the tau codebase pins two facts: tau's MCP **client** is its β.3
milestone and is unbuilt, and its declared MCP tool form is a bare **URL** with
**no auth field**; tau's native `fs.write` is also unbuilt. So we build the
**server**, to the MCP standard, validated against a generic MCP client, ahead of
tau.

The design is specified in
`docs/superpowers/specs/2026-06-16-cairn-mcp-server-design.md`.

## Decision

### A `/mcp` route on the daemon, sharing the one engine

The MCP server is an HTTP route on `cairn-daemon`, sharing the existing
`Arc<Mutex<Engine>>`. It is **not** a separate binary and **never** opens a second
engine over the cairn directory: two engines mutating one git-backed, indexed
directory corrupt the index and git history (the concurrent-writer hazard). All
tool calls route through the existing `dispatch_command`/`dispatch_query`
use-cases under the engine lock — note logic is reused, not reimplemented, and
`cairn-domain`/`cairn-service` stay free of MCP types.

tau's URL-based MCP config also forecloses a stdio MCP server as the tau-facing
surface, independently pointing at an HTTP route.

### Hand-rolled protocol in a new `cairn-mcp` crate

A new transport-blind crate (`cairn-contract` + serde only) holds the MCP wire
types and the tool↔contract mapping, modelled on the hand-rolled
`cairn-plugin-protocol`. We do **not** adopt `rmcp`: the v1 surface is three
methods (`initialize`, `tools/list`, `tools/call`), hand-rolling matches the
repo's existing protocols, and it keeps the daemon request flow identical to the
existing `command_handler`/`query_handler` instead of ceding the request loop to
an SDK and re-imposing the engine-lock invariants on top.

### Complete, hand-authored tool surface

Twelve tools — read (`read_note`, `search_notes`, `backlinks`, `list_notes`),
facilitator (`graph`, `list_tags`, `notes_by_tag`, `note_history`), and
engine-smart mutations (`write_note`, `rename_note`, `delete_note`, `commit`) —
each mapping to one existing contract variant. Schemas are hand-authored thin
defs, not derived from the contract enums (which carry an internal `type` tag and
include out-of-surface variants). The surface is complete so any standard MCP
client works standalone; tau's future native fs is an optimisation, not a
prerequisite.

### Trust model: dual auth + write gating

`/mcp` reuses the daemon's `.cairn/token` (ADR-0010), accepting it **either** as
`Authorization: Bearer <token>` (standard clients) **or** as a `?token=<token>`
query parameter (the only channel tau β.3's bare-URL config leaves open),
constant-time compared. Write tools are gated behind a `--mcp-write` flag (default
**deny**, mirroring the plugin trust posture): `tools/list` omits them and
`tools/call` guards them. `/mcp` is token-gated (unlike `/events`) because it can
mutate.

We chose query-token over a tau-specific transport because it is the smallest
change that keeps one auth secret and works for both standard and headerless
clients on the existing loopback TCP.

## Consequences

### What this enables

- Any standard MCP client can read/search/navigate and (with `--mcp-write`)
  mutate a cairn today; tau gains it when β.3 lands.
- MCP-originated mutations reach `/events` subscribers and plugins for free, by
  reusing `run_command_blocking`'s `EventTap`.
- The MCP path is the authoritative, indexed, link-aware, race-free way to mutate,
  versus best-effort native edits.

### Accepted limitations and deferred increments

- **Query-token log exposure.** A `?token=` can leak into access logs/proxies.
  Mitigated by loopback-only bind and by recording a fixed `path` literal (never
  `req.uri()`) on the `/mcp` span. Header auth is preferred; query-token is the
  headerless fallback.
- **Engine-lock serialization.** `tools/call` serializes on the same mutex as
  `/command`/`/query`; a long write blocks reads. This is the daemon's existing
  single-engine model.
- **Tools only.** No MCP resources/prompts/sampling/SSE in v1.
- **Protocol fidelity.** Hand-rolling risks drift from the MCP spec
  (`protocolVersion` string, `Mcp-Session-Id`, `structuredContent` availability);
  bounded by an inspector compatibility check and verified against the live spec
  at implementation time.
- **Live tau e2e deferred** — tau's MCP client is β.3; the e2e test self-skips.
