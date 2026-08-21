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
[`docs/decisions/0013-mcp-server.md`](docs/decisions/0013-mcp-server.md)). Write
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

## Plugin trust model

The engine is plugin-extensible: a plugin is a directory under
`<cairn>/.cairn/plugins/<name>/` holding a `manifest.toml` and an executable,
which the daemon spawns as a child process to extend the engine.

**Approving a plugin runs it as fully-trusted native code.** A trusted plugin is
spawned with the daemon's full operating-system privileges — the same user,
filesystem, and process access the daemon itself has. The OS sandbox around the
child (Seatbelt/`sandbox-exec` on macOS, bubblewrap on Linux, AppContainer on
Windows) reduces the blast radius of a *trusted* plugin — it denies direct
filesystem writes and direct vault reads, and, unless the plugin declares the
`net` capability, outbound network — but it is **not** a security boundary you
should rely on to run code you do not trust. Approving a plugin is equivalent to
trusting its author, and its exact on-disk contents, to run as you.

The manifest's `capabilities` are likewise **not** a general sandbox: `vault:read`,
`vault:write`, and `vault:events` only gate the host-callback RPC surface (the
plugin asking the daemon to read or write notes); a trusted plugin can still do
anything its process privileges allow directly.

### Approving a plugin

Approval is **interactive and per-exact-version**, via `cairn plugin`:

```sh
# Fetch a plugin from git. It lands UNTRUSTED and will not run.
cairn plugin add https://example.com/some-plugin.git

# Review it: prints its command, content hash, and declared capabilities,
# then prompts y/N on stdin. A non-interactive (piped) answer counts as "no".
cairn plugin trust some-plugin

# List installed plugins with their trust status and available updates.
cairn plugin list
```

Approving prints the `cairn.toml` entry to add under `[plugins]`:

```toml
[[plugins.trusted]]
dir  = "some-plugin"
hash = "sha256:…"   # pins the exact directory contents
```

### What trust pins, and "Drift"

The `hash` pins the plugin's **exact directory contents** (a `sha256:` digest of
the tree). Before spawning a pinned plugin the daemon re-hashes its directory and
compares:

- **pinned** — the hash matches; the plugin spawns.
- **DRIFT** — the contents changed since you pinned them, so the daemon **refuses
  to spawn** it. This is intentional: any change to a trusted plugin's files (an
  update, a tamper) invalidates the pin. Re-review the new version with
  `cairn plugin trust <dir>` and update `hash = "…"` in `cairn.toml` to re-approve.
- **trusted (unpinned)** — the entry is a bare name (`trusted = ["some-plugin"]`)
  with no hash; it spawns, but the daemon warns and prints the hash to pin.
- **untrusted** — not listed in `[plugins].trusted`; never spawned, and its
  manifest is never even parsed.

An empty or absent `[plugins].trusted` list is the secure default: nothing runs.
See [`docs/superpowers/specs/2026-06-11-cairn-plugin-trust-design.md`](docs/superpowers/specs/2026-06-11-cairn-plugin-trust-design.md)
for the design rationale.

## Vocabulary

| Concept | Cairn term |
|---|---|
| The whole note collection (Obsidian: "Vault") | **a cairn** |

All other terminology (note, link, backlink, tag, embed, frontmatter, search,
graph, plugin) is standard and unchanged.
