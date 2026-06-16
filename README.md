# Cairn

Cairn is an open-source, git-backed, Obsidian-class note-management engine
written in Rust. Plain markdown files in a git repository are always the
canonical form — diffable, portable, and CLI-readable on every surface. The
engine is plugin-extensible, built for first-class integration with tau
(Titouan's terminal-native Rust agent runtime), and lives in the
[`tau-rs`](https://github.com/tau-rs) GitHub org. A single note collection is
called **a cairn** — analogous to "a repo" in git.

## Status

Walking skeleton: the engine and CLI are fully working (init, write, read,
search, backlinks, commit). The `tau`/`AgentRuntime` seam is now wired for
interactive use — `cairn ask` streams a note-grounded answer from a `tau serve`
subprocess. The web UI, daemon-supervised tau sidecar, dataflow pipelines, and
CRDT collaboration remain future sub-projects, each present today as a proven
seam.

## License

Dual-licensed under **MIT OR Apache-2.0** — see
[`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

## Build & test

Requires **Rust 1.88** (pinned in `rust-toolchain.toml`). On first compile,
`git2` builds a vendored copy of libgit2; subsequent builds are fast.

```sh
cargo build --workspace
cargo test --workspace
```

## CLI usage

```sh
# Create a new cairn at ./my-notes
cargo run -p cairn-cli -- --cairn ./my-notes init

# Write notes
cargo run -p cairn-cli -- --cairn ./my-notes write a.md "links to [[b]]"
cargo run -p cairn-cli -- --cairn ./my-notes write b.md "the target"

# Read a note
cargo run -p cairn-cli -- --cairn ./my-notes read a.md

# Full-text search (substring match in the skeleton)
cargo run -p cairn-cli -- --cairn ./my-notes search target

# List notes that link to b.md
cargo run -p cairn-cli -- --cairn ./my-notes backlinks b.md

# Commit all changes to git
cargo run -p cairn-cli -- --cairn ./my-notes commit "first"
```

The `--cairn` flag defaults to `.`, so from inside an initialized directory
you can drop it: `cargo run -p cairn-cli -- search target`.

## Architecture

Cairn is structured as a hexagonal Cargo workspace. Pure domain types live in
`cairn-domain`; port traits in `cairn-ports`; concrete adapters
(`LocalFsStore`, `GitVcs`, `InMemoryIndex`, and four Null seams) in
`cairn-infra`; use-cases in `cairn-app`; and the transport-blind
Command/Query/Event contract — with generated TypeScript bindings — in
`cairn-contract` (`crates/cairn-contract/bindings/`). The CLI in `cairn-cli`
is an in-process consumer that validates the full stack end-to-end.

For the full design rationale see
[`docs/superpowers/specs/2026-06-01-cairn-engine-design.md`](docs/superpowers/specs/2026-06-01-cairn-engine-design.md)
and the first architecture decision record at
[`docs/decisions/0001-walking-skeleton.md`](docs/decisions/0001-walking-skeleton.md).

## Daemon trust model

`cairn-daemon` binds `127.0.0.1` only. Its `/command`, `/query`, `/ask`, and
`/mcp` routes require a local bearer token: on startup the daemon writes a random
token to `<cairn>/.cairn/token` (mode `0600`) and requires it as an
`Authorization: Bearer <token>` header. Any client with filesystem access to the
cairn reads that file; on a multi-user host the `0600` permissions restrict that
to the cairn's owner, so another local user cannot drive the daemon. The token
is regenerated each startup.

The `/mcp` route exposes cairn's note operations as
[MCP](https://modelcontextprotocol.io) tools (see
[`docs/decisions/0011-mcp-server.md`](docs/decisions/0011-mcp-server.md)). Write
tools are off by default — pass `--mcp-write` to enable note mutation. Because an
MCP client's config may carry only a bare URL with no header (e.g. tau), `/mcp`
also accepts the same token as a `?token=<token>` query parameter; prefer the
header where the client supports it.

`/health` is an open liveness probe. The `/events` WebSocket is gated by an
Origin allowlist (see [`docs/decisions/0004-daemon-cors.md`](docs/decisions/0004-daemon-cors.md));
cross-origin browser access to the daemon is governed by the same CORS
allowlist. See [`docs/decisions/0010-daemon-auth.md`](docs/decisions/0010-daemon-auth.md)
for the authentication design and its deferred increments (Unix-socket
transport, token-gated events, the browser-UI token channel).

## Vocabulary

| Concept | Cairn term |
|---|---|
| The whole note collection (Obsidian: "Vault") | **a cairn** |

All other terminology (note, link, backlink, tag, embed, frontmatter, search,
graph, plugin) is standard and unchanged.
