# CI

cairn-web-ui runs a **two-tier** pipeline: a lean per-PR gate (fast feedback,
the only required check) and scheduled deep checks (comprehensive, never
gating). Features flow as PRs through the merge queue — there is no
time-based release train.

## Tier 1 — per-PR gate (`ci.yml`)
Runs on every push to `main`, every PR, and `merge_group`. Jobs:
- **web** — `just web-ci`: ESLint, Prettier check, `tsc --noEmit`, Vitest, `vite build`.
- **e2e** — Playwright (Chromium), report artifact on failure.
- **tauri** — `just rust-ci`: `cargo fmt --check`, `clippy -D warnings`, `cargo test` (Linux).
- **ci-summary** — aggregates the above; green only when every job
  succeeded or skipped. **This is the single check to require on `main`.**

The `web` and `tauri` jobs invoke the root [`justfile`](../justfile) rather than
inlining the pnpm/cargo commands, so the per-PR gate and a local `just ci` can
never drift apart. The same fast verbs run pre-commit via `lefthook` — see
the README "Local development" section.

Concurrency cancels superseded runs except on `main` (so the cache write
that warms PR builds is never cancelled mid-write).

## The self-maintaining flywheel
- **`dependabot.yml`** — weekly bumps for GitHub Actions, npm (`web/`), and
  Cargo (`src-tauri/`), grouped by lockstep family.
- **`auto-update-prs.yml`** — updates every behind, non-draft PR (merges `main`
  in via GitHub's "Update branch") when `main` advances, so strict branch
  protection never strands a PR. Behind-ness is measured with the compare API
  (`behind_by`), **not** `mergeStateStatus == "BEHIND"`: with a merge queue
  enabled GitHub reports a behind PR as `CLEAN`, which silently no-opped this
  workflow and stranded five Dependabot PRs for three days in 2026-08.
- **`auto-rerun-flaky.yml`** — reruns a CI run only when *every* failed job
  matches the flaky allowlist (`e2e`); a real failure stays red.

## Tier 2 — release heavy tier (`heavy.yml`)
Triggered on a `v*` tag push (a feature release) and on demand
(`workflow_dispatch`). This is where the heavy lifting for a release happens —
it is **never** a PR/merge gate. Jobs:
- **build-bundle** — cross-OS (Linux/macOS/Windows) `tauri build`, producing
  the app installers (`bundle.active: true, targets: "all"` → deb/AppImage/rpm,
  dmg/app, msi/exe). The frontend is pre-built in `web/`;
  `src-tauri/tauri.ci.conf.json` nulls the config's `beforeBuildCommand` so the
  bundler does not re-run it.
- **sbom** — CycloneDX SBOMs for **both** stacks: `@cyclonedx/cdxgen` over the
  pnpm graph (`web/`) and `cargo-cyclonedx` over the cargo graph (`src-tauri/`).
- **e2e / mutation / coverage** — full-browser Playwright (Chromium/Firefox/
  WebKit), Stryker, and vitest coverage, re-run at release time.
- **release** — *tag-only* (`startsWith(github.ref, 'refs/tags/v')`); publishes
  a draft GitHub Release with the bundles + SBOMs attached. A
  `workflow_dispatch` run has no tag, so this job is skipped — dispatch is an
  inherent **no-publish dry run** that still exercises build + SBOM + signing.
- **heavy-summary** — aggregates results as a **warning** (never `exit 1`), so
  the tier can never be made a required check.

### Signing & OIDC posture
- The GitHub Release publish uses the ephemeral, job-scoped `GITHUB_TOKEN` — no
  long-lived secret, no OIDC wiring. Only the tag-gated `release` job is granted
  `contents: write`; every other job stays `contents: read`.
- The **only** long-lived secrets are the Tauri updater signing key
  (`TAURI_SIGNING_PRIVATE_KEY`) and its password
  (`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), referenced from repo secrets and never
  committed. They are wired into `build-bundle` but currently **inert**: tauri
  signs the *updater artifacts* (`.sig`), which are only emitted when the app
  configures the updater. This app has no `plugins.updater` yet, so the build
  produces the installers but no signatures. Wiring the updater
  (`tauri-plugin-updater` + a `plugins.updater.pubkey` entry in the committed
  config) is a deferred app-side follow-up; once it lands, signing activates
  with **zero** workflow change — just populate the two secrets. Tauri's
  minisign signing has no OIDC/keyless path today, so a stored secret is
  currently unavoidable. **Migration path:** keyless signing
  (cosign / Sigstore) or an OIDC-federated KMS signer once available; cosign +
  SLSA provenance are tracked as phase-2. No cloud-registry auth exists yet, so
  no `id-token: write` OIDC wiring is added now.

## Tier 3 — scheduled drift-catchers (never required)
- **`nightly.yml`** (`0 3 * * *`) — cross-platform Tauri compile
  (Linux/macOS/Windows), full-browser Playwright (Chromium/Firefox/WebKit),
  and `pnpm audit` + `cargo audit`.
- **`mutation-weekly.yml`** (`0 4 * * 1`) — Stryker mutation testing of `web/`.

A red nightly/weekly/heavy run is a signal to investigate, not a merge blocker.

## Make the gate required on `main`
Require **only** `ci-summary` (the scheduled workflows must stay off the PR
path). In the UI: Settings → Branches → rule for `main` → require status check
`ci-summary`. Or via `gh` (run after at least one CI run exists so the context
is known):

```bash
gh api -X PUT repos/tau-rs/cairn-web-ui/branches/main/protection \
  -f 'required_status_checks[strict]=true' \
  -f 'required_status_checks[checks][][context]=ci-summary' \
  -F 'enforce_admins=true' \
  -F 'required_pull_request_reviews=' \
  -F 'restrictions='
```
