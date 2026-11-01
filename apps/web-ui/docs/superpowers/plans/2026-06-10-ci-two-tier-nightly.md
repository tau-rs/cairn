# Two-Tier CI (nightly + flywheel) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring cairn-ui's CI to the `tau` engine's philosophy — a lean per-PR gate (unchanged) plus a self-maintaining dependency/PR flywheel, a nightly comprehensive run, and weekly mutation testing — without a time-based release train.

**Architecture:** The fast `ci.yml` gate is left untouched. We add four scheduled/event-driven workflows (`auto-update-prs`, `auto-rerun-flaky`, `nightly`, `mutation-weekly`), the already-built `dependabot.yml`, a Stryker config, a README badge, and `docs/ci.md`. No application code changes. Each workflow is one self-contained file with one responsibility; "tests" for CI files are YAML validation + a real dispatch/observed run.

**Tech Stack:** GitHub Actions, `gh` CLI, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `pnpm/action-setup` (pnpm 10.14.0), `actions/setup-node` (Node 20), Playwright (chromium/firefox/webkit), `cargo-audit`, Stryker Mutator (`@stryker-mutator/core` + vitest-runner).

**Source spec:** `docs/superpowers/specs/2026-06-10-ci-two-tier-nightly-design.md`.

**Branch:** `ci-two-tier-nightly` (already created; the design commit lives here).

---

## File structure

```
.github/dependabot.yml                    # 3 ecosystems (DONE — in working tree, commit in Task 1)
.github/workflows/auto-update-prs.yml     # SP-1: rebase behind PRs when main advances
.github/workflows/auto-rerun-flaky.yml    # SP-1: retry runs whose only failures are allowlisted-flaky
.github/workflows/nightly.yml             # SP-2: cross-platform tauri build + full-browser e2e + audit
.github/workflows/mutation-weekly.yml     # SP-3: weekly Stryker mutation run
web/stryker.config.json                   # SP-3: Stryker config (vitest runner)
web/package.json                          # SP-3: + Stryker dev deps
README.md                                 # SP-4: + CI badge
docs/ci.md                                # SP-4: two-tier model + flywheel docs
```

Responsibility boundary: each workflow file orchestrates one concern; no workflow mutates committed files. `ci.yml`, `claude.yml`, `claude-review.yml`, `coverage.yml` are **not** touched.

**Note on "tests":** GitHub Actions workflows can't be unit-tested. The verification pattern throughout is: (1) `python3 -c "import yaml; yaml.safe_load(open(<file>))"` to prove the YAML parses, and (2) for runnable workflows, a `gh workflow run` dispatch + `gh run watch` to prove it executes. Both are real, observable checks — not placeholders.

---

## SP-1 — The self-maintaining flywheel

### Task 1: Commit the Dependabot config

**Files:**
- Commit (already created in working tree): `.github/dependabot.yml`

- [ ] **Step 1: Confirm the file exists and parses**

Run:
```bash
cd /Users/titouanlebocq/code/cairn-ui
test -f .github/dependabot.yml && echo "present"
python3 -c "import yaml; d=yaml.safe_load(open('.github/dependabot.yml')); print('ecosystems:', [u['package-ecosystem']+' @ '+u['directory'] for u in d['updates']])"
```
Expected: `present` then `ecosystems: ['github-actions @ /', 'npm @ /web', 'cargo @ /src-tauri']`.

- [ ] **Step 2: Commit**

```bash
git add .github/dependabot.yml
git commit -m "ci: add Dependabot config (github-actions, npm/web, cargo/src-tauri)"
```

---

### Task 2: `auto-update-prs.yml` — rebase behind PRs when main advances

**Files:**
- Create: `.github/workflows/auto-update-prs.yml`

- [ ] **Step 1: Create the workflow**

Create `.github/workflows/auto-update-prs.yml`:

```yaml
name: Auto-update PR branches

# When a commit lands on main, walks every open non-draft PR and
# calls "Update branch" on any that are BEHIND base. GitHub's
# update-branch operation merges main into the PR branch via a
# merge commit; on a clean merge it succeeds (and triggers one
# fresh CI run on the updated PR). On conflict it returns an error
# which this workflow swallows — manual resolution is required.
#
# Why this exists
# ---------------
# Bots (Dependabot) + human + Claude sessions routinely push to main
# while PRs are open. Branch protection is strict, so every PR that
# falls behind needs updating before it can merge. This removes that
# manual friction, and is the workflow the protect-main concurrency
# policy in ci.yml was built to pair with.
#
# Triggers
# --------
# - push to main: the moment main advances, update everyone.
# - workflow_dispatch: manual catch-up.
# - schedule (every 30 min): catch-net if a push trigger didn't fire.
#
# Skips: draft PRs, closed PRs, DIRTY (conflict) PRs, and fork PRs
# (GITHUB_TOKEN can't push to a fork branch — the error is swallowed).
#
# Security: only reads PR metadata and triggers GitHub's own
# "Update branch" API. PR numbers are integers from the API (no
# injection surface); repo name comes from github.repository.

on:
  push:
    branches: [main]
  workflow_dispatch: {}
  schedule:
    - cron: '*/30 * * * *'

concurrency:
  group: auto-update-prs
  cancel-in-progress: true

permissions:
  contents: read
  pull-requests: write

jobs:
  update:
    name: update behind PRs
    runs-on: ubuntu-latest
    steps:
      - name: Update each behind PR
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
          EVENT: ${{ github.event_name }}
        run: |
          set -euo pipefail

          # mergeStateStatus is computed asynchronously by GitHub after
          # a push lands. Wait briefly so BEHIND is accurate.
          if [[ "$EVENT" == "push" ]]; then
            sleep 45
          fi

          mapfile -t prs < <(
            gh pr list --repo "$REPO" --state open --limit 100 \
              --json number,mergeStateStatus,isDraft \
              --jq '.[] | select(.isDraft == false and .mergeStateStatus == "BEHIND") | .number'
          )

          if [[ ${#prs[@]} -eq 0 ]]; then
            echo "No behind PRs to update."
            exit 0
          fi

          echo "Will attempt to update ${#prs[@]} PR(s): ${prs[*]}"
          updated=0
          skipped=0
          for pr in "${prs[@]}"; do
            echo "::group::PR #$pr"
            if gh pr update-branch "$pr" --repo "$REPO" 2>&1; then
              updated=$((updated + 1))
              echo "Updated."
            else
              skipped=$((skipped + 1))
              echo "Skipped (conflict, fork-branch perms, or race)."
            fi
            echo "::endgroup::"
          done

          echo ""
          echo "Done. updated=$updated skipped=$skipped"
```

- [ ] **Step 2: Validate the YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/auto-update-prs.yml')); print('yaml ok')"
```
Expected: `yaml ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/auto-update-prs.yml
git commit -m "ci: auto-update behind PR branches when main advances"
```

---

### Task 3: `auto-rerun-flaky.yml` — retry allowlisted-flaky CI runs

**Files:**
- Create: `.github/workflows/auto-rerun-flaky.yml`

- [ ] **Step 1: Create the workflow**

Create `.github/workflows/auto-rerun-flaky.yml`. Note the `flaky_patterns` are
re-seeded for this frontend/Tauri repo (Playwright `e2e` transients + the claude
`review PR` bot) — tau's Rust/macOS cargo-lock pattern is intentionally dropped.

```yaml
name: Auto-rerun flaky failures

# Detects CI runs whose ONLY failed jobs match a known-flaky pattern
# and auto-reruns the failed jobs up to a bounded attempt count. Keeps
# the rerun loop short for known-noisy CI signals.
#
# Why cron and not workflow_run?
# workflow_run fires reliably only for default-branch history, not for
# feature-branch PR pushes (documented GitHub Actions quirk). A 10-min
# cron scan covers all branches uniformly.

on:
  schedule:
    - cron: "*/10 * * * *"
  workflow_dispatch:
    inputs:
      max_attempts:
        description: "Max workflow run attempts before giving up (default 3)"
        type: string
        required: false
        default: "3"
      window_minutes:
        description: "Look-back window in minutes (default 90)"
        type: string
        required: false
        default: "90"

permissions:
  actions: write
  contents: read

concurrency:
  group: auto-rerun-flaky
  cancel-in-progress: false

jobs:
  scan-and-rerun:
    name: Scan recent CI runs and rerun if flaky
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - name: Find failed CI runs in the look-back window
        env:
          GH_TOKEN: ${{ github.token }}
          REPO: ${{ github.repository }}
          MAX_ATTEMPTS: ${{ inputs.max_attempts || '3' }}
          WINDOW_MIN: ${{ inputs.window_minutes || '90' }}
        run: |
          set -euo pipefail

          # Known-flaky job names. A run is eligible for auto-rerun ONLY
          # if EVERY failed job matches one of these (fixed-string
          # substring match). Be conservative — a non-flaky pattern here
          # would mask real failures by auto-retrying them.
          flaky_patterns=(
            "e2e"        # Playwright browser-launch / timeout transients
            "review PR"  # Claude review bot transient (rate limit, API hiccup)
          )

          cutoff=$(date -u -d "$WINDOW_MIN minutes ago" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || \
                   date -u -v-"${WINDOW_MIN}"M +"%Y-%m-%dT%H:%M:%SZ")
          echo "Scanning CI runs since $cutoff (max_attempts=$MAX_ATTEMPTS)"

          mapfile -t failed_run_ids < <(
            gh api "repos/$REPO/actions/runs?status=failure&per_page=30" \
              --jq --arg cutoff "$cutoff" \
              '.workflow_runs[] | select(.name == "CI" and .created_at > $cutoff) | .id'
          )

          if [ "${#failed_run_ids[@]}" -eq 0 ]; then
            echo "No failed CI runs in window — nothing to do."
            exit 0
          fi

          echo "Failed CI runs in window: ${failed_run_ids[*]}"

          for run_id in "${failed_run_ids[@]}"; do
            echo "::group::Evaluating run $run_id"

            run_info=$(gh api "repos/$REPO/actions/runs/$run_id" \
              --jq '{attempt: .run_attempt, head_sha, head_branch, event}')
            attempt=$(echo "$run_info" | jq -r '.attempt')
            sha=$(echo "$run_info" | jq -r '.head_sha')
            branch=$(echo "$run_info" | jq -r '.head_branch')

            echo "attempt=$attempt sha=${sha:0:7} branch=$branch"

            if [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
              echo "Already at attempt $attempt (>= max $MAX_ATTEMPTS) — skipping."
              echo "::endgroup::"
              continue
            fi

            mapfile -t failed_jobs < <(
              gh api "repos/$REPO/actions/runs/$run_id/jobs?filter=latest" \
                --jq '.jobs[] | select(.conclusion == "failure") | .name'
            )

            if [ "${#failed_jobs[@]}" -eq 0 ]; then
              echo "No failed jobs on most recent attempt — skipping."
              echo "::endgroup::"
              continue
            fi

            echo "Failed jobs: ${failed_jobs[*]}"

            all_flaky=true
            for job in "${failed_jobs[@]}"; do
              matched=false
              for pattern in "${flaky_patterns[@]}"; do
                case "$job" in
                  *"$pattern"*) matched=true; break ;;
                esac
              done
              if [ "$matched" = "false" ]; then
                echo "Job '$job' is NOT in flaky list — will not rerun."
                all_flaky=false
                break
              fi
            done

            if [ "$all_flaky" = "true" ]; then
              echo "All failed jobs are flaky — rerunning (attempt $((attempt + 1)) of $MAX_ATTEMPTS)"
              gh api -X POST "repos/$REPO/actions/runs/$run_id/rerun-failed-jobs" \
                || echo "::warning::rerun API call failed (run no longer rerunnable)"
            fi
            echo "::endgroup::"
          done
```

- [ ] **Step 2: Validate the YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/auto-rerun-flaky.yml')); print('yaml ok')"
```
Expected: `yaml ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/auto-rerun-flaky.yml
git commit -m "ci: auto-rerun CI runs whose only failures are allowlisted-flaky (e2e, review)"
```

---

## SP-2 — Nightly comprehensive run

### Task 4: `nightly.yml` — cross-platform build + full-browser e2e + audit

**Files:**
- Create: `.github/workflows/nightly.yml`

- [ ] **Step 1: Create the workflow**

Create `.github/workflows/nightly.yml`:

```yaml
name: Nightly

# Comprehensive checks kept OFF the per-PR critical path so feature PRs
# stay fast. Runs nightly + on demand. Nothing here is a required check;
# a red nightly is a signal, never a merge gate.

on:
  schedule:
    - cron: "0 3 * * *"
  workflow_dispatch: {}

concurrency:
  group: nightly
  cancel-in-progress: false

permissions:
  contents: read

jobs:
  tauri-cross:
    name: tauri-cross (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Install WebKitGTK + build deps (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
            libayatana-appindicator3-dev librsvg2-dev build-essential cmake
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - uses: pnpm/action-setup@v4
        with:
          version: 10.14.0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - name: Build frontend (produces web/dist embedded by the Rust shell)
        working-directory: web
        run: |
          pnpm install --frozen-lockfile
          pnpm build
      - name: Compile the Tauri shell
        run: cargo build --manifest-path src-tauri/Cargo.toml

  e2e-full-browser:
    name: e2e (${{ matrix.browser }})
    strategy:
      fail-fast: false
      matrix:
        browser: [chromium, firefox, webkit]
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 10.14.0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm exec playwright install --with-deps ${{ matrix.browser }}
      - run: pnpm exec playwright test --browser=${{ matrix.browser }}
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: playwright-report-${{ matrix.browser }}
          path: |
            web/playwright-report/
            web/test-results/
          retention-days: 7

  audit:
    name: audit (npm + cargo)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 10.14.0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - name: pnpm audit (web)
        working-directory: web
        run: pnpm audit --audit-level=high
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-audit
      - name: cargo audit (src-tauri)
        run: cargo audit --file src-tauri/Cargo.lock

  nightly-summary:
    name: nightly-summary
    if: always()
    needs: [tauri-cross, e2e-full-browser, audit]
    runs-on: ubuntu-latest
    steps:
      - name: Aggregate results
        run: |
          results='${{ join(needs.*.result, ' ') }}'
          echo "## Nightly results" >> "$GITHUB_STEP_SUMMARY"
          echo "" >> "$GITHUB_STEP_SUMMARY"
          echo "Job results: $results" >> "$GITHUB_STEP_SUMMARY"
          for r in $results; do
            case "$r" in
              success|skipped) ;;
              *) echo "::error::a nightly job did not succeed ($r)"; exit 1 ;;
            esac
          done
```

> Note: `nightly-summary` makes the workflow's top-level status honest (red if any matrix leg failed) without making nightly a *required* check on `main`. An optional issue-on-failure step is left out by decision (the spec marks it optional); add it later if a red nightly proves too easy to miss.

- [ ] **Step 2: Validate the YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/nightly.yml')); print('yaml ok')"
```
Expected: `yaml ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/nightly.yml
git commit -m "ci: nightly run (cross-platform tauri build + full-browser e2e + audit)"
```

- [ ] **Step 4: Dispatch a real run once the branch is pushed (after Task 8)**

After the branch is pushed and a PR exists, trigger nightly manually and watch it:
```bash
gh workflow run nightly.yml --ref ci-two-tier-nightly
sleep 10
gh run list --workflow=nightly.yml --limit 1
gh run watch "$(gh run list --workflow=nightly.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```
Expected: all four jobs resolve. If `audit` fails on a pre-existing advisory, that is a real finding — record it; do not silence the job. If `tauri-cross` fails on macOS/Windows, read the log: a missing-`web/dist` error means the `pnpm build` step did not run before `cargo build` (it must precede it — verify step order). If `e2e (webkit)` fails to launch, confirm `playwright install --with-deps webkit` completed.

---

## SP-3 — Weekly mutation testing

### Task 5: Stryker config + dev deps + `mutation-weekly.yml`

**Files:**
- Modify: `web/package.json`
- Create: `web/stryker.config.json`
- Create: `.github/workflows/mutation-weekly.yml`

- [ ] **Step 1: Add Stryker dev dependencies**

Run (in `web/`):
```bash
cd /Users/titouanlebocq/code/cairn-ui/web
pnpm add -D @stryker-mutator/core @stryker-mutator/vitest-runner
```
Expected: both packages added to `devDependencies`; `pnpm-lock.yaml` updated.

- [ ] **Step 2: Create the Stryker config**

Create `web/stryker.config.json`:
```json
{
  "$schema": "./node_modules/@stryker-mutator/core/schema/stryker-schema.json",
  "testRunner": "vitest",
  "reporters": ["html", "clear-text", "progress"],
  "coverageAnalysis": "perTest",
  "mutate": [
    "src/**/*.{ts,tsx}",
    "!src/**/*.test.{ts,tsx}",
    "!src/test-setup.ts",
    "!src/types/**",
    "!src/**/*.d.ts"
  ],
  "htmlReporter": { "fileName": "reports/mutation/index.html" }
}
```

- [ ] **Step 3: Verify Stryker runs locally (smoke, may be slow)**

Run (in `web/`):
```bash
pnpm exec stryker run 2>&1 | tail -20
```
Expected: Stryker boots the vitest runner, mutates `src/**`, and prints a mutation score line (e.g. `Mutation score: NN.NN%`). If it errors that the vitest runner can't find the config, confirm `web/vitest.config.ts` (or vite config `test` block) exists — Stryker's vitest-runner auto-discovers it. A low score is acceptable (this is a baseline, non-gating); a *crash* is a config bug to fix before committing.

- [ ] **Step 4: Create the weekly workflow**

Create `.github/workflows/mutation-weekly.yml`:
```yaml
name: Mutation (weekly)

# Stryker mutation testing — measures test-suite STRENGTH (does it catch
# injected faults?), not just coverage. Heavy, so it runs weekly and is
# non-gating: the analog of tau's mutants-scheduled.

on:
  schedule:
    - cron: "0 4 * * 1"
  workflow_dispatch: {}

concurrency:
  group: mutation-weekly
  cancel-in-progress: false

permissions:
  contents: read

jobs:
  mutation:
    name: stryker (web)
    runs-on: ubuntu-latest
    timeout-minutes: 60
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 10.14.0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - name: Run Stryker
        run: pnpm exec stryker run
      - name: Surface the mutation report
        if: always()
        run: |
          echo "## Mutation report" >> "$GITHUB_STEP_SUMMARY"
          echo "HTML report uploaded as an artifact (reports/mutation/)." >> "$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: mutation-report
          path: web/reports/mutation/
          retention-days: 14
```

- [ ] **Step 5: Validate the YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/mutation-weekly.yml')); print('yaml ok')"
```
Expected: `yaml ok`.

- [ ] **Step 6: Ignore Stryker output dirs**

Append to `web/.gitignore` (create the lines if absent):
```
.stryker-tmp
reports
```
Run:
```bash
cd /Users/titouanlebocq/code/cairn-ui/web
printf '\n.stryker-tmp\nreports\n' >> .gitignore
```

- [ ] **Step 7: Commit**

```bash
cd /Users/titouanlebocq/code/cairn-ui
git add web/package.json web/pnpm-lock.yaml web/stryker.config.json web/.gitignore .github/workflows/mutation-weekly.yml
git commit -m "ci: weekly Stryker mutation testing (web)"
```

---

## SP-4 — Docs, badge, branch protection

### Task 6: README badge + `docs/ci.md`

**Files:**
- Modify: `README.md`
- Create: `docs/ci.md`

- [ ] **Step 1: Add the CI badge to the README**

Read the top of `README.md` to find the H1, then insert the badge directly under it. Use:
```markdown
[![CI](https://github.com/tau-rs/cairn-ui/actions/workflows/ci.yml/badge.svg)](https://github.com/tau-rs/cairn-ui/actions/workflows/ci.yml)
```
> Confirm the `owner/repo` slug first: `gh repo view --json nameWithOwner --jq .nameWithOwner`. If it differs from `tau-rs/cairn-ui`, substitute the actual slug in the badge URL before saving.

- [ ] **Step 2: Create `docs/ci.md`**

Create `docs/ci.md`:
```markdown
# CI

cairn-ui runs a **two-tier** pipeline: a lean per-PR gate (fast feedback,
the only required check) and scheduled deep checks (comprehensive, never
gating). Features flow as PRs through the merge queue — there is no
time-based release train.

## Tier 1 — per-PR gate (`ci.yml`)
Runs on every push to `main`, every PR, and `merge_group`. Jobs:
- **web** — ESLint, Prettier check, `tsc --noEmit`, Vitest, `vite build`.
- **e2e** — Playwright (Chromium), report artifact on failure.
- **tauri** — `cargo fmt --check`, `clippy -D warnings`, `cargo test` (Linux).
- **ci-summary** — aggregates the above; green only when every job
  succeeded or skipped. **This is the single check to require on `main`.**

Concurrency cancels superseded runs except on `main` (so the cache write
that warms PR builds is never cancelled mid-write).

## The self-maintaining flywheel
- **`dependabot.yml`** — weekly bumps for GitHub Actions, npm (`web/`), and
  Cargo (`src-tauri/`), grouped by lockstep family.
- **`claude-review.yml`** — AI review of every PR, including Dependabot's
  (`allowed_bots: dependabot`).
- **`auto-update-prs.yml`** — rebases every behind, non-draft PR when `main`
  advances, so strict branch protection never strands a PR.
- **`auto-rerun-flaky.yml`** — reruns a CI run only when *every* failed job
  matches the flaky allowlist (`e2e`, `review PR`); a real failure stays red.

## Tier 2 — scheduled deep checks (never required)
- **`nightly.yml`** (`0 3 * * *`) — cross-platform Tauri compile
  (Linux/macOS/Windows), full-browser Playwright (Chromium/Firefox/WebKit),
  and `pnpm audit` + `cargo audit`.
- **`mutation-weekly.yml`** (`0 4 * * 1`) — Stryker mutation testing of `web/`.

A red nightly/weekly is a signal to investigate, not a merge blocker.

## Make the gate required on `main`
Require **only** `ci-summary` (the scheduled workflows must stay off the PR
path). In the UI: Settings → Branches → rule for `main` → require status check
`ci-summary`. Or via `gh` (run after at least one CI run exists so the context
is known):

\`\`\`bash
gh api -X PUT repos/<owner>/cairn-ui/branches/main/protection \
  -f 'required_status_checks[strict]=true' \
  -f 'required_status_checks[checks][][context]=ci-summary' \
  -F 'enforce_admins=true' \
  -F 'required_pull_request_reviews=' \
  -F 'restrictions='
\`\`\`
```

- [ ] **Step 3: Commit**

```bash
cd /Users/titouanlebocq/code/cairn-ui
git add README.md docs/ci.md
git commit -m "docs(ci): CI badge + two-tier model + branch-protection note"
```

---

### Task 7: Push the branch and open a PR

**Files:** none (git/gh only)

- [ ] **Step 1: Push and open the PR**

```bash
cd /Users/titouanlebocq/code/cairn-ui
git push -u origin ci-two-tier-nightly
gh pr create --fill --title "ci: two-tier CI (flywheel + nightly + weekly mutation)" \
  --body "Implements docs/superpowers/specs/2026-06-10-ci-two-tier-nightly-design.md. Per-PR gate unchanged; adds Dependabot, auto-update-prs, auto-rerun-flaky, nightly (cross-platform build + full-browser e2e + audit), and weekly Stryker mutation."
```

- [ ] **Step 2: Confirm the per-PR gate is still green and unchanged**

```bash
gh pr checks --watch
```
Expected: `web`, `e2e`, `tauri`, `ci-summary` run and pass — the same four as before this PR (the new workflows do not run on the PR event, proving the fast gate was not slowed).

- [ ] **Step 3: Run the nightly dispatch verification from Task 4 Step 4**

(Now that the branch is pushed, execute Task 4 Step 4 to prove `nightly.yml` actually runs end-to-end on real runners.)

---

## Self-Review

**1. Spec coverage:**
- §1 current state untouched → no task modifies `ci.yml`/`claude*.yml`/`coverage.yml`. ✓
- §2 flywheel: dependabot → Task 1; auto-update-prs → Task 2; auto-rerun-flaky (re-seeded allowlist) → Task 3; allowed_bots already present → noted in `docs/ci.md` (Task 6). ✓
- §3 nightly: tauri-cross + e2e-full-browser + audit + nightly-summary → Task 4. ✓
- §4 weekly mutation: Stryker config + deps + workflow → Task 5. ✓
- §5 supporting: README badge + docs/ci.md + branch-protection note → Task 6. ✓
- §6 acceptance: per-PR speed preserved (Task 7 Step 2); flywheel closed (Tasks 2-3); nightly complete (Task 4 + dispatch verify); weekly runs (Task 5); docs+protection (Task 6); no app code changed (file list is `.github/`, `web/package.json`+config, `README.md`, `docs/`). ✓
- §8 decomposition SP-1..SP-4 → Tasks map 1-3 (SP-1), 4 (SP-2), 5 (SP-3), 6 (SP-4), + 7 ship. ✓

**2. Placeholder scan:** every workflow/config is given in full; no TBD/TODO/"handle errors". The `<owner>` in the badge/protection commands is resolved by an explicit `gh repo view` step, not left vague. ✓

**3. Type/identifier consistency:** workflow file names match between the file-structure map, the tasks, and `docs/ci.md`. Job name `ci-summary` is the same string in `docs/ci.md` and the branch-protection context. pnpm version `10.14.0` and Node `20` match `ci.yml` across all new jobs. The nightly `--browser` flag is correct given `playwright.config.ts` defines no `projects` (per the spec's verified note). `nightly-summary` reuses the exact `join(needs.*.result, ' ')` pattern already in `ci.yml`'s `ci-summary`. ✓

**4. Gaps:** `cargo audit --file src-tauri/Cargo.lock` audits the Tauri lockfile from repo root (no `cd` needed). `pnpm audit --audit-level=high` may fail on a transitive advisory — that is intended nightly signal, called out in Task 4 Step 4. Stryker output dirs are git-ignored (Task 5 Step 6) so no report artifacts get committed. ✓
