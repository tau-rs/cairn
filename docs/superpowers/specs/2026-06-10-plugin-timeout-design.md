# Plugin host: read timeout (hung-plugin hardening)

**Date:** 2026-06-10
**Status:** Design — approved, pre-implementation
**Builds on:** the full plugin host (slices 1/2/3a/3b/3c/4, [ADR-0008](../../decisions/0008-plugin-host.md))

## Goal

A non-responding plugin can no longer block the host's read loop indefinitely.
Today every read from a plugin's stdout is an uninterruptible blocking call; a
plugin that hangs (buggy, slow, or malicious) freezes the host — and in the
daemon, because invokes/event-delivery run while holding the engine `Mutex`, it
freezes **all** HTTP commands and queries, not just plugins. This adds a
per-message read timeout: a plugin that goes silent longer than the timeout is
killed and the call fails, instead of hanging forever.

## Why a reader thread (the portability constraint)

Rust's std blocking pipe reads (`BufReader<ChildStdout>::read_line`) **cannot be
interrupted by a timeout** — there is no portable `read_with_timeout` for a child
process pipe. The standard, 3-OS-portable solution is a **per-plugin background
reader thread** that owns the stdout and forwards each NDJSON line down an
`mpsc` channel; the dispatch loop then uses `recv_timeout` instead of a blocking
read. This is the approach taken here.

## Decisions (resolved during brainstorming)

- **Per-message (no-progress) timeout**, not total-invoke: each read waits up to
  the timeout for the *next* message. A plugin actively streaming
  callbacks/responses keeps resetting the clock; only a stall longer than the
  timeout is treated as hung. This catches hangs without killing legitimately
  long-but-progressing work.
- **Default 30s**, overridable. A `DEFAULT_PLUGIN_TIMEOUT` const; `load` uses it; a
  new `load_with_timeout(dir, dur)` lets tests use a short timeout and sets up a
  future `cairn.toml` config seam.
- **Kill on timeout.** A stalled plugin violated the one-in-flight contract; the
  host kills the child and the call returns an error. A killed plugin's channel
  disconnects, so subsequent calls fail fast (no re-hang).
- **Internal to the host.** The only public API change is the additive
  `load_with_timeout`. No protocol/ports/app/sdk/daemon changes; the timeout lives
  entirely in `cairn-infra/src/plugin_host.rs`.

## Components — all in `crates/cairn-infra/src/plugin_host.rs`

### 1. Per-plugin reader thread

In `spawn_plugin`, after taking `child.stdout`, move its `BufReader<ChildStdout>`
into a thread:

```rust
let (tx, rx) = std::sync::mpsc::channel::<std::io::Result<String>>();
let reader = std::thread::spawn(move || {
    let mut stdout = BufReader::new(child_stdout);
    loop {
        let mut line = String::new();
        match stdout.read_line(&mut line) {
            Ok(0) => break,                 // EOF: drop tx -> channel disconnects
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;               // skip blank lines (matches old read_message)
                }
                if tx.send(Ok(line)).is_err() {
                    break;                  // consumer gone (plugin dropped)
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));    // surface the IO error, then stop
                break;
            }
        }
    }
});
```

The thread is the sole reader of stdout. Sending `String` lines (not parsed
messages) keeps the thread message-type-agnostic; the consumer parses.

### 2. `LoadedPlugin` fields

Replace `stdout: BufReader<ChildStdout>` with:

```rust
    rx: std::sync::mpsc::Receiver<std::io::Result<String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    timeout: std::time::Duration,
```

(`child`, `stdin`, `info`, `next_id`, `capabilities` are unchanged.)

### 3. `recv_message` replaces the blocking read

```rust
/// Receive + parse the next message, killing the plugin if it stalls past the
/// timeout. `Ok(None)` on a clean EOF (the reader thread ended).
fn recv_message<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, PortError> {
    match self.rx.recv_timeout(self.timeout) {
        Ok(Ok(line)) => serde_json::from_str(&line).map(Some).map_err(adapt),
        Ok(Err(e)) => Err(adapt(e)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let _ = self.child.kill();
            Err(PortError::Adapter(format!(
                "plugin {} timed out after {:?}",
                self.info.id, self.timeout
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(None),
    }
}
```

This reproduces the old `read_message` contract (blank-skip happens in the reader
thread; `Ok(None)` on EOF; parse error → `Adapter`) and adds the timeout/kill.

### 4. `call` and `call_with_callbacks`

Swap `read_message(&mut self.stdout)?` → `self.recv_message()?` in both. Nothing
else changes — the loop structure, id-matching, and callback servicing are
identical. The timeout now bounds **initialize** (`call`), **invoke**, and **event
delivery** (`call_with_callbacks`).

(The free `read_message` from `cairn-plugin-protocol` is no longer used by the host
once both call sites switch; the protocol crate still exports it for the SDK/example
— do not remove it.)

### 5. `ProcessPluginHost` + the timeout

```rust
/// Default per-message timeout for plugin reads.
pub const DEFAULT_PLUGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl ProcessPluginHost {
    pub fn load(dir: &Path) -> Result<Self, PortError> {
        Self::load_with_timeout(dir, DEFAULT_PLUGIN_TIMEOUT)
    }

    pub fn load_with_timeout(dir: &Path, timeout: Duration) -> Result<Self, PortError> { ... }
}
```

`load_with_timeout` threads `timeout` into `spawn_plugin`, which stores it (and the
reader thread + `rx`) on the `LoadedPlugin`. The existing daemon/CLI call sites use
`load` unchanged (default 30s).

### 6. `Drop`

Keep `child.kill()` + `child.wait()` (which makes the reader's `read_line` return
EOF, ending the thread), then join the reader thread so it isn't leaked:

```rust
impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}
```

Order matters: kill first (unblocks the reader), then join.

## Testing

The timeout must be injectable so tests don't wait 30s — that is what
`load_with_timeout` is for.

| Test | Where | Asserts |
|------|-------|---------|
| `hang` command | `crates/cairn-plugin-example/src/main.rs` | a command whose handler `std::thread::park()`s (blocks forever, no CPU) so the plugin never responds |
| `invoke_times_out_and_kills_plugin` | `crates/cairn-plugin-example/tests/host.rs` | `load_with_timeout(dir, 200ms)`; `invoke("example","hang",…)` returns `PortError::Adapter` (message contains "timed out") **quickly** (well under a few seconds); then a follow-up `invoke("example","echo",…)` on the SAME host also errors (plugin was killed → no re-hang) |
| existing host e2e | `crates/cairn-plugin-example/tests/host.rs` | unchanged + green: normal plugins (echo/noteLen/write/etc.) respond well within the timeout, so no false kills. (If any existing test is slow enough to approach a short timeout, it uses the default `load`, not `load_with_timeout`.) |

Initialize-timeout is covered by the same `recv_message` mechanism (a plugin that
hangs at `initialize` is killed by `spawn_plugin`'s `call`, and `load` skips it);
no separate test binary is added for it this slice.

## Out of scope

- `cairn.toml` `[plugins] timeout_secs` config (the `load_with_timeout` seam is
  ready for it; deferred).
- A total-invoke (wall-clock) timeout — per-message no-progress is the chosen
  semantics.
- Async/tokio rewrite of the host; the OS sandbox (slice 6).

## Risks

- **Reader-thread lifecycle:** the thread must exit on plugin death. `child.kill()`
  → EOF → `read_line` returns 0 → thread breaks. `Drop` joins it. A plugin that
  closes stdout but stays alive also yields EOF → thread exits; the channel
  disconnects → `recv_message` returns `Ok(None)` (handled).
- **Timeout vs the held Mutex:** during the (now bounded) wait, the daemon still
  holds the engine `Mutex`. The timeout caps that at ~30s per silent gap (vs
  infinite today) — a large improvement; fully decoupling delivery is future async
  work.
- **3-OS CI:** `mpsc` + threads are portable; no platform-specific code. The
  existing TOML-literal manifest convention is unaffected.
