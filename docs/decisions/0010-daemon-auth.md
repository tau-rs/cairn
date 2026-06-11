# ADR-0010: Daemon authentication — local bearer token

**Status:** Accepted
**Date:** 2026-06-11

## Context

The daemon binds `127.0.0.1` and, until now, authenticated nothing. CORS
(ADR-0004) is enforced by the browser and so constrains only browser clients;
any local process — or any other user on a multi-user host — could `POST
/command` to write/delete/rename/commit notes (reaching code execution via the
plugin findings) or `POST /query` to read the whole cairn. This is audit
finding S5.

The design for this increment is specified in
`docs/superpowers/specs/2026-06-11-daemon-auth-design.md`.

## Decision

### Local bearer token

`/command` and `/query` require an `Authorization: Bearer <token>` header. The
token is 32 cryptographically-random bytes, hex-encoded (64 chars), written to
`<cairn>/.cairn/token` with mode `0600` and regenerated on every startup. The
comparison is constant-time.

We chose a bearer token over a Unix domain socket because it is the smallest
change that fits the existing TCP + CORS + `AppState`-builder architecture, stays
cross-platform, and keeps a future browser UI working through a standard header.

### Trust model

The credential *is* a `0600` file. "Can call the daemon" therefore collapses to
"can read `<cairn>/.cairn/token`" — on a multi-user host, the cairn's owner only.
A request from another user or any process without read access to that file is
rejected with `401`. On non-Unix platforms the `0600` guarantee does not hold;
the token still gates access but the filesystem permission story is weaker.

### Scope

- `/health` stays open (a contentless liveness probe).
- `/events` keeps its Origin gate (ADR-0004 / audit S2) and is **not** token-gated
  in this increment.
- `AppState::new` is token-less by default (`None` = auth off); only the
  `cairn-daemon` binary sets a token, so in-process/library embedding and the
  handler tests are unaffected.

## Consequences

### What this enables

- A non-browser local actor without read access to `.cairn/token` can no longer
  drive the daemon.
- The token sits alongside the CORS allowlist using the same optional-builder
  pattern, so the change is additive and the existing tests are untouched.

### Accepted limitations and deferred increments

- **Unix domain socket transport** — a future alternative that would also drop
  the loopback TCP exposure.
- **Token-gating `/events`** — defense in depth; the WS keeps its Origin gate
  for now.
- **Persistent / rotatable tokens** — today the token is ephemeral per run.
- **Browser-UI token delivery** — how a served web UI obtains the token (it
  cannot read the local filesystem) is a separate sub-project.
- **Non-Unix permissions** — no `0600` equivalent is enforced off Unix.
