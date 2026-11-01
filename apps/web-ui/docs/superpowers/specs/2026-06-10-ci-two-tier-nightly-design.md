# CI — complete-but-quick, two-tier, feature-focused — design

**Date:** 2026-06-10
**Status:** Approved (brainstorm), pending spec review → writing-plans
**Repo:** cairn-ui (frontend + Tauri desktop shell; mirrors tau-ui, engine repo is `tau`)
**Scope:** Bring cairn-ui's CI up to the `tau` engine's CI philosophy — a fast per-PR
gate plus scheduled deep checks and a self-maintaining dependency/PR flywheel —
without a time-based release train. No application code changes.

---

## 0. Decisions (locked in brainstorm)

- **Two-tier model.** The per-PR gate stays **lean and fast** (only what gates a
  merge). Everything slow or expensive moves to **scheduled** runs (nightly + weekly).
  This is how "complete but quick" is reconciled — completeness lives off the
  critical path of a feature PR.
- **Rust-type philosophy.** Mirror `tau`: fast per-PR gate → single `ci-summary`
  aggregated required check → self-maintaining flywheel (Dependabot +
  auto-update-prs + auto-rerun-flaky) → deep scheduled checks (tau's
  `fuzz-nightly` / `mutants-scheduled` analogs).
- **Feature-focused, not time-release.** No release-train / tag-on-a-schedule
  workflow. Features flow as PRs through the merge queue; "release" is merge-to-`main`.
  Scheduled runs exist for *comprehensive testing*, never for cutting releases.
- **Nightly covers:** cross-platform Tauri build, full-browser e2e, security/dependency audit.
- **Weekly covers:** mutation testing (heavier; tau runs its mutants weekly).
- **Out of scope:** ESLint flat-config migration, Prettier/pnpm config reconciliation,
  SHA-pinning third-party actions, claude.yml/claude-review.yml consolidation. These
  are config churn that does not serve the essence (tracked as deferred).

---

## 1. Current state (what already exists — keep, do not touch)

cairn-ui has already absorbed the `tau` maturity layer's per-PR pieces:

- **`.github/workflows/ci.yml`** — triggers `push`/`pull_request`/`merge_group`.
  - `web` job: `pnpm lint` (eslint via `.eslintrc.cjs`), `format:check`, `typecheck`,
    `test` (vitest), `build`.
  - `e2e` job: `playwright install --with-deps chromium`, `pnpm e2e`, report artifact on failure.
  - `tauri` job: WebKitGTK apt deps, `cargo fmt --check` / `clippy -D warnings` /
    `cargo test` against `src-tauri/Cargo.toml` (Linux only).
  - `ci-summary` job: `if: always()`, `needs: [web, e2e, tauri]`, green only when every
    job succeeded or skipped — the single check branch protection should require.
  - Concurrency: `cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}`
    (protect-`main` cache writes).
- **`.github/workflows/claude.yml`** — @claude mention handler.
- **`.github/workflows/claude-review.yml`** — AI PR review, already has
  `allowed_bots: "dependabot"`.
- **`.github/workflows/coverage.yml`** — vitest coverage, signal-not-gate.
- **`.github/dependabot.yml`** — *added in this work*; 3 ecosystems
  (github-actions `/`, npm `/web`, cargo `/src-tauri`) with grouped families.

**Tier 1 is therefore complete.** This design adds Tiers 2–4 + supporting docs.

---

## 2. Tier 2 — finish the self-maintaining flywheel (feature-focused)

The motivating asymmetry: cairn-ui already copied tau's protect-`main` concurrency
policy (its comment references "the engine repo's policy"), but **not the
`auto-update-prs` workflow that the policy was built to pair with**. It has the
shield without the threat. This tier closes that.

### 2.1 `auto-update-prs.yml` (new)

Port near-verbatim from `tau/.github/workflows/auto-update-prs.yml` — it is
engine-agnostic (pure `gh` CLI over PR metadata).

- **Triggers:** `push` to `main`, `workflow_dispatch`, `schedule: "*/30 * * * *"` (catch-net).
- **Permissions:** `contents: read`, `pull-requests: write`.
- **Concurrency:** group `auto-update-prs`, `cancel-in-progress: true`.
- **Behavior:** on push, `sleep 45` (let GitHub compute `mergeStateStatus`), then
  `gh pr list --state open --json number,mergeStateStatus,isDraft` and call
  `gh pr update-branch` on every non-draft PR whose `mergeStateStatus == "BEHIND"`.
  Swallows conflict/fork-perm errors (manual resolution required there).
- **No adaptation needed** beyond the header comment (drop tau-specific references).

### 2.2 `auto-rerun-flaky.yml` (new)

Port from `tau/.github/workflows/auto-rerun-flaky.yml`, **re-seed the flaky
allowlist for a frontend repo**.

- **Triggers:** `schedule: "*/10 * * * *"`, `workflow_dispatch` (inputs:
  `max_attempts` default 3, `window_minutes` default 90).
- **Permissions:** `actions: write`, `contents: read`.
- **Concurrency:** group `auto-rerun-flaky`, `cancel-in-progress: false`.
- **Flaky allowlist — replace tau's Rust patterns with:**
  - `"e2e"` — Playwright browser-launch / timeout transients on shared runners.
  - `"review PR"` — claude review bot transient (API rate-limit / hiccup).
  - (Explicitly **drop** `"test-stable / macos"` — no Rust workspace on the
    per-PR critical path here.)
- **Behavior unchanged:** scans recent `name == "CI"` failed runs in the window;
  only reruns failed jobs when **every** failed job matches an allowlist pattern
  and `run_attempt < max_attempts`. Conservative by construction — a non-flaky
  failure in the run blocks the rerun.

### 2.3 claude-review `allowed_bots`

Already present (`allowed_bots: "dependabot"`). No change. Confirms Dependabot
PRs get an AI review instead of a red X — the review half of the flywheel.

---

## 3. Tier 3 — `nightly.yml` (complete; off the critical path)

A single nightly workflow holding the heavy checks that must not slow a feature PR.

- **Triggers:** `schedule: "0 3 * * *"` (03:00 UTC) + `workflow_dispatch`.
- **Concurrency:** group `nightly`, `cancel-in-progress: false`.
- **Permissions:** `contents: read` (+ `issues: write` only if the optional
  issue-on-failure step is included).
- **Jobs (parallel, each cached):**

### 3.1 `tauri-cross` — cross-platform desktop build

- `strategy.matrix.os: [ubuntu-latest, macos-latest, windows-latest]`,
  `fail-fast: false`.
- Linux only: install WebKitGTK build deps (reuse the apt block from `ci.yml`'s
  `tauri` job: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
  librsvg2-dev build-essential cmake`).
- `dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache@v2`
  (`workspaces: src-tauri`).
- pnpm/node setup → `pnpm install --frozen-lockfile` → **`pnpm build`**
  (produces `web/dist`, required because `tauri.conf.json` sets
  `frontendDist: ../web/dist` and the Rust shell embeds it at compile time).
- `cargo build --manifest-path src-tauri/Cargo.toml`.
- **Rationale:** compile-verify on all three OSes catches platform-specific
  breakage the Linux-only PR job cannot. Full `tauri build` bundling (installers,
  signing) is a deliberate future extension — compile-verify is the high-value,
  low-config gate.

### 3.2 `e2e-full-browser` — cross-browser Playwright

- `strategy.matrix.browser: [chromium, firefox, webkit]`, `fail-fast: false`.
- `defaults.run.working-directory: web`.
- pnpm/node setup → `pnpm install --frozen-lockfile` →
  `pnpm exec playwright install --with-deps ${{ matrix.browser }}` →
  `pnpm exec playwright test --browser=${{ matrix.browser }}`.
- **Note:** `web/playwright.config.ts` defines **no per-browser `projects`** (single
  default), so the `--browser` flag is the correct selector (it would be ignored if
  projects existed). Upload report artifact on failure (same pattern as `ci.yml`).

### 3.3 `audit` — security / dependency audit

- Web: `pnpm audit --audit-level=high` in `web/`.
- Tauri: `cargo audit` against `src-tauri/` (install via
  `taiki-e/install-action@v2` with `tool: cargo-audit`).
- Fails on high/critical advisories. Because it is nightly, a failure is a
  **signal**, not a merge gate — it never blocks a feature PR.

### 3.4 `nightly-summary`

- `if: always()`, `needs: [tauri-cross, e2e-full-browser, audit]`.
- Aggregates the matrix/job results into `$GITHUB_STEP_SUMMARY`; fails if any
  needed job failed (so the workflow's top-level status is honest).
- **Optional:** open/update a tracking issue on failure (label `ci-nightly`) so a
  red nightly is visible without watching the Actions tab. Decision deferred to the
  plan; the summary job is the baseline.

---

## 4. Tier 4 — `mutation-weekly.yml` (test-suite strength)

The truest analog of tau's `mutants-scheduled`. Mutation testing measures whether
the test suite actually *catches* injected faults — strength, not just coverage.

- **Triggers:** `schedule: "0 4 * * 1"` (Monday 04:00 UTC) + `workflow_dispatch`.
- **Concurrency:** group `mutation-weekly`, `cancel-in-progress: false`.
- **Tooling:** Stryker — dev deps `@stryker-mutator/core` +
  `@stryker-mutator/vitest-runner`; config `web/stryker.config.json`
  (`testRunner: "vitest"`, sensible `mutate` globs excluding `src/types/**`,
  `**/*.test.*`, e2e).
- **Job:** pnpm/node setup → install → `pnpm exec stryker run`; report the mutation
  score to `$GITHUB_STEP_SUMMARY` and upload the HTML report artifact.
- **Non-gating, weekly** — heavy; kept off both the PR path and the nightly path.

---

## 5. Supporting changes

- **README CI badge** — add the `ci.yml` status badge near the top of `README.md`.
  (Nightly/weekly badges optional.)
- **`docs/ci.md`** — document the two-tier model: Tier 1 (per-PR fast gate, the only
  required check) vs Tiers 3–4 (scheduled deep checks, never required), and the
  flywheel (Dependabot → claude-review → auto-update-prs → auto-rerun-flaky).
- **Branch protection** — document (and optionally apply via `gh api`) requiring
  **only `ci-summary`** on `main`. Nightly/weekly are intentionally **not** required
  so they never block a feature merge. One-time repo-settings action, noted as
  manual/optional in the plan.

---

## 6. Acceptance criteria

1. **Per-PR speed preserved.** `ci.yml` is unchanged; a feature PR runs only
   `web`/`e2e`/`tauri` + `ci-summary`. No cross-platform or full-browser work on the
   PR path.
2. **Flywheel closed.** A push to `main` triggers `auto-update-prs`, which rebases
   every behind, non-draft PR. A CI run whose only failures are allowlisted-flaky is
   auto-rerun up to `max_attempts`; a run with any non-flaky failure is left red.
3. **Nightly is complete.** `nightly.yml` builds the Tauri shell on Linux + macOS +
   Windows, runs Playwright on Chromium + Firefox + WebKit, and runs `pnpm audit` +
   `cargo audit`; `nightly-summary` reflects the true aggregate status.
4. **Weekly mutation runs.** `mutation-weekly.yml` produces a Stryker mutation score
   and HTML report artifact; it is non-gating.
5. **Docs + protection.** README shows the CI badge; `docs/ci.md` explains the two
   tiers; branch protection (if applied) requires only `ci-summary`.
6. **No app code changed.** Only `.github/`, `web/package.json` (+ Stryker config),
   `README.md`, and `docs/` are touched.

---

## 7. Non-goals (YAGNI)

- No time-based release / tag-train workflow.
- No full `tauri build` bundling, code-signing, or artifact publishing in nightly
  (compile-verify only).
- No ESLint flat-config migration, Prettier/pnpm reconciliation, SHA-pinning, or
  claude workflow consolidation (deferred config hygiene, tracked separately).
- No making nightly/weekly checks required on `main`.

---

## 8. Suggested decomposition (one spec, phased plan)

Each sub-project is independent and small:

- **SP-1 — Flywheel:** `auto-update-prs.yml` + `auto-rerun-flaky.yml`
  (Dependabot already shipped).
- **SP-2 — Nightly:** `nightly.yml` (`tauri-cross` + `e2e-full-browser` + `audit` +
  `nightly-summary`).
- **SP-3 — Weekly mutation:** `mutation-weekly.yml` + `web/stryker.config.json` +
  dev deps.
- **SP-4 — Docs & protection:** README badge + `docs/ci.md` + branch-protection note.

## File-change summary

- **New workflows:** `.github/workflows/auto-update-prs.yml`,
  `.github/workflows/auto-rerun-flaky.yml`, `.github/workflows/nightly.yml`,
  `.github/workflows/mutation-weekly.yml`.
- **New config:** `web/stryker.config.json`.
- **Modified:** `web/package.json` (Stryker dev deps), `README.md` (badge).
- **New docs:** `docs/ci.md`.
- **Already shipped this work:** `.github/dependabot.yml`.
- **Untouched:** `ci.yml`, `claude.yml`, `claude-review.yml`, `coverage.yml`.
