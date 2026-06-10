# ADR-0004: Daemon CORS — deny-by-default allowlist + config file

**Status:** Accepted
**Date:** 2026-06-01

## Context

The web UI (e.g. a Vite dev server on `http://localhost:5173`) runs on a
different origin than the daemon (`http://127.0.0.1:7777`). Browsers enforce the
Same-Origin Policy: cross-origin `fetch` calls are blocked unless the server
sends the appropriate `Access-Control-Allow-Origin` header.

The daemon binds `127.0.0.1`, but loopback binding does **not** protect against
a malicious web page the user visits — that page runs in the user's browser and
can reach `127.0.0.1:7777`. CORS is the only gate. Responding with
`Access-Control-Allow-Origin: *` would let any visited website silently
read/write the user's cairn. This is a real localhost-CORS / DNS-rebinding risk
for a daemon that mutates user data.

The design for this sub-project is specified in
`docs/superpowers/specs/2026-06-01-daemon-cors-design.md`.

## Decision

### Deny-by-default allowlist

CORS is configured as an **explicit allowlist that denies by default**. No
cross-origin origin is allowed unless it is explicitly named. An empty allowlist
means no browser UI on another origin can reach the daemon.

A `*` entry is filtered out before building the layer: it would panic
`tower_http::cors::AllowOrigin::list` and has no place in the allowlist model.
Malformed entries that cannot be parsed as an HTTP `HeaderValue` are silently
skipped.

### Allowed origins: config file ∪ CLI flag

Allowed origins come from two sources, merged (union, deduplicated):

1. **`cairn.toml`** — a per-cairn TOML settings file at `<cairn>/cairn.toml`.
   CORS origins live under `[cors].origins`:

   ```toml
   [cors]
   origins = ["http://localhost:5173", "http://127.0.0.1:5173"]
   ```

   The `Config` struct (`crates/cairn-daemon/src/config.rs`) is
   `#[serde(default)]` so future sections can be added without breaking existing
   files.

2. **`--cors-origin <ORIGIN>`** — a repeatable CLI flag for per-run additions
   without editing the file.

`--config <PATH>` overrides the default location; if given, `Config::load(path)`
is called and a missing/invalid file is a hard error. If omitted,
`Config::load_default(&cairn)` is used: the file is loaded if it exists; a
missing default file is fine and yields an empty config.

### CORS layer wiring

`cors_layer(origins: &[String])` (in `crates/cairn-daemon/src/lib.rs`) builds a
`tower_http::cors::CorsLayer` with:

- **Allowed methods:** GET, POST, OPTIONS.
- **Allowed headers:** `content-type`.
- **Credentials:** none.

The layer is applied in `main.rs` **after** `build_router`:

```rust
let app = build_router(state.clone()).layer(cors_layer(&origins));
```

`build_router` is unchanged; the existing handler tests are unaffected.

### Startup message

On startup the daemon prints the effective allowlist, or — when empty — a hint:

```
CORS: no cross-origin origins allowed (add [cors].origins to <cairn>/cairn.toml or pass --cors-origin)
```

### First config-file surface

`cairn.toml` is the **first config-file surface** for the daemon. The `Config`
struct is intentionally minimal but extensible: new top-level sections slot in
via `#[serde(default)]` without breaking existing files or code.

## Consequences

### What this enables

- A browser UI served from a known origin can be allowlisted and call the daemon
  without any browser CORS error, with no wildcard exposure.
- The per-cairn `cairn.toml` provides a persistent, VCS-trackable way to record
  trusted origins; `--cors-origin` is a frictionless escape hatch for one-off
  runs.

### Accepted limitations and known seams

- **UI must opt in.** A browser UI on an origin not in the allowlist is blocked
  by the browser; developers must either add their dev-server origin to
  `cairn.toml` or pass `--cors-origin`.
- **WebSocket `/events` validates `Origin` directly.** Browsers connect
  cross-origin to WebSocket endpoints without a preflight, so the CORS layer
  does **not** protect that route. The daemon therefore checks the `Origin`
  header against the same allowlist inside `events_handler` and rejects
  (HTTP 403) a missing or non-allowlisted origin before upgrading (audit S2).
  The UI's origin must be allowlisted (via `cairn.toml` or `--cors-origin`) for
  the event stream just as for HTTP.
- **Auth/TLS/credentials deferred.** No credentials (`withCredentials`) support
  today; adding it would require a non-`*` `Allow-Origin` (already satisfied)
  and `allow_credentials(true)` in the layer — a small additive change.
- **Network exposure beyond loopback deferred.** The daemon still binds
  `127.0.0.1` only; CORS does not change that.
- **Other config keys deferred.** `cairn.toml` only carries `[cors].origins`
  today; the extensible schema is ready for future sections (port, log level,
  etc.).
