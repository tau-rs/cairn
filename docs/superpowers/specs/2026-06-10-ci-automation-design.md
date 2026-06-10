# CI Automation + First CI ADR — Design Spec

**Date:** 2026-06-10
**Status:** Approved (design); ready for implementation planning
**Builds on:** the CI on `main` (`ci.yml`, `claude-review.yml`, `claude.yml`, `coverage.yml`; `setup-rust` composite action; `.config/nextest.toml`; `dependabot.yml`; branch protection requiring `ci-summary`).

---

## 1. Goal

Adopt the **CI philosophy** that tau evolved (documented in tau's ADR-0018 and its
`2026-05-17-ci-upgrades-round-1` spec) into cairn — not by copying tau's YAML, but by
extracting the reasoning and reconciling it with the CI stance cairn already holds.

Concretely, deliver:

1. **`auto-update-prs.yml`** — when `main` advances, auto-update every behind, non-draft PR
   so strict branch protection stops being manual-update friction.
2. **`auto-rerun-flaky.yml`** — a *bot-only*, *corrected* adaptation of tau's flaky-rerun
   workflow: rerun a transient Claude-review failure, and **nothing else**.
3. **ADR-0009** — cairn's first CI architecture decision record, broad scope: it documents
   the new automations *and* the already-made-but-never-written-down CI decisions (the
   `retries=0` divergence from tau, the "mirrors-tau-simplified" baseline, the
   branch-protection model).

This is the first time cairn's CI is treated as a *designed, documented* system rather than
"mirrors tau, simplified" with the reasoning living only in commit messages and memory.

---

## 2. The philosophy being copied (and where cairn diverges)

Distilled from tau ADR-0018 + the round-1 spec, mapped onto cairn:

| Tau principle | Cairn adoption |
|---|---|
| CI is a **designed, documented** system (ADR per decision) | → **ADR-0009** (this sub-project) |
| **Reduce the friction strict branch protection creates** | → the two workflows |
| **Never trade test signal/coverage for speed** | → keep `retries=0`; confine *all* flaky-masking to the non-test Claude bot |
| Conservative · cost-aware · **phased, independently revertable** delivery | → two standalone files, each revertable alone; neither touches the merge gate |

**The central divergence.** Tau masks parallelism flakes at *two* layers: `.config/nextest.toml`
`retries = 2` in CI, plus a workflow-level `auto-rerun-flaky` that reruns any run whose only
failed jobs match known-flaky patterns (in tau, `test-stable / macos` + `review PR`). Cairn
deliberately chose the *opposite* at the test layer:

```toml
# cairn .config/nextest.toml
[profile.ci]
# CI: NO retries. A flake is real signal — surface it, don't hide it.
retries = 0
```

Importing tau's `auto-rerun-flaky` wholesale would contradict that stance. So cairn adopts the
*friction-reduction philosophy* (the "why") while honoring its own *surface-flakes* stance
(the "how"): the rerun is scoped so it can **never** touch a test job.

---

## 3. Decisions (locked during brainstorming)

1. **Flaky policy:** adopt `auto-update-prs` **and** an `auto-rerun-flaky` scoped to the
   `review PR` Claude-bot job **only** — never any test job. A bot rate-limit hiccup is an
   external-service transient, not a code-signal flake, so rerunning it does not violate
   `retries=0`. `retries=0` stays exactly as-is.
2. **ADR scope:** **broad** — ADR-0009 documents cairn's whole CI philosophy, including the
   `retries=0` divergence and the dropped tau-specific jobs, not just the two new workflows.
3. **Scanner target (corrects a tau bug):** cairn's `auto-rerun-flaky` scans the
   **"Claude review"** workflow, **not** "CI". In tau the workflow scans `.name == "CI"` while
   the `review PR` job actually lives in the separate "Claude review" workflow — so tau's
   `review PR` pattern never matches (dead config). Scanning "Claude review" both fixes that
   and *structurally* guarantees no `test`/`clippy`/`fmt`/`ci-summary` job is ever rerun.
4. **Honest caveat (documented in the ADR):** `review PR` is **not** a required check in cairn
   (branch protection requires only `ci-summary`). So the rerun's payoff is "actually obtain
   the AI review that hit a rate-limit, and clear the spurious red ✗," not "unblock a merge."
   Modest but real, and consistent with the chosen policy.

---

## 4. Component A — `auto-update-prs.yml`

Ported ~verbatim from tau (the workflow is repo-agnostic; only the prose rationale is
reworded to cite cairn's contributors and gate).

- **Triggers:** `push` to `main`; `workflow_dispatch`; `schedule` every 30 min (catch-net for
  a missed push trigger).
- **Logic:** on a `push` event, `sleep 45` (GitHub computes `mergeStateStatus` asynchronously),
  then `gh pr list --state open` filtered to `isDraft == false && mergeStateStatus == "BEHIND"`,
  and `gh pr update-branch` each. Conflicts / fork-branch-permission errors are swallowed
  (logged as "skipped"), never fatal.
- **Concurrency:** `group: auto-update-prs`, `cancel-in-progress: true`.
- **Permissions:** `contents: read`, `pull-requests: write`.
- **Why cairn needs it:** branch protection requires `ci-summary` strict/up-to-date, and
  Dependabot (daily) + concurrent Claude sessions routinely push `main` while PRs are open.
  Every PR that falls behind must be updated before it can merge — this removes the manual
  `gh pr update-branch` step.

## 5. Component B — `auto-rerun-flaky.yml`

A corrected, narrowed adaptation of tau's workflow.

- **Triggers:** `schedule` every 10 min; `workflow_dispatch` with inputs `max_attempts`
  (default `"3"`) and `window_minutes` (default `"90"`).
- **Scan:** failed runs of the **"Claude review"** workflow inside the look-back window
  (`gh api .../actions/runs?status=failure` filtered by `.name == "Claude review"` and
  `created_at > cutoff`).
- **Flaky patterns:** exactly one — `"review PR"`. The run is rerun only if **every** failed
  job in it matches (fixed-string substring), and only while `run_attempt < max_attempts`.
  Rerun via `POST .../rerun-failed-jobs`; a failed rerun call is a `::warning::`, not fatal.
- **Concurrency:** `group: auto-rerun-flaky`, `cancel-in-progress: false`.
- **Permissions:** `actions: write`, `contents: read`.
- **Structural invariant:** because the scan is pinned to the "Claude review" workflow, no
  job from the "CI" workflow (`fmt`, `clippy`, `cargo-deny`, `test`, `doc-tests`,
  `locked-check`, `ci-summary`) is ever in scope. `retries=0` for tests is preserved by
  construction, not by the pattern list.

## 6. Component C — ADR-0009 (broad CI philosophy)

`docs/decisions/0009-ci-automation.md`, following cairn's ADR template (Status / Date /
Context / Decisions / Consequences / Alternatives). It records:

- **The two automations** and their rationale (§4, §5).
- **The `retries=0` divergence from tau** — why cairn surfaces flakes instead of masking them,
  and why the bot-only rerun is consistent with that (the bot transient is not a code-signal
  flake; the scan structurally excludes test jobs).
- **The "mirrors-tau-simplified" baseline, finally written down:** what cairn *kept* from tau
  (3-OS `test` matrix, `cargo-deny` gate, `ci-summary` single aggregate required check,
  `merge_group` trigger, the main-branch cache-save guard, the `setup-rust` composite,
  Dependabot with dep-grouping, the Claude bots) and what it *dropped* as not warranted at
  cairn's size (fuzz, mutants, e2e/build-fixtures artifact passing, mdbook docs-deploy).
- **The branch-protection model:** `ci-summary` required (strict/up-to-date) + PR required,
  `enforce_admins = false` so the owner can bypass in emergencies.

---

## 7. Consistency with cairn's existing principles

- **`retries=0` is untouched.** The only auto-rerun is confined to a non-test, non-required,
  external-bot job, in a *separate workflow* the scanner is pinned to.
- **No coverage change.** No test, OS, or job is added, removed, or thinned.
- **Independently revertable.** Two standalone files; deleting either leaves the merge gate
  and every other workflow intact. Neither is wired into `ci-summary` or branch protection.

## 8. Security

Both workflows consume only owner-controlled context (`github.repository`, `github.token` /
`secrets.GITHUB_TOKEN`) plus integer-defaulted `workflow_dispatch` inputs. No untrusted PR /
issue / comment text flows into a shell `run:` step. PR numbers come from the API as integers
(no injection surface). `head_branch` is only ever echoed to logs, double-quoted, never
eval'd. This matches the security posture documented inline in tau's versions.

## 9. Verification

- **YAML:** parse all of `.github/workflows/*.yml` (Python `yaml.safe_load`); `yamllint` if
  available.
- **Job-name match:** confirm cairn's Claude-review workflow name is `Claude review` and its
  job is `review PR` (so the `auto-rerun-flaky` pattern matches a real job).
- **`auto-update-prs`:** effect only manifests on `main` over time; verified by code review of
  the filter expression + an optional `workflow_dispatch` run that lists behind PRs and exits
  cleanly when none are behind.
- **`auto-rerun-flaky`:** `workflow_dispatch` run confirms it scans "Claude review" and
  no-ops ("nothing to do") when no flaky failure is in the window.

## 10. Out of scope

- Tau's heavier CI surface (fuzz, mutants, e2e/build-fixtures, mdbook docs-deploy).
- Flipping `retries=0` / any change to the nextest profiles.
- Full GitHub merge-queue enablement (cairn already has the `merge_group` *trigger*; turning
  on the queue is a separate branch-protection decision).
- SHA-pinning third-party actions (tau deferred this too; Dependabot manages version bumps).
- Any change to `ci.yml`, `coverage.yml`, the `setup-rust` action, or `dependabot.yml`.

## 11. Delivery

One PR off `main` (branch `ci/auto-rerun-and-update-prs`): the two workflow files + ADR-0009.
Runs through the standard `ci-summary` gate. The two workflows are independent files, so a
post-merge problem with either can be reverted in isolation without rolling back the other or
the ADR.
