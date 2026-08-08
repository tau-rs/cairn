# net / agent capability enforcement — Design

> Task **C2**. Adds an `agent` capability enforced through the host-callback gate
> (the way `vault:read` / `vault:write` are), backed by the existing
> `AgentRuntime` seam. Confirms `net` is already sandbox-enforced and proves
> `agent` is host-only (opens no sandbox network).
>
> **Base / sequencing:** stacked on **C1** (`plugin-trust-capability-vocabulary`,
> PR #40), which replaces the free-form capability strings with a typed
> `Capability` enum. C2's change is *additive* to that enum (one new variant).
> Lands as a stack **C1 → C2**. If C1 stalls, the fallback is string consts on
> `main` (`CAP_AGENT: &str = "agent"`), migrated to the enum when C1 lands.

## Problem

`CAP_NET` exists but the plugin capability story has two gaps the C2 task named:

1. **No `agent` capability at all.** There is no way for a trusted plugin to ask
   the host to run an AI-agent query, and therefore nothing gating it.
2. **The task framed `net` + `agent` as symmetric** ("wire both into
   `required_cap`"). They are not. C1's design pins down **two enforcement
   domains**, and the two capabilities live in different ones.

## Two enforcement domains (from C1)

| Domain | Capabilities | Enforced by | Is it a host callback? |
|---|---|---|---|
| **1 — host-channel (vault)** | `vault:read`, `vault:write`, `vault:events` | `required_cap` → `service_callback` | **Yes** — plugin→host JSON-RPC |
| **2 — OS sandbox (process)** | `net`, `exec`, `fs:read` | Seatbelt / bubblewrap / AppContainer profile | **No** — jails the child process |

Consequences that shape this design:

- **`net` is Domain 2 and already enforced** (`sandbox_caps` maps
  `Capability::Net` → `SandboxCapabilities { net: true }`, which the profiles in
  `cairn-infra/src/sandbox.rs` turn into network rules, with passing behavioral
  tests). It gates **no callback**. C2 adds **no `net` logic** — putting `net`
  into `required_cap` would be incorrect (there is no `net` callback to gate).
- **`agent` is naturally Domain 1**: the host already owns an agent runtime
  (`cairn_ports::AgentRuntime`, tau; `NullRuntime` seam). The plugin asks the
  host to run it via a gated callback. It needs **no** sandbox network of its own
  — the *host* makes the tau call. So `agent` is **host-only**.

## Decision

Add `agent` as a **Domain-1 host-mediated callback**, gated in `required_cap`
exactly like `vault:read`. `net` stays a Domain-2 sandbox capability, unchanged.
This is the only reading in which the task's instruction — "enforce through the
callback gate the way fs:read/fs:write are" — is literally satisfiable.

## Design

### 1. Protocol (`cairn-plugin-protocol`) — additive to C1

```rust
/// Plugin -> host: run an AI agent query. Requires the `agent` capability.
pub const METHOD_AGENT: &str = "host/agent";

// New variant on C1's Capability enum (Domain 1 — host-mediated).
#[serde(rename = "agent")] Agent,
//   wire()          => "agent"
//   summary()       => "run an AI agent query via the host"
//   enforced_today() => true   // host-RPC gate is live, like the vault:* trio

/// Params of the `host/agent` callback.
pub struct AgentParams { pub prompt: String }
/// Result of `host/agent`: the completed agent answer.
pub struct AgentResult { pub answer: String }
```

`host/agent` is request/response: the plugin sends a prompt and receives the
completed answer as one string (not a stream — the plugin protocol has no
streaming callback).

### 2. Port + engine wiring

```rust
// cairn_ports::PluginCallbacks — mirrors read_note; gated on `agent`.
fn run_agent(&mut self, prompt: &str) -> Result<String, PortError>;
```

`Engine` does **not** hold an `AgentRuntime` today — it lives on the daemon's
`AppState.runtime` and is passed *into* `augmented_answer` as a parameter, and
`EngineCallbacks` holds only `engine + sink`. Rather than thread a `runtime`
parameter through `invoke_plugin_command` and its ~35 `dispatch_command` call
sites, **the `Engine` owns the runtime** (no signature changes on that chain):

- `Engine` gains `runtime: Option<Arc<dyn AgentRuntime + Send + Sync>>`
  (default `None`) plus a `set_runtime` setter, mirroring `set_plugin_host`.
  `EngineCallbacks` already borrows `engine`, so `run_agent` reaches the runtime
  via `self.engine.runtime` — no new field.
- `EngineCallbacks::run_agent(prompt)` **buffers** the run: it clones the `Arc`
  and calls `runtime.answer(prompt, &mut sink)` with an internal sink that
  concatenates `AgentEvent::TextDelta`s; on the run's completion (`answer`
  returns `Ok`) it returns the buffer. An `AgentEvent::Failed` (or an `Err`, or
  no runtime configured) becomes `PortError::Adapter`.
- Wiring: `cairn-daemon/src/main.rs` calls `engine.set_runtime(runtime.clone())`
  after the runtime is constructed and before the engine is moved into
  `AppState` (both hold the same `Arc`, so `/ask` is unaffected). `cairn-service`
  and the CLI need **no** change; `cairn-app` unit tests inject a stub via
  `set_runtime`.

Only the invoke path reads the runtime in this increment. `dispatch_plugin_event`
and `render_note` (content processing) do not: `ReadOnlyCallbacks::run_agent`
denies during content processing — agent calls from an event handler or a
content processor are out of scope here.

### 3. Gate (`cairn-infra::plugin_host`)

```rust
// required_cap
METHOD_AGENT => Some(Capability::Agent),
```

plus a `METHOD_AGENT` arm in `service_callback` that deserializes `AgentParams`,
calls `callbacks.run_agent(&p.prompt)`, and returns `AgentResult { answer }`.
A plugin that has not declared `agent` is refused with `CALLBACK_DENIED` before
dispatch — identical to the `vault:read` path.

### 4. Sandbox — `agent` is host-only

`sandbox_caps` is **unchanged**: `Capability::Agent` maps to no sandbox power
(`net: false`). A plugin declaring only `agent` is still fully network-jailed;
the outbound tau call is made by the host, not the plugin. This answers the
task's "verify agent maps sensibly or is host-only": **host-only**.

### 5. SDK + example plugin (e2e surface)

- `cairn-plugin-sdk`: `Host::agent(&mut self, prompt: &str) -> Result<String>`
  (sends `host/agent`, returns `AgentResult.answer`).
- `cairn-plugin-example`: a new `ask` command that calls `host.agent(prompt)`,
  so the real-subprocess host test exercises the gated call end-to-end.

## Test plan (deny + allow, per the DoD)

- **Protocol unit:** `Capability::Agent` round-trips via its `"agent"` wire
  string (extends C1's roundtrip test).
- **Infra unit:** `required_cap(METHOD_AGENT) == Some(Capability::Agent)`;
  `sandbox_caps(&[Capability::Agent]).net == false`.
- **Engine unit (`cairn-app`):** with a stub `AgentRuntime` emitting
  `TextDelta("Hel") TextDelta("lo") Completed`, `run_agent` returns `"Hello"`;
  a stub emitting `Failed` yields `Err`.
- **E2e (`cairn-plugin-example/tests/host.rs`):** using `PermissiveSandbox` and
  a stub runtime double —
  - `ask` **with** `capabilities=["agent"]` → the stub's answer.
  - `ask` **without** the cap → `CALLBACK_DENIED` surfaced as `PortError::Adapter`
    (matches the existing `note_len_denied_without_capability` shape).
- **Net (already covered):** the behavioral network tests in `sandbox.rs`
  (`seatbelt_behavioral_*`, `bwrap_*`) already prove deny/allow; C2 adds no new
  net path, only the `agent`-is-host-only assertion above.

## Known limitation (acceptable for v1)

The `agent` callback runs synchronously inside the daemon's engine lock, like
every host callback today, so a long agent run blocks other engine operations.
`/ask` avoids this by streaming lock-free after gathering context under the lock;
a plugin callback cannot without a larger redesign. Noted as a follow-up, not
addressed here.

## Out of scope

- Host-mediated `net` (a `host/fetch` proxy) — rejected in favor of keeping
  `net` in Domain 2 (Option B in brainstorming).
- Streaming agent results to the plugin; agent calls from event handlers or
  content processors.
- Any change to `net` / `exec` / `fs:read` enforcement.

## Files touched

`cairn-plugin-protocol`, `cairn-ports`, `cairn-infra/src/plugin_host.rs`,
`cairn-app/src/lib.rs`, `cairn-daemon/src/main.rs`, `cairn-plugin-sdk`,
`cairn-plugin-example`. (`cairn-service` and the CLI are **not** touched — the
engine-owned-runtime design avoids threading through `dispatch_command`.)

## Definition of done

Deny + allow paths covered by the tests above; `cargo test`, `cargo clippy
--locked`, and `cargo fmt --check` green.
