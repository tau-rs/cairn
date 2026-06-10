# CI Automation + First CI ADR — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two GitHub Actions automation workflows (auto-update behind PRs; bot-only flaky-rerun) and cairn's first CI architecture decision record (ADR-0009), adopting tau's CI *philosophy* while honoring cairn's deliberate `retries=0` "surface flakes" stance.

**Architecture:** Two standalone workflow files under `.github/workflows/`, each independently revertable and neither wired into the `ci-summary` merge gate. `auto-update-prs.yml` ports tau's version ~verbatim. `auto-rerun-flaky.yml` is a *corrected, narrowed* adaptation: it scans the **"Claude review"** workflow (not "CI") with a single `review PR` flaky pattern, so it can never rerun a test job. ADR-0009 documents the philosophy broadly.

**Tech Stack:** GitHub Actions (YAML), `gh` CLI + `gh api`, bash. No Rust/cargo changes. Verification = YAML parse + structural `grep` assertions + optional `workflow_dispatch`.

**Branch:** `ci/auto-rerun-and-update-prs` (already created off `main`; the design spec `docs/superpowers/specs/2026-06-10-ci-automation-design.md` is already committed here as `d6d35a7`).

**Note on existing drafts:** `auto-update-prs.yml` and `auto-rerun-flaky.yml` already exist *uncommitted* on the branch from a pre-brainstorm draft. `auto-update-prs.yml` already matches this plan. `auto-rerun-flaky.yml`'s draft is **superseded** (it scans "CI" with a `test / macos` pattern) and Task 2 fully overwrites it.

---

### Task 1: Finalize `auto-update-prs.yml`

**Files:**
- Create/confirm: `.github/workflows/auto-update-prs.yml`

- [ ] **Step 1: Write the workflow file**

Write `.github/workflows/auto-update-prs.yml` with exactly this content:

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
# Dependabot and concurrent Claude sessions routinely push to main
# while PRs are open. Branch protection requires the ci-summary check
# to be strict/up-to-date, so every PR that falls behind must be
# updated before it can merge — manual `gh pr update-branch` calls
# add friction. This workflow removes the friction.
#
# Triggers
# --------
# - push to main: the moment main advances, update everyone.
# - workflow_dispatch: manual run for ad-hoc catch-up.
# - schedule (every 30 min): catch-net in case a push trigger
#   didn't fire (e.g. workflow service hiccup). Cheap — just lists
#   PRs and exits quickly when nothing is behind.
#
# Skips
# -----
# - Draft PRs: WIP, not ready to merge.
# - Closed PRs: obvious.
# - PRs with DIRTY mergeStateStatus: conflict, can't auto-resolve.
# - Same-repo branches only: fork PRs use the fork's branch which
#   GITHUB_TOKEN can't push to anyway. gh CLI handles this; we
#   swallow the resulting error.
#
# Security
# --------
# This workflow only reads PR metadata and triggers GitHub's own
# "Update branch" operation via the API. No user-controlled fields
# flow into shell commands. PR numbers come from the API response
# (integers, no injection surface). Repository name comes from
# the `github.repository` context (owner-controlled).

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

          # mergeStateStatus is computed asynchronously by GitHub
          # after a push lands. Wait briefly so the BEHIND state is
          # accurate when we query.
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

- [ ] **Step 2: Validate it parses as YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/auto-update-prs.yml')); print('OK')"
```
Expected: `OK`

- [ ] **Step 3: Assert the security invariant (no untrusted input in shell)**

Run:
```bash
grep -nE 'github\.event\.(pull_request|issue|comment|head)' .github/workflows/auto-update-prs.yml || echo "CLEAN: no untrusted event fields"
```
Expected: `CLEAN: no untrusted event fields`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/auto-update-prs.yml
git commit -m "ci: auto-update behind PR branches on main advance"
```

---

### Task 2: Rewrite `auto-rerun-flaky.yml` (bot-only, scans "Claude review")

**Files:**
- Overwrite: `.github/workflows/auto-rerun-flaky.yml` (replaces the superseded draft)

- [ ] **Step 1: Overwrite the workflow file**

Write `.github/workflows/auto-rerun-flaky.yml` with exactly this content (note: scans `"Claude review"`, single `"review PR"` pattern):

```yaml
name: Auto-rerun flaky failures

# Detects "Claude review" workflow runs whose ONLY failed jobs match a
# known-flaky pattern, and auto-reruns the failed jobs up to a bounded
# attempt count.
#
# SCOPE — deliberately narrow (see ADR-0009)
# ------------------------------------------
# This scans the "Claude review" workflow ONLY, never "CI". Cairn's
# .config/nextest.toml sets `retries = 0` in CI on purpose ("a flake is
# real signal — surface it, don't hide it"), so test/clippy/fmt failures
# must NEVER be auto-rerun. The only auto-rerun target is the Claude
# review bot, whose transient failures (API rate-limit / hiccup) are an
# external-service issue, not a code-signal flake. Pinning the scan to
# the "Claude review" workflow makes "never rerun a test job" a
# structural guarantee, not a pattern-list discipline.
#
# Note: `review PR` is NOT a required check (branch protection requires
# only `ci-summary`). The payoff here is obtaining the AI review that hit
# a rate-limit and clearing the spurious red ✗ — not unblocking a merge.
#
# Why cron and not `workflow_run`?
# `workflow_run` events fire reliably only for runs on the default
# branch's history, not for feature-branch PR pushes (documented GitHub
# Actions quirk). A 10-min cron scan covers all branches uniformly.

on:
  schedule:
    # Every 10 minutes. Cron expression starts at the minute granularity.
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
    name: Scan recent Claude review runs and rerun if flaky
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - name: Find failed Claude review runs in the look-back window
        env:
          GH_TOKEN: ${{ github.token }}
          REPO: ${{ github.repository }}
          MAX_ATTEMPTS: ${{ inputs.max_attempts || '3' }}
          WINDOW_MIN: ${{ inputs.window_minutes || '90' }}
        run: |
          set -euo pipefail

          # Known-flaky job names. A workflow run is only eligible for
          # auto-rerun if EVERY failed job in the run matches one of
          # these patterns (fixed-string substring match). Be
          # conservative — adding a non-flaky pattern here would mask
          # real failures by auto-retrying them. Cairn keeps this to the
          # Claude review bot ONLY (see the header + ADR-0009).
          flaky_patterns=(
            "review PR"   # Claude review bot transient (rate limit, API hiccup)
          )

          # ISO-8601 cutoff: now - WINDOW_MIN
          cutoff=$(date -u -d "$WINDOW_MIN minutes ago" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || \
                   date -u -v-"${WINDOW_MIN}"M +"%Y-%m-%dT%H:%M:%SZ")
          echo "Scanning Claude review runs since $cutoff (max_attempts=$MAX_ATTEMPTS)"

          # Find recent "Claude review" workflow_runs with conclusion=failure.
          mapfile -t failed_run_ids < <(
            gh api "repos/$REPO/actions/runs?status=failure&per_page=30" \
              --jq --arg cutoff "$cutoff" \
              '.workflow_runs[] | select(.name == "Claude review" and .created_at > $cutoff) | .id'
          )

          if [ "${#failed_run_ids[@]}" -eq 0 ]; then
            echo "No failed Claude review runs in window — nothing to do."
            exit 0
          fi

          echo "Failed Claude review runs in window: ${failed_run_ids[*]}"

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
              echo "No failed jobs (workflow likely succeeded on most recent attempt) — skipping."
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
              echo "All failed jobs are flaky — rerunning failed jobs (attempt $((attempt + 1)) of $MAX_ATTEMPTS)"
              gh api -X POST "repos/$REPO/actions/runs/$run_id/rerun-failed-jobs" \
                || echo "::warning::rerun API call failed (likely workflow no longer in rerunnable state)"
            fi
            echo "::endgroup::"
          done
```

- [ ] **Step 2: Validate it parses as YAML**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/auto-rerun-flaky.yml')); print('OK')"
```
Expected: `OK`

- [ ] **Step 3: Assert the structural invariants**

The scan must target "Claude review" (never "CI"), and the flaky list must contain only `review PR` (no test/clippy/fmt patterns). Run:

```bash
grep -q 'select(.name == "Claude review"' .github/workflows/auto-rerun-flaky.yml \
  && echo "OK: scans Claude review" || echo "FAIL: wrong scan target"
grep -qE 'name == "CI"|test / macos|"clippy"|"fmt"' .github/workflows/auto-rerun-flaky.yml \
  && echo "FAIL: a test/CI pattern leaked in" || echo "OK: no test/CI patterns"
```
Expected: `OK: scans Claude review` then `OK: no test/CI patterns`

- [ ] **Step 4: Confirm the flaky pattern matches a real cairn job**

The pattern `review PR` must match the actual job name in `claude-review.yml`, and that workflow must be named `Claude review`. Run:

```bash
grep -q '^name: Claude review$' .github/workflows/claude-review.yml \
  && grep -qE 'name: review PR' .github/workflows/claude-review.yml \
  && echo "OK: 'Claude review' workflow has a 'review PR' job" || echo "FAIL: job/workflow name mismatch"
```
Expected: `OK: 'Claude review' workflow has a 'review PR' job`

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/auto-rerun-flaky.yml
git commit -m "ci: bot-only auto-rerun for transient Claude review failures

Scans the 'Claude review' workflow (not 'CI') so no test job is ever
rerun — preserves the deliberate retries=0 'surface flakes' stance.
Corrects tau's dead 'review PR' pattern (tau scanned CI-only)."
```

---

### Task 3: Write ADR-0009 (broad CI philosophy)

**Files:**
- Create: `docs/decisions/0009-ci-automation.md`

- [ ] **Step 1: Write the ADR**

Write `docs/decisions/0009-ci-automation.md` with exactly this content:

```markdown
# ADR-0009: CI automation + CI philosophy

**Status:** Accepted
**Date:** 2026-06-10

## Context

Cairn's CI was set up "mirrors tau, simplified" with the reasoning living only in
commit messages and session memory — cairn had no CI ADR. Tau, by contrast, treats
CI as a designed, documented system (tau ADR-0018, the `2026-05-17-ci-upgrades`
spec). This ADR adopts tau's CI *philosophy* into cairn and, for the first time,
writes cairn's CI decisions down.

The immediate trigger: cairn has strict branch protection (`ci-summary` required,
strict/up-to-date) plus Dependabot (daily) and concurrent Claude sessions pushing
`main`. Open PRs constantly fall behind and must be updated before they can merge.
Tau solved the same friction with two automation workflows — but one of them
(flaky-rerun) masks flakes, which conflicts with a stance cairn already chose on
purpose.

## Decision

**1. Adopt the friction-reduction philosophy, not tau's YAML.** Add
`.github/workflows/auto-update-prs.yml` (ported ~verbatim): on every push to `main`
(plus a 30-min cron catch-net), update every open non-draft PR that is `BEHIND`.
This is pure friction reduction with no signal trade-off.

**2. Preserve cairn's `retries = 0` "surface flakes" stance.** `.config/nextest.toml`
sets `retries = 0` in CI deliberately ("a flake is real signal — surface it, don't
hide it") — the opposite of tau's `retries = 2` + indiscriminate flaky-rerun. Cairn
keeps `retries = 0` unchanged and does NOT mask test flakes.

**3. Bot-only, structurally-guaranteed flaky-rerun.** Add
`.github/workflows/auto-rerun-flaky.yml`, but scoped so it can never touch a test
job. It scans the **"Claude review"** workflow (NOT "CI") and reruns only when every
failed job matches `review PR`. A Claude-review failure is an external-service
transient (API rate-limit), not a code-signal flake. Pinning the scan to the "Claude
review" workflow makes "never rerun a test job" a structural property, not a
pattern-list discipline. (This also corrects a latent bug in tau's version, which
scans "CI" while its `review PR` job lives in the separate "Claude review" workflow —
so tau's `review PR` pattern never actually matches.) Note: `review PR` is not a
required check, so the payoff is obtaining the rate-limited review and clearing the
spurious ✗, not unblocking a merge.

**4. Both workflows are independently revertable.** Two standalone files, neither
wired into `ci-summary` or branch protection; either can be deleted in isolation.

## The cairn CI baseline (documented here for the first time)

**Kept from tau:** the 3-OS (`linux`/`macos`/`windows`) `test` matrix via
`cargo-nextest`; `cargo fmt`/`clippy --locked`/`cargo-deny` gates; doctests on
`cargo test --doc`; a `locked-check` MSRV (1.88) build; a single `ci-summary`
aggregate job as the only required check; the `merge_group` trigger; the
main-branch-only cache-save guard (`save-if: github.ref == 'refs/heads/main'` plus
`cancel-in-progress: github.ref != 'refs/heads/main'`); the `setup-rust` composite
action (toolchain + rust-cache + optional nextest/sccache/mold); Dependabot with
crate-family grouping; and the two Claude bots (`claude.yml` mention bot,
`claude-review.yml` auto-review).

**Dropped as not warranted at cairn's size:** tau's fuzz-nightly, scheduled mutation
testing, e2e/`build-fixtures` artifact-passing pipeline, and the mdbook docs-deploy
workflows.

**Branch protection:** `ci-summary` required (strict/up-to-date) + a PR required (0
approvals); `enforce_admins = false` so the owner can bypass in emergencies.

## Consequences

PRs that fall behind `main` are updated automatically, so strict branch protection
stops being manual-update friction. Transient Claude-review failures self-heal
without masking any real test signal — `retries = 0` is untouched and no CI/test job
is ever auto-rerun (guaranteed by the scan target, not a pattern list). Cairn now has
a written CI philosophy to extend.

Costs: two scheduled workflows consume a small amount of Actions minutes (both no-op
quickly when there's nothing to do). The `auto-update-prs` effect is only observable
on `main` over time, not in-PR. Security: both workflows use only owner-controlled
context (`github.repository`, `github.token`) and integer `workflow_dispatch` inputs;
no untrusted PR/issue text reaches a shell command.

## Alternatives considered

- **Port tau's `auto-rerun-flaky` as-is** (scan "CI", include `test / macos`).
  Rejected: it masks test flakes, contradicting cairn's deliberate `retries = 0`
  stance, and carries tau's dead-`review PR`-pattern bug.
- **Skip flaky-rerun entirely** (auto-update-prs only). Viable and fully preserves
  the surface-flakes stance, but leaves the Claude review bot's external rate-limit
  transients as recurring spurious ✗ marks. The bot-only scope captures the value
  without the cost.
- **Flip `retries = 0` → `2` to match tau.** Rejected: cairn intentionally surfaces
  flakes as signal; this ADR reaffirms that.
- **Enable the full GitHub merge queue.** Out of scope — cairn already has the
  `merge_group` trigger; turning the queue on is a separate branch-protection
  decision.

## References

- Spec: [`docs/superpowers/specs/2026-06-10-ci-automation-design.md`](../superpowers/specs/2026-06-10-ci-automation-design.md)
- Plan: [`docs/superpowers/plans/2026-06-10-ci-automation.md`](../superpowers/plans/2026-06-10-ci-automation.md)
- Tau prior art: tau ADR-0018 (CI optimization), tau `2026-05-17-ci-upgrades-round-1` spec.
```

- [ ] **Step 2: Validate internal doc links resolve**

Run:
```bash
test -f docs/superpowers/specs/2026-06-10-ci-automation-design.md \
  && test -f docs/superpowers/plans/2026-06-10-ci-automation.md \
  && echo "OK: referenced spec + plan exist"
```
Expected: `OK: referenced spec + plan exist`

- [ ] **Step 3: Commit**

```bash
git add docs/decisions/0009-ci-automation.md
git commit -m "docs(adr): ADR-0009 CI automation + CI philosophy"
```

---

### Task 4: Whole-branch verification

**Files:** none (verification only)

- [ ] **Step 1: All workflows parse**

Run:
```bash
python3 -c "import yaml,glob; [yaml.safe_load(open(f)) for f in glob.glob('.github/workflows/*.yml')]; print('all workflow YAML parses OK')"
```
Expected: `all workflow YAML parses OK`

- [ ] **Step 2: Re-assert the key invariant (no test/CI pattern in the rerun workflow)**

Run:
```bash
grep -qE 'name == "CI"|test / macos|"clippy"|"fmt"' .github/workflows/auto-rerun-flaky.yml \
  && echo "FAIL" || echo "OK: rerun workflow never targets a test/CI job"
```
Expected: `OK: rerun workflow never targets a test/CI job`

- [ ] **Step 3: Confirm the branch diff is exactly the three new files**

Run:
```bash
git diff --name-only main...HEAD
```
Expected (4 files — spec was committed earlier, plus the three from this plan):
```
docs/decisions/0009-ci-automation.md
docs/superpowers/plans/2026-06-10-ci-automation.md
docs/superpowers/specs/2026-06-10-ci-automation-design.md
.github/workflows/auto-rerun-flaky.yml
.github/workflows/auto-update-prs.yml
```
(Order may vary; the set is what matters. The plan file is added when committed.)

- [ ] **Step 4: Stop and hand off**

Do NOT push or open a PR automatically. Report completion and ask the user whether to
push the branch and open a PR (it will run through the `ci-summary` gate and the
Claude auto-review).

---

## Self-Review

**Spec coverage:**
- Spec §4 (auto-update-prs) → Task 1. ✓
- Spec §5 (auto-rerun-flaky, "Claude review" scan, `review PR` only) → Task 2 (+ structural assertions in Steps 3-4). ✓
- Spec §6 (broad ADR-0009: automations, retries=0 divergence, kept/dropped baseline, branch protection) → Task 3. ✓
- Spec §7 (independently revertable, no coverage change) → covered by file structure + ADR Decision 4. ✓
- Spec §8 (security) → Task 1 Step 3 assertion + ADR Consequences. ✓
- Spec §9 (verification: YAML parse, job-name match) → Task 1 Step 2, Task 2 Steps 2-4, Task 4. ✓
- Spec §11 (one PR, independently revertable, hand off — don't auto-push) → Task 4 Step 4. ✓

**Placeholder scan:** none — every file's full content is inline; every command has expected output.

**Type/name consistency:** scan target string `"Claude review"` and pattern `"review PR"` are identical across Task 2, its assertions, and ADR-0009. Workflow/job names match `claude-review.yml` (`name: Claude review`, job `review PR`). MSRV stated as 1.88 (matches `rust-toolchain.toml`). File paths in the ADR's References match the actual spec/plan paths.
