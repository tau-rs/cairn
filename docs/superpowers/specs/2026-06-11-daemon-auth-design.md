# Daemon local bearer-token authentication — design

**Date:** 2026-06-11
**Finding:** audit `security.md` S5 — "Daemon has no authentication (LoopbackTrust)" (Medium).
**Scope:** first increment only. One finding, one PR.

## Problem

The `cairn-daemon` authenticates nothing. It binds `127.0.0.1`, and the only
gate is a deny-by-default CORS allowlist — but CORS is enforced *by the browser*,
so it does not constrain non-browser clients at all. Any local process, or any
other user on a multi-user host, can `POST /command` to write, delete, rename,
and commit notes (and, via S1/S3, reach code execution) or `POST /query` to read
the entire cairn.

## Goal

Turn "any local actor can drive the daemon" into "only the holder of a local
credential can," with the smallest viable change, without disturbing the
already-landed WebSocket Origin gate (S2) or the plugin trust model (S3).

## Decision: local bearer token

A bearer token, not a Unix domain socket. The token is the smallest change that
slots into the existing TCP + CORS + `AppState` builder architecture; it stays
cross-platform and keeps the browser UI (the daemon's only real consumer)
working via a standard `Authorization` header. UDS would rewrite the transport,
strand the browser UI, and drop Windows — out of scope here, listed as a
deferred alternative.

### Trust model

The token is written to `<cairn>/.cairn/token` with mode `0600`. The credential
therefore **is** a file readable only by the cairn's owner. "Can call the
daemon" collapses to "can read `<cairn>/.cairn/token`" — on a multi-user host,
the owner only. A `curl` from another user, or any local process without read
access to that file, receives `401`.

This is the same trust outcome a UDS would give (owner-only via filesystem
permissions), reached without changing the transport.

## Components

### Token lifecycle — regenerate every startup

Each `cairn-daemon` run generates a fresh token and overwrites
`<cairn>/.cairn/token`. The previous token becomes invalid immediately. The
legitimate client always has filesystem access to the cairn (that is the trust
model), so it re-reads the file on connect; there is no benefit to a persistent
secret and no rotation story to maintain. This is also the least code.

### Token generation (`main.rs`, startup)

- 32 cryptographically-random bytes, hex-encoded to a 64-char lowercase string.
- Randomness from `getrandom` (small, already transitively present); fall back
  to `rand` only if a direct dependency on `getrandom` proves awkward.
- Written **before** the daemon prints its listen line and begins serving, so it
  never serves a request before the token file exists.
- Unix: create with `OpenOptions::new().write(true).create(true).truncate(true)`
  plus `.mode(0o600)`, so the file is never briefly world-readable. Overwrite on
  each start.
- Non-Unix: best-effort write (no `0600` guarantee); the platform gap is noted
  in the trust-model docs.
- A write failure is a **hard error**: the daemon exits rather than serve
  unauthenticated (fail closed).
- Startup print: `auth: bearer token at <cairn>/.cairn/token (clients read this file)`.
  The token value is **never** printed or logged.

### State (`lib.rs`)

`AppState` gains `token: Option<Arc<str>>`, mirroring the existing
`allowed_origins` builder:

```rust
pub struct AppState {
    engine: Arc<Mutex<CairnEngine>>,
    events: broadcast::Sender<WireEvent>,
    allowed_origins: Arc<[String]>,
    token: Option<Arc<str>>,   // None = auth disabled
}

// AppState::new(engine)        → token: None   (in-process / library / tests)
// .with_token(tok)             → token: Some   (the binary always sets this)
```

`AppState::new` stays token-less so the in-process/library default and the
existing handler tests are unchanged. The **binary always** calls
`.with_token(...)`, so the shipped daemon is secure by default.

### Auth middleware (`lib.rs`)

```
require_token(State, headers, request, next) -> Response
  ├─ state.token == None                  → next (auth disabled)
  ├─ Authorization: Bearer <v>,
  │    const_time_eq(v.as_bytes(), token) → next (handler runs)
  └─ missing / non-Bearer / non-UTF-8 /
     length-or-value mismatch             → 401 + WWW-Authenticate: Bearer
```

- Comparison is constant-time (length-checked) so a wrong token cannot be
  recovered by timing. Implemented as a small hand-rolled byte compare; no new
  crypto dependency.
- Deny-by-default, exactly like the CORS and Origin gates: anything that is not
  a present, well-formed, exactly-matching `Bearer` credential is `401`.

### Routing (`build_router`)

`/command` and `/query` are built as a sub-router carrying the auth layer, then
merged with the open `/health` route and the Origin-gated `/events` route. The
`/events` handler is untouched (keeps its S2 Origin gate); `/health` stays open
as a contentless liveness probe.

```
/command  POST  → auth layer → command_handler
/query    POST  → auth layer → query_handler
/events   GET   → (Origin gate, unchanged)        ← not token-gated this PR
/health   GET   → health_handler                  ← open
```

## Data flow

```
STARTUP                              REQUEST
cairn-daemon                         client (has FS access to cairn)
  generate 32 random bytes            read <cairn>/.cairn/token
  write .cairn/token (0600)           POST /command
  AppState.with_token(tok)            Authorization: Bearer <token>
  serve                                 → auth middleware
                                          ├ match → handler (200…)
                                          └ else  → 401
```

## Error handling

| Condition | Result |
|---|---|
| No `Authorization` header | 401 |
| Header not `Bearer <token>` | 401 |
| Non-UTF-8 header value | 401 |
| Token length/value mismatch | 401 (constant-time) |
| `token == None` (library/test default) | pass through |
| Token file write fails at startup | daemon exits (fail closed) |

## Testing (TDD — failing test first)

New integration tests in `crates/cairn-daemon/tests/` (an `auth.rs` file):

1. `no_token_is_401` — `/command` with no header → 401. **Written first, RED.**
2. `correct_token_succeeds` — `Authorization: Bearer <tok>` → 200.
3. `wrong_token_is_401`.
4. `malformed_header_is_401` (non-`Bearer` scheme).
5. `query_also_gated` — `/query` with no header → 401.
6. `health_open_without_token` — `/health` with no header → 200.

These build the router via `AppState::new(...).with_token("…")`.

The existing `http.rs` tests keep using token-less `AppState::new`, proving the
default-off (in-process) path stays green.

A unit test for the token-file writer asserts mode `0600` on Unix.

## Out of scope (deferred follow-ups, listed in PR body)

- Unix-domain-socket transport as an alternative.
- Token-gating `/events` (defense in depth; the WS keeps its Origin gate this PR).
- Persistent / rotatable tokens.
- Browser-UI token delivery channel (how a served web UI obtains the token).
- Mutex-poisoning (S6) and plugin trust (S3) — separate findings.

## Documentation

- New ADR `docs/decisions/0010-daemon-auth.md`.
- README "Daemon trust model" note.
- PR body cites S5, documents the trust model, lists the deferred increments
  above.
