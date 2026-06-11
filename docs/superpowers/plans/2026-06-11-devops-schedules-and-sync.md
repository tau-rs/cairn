# Cairn DevOps — Anti-drift sync + T3 schedules + lint-config alignment

> **For agentic workers:** This is DevOps/CI config work. Per the brief,
> **verification is over unit-TDD** — GitHub Actions workflows are validated by
> dispatching them and observing real runs, not by unit tests. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close cairn audit gaps D6 (cross-repo anti-drift sync), D7 (T3
scheduled drift-catchers), and D9 (lint-config alignment) in one coherent,
additive, **non-gating** PR — none of it on the release critical path, none
wired into `ci-summary`.

**Architecture:**
- **D6 (pull-model sync):** A self-hosted `sync-template.yml` that pulls the
  canonical lint/policy surface from the source-of-truth repo (`tau-rs/tau`),
  diffs it against cairn's, and opens a **reviewable PR in cairn** on drift
  (never auto-merge). Pull (not push) because tau is the established single
  *source* — its own `sync.yml` excludes `sync-template.yml` from propagation so
  no sibling becomes a second writer. A pull model lets cairn own the
  merge/decline decision, needs no cross-repo PAT (reads a public repo, writes a
  local PR with `GITHUB_TOKEN`), and matches the brief's "Renovate file-sync"
  option.
- **D7 (schedules):** Three self-contained scheduled workflows mirrored from
  tau's canonical template — `fuzz-nightly.yml` (nightly), `mutants-scheduled.yml`
  (weekly), `security-daily.yml` (daily CVE/dependency review). Self-contained
  (not `workflow_call` into a heavy tier) **exactly as tau's are** — so they do
  NOT depend on session 82's `heavy.yml` existing; shared definitions are kept
  aligned by the sync mechanism, not by runtime coupling. All adapted to cairn's
  crates; fuzz uses a "skip if fuzz dir absent" precheck so it runs green before
  any harness lands.
- **D9 (lint align):** Bring `clippy.toml` / `rustfmt.toml` to byte-parity with
  tau's shared config, and add them to the sync surface so they stay aligned.

**Tech Stack:** GitHub Actions, `cargo-fuzz`, `cargo-mutants`, `cargo-audit`,
`osv-scanner`, `peter-evans/create-pull-request`. All third-party actions
**SHA-pinned** (reusing tau's already-vetted pins; session 81's convention).

---

## Non-negotiable constraints (from the brief)

- Additive only. Do **not** modify `ci.yml`, `coverage.yml`, or `ci-summary`.
- Every new workflow is **non-gating**: separate workflow files, never added to
  branch protection's required `ci-summary` check.
- Every job has `timeout-minutes` and least-privilege `permissions`.
- The sync opens a **reviewable PR**, never auto-merges / force-pushes.
- SHA-pin all third-party actions with a `# vX.Y.Z` trailing comment.
- Out of scope (note as deferred): projen-style synth-diff generator; OIDC/cosign
  (D8 / phase-2); push-direction sync (tau owns that).

---

## File Structure

- Create `.github/workflows/sync-template.yml` — D6 pull-sync → reviewable PR.
- Create `.github/workflows/fuzz-nightly.yml` — D7 nightly cargo-fuzz.
- Create `.github/workflows/mutants-scheduled.yml` — D7 weekly cargo-mutants.
- Create `.github/workflows/security-daily.yml` — D7 daily CVE/dependency review.
- Modify `clippy.toml` — D9 align to tau's stub + ADR comment.
- Modify `rustfmt.toml` — D9 add `max_width = 100`.

---

## SHA pins (reuse tau's vetted pins; verified 2026-06-11)

| action | SHA | tag |
|---|---|---|
| actions/checkout | `df4cb1c069e1874edd31b4311f1884172cec0e10` | v6 |
| actions/cache | `27d5ce7f107fe9357f9df03efb73ab90386fccae` | v5 |
| actions/upload-artifact | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` | v7 |
| actions/download-artifact | `018cc2cf5baa6db3ef3c5f8a56943fffe632ef53` | v6 |
| dtolnay/rust-toolchain (nightly) | `5b842231ba77f5c045dba54ac5560fed2db780e2` | nightly |
| dtolnay/rust-toolchain (stable) | `29eef336d9b2848a0b548edc03f92a220660cdb8` | stable |
| Swatinem/rust-cache | `e18b497796c12c097a38f9edb9d0641fb99eee32` | v2 |
| taiki-e/install-action | `fa8484446eeba15720aa701ffc5dcb6dfa092ff1` | nextest |
| rustsec/audit-check | `69366f33c96575abad1ee0dba8212993eecbe998` | v2.0.0 |
| google/osv-scanner-action | `9a498708959aeaef5ef730655706c5a1df1edbc2` | v2.3.8 |
| peter-evans/create-pull-request | `5f6978faf089d4d20b00c7766989d076bb2fc7f1` | v8.1.1 |

---

## Task 1: D9 — align lint configs

**Files:** Modify `clippy.toml`, `rustfmt.toml`.

- [ ] **Step 1:** Set `clippy.toml` to tau's shared stub:
  ```
  # Empty stub. Clippy lint configuration is added per-need with ADR justification.
  # Lint denial level is set per-crate in lib.rs / main.rs and via CI flags.
  ```
- [ ] **Step 2:** Set `rustfmt.toml` to tau's shared config:
  ```
  edition = "2021"
  max_width = 100
  ```
- [ ] **Step 3:** Verify fmt still passes against the new width:
  Run: `cargo fmt --all -- --check`
  Expected: exit 0 (no reformatting needed). If it reformats, run `cargo fmt --all`
  and include the formatting churn in the commit — note it in the PR body.

## Task 2: D7 — nightly fuzz

**Files:** Create `.github/workflows/fuzz-nightly.yml`.

- [ ] **Step 1:** Mirror tau's `fuzz-nightly.yml` structure (nightly cron
  `0 4 * * *` + dispatch, `permissions: contents: read` + `issues: write`,
  per-job `timeout-minutes`, SHA-pinned actions). Adapt the matrix to cairn's
  audit-flagged fuzz surfaces, each with the **"skip if fuzz dir absent"
  precheck** so the job is green before harnesses land:
  - `cairn-domain` / `crates/cairn-domain/fuzz` / target `note_path` (NotePath parsing)
  - `cairn-plugin-protocol` / `crates/cairn-plugin-protocol/fuzz` / target `manifest_decode`
- [ ] **Step 2:** Header comment documents: non-blocking, off release path, and
  that harness definitions converge with session 82's `heavy.yml` via the sync
  mechanism (shared `cargo-fuzz` version `=0.13.2`, same patterns).

## Task 3: D7 — weekly mutants

**Files:** Create `.github/workflows/mutants-scheduled.yml`.

- [ ] **Step 1:** Mirror tau's `mutants-scheduled.yml` (weekly cron `0 6 * * 0`
  + dispatch with `crate`/`timeout_seconds` inputs, `timeout-minutes: 240`,
  read-only cache, `cargo-mutants --locked --version =27.1.0`, upload report
  `always()`). Matrix = cairn crates, parsers/kernel first:
  `cairn-domain, cairn-plugin-protocol, cairn-ports, cairn-infra, cairn-service,
  cairn-app, cairn-contract, cairn-cli, cairn-daemon, cairn-plugin-sdk`.
- [ ] **Step 2:** Header documents non-blocking / off release path.

## Task 4: D7 — daily security / dependency review

**Files:** Create `.github/workflows/security-daily.yml`.

- [ ] **Step 1:** Mirror tau's `security-daily.yml` (daily cron `0 4 * * *` +
  dispatch, cargo-audit + osv-scanner, diff-vs-yesterday → file issue on NEW
  advisories only). This is the scheduled "dependency-review" of the brief's D7
  (GitHub's `dependency-review-action` only works in PR context, which would be
  a gate — out of scope here). `continue-on-error` on osv so it never blocks.

## Task 5: D6 — pull-model template sync

**Files:** Create `.github/workflows/sync-template.yml`.

- [ ] **Step 1:** Workflow: weekly cron + `workflow_dispatch` with a
  `dry_run` boolean input (default **true**). `permissions: contents: write` +
  `pull-requests: write` (local PR only). `concurrency` singleton.
- [ ] **Step 2:** Steps: checkout cairn → fetch the canonical files from
  `tau-rs/tau` `main` via raw URL (public, no auth) into the working tree,
  overwriting cairn's copies → `git diff` to log drift → if `dry_run`, stop after
  logging → else open/update ONE reviewable PR via `peter-evans/create-pull-request`
  (fixed branch `ci/sync-template`, labels `sync`,`ci-template`, body explains
  cairn owns the merge/decline decision; never auto-merge).
- [ ] **Step 3:** Synced surface = the byte-shareable policy files where a raw
  overwrite yields a *correct, mergeable* PR: `clippy.toml`, `rustfmt.toml`.
  A clearly-commented `FILES` array at the top makes extending one line. Header
  documents: (a) why pull not push (tau = single source), (b) crate-bearing
  workflows (ci.yml/heavy/tier2/schedules) are intentionally NOT auto-overwritten
  because crate-name divergence would produce a broken PR — their convergence is
  manual review until a projen-style generator lands (deferred phase-2), and
  (c) disable this workflow if the org later activates tau's push direction (to
  avoid duplicate PRs).

## Task 6: Verification (verification-before-completion)

- [ ] **Step 1:** `actionlint` (or `gh workflow view`) lints all four new YAMLs
  clean. Run locally: `actionlint .github/workflows/*.yml` if available.
- [ ] **Step 2:** Push the branch. Dispatch each workflow on the branch and
  capture real output:
  - `gh workflow run sync-template.yml --ref <branch> -f dry_run=true` → confirm
    it logs the drift diff and opens NO PR.
  - `gh workflow run fuzz-nightly.yml --ref <branch>` → confirm fuzz jobs SKIP
    green (no fuzz dirs yet).
  - `gh workflow run security-daily.yml --ref <branch>` → confirm it runs.
  - `gh workflow run mutants-scheduled.yml --ref <branch> -f crate=cairn-contract
    -f timeout_seconds=60` → confirm the machinery starts (mutation findings are
    signal, not a workflow defect).
- [ ] **Step 3:** Confirm none of the four appear in `ci.yml`'s `ci-summary`
  `needs:` list (grep) — structurally non-gating.

## Task 7: Review + ship

- [ ] **Step 1:** `requesting-code-review` — verify T3 jobs cannot block release
  and the sync is reviewable-PR not silent push.
- [ ] **Step 2:** Commit (`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`),
  push, `gh pr create -R tau-rs/cairn --base main`, cite D6 + D7 + D9, note
  deferred D8/projen/push-direction. STOP — no merge.
