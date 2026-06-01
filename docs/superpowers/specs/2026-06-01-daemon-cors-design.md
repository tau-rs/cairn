# Daemon CORS + Config File — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main`.

---

## 1. Goal

Let a browser-based UI served from another origin (e.g. a Vite dev server on
`http://localhost:5173`) call `cairn-daemon` over HTTP. CORS is an **explicit
allowlist that denies by default** — the safe choice for a localhost daemon that
mutates user data (a permissive `*` would let any visited website read/write the
cairn). Allowed origins come from a per-cairn **TOML settings file** and/or a CLI
flag.

---

## 2. Why deny-by-default (rationale)

The daemon binds `127.0.0.1`, but loopback binding does **not** protect against a
malicious *web page* the user visits — that page runs in the user's browser and can
reach `127.0.0.1:7777`; CORS is the only gate. So `Access-Control-Allow-Origin: *`
is a real localhost-CORS/DNS-rebinding risk. The allowlist denies every origin not
explicitly trusted.

---

## 3. Settings file (`cairn.toml`)

TOML, loaded per-cairn. Schema (v1, extensible — the `Config` struct is
`#[serde(default)]` so future sections slot in without breaking):
```toml
[cors]
origins = ["http://localhost:5173", "http://127.0.0.1:5173"]
```

Config types (in `cairn-daemon` lib):
```rust
#[derive(Debug, Default, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub cors: CorsConfig,
}
#[derive(Debug, Default, serde::Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub origins: Vec<String>,
}
```
Loading:
- `Config::load(path) -> Result<Config, String>` — read + `toml::from_str`; **errors**
  if the file can't be read or parsed.
- `Config::load_default(cairn) -> Result<Config, String>` — load `<cairn>/cairn.toml`
  if it exists, else `Ok(Config::default())` (a missing default file is fine).

---

## 4. CLI flags (`main.rs`)

- `--config <PATH>` — explicit settings-file path. If given, `Config::load(path)` (a
  missing/invalid file is a hard error). If omitted, `Config::load_default(&cli.cairn)`.
- `--cors-origin <ORIGIN>` — repeatable (`Vec<String>`), quick per-run additions.

**Effective allowlist** = `config.cors.origins` ∪ `--cors-origin` values, de-duplicated.
Empty → no cross-origin origin is allowed (cross-origin browser requests are blocked).

Startup prints the effective CORS mode: the allowlist, or — when empty — a hint:
`CORS: no cross-origin origins allowed (add [cors].origins to <cairn>/cairn.toml or pass --cors-origin)`.

---

## 5. CORS layer (`cairn-daemon` lib)

```rust
pub fn cors_layer(origins: &[String]) -> tower_http::cors::CorsLayer;
```
- Allows exactly the listed origins (parsed to `HeaderValue`; unparseable entries are
  skipped). An empty list allows no origin.
- Methods: `GET, POST, OPTIONS`. Headers: `content-type`. No credentials.
- tower-http's origin-list reflects a matching request `Origin` back and handles the
  preflight `OPTIONS` (sent before `POST /command`/`/query`).

**Wiring:** `main.rs` applies it after `build_router`:
`let app = build_router(state.clone()).layer(cors_layer(&origins));`
`build_router` is **unchanged** (existing tests untouched). The WebSocket `/events`
route is not subject to CORS preflight (browsers connect cross-origin to WS without
it), so it is unaffected; the layer on that route is harmless.

---

## 6. Dependencies

New workspace deps: `tower-http` (feature `cors`) and `toml`; `serde` (already present)
gains `derive` use for `Config`. Pure Rust — CI's cargo-deny + locked-MSRV vet them;
pin transitive versions if the 1.85 MSRV needs it.

---

## 7. Testing (deterministic, no browser)

- **config:** `toml::from_str::<Config>` for a `[cors] origins=[...]` document → the
  origins; an empty/sectionless document → empty origins; `Config::load` from a temp
  file; `load_default` returns default when `<cairn>/cairn.toml` is absent.
- **cors_layer (via `oneshot`):** with allowlist `["http://localhost:5173"]`, a
  `POST /query` carrying `Origin: http://localhost:5173` → response has
  `access-control-allow-origin: http://localhost:5173`; a request with
  `Origin: http://evil.example` → **no** matching allow-origin header; an `OPTIONS`
  preflight to `/command` from the allowed origin → returns the allow-methods/headers.
- **merge:** effective allowlist is the union of file origins and `--cors-origin` flags
  (test the merge helper if extracted, or assert via the layer).

---

## 8. Docs

- ADR-0004 (`docs/decisions/0004-daemon-cors.md`): deny-by-default CORS allowlist +
  the localhost-CORS rationale; the `cairn.toml` config file (and that it's the first
  config-file surface, extensible); `--config`/`--cors-origin`.
- Handoff update: a short "Running the daemon for a browser UI" note — the UI's dev
  origin must be allowlisted (sample `cairn.toml` or `--cors-origin http://localhost:5173`),
  else the browser blocks requests. WebSocket needs no CORS config.

---

## 9. Out of scope

Authentication, TLS, credentials/cookies (none today); network exposure beyond
loopback; any non-CORS config keys (the schema is intentionally minimal but
extensible).
