# Cairn T2 HEAVY Tier + Release Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. DevOps work is **verification over unit-TDD** (per brief): the "tests" here are local YAML/build smoke checks plus one real CI run, not unit tests.

**Goal:** Add `.github/workflows/heavy.yml` — the T2 HEAVY tier that runs on `push: tags: ['v*']` + `workflow_dispatch`, performing release-grade verification (full OS matrix, explicit MSRV, feature-powerset, fuzz, mutants, coverage), generating an SBOM, and building + publishing `cairn` / `cairn-daemon` binaries to a GitHub Release. Closes audit gaps **D3** (no HEAVY tier) and **D4** (no supply-chain artifacts).

**Architecture:** A single self-contained workflow file (no `workflow_call` SPOF, per devops.md §2). Reuses the existing `./.github/actions/setup-rust` composite for toolchain + caching. All third-party actions are SHA-pinned (session 81 dependency not yet merged, so we pin ourselves). A new detached `fuzz/` cargo crate (its own workspace) holds two libFuzzer targets for the parse/validation surfaces in `security.md`. The release build is a per-OS matrix that uploads workflow artifacts; a single `release-publish` job (only on `v*` tags, the only job with `contents: write`) collects them + the SBOM and creates the GitHub Release.

**Tech Stack:** GitHub Actions, Rust 1.88 (pinned via `rust-toolchain.toml`) + nightly (fuzz only), `cargo-hack`, `cargo-fuzz` + `libfuzzer-sys`, `cargo-mutants`, `cargo-llvm-cov`, `cargo-cyclonedx`, `softprops/action-gh-release`.

---

## Key decisions

**OS-matrix placement (brief step 2).** **Keep `ci.yml`'s full linux/macos/windows matrix in T1 as a documented intentional variation, AND also run the full matrix in `heavy.yml`.** Rationale: cairn is a git- and filesystem-heavy app whose path handling is provably OS-divergent (security finding **S7**: `NotePath` mishandles Windows drive/UNC paths). Catching OS-specific path bugs at PR time is worth more than a sub-10-min gate here. The B+C model explicitly permits declining a sync to preserve a justified variation (devops.md §3). This keeps the change **purely additive** — `ci.yml` is not touched, so the PR cannot regress the existing gate. heavy.yml re-runs the matrix as release verification on the pinned toolchain.

**Release gating.** `release-publish` `needs:` only the hard-correctness jobs (`os-matrix`, `msrv`, `feature-powerset`, `sbom`, `release-build`). The slow drift-catchers (`fuzz`, `mutants`, `coverage`) run in parallel and are **advisory** — `mutants` uses `continue-on-error` (surviving mutants are signal, not a release blocker, matching coverage's "measurement not gating" stance in coverage.yml). `release-publish` additionally guards on `if: startsWith(github.ref, 'refs/tags/v')` so `workflow_dispatch` exercises everything *except* publishing a release.

**justfile wiring (brief).** Session 80's `justfile` is **not yet on main** (verified: no `justfile` at repo root, `cargo hack` etc. not yet wrapped). So heavy.yml invokes `cargo` directly — consistent with how `ci.yml` invokes cargo directly today. When the justfile lands (D5), these `run:` lines converge onto `just heavy:*` verbs. This is documented in the PR body.

**Phase-2 explicitly excluded.** No cosign, no SLSA provenance, no `id-token: write`. SBOM (cyclonedx) is the only supply-chain artifact (canonical T2 core).

## Pinned action SHAs (resolve-verified 2026-06-11)

| Action | SHA | Comment |
|---|---|---|
| `actions/checkout` | `df4cb1c069e1874edd31b4311f1884172cec0e10` | `# v6.0.3` |
| `actions/upload-artifact` | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` | `# v7.0.1` |
| `actions/download-artifact` | `37930b1c2abaa49bbe596cd826c3c89aef350131` | `# v7` |
| `taiki-e/install-action` | `7a79fe8c3a13344501c80d99cae481c1c9085912` | `# v2.81.10` |
| `softprops/action-gh-release` | `3bb12739c298aeb8a4eeaf626c5b8d85266b0e65` | `# v2` |

`./.github/actions/setup-rust` is a local action (no pin needed). The actions it wraps are pinned by session 81 separately; this PR does not re-pin them.

## File structure

- Create: `fuzz/Cargo.toml` — detached fuzz crate (own `[workspace]`), deps on `libfuzzer-sys`, `cairn-domain`, `cairn-plugin-protocol`, `toml`.
- Create: `fuzz/fuzz_targets/note_path.rs` — fuzz `cairn_domain::NotePath::new`.
- Create: `fuzz/fuzz_targets/plugin_manifest.rs` — fuzz `toml::from_str::<cairn_plugin_protocol::Manifest>`.
- Create: `fuzz/.gitignore` — ignore `corpus/`, `artifacts/`, `coverage/`, `target/`, `Cargo.lock`.
- Modify: `Cargo.toml` — add `exclude = ["fuzz"]` to `[workspace]` (defensive; keeps `cargo --workspace` from descending into the detached fuzz crate).
- Create: `.github/workflows/heavy.yml` — the T2 HEAVY tier.

---

## Task 1: Detached fuzz crate + targets

**Files:**
- Modify: `Cargo.toml` (`[workspace]` table)
- Create: `fuzz/Cargo.toml`, `fuzz/fuzz_targets/note_path.rs`, `fuzz/fuzz_targets/plugin_manifest.rs`, `fuzz/.gitignore`

- [ ] **Step 1: Add `exclude` to root workspace**

In `Cargo.toml`, change the `[workspace]` block to add `exclude` after `members = [...]`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/cairn-domain",
    "crates/cairn-plugin-protocol",
    "crates/cairn-plugin-sdk",
    "crates/cairn-ports",
    "crates/cairn-infra",
    "crates/cairn-app",
    "crates/cairn-contract",
    "crates/cairn-service",
    "crates/cairn-cli",
    "crates/cairn-daemon",
    "crates/cairn-plugin-example",
]
exclude = ["fuzz"]
```

- [ ] **Step 2: Create `fuzz/Cargo.toml`**

```toml
[package]
name = "cairn-fuzz"
version = "0.0.0"
edition = "2021"
publish = false

# Detached from the parent workspace so cargo-fuzz's nightly/sanitizer
# build and libfuzzer-sys deps never touch the main `--locked` graph.
[workspace]

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
cairn-domain = { path = "../crates/cairn-domain" }
cairn-plugin-protocol = { path = "../crates/cairn-plugin-protocol" }
toml = "1"

[[bin]]
name = "note_path"
path = "fuzz_targets/note_path.rs"
test = false
doc = false
bench = false

[[bin]]
name = "plugin_manifest"
path = "fuzz_targets/plugin_manifest.rs"
test = false
doc = false
bench = false
```

- [ ] **Step 3: Create `fuzz/fuzz_targets/note_path.rs`**

```rust
#![no_main]
//! Fuzz `NotePath::new` — the path-validation surface behind every write
//! (security.md S1/S4/S7). The invariant under test: parsing arbitrary
//! input must never panic; it returns `Ok` or a `NotePathError`.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = cairn_domain::NotePath::new(s);
    }
});
```

- [ ] **Step 4: Create `fuzz/fuzz_targets/plugin_manifest.rs`**

```rust
#![no_main]
//! Fuzz plugin manifest TOML parsing (security.md S1/S3) — the daemon
//! deserializes untrusted `.cairn/plugins/*/manifest.toml` into `Manifest`
//! (plugin_host.rs:422). Parsing arbitrary bytes must never panic.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = toml::from_str::<cairn_plugin_protocol::Manifest>(s);
    }
});
```

- [ ] **Step 5: Create `fuzz/.gitignore`**

```gitignore
target/
corpus/
artifacts/
coverage/
Cargo.lock
```

- [ ] **Step 6: Verify the main workspace is unaffected**

Run: `cargo metadata --no-deps --format-version=1 --offline >/dev/null 2>&1 || cargo metadata --no-deps --format-version=1 >/dev/null`
Expected: succeeds; the `exclude` keeps `fuzz` out of the workspace.

Run: `test -f fuzz/Cargo.toml && test -f fuzz/fuzz_targets/note_path.rs && test -f fuzz/fuzz_targets/plugin_manifest.rs && echo OK`
Expected: `OK`

- [ ] **Step 7: Verify fuzz targets build (requires nightly + cargo-fuzz; best-effort locally)**

Run (only if nightly + cargo-fuzz available locally): `cd fuzz && cargo +nightly fuzz build 2>&1 | tail -20`
Expected: builds both targets. If nightly/cargo-fuzz unavailable locally, skip — CI is the authoritative check. Do NOT block on this locally.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml fuzz/
git commit -m "build(fuzz): add detached cargo-fuzz crate for NotePath + plugin manifest"
```

---

## Task 2: Author `heavy.yml`

**Files:**
- Create: `.github/workflows/heavy.yml`

- [ ] **Step 1: Write the full workflow**

```yaml
name: heavy

# T2 HEAVY tier — release-grade verification + supply-chain + GitHub Release.
# Runs on v* tag pushes (the "heavy lifting on feature release") and on
# manual workflow_dispatch (which exercises every job EXCEPT publishing a
# release). The fast PR gate lives in ci.yml and is untouched by this file.
on:
  push:
    tags: ['v*']
  workflow_dispatch:

# Don't cancel an in-flight release. A tag run that's writing artifacts or
# the GitHub Release must finish.
concurrency:
  group: heavy-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: false

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
  CARGO_INCREMENTAL: 0

# Least privilege by default; only release-publish elevates to contents: write.
permissions:
  contents: read

jobs:
  os-matrix:
    name: os-matrix / ${{ matrix.os == 'ubuntu-latest' && 'linux' || matrix.os == 'macos-latest' && 'macos' || 'windows' }}
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: ${{ matrix.os }}
          with-nextest: true
          with-sccache: true
          with-mold: true
      - run: cargo nextest run --profile ci --workspace --all-targets
      - run: cargo test --workspace --doc

  msrv:
    # Explicit MSRV build, distinct from ci.yml's --locked lock guard:
    # install 1.88 and check against it directly.
    name: msrv (1.88)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: "1.88"
          shared-key: linux
          with-mold: true
      - run: cargo +1.88 check --workspace --all-targets --locked

  feature-powerset:
    # Check every combination of feature flags. Cairn currently declares no
    # cargo features, so this reduces to a base check today; it gains value
    # the moment feature seams are added (zero-cost insurance).
    name: feature-powerset
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux
          with-mold: true
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: cargo-hack
      - run: cargo hack --feature-powerset --workspace check --all-targets

  fuzz:
    # Short, bounded smoke fuzz of the parse/validation surfaces flagged in
    # security.md (NotePath, plugin manifest TOML). Long campaigns belong in
    # the T3 nightly schedule (session 83); here we only prove targets build
    # and survive a 60s run without crashing.
    name: fuzz (smoke)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: nightly
          shared-key: linux
          with-mold: true
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: cargo-fuzz
      - name: Fuzz NotePath::new
        run: cargo +nightly fuzz run note_path -- -max_total_time=60 -rss_limit_mb=2048
        working-directory: fuzz
      - name: Fuzz plugin manifest TOML
        run: cargo +nightly fuzz run plugin_manifest -- -max_total_time=60 -rss_limit_mb=2048
        working-directory: fuzz
      - name: Upload any crash artifacts
        if: failure()
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: fuzz-artifacts-${{ github.sha }}
          path: fuzz/artifacts/
          if-no-files-found: ignore
          retention-days: 14

  mutants:
    # Mutation testing over the workspace. Advisory, NOT a release blocker:
    # surviving mutants are a test-quality signal (cf. coverage.yml's
    # "measurement not gating"). continue-on-error keeps a v* release from
    # being held hostage by a single surviving mutant; results are uploaded.
    name: mutants (advisory)
    runs-on: ubuntu-latest
    timeout-minutes: 60
    continue-on-error: true
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux
          with-nextest: true
          with-mold: true
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: cargo-mutants
      - run: cargo mutants --workspace --no-shuffle --timeout 120 -j 2
      - name: Upload mutants report
        if: always()
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: mutants-${{ github.sha }}
          path: mutants.out/
          if-no-files-found: ignore
          retention-days: 14

  coverage:
    # Coverage on release. Mirrors coverage.yml's cargo-llvm-cov logic
    # (self-contained per devops.md's no-workflow_call principle). Signal,
    # never a gate.
    name: coverage
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          components: llvm-tools-preview
          shared-key: linux
          with-nextest: true
          with-sccache: true
          with-mold: true
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: cargo-llvm-cov
      - name: Generate lcov coverage
        run: |
          cargo llvm-cov nextest \
            --workspace --no-fail-fast \
            --lcov --output-path lcov.info \
            --ignore-filename-regex '(tests/|/target/|/build\.rs$)'
      - name: Upload lcov artifact
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: lcov-release-${{ github.sha }}
          path: lcov.info
          retention-days: 14

  sbom:
    # Supply-chain core (D4): CycloneDX SBOM for the workspace. Uploaded as a
    # workflow artifact and (on tag) attached to the GitHub Release.
    name: sbom (cyclonedx)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux
          with-mold: true
      - name: Install cargo-cyclonedx
        run: cargo install cargo-cyclonedx --version ^0.5 --locked
      - name: Generate SBOM
        run: cargo cyclonedx --all --format json
      - name: Aggregate SBOMs into one artifact dir
        shell: bash
        run: |
          mkdir -p sbom
          # cargo-cyclonedx writes a <crate>.cdx.json next to each crate's
          # Cargo.toml; collect them all under sbom/.
          find . -name '*.cdx.json' -not -path './sbom/*' -exec cp --parents {} sbom/ \;
          ls -R sbom
      - name: Upload SBOM artifact
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: sbom-${{ github.sha }}
          path: sbom/
          retention-days: 90

  release-build:
    # Build the shipping binaries (cairn, cairn-daemon) per OS and package
    # them. Runs on dispatch too, so a manual run verifies the build/package
    # path without publishing a release.
    name: release-build / ${{ matrix.os == 'ubuntu-latest' && 'linux' || matrix.os == 'macos-latest' && 'macos' || 'windows' }}
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: ${{ matrix.os }}
          with-sccache: true
          with-mold: true
      - name: Build release binaries
        run: cargo build --release --locked --bin cairn --bin cairn-daemon
      - name: Package (unix)
        if: runner.os != 'Windows'
        shell: bash
        run: |
          os='${{ matrix.os == 'ubuntu-latest' && 'linux' || 'macos' }}'
          name="cairn-${os}-x86_64"
          mkdir -p "dist/${name}"
          cp target/release/cairn target/release/cairn-daemon "dist/${name}/"
          cp README.md LICENSE-MIT LICENSE-APACHE "dist/${name}/"
          tar -C dist -czf "dist/${name}.tar.gz" "${name}"
      - name: Package (windows)
        if: runner.os == 'Windows'
        shell: bash
        run: |
          name="cairn-windows-x86_64"
          mkdir -p "dist/${name}"
          cp target/release/cairn.exe target/release/cairn-daemon.exe "dist/${name}/"
          cp README.md LICENSE-MIT LICENSE-APACHE "dist/${name}/"
          (cd dist && 7z a "${name}.zip" "${name}" >/dev/null)
      - name: Upload packaged binary
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: dist-${{ matrix.os }}
          path: |
            dist/*.tar.gz
            dist/*.zip
          if-no-files-found: error
          retention-days: 14

  release-publish:
    # The ONLY job with contents: write, and the ONLY tag-gated job. Collects
    # every packaged binary + the SBOM and creates the GitHub Release. Skipped
    # on workflow_dispatch (no tag) so manual runs never publish.
    name: release-publish
    if: startsWith(github.ref, 'refs/tags/v')
    needs: [os-matrix, msrv, feature-powerset, sbom, release-build]
    runs-on: ubuntu-latest
    timeout-minutes: 15
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131 # v7
        with:
          path: artifacts
      - name: Stage release assets
        shell: bash
        run: |
          mkdir -p release
          find artifacts -type f \( -name '*.tar.gz' -o -name '*.zip' \) -exec cp {} release/ \;
          # Bundle the SBOM set into a single archive for the release.
          if [ -d artifacts/sbom-${{ github.sha }} ]; then
            tar -C artifacts/sbom-${{ github.sha }} -czf release/cairn-sbom-${{ github.ref_name }}.tar.gz .
          fi
          ls -l release
      - name: Create GitHub Release
        uses: softprops/action-gh-release@3bb12739c298aeb8a4eeaf626c5b8d85266b0e65 # v2
        with:
          files: release/*
          generate_release_notes: true
          # Pre-1.0 / pre-release tags (containing a hyphen, e.g. v1.0.0-rc1)
          # are marked as prereleases.
          prerelease: ${{ contains(github.ref_name, '-') }}
          fail_on_unmatched_files: true
```

- [ ] **Step 2: Lint the workflow with actionlint (if available)**

Run: `command -v actionlint >/dev/null && actionlint .github/workflows/heavy.yml || echo "actionlint not installed — skipping (CI parses it authoritatively)"`
Expected: no errors, or the skip message.

- [ ] **Step 3: YAML well-formedness check**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/heavy.yml')); print('YAML OK')"`
Expected: `YAML OK`

- [ ] **Step 4: Confirm no unintended ci.yml change & all jobs have timeouts**

Run: `git diff --stat origin/main -- .github/workflows/ci.yml` → Expected: empty (ci.yml untouched).
Run: `grep -c 'timeout-minutes:' .github/workflows/heavy.yml` → Expected: `9` (one per job).
Run: `grep -c 'contents: write' .github/workflows/heavy.yml` → Expected: `1` (release-publish only).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/heavy.yml docs/superpowers/plans/2026-06-11-heavy-tier-release.md
git commit -m "feat(ci): add T2 heavy tier + release build (audit D3, D4)"
```

---

## Task 3: Verify with a real CI run (verification-before-completion)

**This is the authoritative test.** Per the brief, trigger heavy.yml and confirm each job is green and the Release step produces binaries + SBOM.

- [ ] **Step 1: Push the branch**

```bash
git branch --show-current   # confirm: cairn-devops-heavy-tier-release
git push -u origin cairn-devops-heavy-tier-release
```

- [ ] **Step 2: Trigger via a throwaway prerelease tag**

`workflow_dispatch` cannot be triggered for a workflow that isn't yet on the default branch, so use a tag push (the brief's stated fallback). The tag's hyphen marks it a prerelease.

```bash
git tag v0.0.0-heavytest1
git push origin v0.0.0-heavytest1
```

- [ ] **Step 3: Watch the run to completion**

```bash
sleep 20
run_id=$(gh run list --workflow=heavy.yml --branch v0.0.0-heavytest1 --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$run_id" --exit-status || true
gh run view "$run_id"
```
Expected: `os-matrix` (×3), `msrv`, `feature-powerset`, `fuzz`, `coverage`, `sbom`, `release-build` (×3), `release-publish` all succeed. `mutants` may show as failed-but-continue (advisory) — that is acceptable and expected.

- [ ] **Step 4: Confirm the Release has binaries + SBOM**

```bash
gh release view v0.0.0-heavytest1 --json assets --jq '.assets[].name'
```
Expected: `cairn-linux-x86_64.tar.gz`, `cairn-macos-x86_64.tar.gz`, `cairn-windows-x86_64.zip`, `cairn-sbom-v0.0.0-heavytest1.tar.gz`.

- [ ] **Step 5: Capture evidence**

```bash
gh run view "$run_id" --json jobs --jq '.jobs[] | {name: .name, conclusion: .conclusion}' | tee .context/heavy-run-evidence.json
gh release view v0.0.0-heavytest1 --json assets --jq '[.assets[].name]' | tee -a .context/heavy-run-evidence.json
```

- [ ] **Step 6: Clean up the throwaway release + tag**

```bash
gh release delete v0.0.0-heavytest1 --yes --cleanup-tag
git tag -d v0.0.0-heavytest1
git push origin :refs/tags/v0.0.0-heavytest1 2>/dev/null || true
```
Expected: release + remote tag removed. The heavy.yml workflow run history remains as evidence.

---

## Task 4: Code review + PR (requesting-code-review)

- [ ] **Step 1:** Invoke `superpowers:requesting-code-review`. Verify checklist (brief step 5): every job has `timeout-minutes`; `permissions` least-priv with `contents: write` only on `release-publish`; no cosign/SLSA/`id-token` crept in; ci.yml unchanged; all actions SHA-pinned.

- [ ] **Step 2: Open the PR (no merge)**

```bash
gh pr create -R tau-rs/cairn --base main \
  --title "feat(ci): T2 heavy tier + release build (audit D3, D4)" \
  --body "<see PR body in plan>"
```
Cite D3 + D4, document the OS-matrix decision, note justfile-not-yet-present, and state phase-2 (cosign/SLSA) is deferred. STOP — no merge.

---

## Self-review checklist

- **D3 coverage:** os-matrix ✓, msrv ✓, feature-powerset ✓, fuzz ✓, mutants ✓, coverage ✓, release-build ✓ — all present.
- **D4 coverage:** sbom job (cyclonedx) ✓, attached to release ✓.
- **Triggers:** `push: tags: ['v*']` ✓ + `workflow_dispatch` ✓.
- **Hardening:** top-level `permissions: contents: read` ✓; per-job `contents: write` only on release-publish ✓; concurrency group ✓; `timeout-minutes` on all 9 jobs ✓.
- **SHA pins:** all 5 third-party actions pinned ✓.
- **Phase-2 excluded:** no cosign/SLSA/id-token ✓.
- **Additive:** ci.yml untouched ✓ (OS-matrix kept as documented variation).
