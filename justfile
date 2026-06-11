# cairn workspace task runner — canonical verbs shared with CI and lefthook so
# "local == CI" and the same muscle memory works across the sibling repos.
#
# Each recipe carries ONLY the cargo command string (identical to the matching
# CI job / lefthook command). The execution environment is supplied by the
# CALLER:
#   - CI        sets CARGO_INCREMENTAL=0 at the workflow `env:` level.
#   - lefthook  sets CARGO_INCREMENTAL=0 / CARGO_TARGET_DIR per command, and
#               appends `--target-dir` to `just test` for its isolation.
# `just` passes the inherited environment through to recipe shells, so the
# executed cargo invocation + env is byte-equivalent to running the command
# directly. Do NOT bake CARGO_TARGET_DIR into a recipe — it would clobber
# lefthook's per-command target dirs.
#
# cairn has no xtask, so every recipe delegates straight to cargo.

# List the available recipes (default when `just` is run with no arguments).
default:
    @just --list

# Format check — mirrors the `rustfmt` CI job (ci.yml) and lefthook pre-commit.
fmt:
    cargo fmt --all -- --check

# Lint — mirrors the `clippy` CI job (ci.yml) and lefthook pre-commit.
lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# `--profile ci` ⇒ retries=0 (a flake is signal); lefthook now shares this
# profile so local == CI. Extra args are forwarded to nextest so callers can
# append flags — lefthook appends `--target-dir target/lefthook/test` for its
# per-command isolation.

# Test — mirrors the `test` CI job (ci.yml) and lefthook pre-commit.
test *args:
    cargo nextest run --profile ci --workspace --all-targets {{args}}

# nextest does not run doctests, so this is a distinct verb, not part of `test`.

# Doc tests — mirrors the `doc-tests` CI job (ci.yml) and lefthook pre-push.
doc-test:
    cargo test --workspace --doc

# `--all-features` is a GLOBAL flag (before the subcommand) in cargo-deny 0.14+,
# and the cargo-deny-action passes it that way too (arguments → command), so this
# is byte-for-byte what CI runs: `cargo-deny --all-features check`.

# Dependency / license / advisory audit — mirrors the `cargo-deny` CI job.
deny:
    cargo deny --all-features check

# `--locked` fails if Cargo.lock drifts off the 1.88 MSRV pin.

# Lockfile / MSRV-pin guard — mirrors the `locked-check` CI job and pre-push.
locked-check:
    cargo check --workspace --all-targets --locked

# The same set the CI fast tier (ci.yml) runs, so "passes `just ci`" ⇒ "passes CI".

# Full local gate — everything a PR must pass.
ci: fmt lint test doc-test deny locked-check

# Auto-fix: apply rustfmt + machine-applicable clippy suggestions in place.
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# cairn has no `[features]` and no xtask, so feature-powerset / fuzz / mutants /
# SBOM are not available locally — those land with the heavy.yml tier in a later
# session. Run before tagging.

# Local approximation of the T2 "heavy" release tier — full gate + release build.
heavy: ci
    cargo build --release -p cairn-cli -p cairn-daemon
