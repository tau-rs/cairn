# cairn-web-ui

[![CI](https://github.com/tau-rs/cairn/actions/workflows/ci.yml/badge.svg)](https://github.com/tau-rs/cairn/actions/workflows/ci.yml)

Lives in the cairn monorepo since 2026-11 ([tau-rs/cairn#244](https://github.com/tau-rs/cairn/issues/244)); the former standalone repo is archived.

Web-tech UI for [Cairn](https://github.com/tau-rs/cairn) — an open-source,
git-backed, Obsidian-class note app. Part of the [`tau-rs`](https://github.com/tau-rs) org.

Cairn's Rust engine (`../../crates`) is a transport-blind hexagon exposing one
async contract — commands, queries, and an event stream. This directory is the
web UI that consumes that contract, designed to run unchanged over an
in-process (Tauri) or networked (daemon) transport. The Tauri shell depends on
the engine crates by path, and the web app reads the generated TypeScript
bindings straight from `crates/cairn-contract/bindings/` (alias
`@cairn-contract/*`), so the UI can never drift from the engine it ships with.

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

This directory has two stacks — a pnpm web frontend (`web/`) and a cargo Tauri
shell (`src-tauri/`, its own Cargo workspace). The [`justfile`](justfile) here is
the single source of truth for the dev verbs; each one fans out to **both**
stacks, and CI calls the same recipes (`just -f apps/web-ui/justfile web-ci` /
`rust-ci` from the repo root) so "passes locally" matches CI. Run `just` from
`apps/web-ui`, or `just -f apps/web-ui/justfile <verb>` from the root.

Prerequisites: [`just`](https://github.com/casey/just) and
[`lefthook`](https://github.com/evilmartians/lefthook) (`brew install just lefthook`),
plus `pnpm` and a Rust toolchain.

```bash
cd apps/web-ui
just                    # list every verb
```

Git hooks are installed once at the repo root (`lefthook install` there); the
root `lefthook.yml` runs this stack's `fmt`, `lint` and `test` verbs on commits
that touch `apps/web-ui/web/`.

| Verb | web/ (pnpm) | src-tauri/ (cargo) |
|------|-------------|--------------------|
| `just fmt`  | `prettier --write` | `cargo fmt` |
| `just lint` | `eslint .` + `tsc --noEmit` | `clippy -D warnings` |
| `just test` | `vitest run` | `cargo test` |
| `just deny` | `pnpm audit` | `cargo deny check` (skips until `deny.toml` lands) |
| `just fix`  | `eslint --fix` + `prettier --write` | `cargo fmt` |
| `just ci`   | the full web PR gate | the full rust PR gate |

The pre-commit hook runs the fast verbs only (`fmt`, `lint`, `test`); heavy
work (e2e, mutation, OS matrix, native bundle) stays in CI
(`.github/workflows/{ci,web-nightly,web-mutation-weekly,heavy}.yml` at the root).

## License

Dual-licensed under MIT OR Apache-2.0, matching the rest of `tau-rs`.
