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
