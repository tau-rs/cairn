# Cairn — Diagnostics & Observability Findings

---

## G1. No structured logging / tracing anywhere

**Severity: Medium**

**Locations:**
- `crates/cairn-daemon/src/main.rs:79, 84, 105, 116-121, 134, 144` (`println!`)
- `crates/cairn-daemon/src/main.rs:91-94, 107, 136` and `lib.rs:135, 141` (`eprintln!`)
- `crates/cairn-infra/src/plugin_host.rs:369, 484` (`eprintln!`)
- workspace `Cargo.toml` — no `tracing`/`log` dependency at all

**Description.** The daemon — a long-running network service — has no logging
framework. All operational output is ad-hoc `println!`/`eprintln!` with no
levels, no timestamps, no spans, and no request logging. There is no way to
correlate a request to the events/plugin calls it produced, or to raise/lower
verbosity.

**Impact.** In production the daemon is effectively undebuggable: no access log,
no error log with context, no way to trace a slow or failing command. Failures
during watch/plugin dispatch are invisible unless someone is reading stderr.

**Recommendation.** Adopt `tracing` + `tracing-subscriber` with a per-request span
(method, path, command type, duration, outcome), instrument plugin invokes and the
watch loop, and replace the `println!`/`eprintln!` calls with leveled events.

## G2. `LocalFsStore::read` collapses all IO errors to `NotFound`

**Severity: Medium**

**Location:** `crates/cairn-infra/src/localfs.rs:78-80`

**Description.** `fs::read_to_string(...).map_err(|_| PortError::NotFound(path))`
discards the underlying `io::Error` entirely, so permission denied, is-a-directory,
too-many-open-files, and genuine absence are indistinguishable.

**Impact.** Real storage problems are misreported as missing notes, both to users
and in any future logs; the actual `io::ErrorKind` is lost for diagnosis. (See
also design.md D10.)

**Recommendation.** Match on `ErrorKind`: only `NotFound` → `PortError::NotFound`;
everything else → `PortError::Adapter` carrying the original error (and ideally a
typed `#[source]`).

## G3. State-parse failure is swallowed; silent cold rebuild

**Severity: Medium**

**Locations:**
- `crates/cairn-app/src/lib.rs:557-575` (`parse_state` → `Result<_, ()>`)
- `crates/cairn-app/src/lib.rs:138-146` (`reconcile` falls back to `reconcile_cold` on `Err(())`)

**Description.** When `.cairn/state.json` is unreadable/incompatible, the error is
reduced to `()` and the engine silently does a full rebuild — with no log line
explaining that the warm-start path was abandoned or why.

**Impact.** A recurring corrupt/incompatible state file causes a silent full
re-index on every startup (slow), with nothing in the output to point at the
cause.

**Recommendation.** Propagate a real error from `parse_state` and emit a `warn`
("state.json rejected: <reason>; rebuilding") before falling back.

## G4. Best-effort error swallowing without trace at call sites

**Severity: Low**

**Locations:**
- `crates/cairn-daemon/src/lib.rs:43, 56` (`let _ = self.0.send(...)`)
- `crates/cairn-app/src/lib.rs:503-512` (`dispatch_plugin_event` is best-effort; handler results dropped)
- `crates/cairn-daemon/src/lib.rs:200-210` (WS forward: serialize failure → `continue`, lag → `continue`)

**Description.** Several intentionally best-effort paths drop errors with no
observability hook. Most are individually defensible (no subscribers, lagged
broadcast), but collectively there is no counter/log for dropped events, failed
serializations, or swallowed plugin-handler errors during event dispatch.

**Impact.** Silent event loss (e.g. a slow WS client lag-dropping) is invisible;
plugin event-handler failures during command-driven dispatch vanish.

**Recommendation.** Emit a `debug`/`warn` (or a metric) on lag-drop and on
serialize failure; have `dispatch_plugin_event` report per-plugin handler errors
(the process host already logs delivery errors at `plugin_host.rs:484` — make the
engine-level path consistent).

## G5. `spawn_blocking` panics return a generic 500 with no structured capture

**Severity: Low**

**Locations:**
- `crates/cairn-daemon/src/lib.rs:157-171` (`service_response` maps `JoinError` → generic "internal error")
- `crates/cairn-daemon/src/lib.rs:174, 180` (handlers)

**Description.** Correctly, panic text is not leaked to clients. But the join error
(and thus the fact that a worker panicked) is also not logged with any context;
only the default panic hook's stderr line remains, uncorrelated to the request.

**Impact.** A panicking command path is hard to attribute; combined with the
mutex-poisoning issue (security.md S6) the daemon can wedge with little signal.

**Recommendation.** Log the `JoinError` at `error` level with the request context
(within the per-request span from G1) before returning the generic 500.
