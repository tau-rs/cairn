# Cairn-as-MCP-server — note operations as agent tools

**Issue:** the named follow-up from the [tau augmented-answer
design](2026-06-14-tau-augmented-answer-design.md): "Promote retrieval to
cairn-as-MCP-server so agents fetch notes as tools."

**Builds on:** the v1 `cairn ask` seam (`AgentRuntime` port + `TauServeRuntime`),
the daemon's HTTP transport ([ADR-0002](../decisions/0002-transport.md)) and its
bearer-token trust model ([ADR-0010](../decisions/0010-daemon-auth.md)), the
existing engine use-cases (`dispatch_command`/`dispatch_query` in
`crates/cairn-service/src/lib.rs`), and the file watcher
([ADR-0003](../decisions/0003-file-watcher.md),
[ADR-0005](../decisions/0005-in-process-watcher.md)).

**Tau side (pinned facts, not built here):** tau's MCP **client** is its β.3
milestone and is **unbuilt**. tau project config can declare a tool as
`[tools.x] mcp = "<url>"` (`ToolBody::Mcp(String)` in
`tau-pkg/src/project/project.rs`), but no runtime dials it; the declared form is
**URL-based** (no stdio) with **no auth field**. tau's native `fs.write` is
likewise unbuilt (only `fs-read` ships; `fs.write` is a modeled capability with
no plugin). So both arrows below are built **ahead of** tau and validated against
a generic MCP client.

## Problem

v1 proved the **cairn → tau** arrow: cairn drives `tau serve`, injecting a fixed
top-K of pre-retrieved notes into the prompt (`gather_answer_context`). The agent
is a black box that emits text; it cannot ask for a note it wasn't handed. If the
right note missed the top-K, the agent is blind to it, and it can never mutate
notes.

This adds the **tau → cairn** arrow: the agent fetches and mutates notes
autonomously through tools, instead of v1's context injection. Because tau's MCP
client is β.3, the deliverable is the **server**, built to the MCP standard so any
standard MCP client (Claude Desktop, `@modelcontextprotocol/inspector`) can use
it today, ready for tau when β.3 lands.

## Architecture

```
   agent
     ├─ native fs tools (future tau) ─► raw note bytes / settings   (best-effort)
     │                                     │ files change on disk
     │                                     ▼
     │                               daemon WATCHER ─► re-index + graph  (+ auto-commit)
     │
     └─ MCP client ─► daemon /mcp ─► dispatch_command/query ─► ONE Engine ─► git/index/graph
                        authoritative · link-aware · race-free
```

Two access paths, deliberately asymmetric:

- **MCP (`/mcp`) — authoritative.** Tool calls route through the **existing**
  `dispatch_command`/`dispatch_query` use-cases over the daemon's single
  `Arc<Mutex<Engine>>`. Indexed, link-aware, race-free (held under the engine
  lock). No note logic is reimplemented; `cairn-domain`/`cairn-service` stay pure
  (no MCP types leak in).
- **Native fs edits — best-effort.** Synced by the watcher (hardened — see the
  [sync-hardening ADR](../decisions/0012-external-edit-sync-hardening.md)). Loses
  link-rewrite on rename and is last-writer-wins under races; "prefer MCP write"
  is the documented guidance.

**One engine owner** is the load-bearing constraint: `/mcp` shares the daemon's
engine and never opens a second one over the git dir, which would corrupt the
index/git (the concurrent-writer hazard). This is why the server is a daemon
**route**, not a separate binary.

## Scope

A new `cairn-mcp` crate and a `/mcp` route on `cairn-daemon` exposing the
**complete** note surface — read, write, and facilitator (graph/search/tags) —
plus the engine-smart mutations (link-aware rename, delete, commit). Validated
against a generic MCP client; the live-tau path self-skips.

The companion native-edit sync hardening is specified in
[ADR-0012](../decisions/0012-external-edit-sync-hardening.md) and ships as a
second PR.

## Design

### `cairn-mcp` crate (transport-blind, pure)

Depends only on `cairn-contract`, `serde`, `serde_json` — no axum/tokio/domain/
service. It is to MCP what `cairn-service` is to the contract: a mapper. Modelled
on the hand-rolled `crates/cairn-plugin-protocol/src/lib.rs`.

- **Wire types:** `McpRequest`, `McpResponse`, `RpcError`, `InitializeResult`,
  `ToolDef`, `ToolResult`, content blocks; `protocolVersion` + JSON-RPC error-code
  constants.
- **Mapping:** `ToolDispatch { Command(cairn_contract::Command), Query(cairn_contract::Query) }`
  and pure functions `tools_list(write_enabled) -> Vec<ToolDef>`,
  `parse_tool_call(name, args) -> Result<ToolDispatch, McpError>`,
  `render_query_result` / `render_command_result`, and `map_error` (tool-level
  errors → `isError: true`).

**Protocol: hand-rolled** (do not pull `rmcp`). The v1 surface is three methods —
`initialize`, `tools/list`, `tools/call` — i.e. a few hundred lines of serde plus
a dispatch match. Hand-rolling matches the repo's existing protocols
(plugin-protocol, the tau wire client) and keeps the daemon request flow
identical to `command_handler`/`query_handler`, rather than ceding the request
loop to an SDK and then re-imposing the engine-lock invariants on top. The
spec-fidelity risk is bounded by an inspector compatibility check.

### Tool surface

Schemas are **hand-authored** thin defs (not derived from the contract enums,
which carry an internal `type` tag and include out-of-surface variants like
`InvokePluginCommand`/`RestoreNote`). Each tool maps to one existing contract
variant. Read tools are always listed; write tools only when write is enabled.

| MCP tool | R/W | Maps to | Input |
|---|---|---|---|
| `read_note` | R | `Query::GetNote` | `path` |
| `search_notes` | R | `Query::Search` | `query` |
| `backlinks` | R | `Query::GetBacklinks` | `path` |
| `list_notes` | R | `Query::ListNotes` | — |
| `graph` | R | `Query::GetGraph` | — |
| `list_tags` | R | `Query::ListTags` | — |
| `notes_by_tag` | R | `Query::NotesByTag` | `tag` |
| `note_history` | R | `Query::NoteHistory` | `path` |
| `write_note` | W | `Command::WriteNote` | `path`, `contents` |
| `rename_note` | W | `Command::RenameNote` | `from`, `to` |
| `delete_note` | W | `Command::DeleteNote` | `path` |
| `commit` | W | `Command::Commit` | `message` |

Structured tools also emit a JSON `structuredContent` block alongside the
canonical text block (dropped if the pinned `protocolVersion` predates it).

### Daemon wiring

Tools are request/response — they follow `command_handler`/`query_handler`, not
the SSE `ask_handler`. A `tools/call` deserializes the JSON-RPC envelope →
`cairn_mcp::parse_tool_call` → `ToolDispatch` →
`spawn_blocking(state.run_command_blocking / run_query_blocking)` → render. Reusing
`run_command_blocking` means its `EventTap` already broadcasts
`NoteChanged`/`Committed` to `/events` subscribers and plugins for
MCP-originated mutations — no new event path. `ServiceError → ContractError →` MCP
tool error; a worker `JoinError →` a generic internal error (never leaking panic
text, mirroring `service_response`).

`/mcp` is registered with its **own** auth layer (not the header-only `protected`
group). `AppState` gains `mcp_write: bool` (default false) with a `with_mcp_write`
builder.

### Trust model

See [ADR-0011](../decisions/0011-mcp-server.md). Summary: `/mcp` accepts the
daemon's bearer token **either** as `Authorization: Bearer` (standard clients)
**or** as a `?token=` query parameter (the headerless fallback for tau β.3's
bare-URL config), constant-time compared, reusing `.cairn/token`. Write tools are
gated behind a `--mcp-write` flag (default deny, mirroring the plugin trust
posture): `tools/list` filters them out and `tools/call` guards them.

## Testing

All hermetic — axum `oneshot`, a temp cairn, `TantivyIndex::in_memory()`; no
network, no `TAU_BIN`.

- **Unit (`cairn-mcp`):** `parse_tool_call` per tool + unknown-tool/missing-field
  errors; `render_*` per response variant; `tools_list(false)` excludes the four
  write tools, `(true)` includes all twelve; `map_error` per `ContractError`.
- **Integration (`crates/cairn-daemon/tests/mcp.rs`):** `initialize` +
  `tools/list` + a `write_note` → `read_note` round-trip; the write path under
  `with_mcp_write(true)`; write-gating under `(false)`; auth (missing → 401,
  bearer → ok, `?token=` → ok, wrong → 401); a `/events` subscriber observing an
  MCP `write_note`; a missing-path read → `isError: true` (not HTTP 404).
- **Compatibility:** a documented manual `mcp-inspector` smoke check. The live
  tau e2e self-skips (the `TAU_BIN` pattern), since tau's MCP client is β.3.

## Non-goals (deferred)

- MCP **resources / prompts / sampling / SSE** features — v1 is tools only.
- **Live tau e2e** — tau's MCP client is β.3 (unbuilt); the tau-side fs-write
  plugin and agent settings-editing are tau's work.
- The native-edit sync hardening ships as a **second PR** (ADR-0012).
- Token-usage accounting, multi-agent orchestration — per the v1 spec, still
  deferred.

## Follow-up

- Wire tau β.3 to `/mcp` once tau's MCP host runtime lands; promote the live e2e
  from self-skip to real.
- Reassess a token-scoped or per-agent capability model if multiple agents with
  different trust levels share one daemon.
