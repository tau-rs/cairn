# `v*`-tag HEAVY release tier (`heavy.yml`) Implementation Plan

> **For agentic workers:** This is release-pipeline CONFIG — verification over
> unit-test TDD (per the brief). Steps carry exact YAML and verification
> commands rather than test-first cycles. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add a `push: tags: v*` + `workflow_dispatch` HEAVY tier (T2 in the
canonical model, Diagram 2 of `audit/devops.md`) that builds + bundles + signs
the cross-OS Tauri release artifacts, generates CycloneDX SBOMs for BOTH the
pnpm and cargo stacks, folds in the heavy verification work (3-browser e2e,
Stryker mutation, coverage), and attaches the bundles + SBOMs to a GitHub
Release — closing G3/G4/G5 and documenting OIDC posture (G14).

**Architecture:** One self-contained `heavy.yml` (B-pillar: no `workflow_call`
indirection). Least-privilege `permissions: contents: read` at top level; only
the tag-gated `release` job is granted `contents: write`. Every new `uses:` is
pinned by commit SHA (consistent with session 91). The Tauri build uses
`tauri-apps/tauri-action` with a committed CI config patch
(`src-tauri/tauri.ci.conf.json`) that neutralises the repo's
`beforeBuildCommand` (the frontend is pre-built explicitly in `web/`, mirroring
the proven `nightly.yml` approach). Signing keys are referenced from secrets,
never committed. `nightly.yml` / `mutation-weekly.yml` schedules stay untouched
as T3 drift-catchers. A `workflow_dispatch` run is inherently a dry-run: the
release-publish job is guarded by `startsWith(github.ref, 'refs/tags/v')`, so
dispatch exercises build + SBOM + signing WITHOUT publishing.

**Tech Stack:** GitHub Actions, Tauri v2 (`tauri-action`), CycloneDX
(`@cyclonedx/cdxgen` for pnpm, `cargo-cyclonedx` for cargo), pnpm 10.14.0,
Rust stable, Playwright, Stryker, Vitest coverage.

---

## File Structure

- **Create** `.github/workflows/heavy.yml` — the T2 release/heavy tier.
- **Create** `src-tauri/tauri.ci.conf.json` — CI-only Tauri config patch that
  sets `beforeBuildCommand` to empty so `tauri-action` does not re-run (and
  fail on) `pnpm build` from the wrong directory; the frontend is pre-built in
  the job. Passed via `tauri-action`'s `args: --config tauri.ci.conf.json`.
- **Modify** `docs/ci.md` — document the new Tier-2 release tier and the OIDC
  posture for the signing/publish step (G14), with the migration path.

Unchanged on purpose: `ci.yml` (T1 gate — session 91 scope), `nightly.yml` and
`mutation-weekly.yml` (T3 schedules), `coverage.yml`. The HEAVY tier must NOT
become a PR merge gate.

### Pinned action SHAs (resolved 2026-06-11)

| action | tag | commit SHA |
|--------|-----|-----------|
| actions/checkout | v6.0.3 | `df4cb1c069e1874edd31b4311f1884172cec0e10` |
| actions/setup-node | v6.4.0 | `48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e` |
| pnpm/action-setup | v6.0.8 | `0e279bb959325dab635dd2c09392533439d90093` |
| actions/upload-artifact | v7.0.1 | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` |
| actions/download-artifact | v8.0.1 | `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` |
| Swatinem/rust-cache | v2.9.1 | `c19371144df3bb44fab255c43d04cbc2ab54d1c4` |
| dtolnay/rust-toolchain | stable | `29eef336d9b2848a0b548edc03f92a220660cdb8` |
| taiki-e/install-action | v2.81.10 | `7a79fe8c3a13344501c80d99cae481c1c9085912` |
| tauri-apps/tauri-action | action-v0.6.2 | `84b9d35b5fc46c1e45415bdb6144030364f7ebc5` |
| softprops/action-gh-release | v2.6.2 | `3bb12739c298aeb8a4eeaf626c5b8d85266b0e65` |

---

## Task 1: CI Tauri config patch

**Files:** Create `src-tauri/tauri.ci.conf.json`

- [ ] **Step 1:** Write the patch (neutralises `beforeBuildCommand`; frontend
  is built explicitly in the job):

```json
{
  "build": {
    "beforeBuildCommand": ""
  }
}
```

- [ ] **Step 2:** Verify it is valid JSON and a strict subset of the schema:

Run: `python3 -c "import json;json.load(open('src-tauri/tauri.ci.conf.json'))" && echo OK`
Expected: `OK`

---

## Task 2: `heavy.yml` — triggers, permissions, concurrency

**Files:** Create `.github/workflows/heavy.yml`

- [ ] **Step 1:** Header — `push: tags: v*` + `workflow_dispatch`, top-level
  least-privilege `contents: read`, a non-cancelling concurrency group, and an
  OIDC posture comment block.

- [ ] **Step 2:** Verify it parses:

Run: `python3 -c "import yaml;yaml.safe_load(open('.github/workflows/heavy.yml'))" && echo OK`
Expected: `OK` (after all jobs are added)

---

## Task 3: `build-bundle` matrix job (G4 — signed Tauri artifacts)

Matrix `os: [ubuntu-latest, macos-latest, windows-latest]`, `contents: read`,
`timeout-minutes: 90`. Steps: checkout → (Linux) webkit deps → rust toolchain +
cache → pnpm + node → `pnpm --dir web install --frozen-lockfile` →
`pnpm --dir web build` → `tauri-action` with `projectPath: src-tauri`,
`args: --config tauri.ci.conf.json`, signing env from secrets, NO `tagName`
(build-only). Upload `src-tauri/target/release/bundle/**` (+ `.sig`) as a
per-OS artifact.

- [ ] Verify in CI: bundle artifacts + `.sig` files appear (see verification).

---

## Task 4: `sbom` job (G5 — CycloneDX npm + cargo)

`contents: read`, `timeout-minutes: 20`. Generate:
- pnpm: `npx -y @cyclonedx/cdxgen@latest -t pnpm -o sbom-web.cdx.json` in `web/`.
- cargo: `cargo cyclonedx --manifest-path src-tauri/Cargo.toml --format json`
  (installed via `taiki-e/install-action` tool `cargo-cyclonedx`).

Upload both as a `sbom` artifact.

- [ ] Verify in CI: both `*.cdx.json` are valid CycloneDX (`bomFormat` field).

---

## Task 5: `e2e`, `mutation`, `coverage` jobs (folded heavy verification)

Port the step bodies verbatim from `nightly.yml` (3-browser Playwright matrix),
`mutation-weekly.yml` (Stryker), and `coverage.yml` (vitest coverage), each
`contents: read` with a `timeout-minutes`. These mirror the T3 work so the
release tier re-runs it at release time; the schedules stay as drift-catchers.

---

## Task 6: `release` job (G3/G4 — GitHub Release)

`needs: [build-bundle, sbom]`, `if: startsWith(github.ref, 'refs/tags/v')`,
`permissions: contents: write` (the ONLY job with write). Download all
artifacts → publish a draft GitHub Release via `softprops/action-gh-release`
with `files:` = bundles + SBOMs. Skipped on `workflow_dispatch` (no tag) ⇒
dispatch is a no-publish dry run.

---

## Task 7: `heavy-summary` job (non-gating aggregator)

`if: always()`, `needs:` all real jobs. Mirrors `nightly-summary` but emits a
`::warning::` (not a failing `exit 1`) — a red HEAVY tier is a signal, never a
gate. Confirms the tier cannot block a PR/merge.

---

## Task 8: Document OIDC posture (G14) in `docs/ci.md`

Add a Tier-2 section: the GitHub Release publish uses the ephemeral
`GITHUB_TOKEN` (no long-lived secret). The Tauri updater signing key
(`TAURI_SIGNING_PRIVATE_KEY` + password) is the only long-lived secret, because
Tauri's minisign signing has no OIDC/keyless path today. Migration path: move to
keyless signing (cosign/Sigstore — phase-2 G15) or an OIDC-federated KMS signer
once available; no cloud-registry auth exists yet, so no other OIDC wiring is
needed now.

---

## Verification (replaces TDD — exercise WITHOUT a real publish)

1. `actionlint` the new workflow (downloaded pinned) — zero errors.
2. `python3 -c "import yaml; yaml.safe_load(...)"` parses `heavy.yml`,
   `nightly.yml`, `mutation-weekly.yml` (schedules still parse).
3. Push the branch; trigger `heavy.yml` via `workflow_dispatch` (no tag ⇒
   release job skipped). Generate a THROWAWAY Tauri signing key, set it as a
   repo secret (`gh secret set`) so the signing step genuinely runs; the key is
   never committed.
4. Capture REAL run output: matrix build legs, the two CycloneDX SBOM files,
   the produced bundle artifact list, and the signing step (`.sig` outputs).
5. Confirm the `release` job is `skipped` on the dispatch run (no publish).

## Out of scope (deferred — do NOT do here)

- T1 gate hardening, `justfile`, `lefthook`, SHA-pinning the OTHER workflows,
  Renovate, composite actions (G1/G2/G6/G7/G13 — other sessions).
- cosign signing + SLSA provenance (G15 — phase-2).
