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
search, backlinks, commit). The web UI, engine-plugin host, tau/`AgentRuntime`
adapter, daemon transport, and CRDT collaboration are future sub-projects,
each present today as a proven seam (a port trait with a Null/stub adapter).

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

## Vocabulary

| Concept | Cairn term |
|---|---|
| The whole note collection (Obsidian: "Vault") | **a cairn** |

All other terminology (note, link, backlink, tag, embed, frontmatter, search,
graph, plugin) is standard and unchanged.
