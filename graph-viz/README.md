# cairn graph-viz (standalone, B1)

Standalone browser graph-viz targeting a running `cairn-daemon` on current-main.
Folds into `cairn-web-ui` at roadmap item D1 — see
`docs/superpowers/specs/2026-08-08-standalone-graph-viz-design.md`.

## Run

1. Start the daemon against a real vault:

   ```bash
   cargo run -p cairn-daemon -- --cairn /path/to/vault --port 7777
   ```

2. Point the app at that vault (so the Vite proxy can read `.cairn/token`) and start it:

   ```bash
   cd graph-viz
   pnpm install
   CAIRN_VAULT=/path/to/vault pnpm dev
   ```

   Optional: `CAIRN_DAEMON=http://127.0.0.1:7777` (default). Open the printed URL.

The Vite proxy reads `<vault>/.cairn/token` per request and injects the bearer
token, so the browser talks same-origin — no CORS, token never in browser JS.

## Modes
- **Live** — full HEAD graph.
- **Scrub** — slide over vault history; releases fire `graph_at`.
- **Diff** — pick from→to; releases fire `graph_diff`; added=green, removed=ghost-red, changed=amber.

## Contract bindings
`pnpm sync-contract` re-vendors `../crates/cairn-contract/bindings/*.ts` into `src/contract/`.
These are generated (ts-rs) — do not edit by hand.

## Portability
`src/client/` and `src/graph/` are the portable modules that fold into
`cairn-web-ui` at D1; the Vite shell (`index.html`, `vite.config.ts`, `main.tsx`,
`App.tsx`) is throwaway. The portable modules never import from the shell.
