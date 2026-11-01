# justfile — the single source of truth for cairn-ui's dev verbs.
#
# Every verb fans out to BOTH stacks: the pnpm web frontend (`web/`) and the
# cargo Tauri shell (`src-tauri/`). CI calls the same recipes (see `ci.yml`)
# and `lefthook` calls the fast ones pre-commit, so "passes locally" can never
# silently diverge from "passes in CI".
#
# Install `just`: https://github.com/casey/just (`brew install just`).
# Run `just` with no args to list the verbs.

# Cargo workspace lives under src-tauri/; web pnpm project under web/.
manifest := "src-tauri/Cargo.toml"

# List the available verbs.
default:
    @just --list

# Format both stacks in place.
fmt: web-fmt rust-fmt

# Format the web frontend (prettier --write).
web-fmt:
    cd web && pnpm format

# Format the Rust shell (cargo fmt).
rust-fmt:
    cargo fmt --manifest-path {{manifest}}

# Lint both stacks (eslint + tsc, clippy -D warnings).
lint: web-lint rust-lint

# Lint the web frontend (eslint + tsc --noEmit).
web-lint:
    cd web && pnpm lint && pnpm typecheck

# Lint the Rust shell (clippy, warnings as errors).
rust-lint:
    cargo clippy --manifest-path {{manifest}} -- -D warnings

# Run fast unit tests for both stacks (no e2e, no mutation).
test: web-test rust-test

# Run the web unit tests (vitest run).
web-test:
    cd web && pnpm test

# Run the Rust unit tests (cargo test).
rust-test:
    cargo test --manifest-path {{manifest}}

# Production-build both stacks.
build: web-build rust-build

# Build the web frontend (tsc -b && vite build).
web-build:
    cd web && pnpm build

# Build the Rust shell (cargo build).
rust-build:
    cargo build --manifest-path {{manifest}}

# Supply-chain audit for both stacks.
deny: web-deny rust-deny

# Audit web dependencies (pnpm audit). --audit-level=high so the gate does not
# flap on low/moderate advisories (which carry no fix on the merge path).
web-deny:
    cd web && pnpm audit --audit-level=high

# Audit Rust dependencies (advisories/licenses/bans/sources). Guarded so the
# verb still works on a checkout without cargo-deny installed; CI installs it.
rust-deny:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-deny >/dev/null 2>&1 && [ -f deny.toml ]; then
        cargo deny --manifest-path {{manifest}} check
    else
        echo "skip: cargo-deny and/or deny.toml absent"
    fi

# Auto-fix lint where possible, then format, for both stacks.
fix: web-fix rust-fix

# Auto-fix + format the web frontend (eslint --fix && prettier --write).
web-fix:
    cd web && pnpm lint:fix && pnpm format

# Format the Rust shell (cargo fmt).
rust-fix:
    cargo fmt --manifest-path {{manifest}}

# The exact per-PR gate for both stacks; ci.yml calls web-ci / rust-ci.
ci: web-ci rust-ci

# Web PR gate, verbatim ci.yml: lint, format:check, typecheck, test, build.
web-ci:
    cd web && pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build

# Rust PR gate: fmt --check, clippy -D warnings, then a --locked test + build so
# a stale Cargo.lock or a link/bundle break fails on the PR (not in nightly).
rust-ci:
    cargo fmt --manifest-path {{manifest}} --check
    cargo clippy --manifest-path {{manifest}} -- -D warnings
    cargo test --locked --manifest-path {{manifest}}
    cargo build --locked --manifest-path {{manifest}}

# Placeholder for the T2 release tier (OS matrix, e2e, mutation, SBOM); later session.
heavy:
    @echo "heavy tier (e2e + mutation + OS matrix + SBOM + signed release) lands in a later session"
