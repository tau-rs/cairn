# Daemon-supervised tau sidecar — one long-lived `tau serve`, shared across requests

**Issue:** the v1.1 follow-up named by the
[tau augmented-answer design](./2026-06-14-tau-augmented-answer-design.md). v1
shipped `cairn ask` against a **one-shot** `tau serve` (spawn → stream → SIGKILL
on drop). This promotes the daemon's runtime to a **single long-lived,
supervised** `tau serve` process, shared across requests.

**Builds on:** the existing `AgentRuntime` port
(`crates/cairn-ports/src/lib.rs:294`) and its `NullRuntime` seam
(`crates/cairn-infra/src/seams.rs:39`); the serve-mode client
(`crates/cairn-infra/src/tau/client.rs`) and process primitive
(`crates/cairn-infra/src/tau/process.rs`); the daemon's runtime wiring
(`crates/cairn-daemon/src/main.rs:139`). It reuses the **plugin host's**
supervision patterns — bounded reads via a reader-thread + `recv_timeout`
(`crates/cairn-infra/src/plugin_host.rs:122`) and kill-on-drop — applied to a
long-lived child.

## Problem

The daemon's `AgentRuntime` is one-shot: every `answer()` spawns a fresh
`tau serve`, handshakes, streams, and SIGKILLs on drop
(`crates/cairn-infra/src/tau/runtime.rs:23`). For a daemon answering repeated
`POST /ask` requests this is wrong on three counts:

1. **Cold every time.** Each request pays process-spawn + handshake + cold
   model/tool caches. A warm, reused process is the point of `tau serve`.
2. **No supervision.** `process.rs` carries two explicit v1.1 TODOs: the
   readiness read is **unbounded** (a started-but-silent tau hangs `spawn`
   forever — `process.rs:56`), and shutdown is an **immediate SIGKILL** with no
   grace (`process.rs:89`). Neither is acceptable for a process the daemon keeps
   alive and must restart on crash.
3. **No concurrency story.** One shared process means two simultaneous `/ask`
   requests would interleave JSON-RPC lines on the same stdio. The one-shot model
   sidestepped this by giving every request its own process; a shared process
   must address it explicitly.

## Scope

A daemon-owned, supervised, long-lived `tau serve`:

- Long-lived child, spawned lazily on first use, reused across requests.
- Readiness-read **timeout** (TODO #1).
- **Graceful shutdown** (TODO #2): close stdin → wait-with-grace → kill.
- **Restart/health policy** on crash, with a crash-loop backoff.
- **Safe concurrent `answer()`** against the one process (serialized).

Out of scope (Non-goals, below): multiplexed concurrent runs, a proactive health
ping, a `[tau]` TOML config section, the web chat panel, MCP-server, centaur.

## Design

Core stays pure: `cairn-domain` and `cairn-service` never import tau. Supervision
lives entirely behind the `AgentRuntime` port in `cairn-infra`, constructed and
owned by the daemon process. The port signature is **unchanged**
(`answer(&self, prompt, sink)`), so nothing inward ripples.

```
 POST /ask ──► engine "augmented answer" (gather context, lock released)
                 │  AgentRuntime::answer(prompt, sink)         (unchanged port)
                 ▼
        TauSidecar  (cairn-infra)  ── Arc<dyn AgentRuntime>, owned by the daemon
                 │  Mutex<State>   ← serializes concurrent answers (one run at a time)
                 │  ensure-alive → run_streaming → leave warm
                 ▼
        tau serve  (ONE long-lived process, daemon-supervised)
```

### Module layout (`cairn-infra/src/tau/`)

| File | Change |
|---|---|
| `process.rs` (`TauServe`) | **Improve** the shared primitive: bounded readiness read; graceful-shutdown `Drop`; an `is_alive()` liveness check. Benefits both runtimes. |
| `supervisor.rs` (**new**) | `TauSidecar` — the long-lived, supervised `AgentRuntime`. |
| `runtime.rs` (`TauServeRuntime`) | **Unchanged** — the one-shot, retained for the CLI. |
| `config.rs`, `client.rs`, `wire.rs` | Unchanged. |
| `mod.rs` | Re-export `TauSidecar`. |

The CLI (`cairn ask`, `crates/cairn-cli/src/main.rs:282`) keeps the one-shot
`TauServeRuntime`: a CLI process is itself short-lived and runs one ask per
invocation, so supervising a long-lived child it would immediately kill buys
nothing. Supervision is a daemon concern.

### `TauServe` improvements (`process.rs`)

**Readiness-read timeout (TODO #1).** Today `connect()` blocks on
`stderr.read_line()` for the readiness marker with no bound. Replace with the
plugin host's bounded-read shape: spawn a short-lived thread that reads the one
readiness line and sends it over a channel; `recv_timeout(READY_TIMEOUT)` on the
main path. On timeout → `child.kill()` + `wait()` → `Err` ("tau serve: timed out
waiting for readiness"). Cross-platform (std threads + channels; std pipe reads
cannot be interrupted directly, hence the thread).

**Graceful shutdown (TODO #2).** `Drop` becomes: **close stdin** (drop the
writer → EOF; `tau serve` exits on stdin EOF, as plugins do) → **poll
`child.try_wait()`** in a short sleep-loop up to `SHUTDOWN_GRACE` → **`kill()` +
`wait()`** only if still running. This requires `Drop` to release the client's
`ChildStdin` *before* waiting; `TauServe` is restructured so the writer can be
dropped independently of the reader (e.g. the stdin handle held in an `Option`
taken at shutdown). Replaces today's unconditional SIGKILL.

**Liveness.** Add `fn is_alive(&mut self) -> bool` (via `try_wait()`: `Ok(None)`
⇒ alive). Used by the supervisor to decide reuse-vs-respawn.

### `TauSidecar` — the supervisor (`supervisor.rs`)

```rust
pub struct TauSidecar {
    config: TauConfig,
    spawn: Box<dyn Fn(&TauConfig) -> Result<Box<dyn TauChannel>, PortError> + Send + Sync>,
    state: Mutex<State>,
}
struct State { conn: Option<Box<dyn TauChannel>>, backoff: Backoff }
```

`answer(&self, prompt, sink)`:

1. **Lock** `state` — this is the serialization. Concurrent `answer()` calls
   queue here; the second `/ask` waits for the first to finish streaming. (A
   local single-user note app rarely hits this; see ADR for the rationale and
   the multiplex upgrade path.)
2. **Ensure alive.** If `conn` is `None` or `!is_alive()`, drop any dead
   connection and spawn a fresh one via `self.spawn`, gated by `backoff`
   (below). Reuse the warm connection otherwise.
3. **Run.** `conn.run_streaming(&config.agent, prompt, sink)`.
4. **No auto-retry of the failed run.** If the process dies mid-stream the client
   already emits `AgentEvent::Failed` and returns `Ok`; the run is not replayed
   (the prompt may be non-idempotent and partial tokens already streamed).
   Instead, the dead connection is cleared so the **next** `answer()` respawns.
   Restart means "the next request gets a healthy process," not "redo this one."

A spawn/handshake failure returns `Err(PortError)`; the daemon's `/ask` producer
maps that to a single `AgentEvent::Failed` on the sink (existing behavior,
`cairn-daemon/src/lib.rs:396`), so a configured-but-broken tau produces a clean
streamed error, never a hang or a crash.

**Health policy: lazy detect-on-use.** Liveness is checked before each run; there
is **no** background `meta.ping` timer thread. The only consumer is `answer()`,
so a proactive ping would add a thread for marginal benefit (keeping warm /
detecting silent death between asks). Documented as the deferred follow-up.

**Crash-loop guard (`Backoff`).** A pure, unit-testable struct:

```rust
struct Backoff { consecutive_failures: u32, last_attempt: Option<Instant> }
```

- `record_success()` resets `consecutive_failures` to 0.
- On a respawn, if the previous attempt failed, sleep for a bounded exponential
  delay before retrying: `BACKOFF_BASE * 2^(n-1)`, capped at `BACKOFF_CAP`
  (base 100 ms, cap 5 s). This prevents a permanently-broken tau from
  hot-looping the daemon while keeping the happy path (no prior failure) delay-free.

The delay schedule is computed by a pure `next_delay(failures) -> Duration` so it
is tested deterministically without a clock; the single `Instant`/`sleep` lives
at the call site.

### `TauChannel` seam (testability)

An internal trait abstracts the supervisor's view of a connection so the state
machine is testable without a real process:

```rust
trait TauChannel: Send {
    fn is_alive(&mut self) -> bool;
    fn run_streaming(&mut self, agent: &str, prompt: &str, sink: &mut dyn AgentSink)
        -> Result<(), PortError>;
}
```

`TauServe` implements it (production). `TauSidecar::new(config)` wires the real
spawner (builds a `TauServe`); a test-only `TauSidecar::with_spawner(config,
spawn_fn)` injects a fake `TauChannel` backed by in-memory pipes (as
`client.rs`'s tests already do) — no subprocess, fully hermetic and
cross-platform.

### Daemon wiring (`cairn-daemon/src/main.rs:139`)

Swap the one-shot for the sidecar; everything else is unchanged:

```rust
Some(cfg) => Arc::new(cairn_infra::TauSidecar::new(cfg)),   // was TauServeRuntime::new(cfg)
None      => Arc::new(cairn_infra::NullRuntime),            // unchanged
```

Still `Arc<dyn AgentRuntime + Send + Sync>`. `TauSidecar` is `Send + Sync` (a
`Mutex<State>` over `Send` contents). **Spawn is lazy** — on first `/ask`, not at
boot — so a misconfigured tau surfaces a clean streamed error on first ask rather
than failing daemon startup; eager warm-at-boot is a trivial future toggle. The
default build (no `TAU_BIN`) stays on `NullRuntime`, exactly as today.

**Shutdown.** `TauSidecar`/`TauServe::Drop` runs the graceful shutdown when the
`Arc` is dropped. `axum::serve` runs until the process is signalled, so on a bare
SIGKILL nothing runs (unavoidable); wiring a SIGTERM/Ctrl-C handler that drops
the runtime so `Drop` fires on a clean daemon exit is a small **optional** extra,
noted but not required. The graceful-shutdown *logic* lives in `Drop` and is
tested directly against the stub binary regardless.

### Constants (`cairn-infra`, alongside `DEFAULT_PLUGIN_TIMEOUT`)

| Constant | Value | Meaning |
|---|---|---|
| `READY_TIMEOUT` | 10 s | Max wait for tau's stderr readiness line before kill + error. |
| `SHUTDOWN_GRACE` | 2 s | Max wait after stdin-close before SIGKILL on drop. |
| `BACKOFF_BASE` / `BACKOFF_CAP` | 100 ms / 5 s | Crash-loop respawn backoff bounds. |

No `[tau]` TOML section this slice (deferred with the rest of the v1.1 config
work); constants mirror the plugin host's `DEFAULT_PLUGIN_TIMEOUT` precedent.

## Testing (CI stays hermetic — Linux/macOS/Windows matrix)

- **Supervisor state machine** — pure unit tests via the `TauChannel` seam with
  an in-memory fake: (a) **serialize** — two `answer()` calls do not interleave;
  (b) **reuse** — a second `answer()` reuses the same connection (no respawn) when
  it is alive; (c) **respawn-on-death** — a connection reporting `!is_alive()`
  triggers exactly one fresh spawn; (d) **no-retry** — a mid-run failure is not
  replayed, but the next call respawns; (e) **spawn failure** surfaces a clean
  `Err`.
- **`Backoff`** — deterministic unit tests of `next_delay` (0 on first attempt,
  exponential growth, cap, reset on success). No clock.
- **Process lifecycle** — a **cross-platform Rust `tau-stub` binary** with modes:
  *ready-then-canned-run* (graceful-exit-on-EOF + happy path), *never-ready*
  (readiness timeout fires, child reaped), *exit-immediately* (respawn path). An
  integration test in `cairn-infra/tests/` locates it via
  `env!("CARGO_BIN_EXE_tau-stub")`. Packaging settled in the plan (candidate: a
  `[[bin]]` gated behind a `test-stub` feature so it is excluded from release
  builds). Pure Rust ⇒ runs on all three CI OSes.
- **Live** — the existing `live_run_streams_when_tau_present`
  (`process.rs:113`) keeps self-skipping when `TAU_BIN` is unset; add a
  self-skipping "two asks reuse one supervised process" check.

## Non-goals (deferred)

- **Multiplexed concurrent runs** — a reader-thread demux routing `runtime.event`
  by `params.id` to per-request channels. The protocol already tags events with
  `id`, so this is a drop-in upgrade *inside* `TauSidecar` with no port change.
  Not justified for a local single-user app (see ADR-0011).
- **Proactive health ping** — a background `meta.ping` timer to keep the process
  warm and detect silent death between requests.
- **`[tau]` TOML config** — `bin`/`agent`/`project`/timeouts in `cairn.toml`;
  this slice stays env-driven (`TauConfig::from_env`) with infra constants.
- **SIGTERM→graceful-shutdown daemon handler** — so `Drop` fires on a clean exit.
- **Web chat panel** (`cairn-web-ui`), **cairn-as-MCP-server**, **centaur** — per
  the v1 design's non-goals.

## Follow-up issues (opened on merge)

- Multiplex concurrent runs if real contention appears (id-demux reader thread).
- Proactive `meta.ping` health check + warm-at-boot toggle.
- `[tau]` TOML config section (supersedes env-only `TauConfig`).
