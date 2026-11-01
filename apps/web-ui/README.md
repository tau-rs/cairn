# cairn-web-ui

[![CI](https://github.com/tau-rs/cairn-web-ui/actions/workflows/ci.yml/badge.svg)](https://github.com/tau-rs/cairn-web-ui/actions/workflows/ci.yml)

Web-tech UI for [Cairn](https://github.com/tau-rs/cairn) — an open-source,
git-backed, Obsidian-class note app. Part of the [`tau-rs`](https://github.com/tau-rs) org.

Cairn's Rust engine (`tau-rs/cairn`) is a transport-blind hexagon exposing one
async contract — commands, queries, and an event stream. This repo is the
web UI that consumes that contract, designed to run unchanged over an
in-process (Tauri) or networked (daemon) transport.

## Status

Greenfield. Roadmap and architecture are being defined; see `docs/`.

## Platforms

Desktop (macOS / Linux / Windows) is the supported target, built with Tauri v2.
The mobile targets (iOS / Android) are scaffolded so the shell builds and
launches, but **opening a cairn on mobile is not yet wired** — it needs a
mobile-specific file-access story (iOS document picker / security-scoped
bookmarks, Android SAF) and `git2` cross-compilation, which is a follow-up
phase. On mobile the app currently shows the "Open a cairn…" empty state only.

## Local development

The repo has two stacks — a pnpm web frontend (`web/`) and a cargo Tauri shell
(`src-tauri/`). A root [`justfile`](justfile) is the single source of truth for
the dev verbs; each one fans out to **both** stacks, and CI calls the same
recipes (`just web-ci` / `just rust-ci`) so "passes locally" matches CI.

Prerequisites: [`just`](https://github.com/casey/just) and
[`lefthook`](https://github.com/evilmartians/lefthook) (`brew install just lefthook`),
plus `pnpm` and a Rust toolchain.

```bash
lefthook install        # one-time: wire the pre-commit hook (fmt + lint + test)
just                    # list every verb
```

| Verb | web/ (pnpm) | src-tauri/ (cargo) |
|------|-------------|--------------------|
| `just fmt`  | `prettier --write` | `cargo fmt` |
| `just lint` | `eslint .` + `tsc --noEmit` | `clippy -D warnings` |
| `just test` | `vitest run` | `cargo test` |
| `just deny` | `pnpm audit` | `cargo deny check` (skips until `deny.toml` lands) |
| `just fix`  | `eslint --fix` + `prettier --write` | `cargo fmt` |
| `just ci`   | the full web PR gate | the full rust PR gate |

The `lefthook` pre-commit hook runs the fast verbs only (`fmt`, `lint`,
`test`); heavy work (e2e, mutation, OS matrix, native bundle) stays in CI.

## License

Dual-licensed under MIT OR Apache-2.0, matching the rest of `tau-rs`.
