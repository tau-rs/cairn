# ADR-0011: tau sidecar supervision (long-lived, serialized, restart-on-use)

**Status:** Accepted
**Date:** 2026-06-16

## Context

v1 (`cairn ask`, ADR-context in the
[tau augmented-answer design](../superpowers/specs/2026-06-14-tau-augmented-answer-design.md))
ran the `AgentRuntime` as a **one-shot**: each `answer()` spawned a fresh
`tau serve`, streamed, and SIGKILLed on drop. That is cold on every request and
carries two unfinished v1.1 TODOs (unbounded readiness read; no graceful
shutdown). The daemon needs **one long-lived `tau serve`**, supervised and shared
across `POST /ask` requests. Sharing one process forces three real architectural
choices: how concurrent requests share the stdio, what happens on crash, and how
shutdown is sequenced.

## Decision

A `TauSidecar` adapter (`cairn-infra/src/tau/supervisor.rs`) behind the unchanged
`AgentRuntime` port, owned by the daemon as `Arc<dyn AgentRuntime>`. The pure
core (`cairn-domain`, `cairn-service`) is untouched.

1. **Serialize, do not multiplex.** One `tau serve`; concurrent `answer()` calls
   queue on a `Mutex` (one run at a time). Cairn is a local, single-user note
   app — simultaneous asks are rare, and "the second waited a few seconds" is an
   acceptable outcome. This matches the plugin host's existing
   one-in-flight-per-child invariant and is correct even if `tau serve` is
   internally single-run (which we cannot confirm from this repo). The serve
   protocol already tags `runtime.event` frames with the request `id`, so a
   multiplexed upgrade (reader-thread demux → per-request channels) is a drop-in
   change *inside* `TauSidecar` with no port, daemon, or core change — deferred
   until real contention appears.

2. **Lazy restart-on-use with crash-loop backoff.** The process spawns lazily on
   first `answer()` and is reused while alive. Liveness is checked before each run
   (`try_wait`); a dead process is respawned. A run that fails mid-stream is **not
   replayed** (non-idempotent prompt; partial tokens already streamed) — the
   connection is dropped so the *next* request gets a healthy process. Repeated
   spawn failures are throttled by a bounded exponential backoff (100 ms → 5 s).
   No background `meta.ping` thread: the sole consumer is `answer()`, so
   detect-on-use suffices and avoids a timer thread. Proactive ping is deferred.

3. **Graceful shutdown.** `Drop` closes stdin (EOF; `tau serve` exits on it, like
   plugins) → polls `try_wait()` up to a 2 s grace → SIGKILL + reap only if still
   alive. Replaces the v1 unconditional SIGKILL. The readiness read is likewise
   bounded (10 s) via a reader-thread + `recv_timeout`, reusing the plugin host's
   bounded-read pattern (std pipe reads are not interruptible directly).

The CLI keeps the one-shot `TauServeRuntime`: a CLI process is short-lived and
runs one ask, so supervision buys nothing there. Both runtimes share the improved
`TauServe` primitive (bounded readiness read + graceful shutdown).

## Consequences

The daemon serves repeated asks against a warm, supervised process; a crashed tau
self-heals on the next request without daemon restart; a misconfigured/broken tau
yields a clean streamed `Failed` (lazy spawn surfaces it on first ask, not at
boot) rather than a hang or panic. `NullRuntime` remains the default when
`TAU_BIN` is unset — the default build composes and runs with no tau, exactly as
before.

Accepted limitations: concurrent asks serialize (head-of-line blocking on a long
run); silent death between requests is caught only on the next ask (no proactive
ping); `Drop` does not run on a bare SIGKILL of the daemon (a SIGTERM handler that
drops the runtime is a noted optional follow-up). Configuration stays env-driven
(`TauConfig::from_env`) with infra constants; the `[tau]` TOML section is deferred.

Tests stay hermetic across the Linux/macOS/Windows CI matrix: the supervision
state machine and backoff are pure unit tests over an injected in-memory
`TauChannel` fake; process lifecycle (readiness timeout, graceful exit, respawn)
is covered by a cross-platform Rust `tau-stub` binary; the live end-to-end test
self-skips when `TAU_BIN` is unset.
