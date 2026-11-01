# T1 CI Gate Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the cairn-ui PR fast gate (`ci.yml`) and `nightly.yml` with the canonical cross-cutting DevOps controls — SHA-pinned actions, Renovate, per-job timeouts, a T1 `cargo build`, web/rust changes-detection, and a `cargo-deny` + lockfile + audit supply-chain step — without folding in the release/heavy tier (session 92) or the lefthook/composite-action work (G13, deferred).

**Architecture:** This is CI configuration, so the discipline is *verification*, not unit-test TDD. Each task produces an independently verifiable change (a workflow edit, a config file) checked with a grep, a local `just`/`cargo` run, or a CI run. New supply-chain and build checks are routed through `just` verbs so local ≡ CI (Diagram 3 of `audit/devops.md`). Session 90's canonical full `justfile` (G1) is **not yet in `main` and has no open PR**; this PR ships a minimal forward-compatible `justfile` carrying only the verbs these new checks need (`deny`, `deny-web`, `deny-rust`, `build-rust`). When session 90 lands its full justfile, the aggregate `deny` verb is shared and the per-stack sub-verbs are additive — a trivial merge.

**Tech Stack:** GitHub Actions YAML, `just` 1.x, `cargo-deny` 0.18, `pnpm` 10, `dorny/paths-filter` v3, `renovate.json`.

**Scope (from brief 91):** G6 (SHA pins), G7 (Renovate), G12 (timeouts on ci+nightly), G10 (cargo build on T1), G9 (path filter), G8/G11 (cargo-deny + lockfile + audit on T1). **Out of scope, note as deferred:** G1 full justfile + repointing existing web/tauri verbs, G2 lefthook, G3/G4 heavy/release tier (session 92), G5 SBOM, G13 composite actions, G14 OIDC docs, G15 cosign/SLSA.

**Resolved action SHAs (pinned to the SAME versions currently in use — no upgrades):**

| action | current tag | pinned SHA | version comment |
|--------|-------------|------------|-----------------|
| `actions/checkout` | `@v6` | `df4cb1c069e1874edd31b4311f1884172cec0e10` | `# v6.0.3` |
| `pnpm/action-setup` | `@v6` | `0e279bb959325dab635dd2c09392533439d90093` | `# v6.0.8` |
| `actions/setup-node` | `@v6` | `48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e` | `# v6.4.0` |
| `actions/upload-artifact` | `@v7` | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` | `# v7.0.1` |
| `dtolnay/rust-toolchain` | `@stable` | `29eef336d9b2848a0b548edc03f92a220660cdb8` | `# stable channel` |
| `Swatinem/rust-cache` | `@v2` | `e18b497796c12c097a38f9edb9d0641fb99eee32` | `# v2` |
| `taiki-e/install-action` | `@v2` | `7a79fe8c3a13344501c80d99cae481c1c9085912` | `# v2.81.10` |
| `anthropics/claude-code-action` | `@beta` | `28f83620103c48a57093dcc2837eec89e036bb9f` | `# beta` |
| `dorny/paths-filter` (new) | `@v3` | `d1c1ffe0248fe513906c8e24db8ea791d46f8590` | `# v3.0.3` |

> Re-resolve before committing if days have passed (a moving tag may have advanced):
> `git ls-remote https://github.com/<owner>/<repo> refs/tags/<tag>^{} refs/tags/<tag>`
> Use the dereferenced (`^{}`) commit for annotated tags.

---

## File Structure

- **Create** `justfile` (repo root) — minimal forward-compatible verbs: `deny`, `deny-web`, `deny-rust`, `build-rust`. Routes the new checks so local ≡ CI.
- **Create** `deny.toml` (repo root) — `cargo-deny` config (advisories/licenses/bans/sources) tuned to pass on the current `src-tauri` tree.
- **Create** `renovate.json` (repo root) — bumps the SHA pins (with version comments) and acts as the workflow sync bot.
- **Modify** `.github/workflows/ci.yml` — SHA pins, timeouts, `changes` path-filter job, `just deny-web` in `web`, `just deny-rust` + `just build-rust` + `--locked` in `tauri`, `changes` added to `ci-summary` needs.
- **Modify** `.github/workflows/nightly.yml` — SHA pins, timeouts.
- **Modify** `.github/workflows/coverage.yml`, `mutation-weekly.yml`, `auto-rerun-flaky.yml`, `auto-update-prs.yml`, `claude.yml`, `claude-review.yml` — SHA pins only (timeouts already present where the brief scoped them; do NOT add timeouts to these per brief scope).
- **Modify** `audit/devops.md` — flip the checklist boxes this PR closes; mark deferred items.

---

## Task 1: Minimal `justfile` (forward-compatible deny/build verbs)

**Files:**
- Create: `justfile`

**Context:** `just` runs recipes in the justfile's directory (verified: recipe CWD = repo root regardless of where `just` is invoked), so explicit `web/` and `src-tauri/` paths resolve correctly even when CI calls a verb from the `web` working-directory.

- [ ] **Step 1: Write the justfile**

```just
# cairn-ui task runner — MINIMAL subset.
#
# Session 90 (audit/devops.md G1) owns the FULL canonical justfile
# (fmt/lint/test/deny/ci/heavy/fix fanning out to pnpm AND cargo). That
# work is not yet in main. This file carries ONLY the verbs the T1 gate
# hardening (session 91) introduces, so CI routes the new supply-chain
# and build checks through `just` rather than inlining them (local ≡ CI,
# Diagram 3). When session 90 lands, the aggregate `deny` verb is shared
# and the per-stack sub-verbs are additive.

# Supply-chain audit across BOTH stacks (Diagram 3: just deny → pnpm audit + cargo deny check).
deny: deny-web deny-rust

# Web dependency audit (npm advisories).
deny-web:
    pnpm -C web audit --audit-level=high

# Rust advisories + licenses + bans + sources.
deny-rust:
    cargo deny --manifest-path src-tauri/Cargo.toml check

# Compile the Tauri shell with a locked lockfile (T1 link/bundle check).
build-rust:
    cargo build --locked --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 2: Verify the recipes resolve (list + dry-run from a subdir)**

Run: `cd web && just --justfile ../justfile --list; cd ..`
Expected: lists `deny`, `deny-web`, `deny-rust`, `build-rust`. (`just` finds the root justfile by searching upward.)

- [ ] **Step 3: Commit**

```bash
git add justfile
git commit -m "build: add minimal justfile with deny/build verbs for T1 gate

Forward-compatible subset of session 90's canonical justfile (G1). Routes
the new supply-chain (cargo-deny, pnpm audit) and tauri build checks
through just so local matches CI.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 2: `deny.toml` + verify `just deny-rust` passes locally

**Files:**
- Create: `deny.toml`

**Context:** cairn-* workspace crates are git-pinned by rev (see project memory), so `sources.unknown-git` must allow git sources or cargo-deny fails on them. The license `allow` list must cover every license in the `src-tauri` tree; tune it by running `cargo deny check` until green.

- [ ] **Step 1: Write the initial deny.toml**

```toml
# cargo-deny configuration for the src-tauri Rust shell.
# Run via `just deny-rust` (local + CI T1 gate). Tuned to pass on the
# current dependency tree; widen `licenses.allow` if a new dep introduces
# a permissive license not yet listed.

[advisories]
version = 2
# RustSec advisory DB. Fail the gate on any unfixed vuln or yanked crate.
yanked = "deny"
ignore = []

[licenses]
version = 2
confidence-threshold = 0.8
# Permissive licenses present in (or expected for) the Tauri dep tree.
# Extend as needed — `cargo deny check licenses` names any rejected one.
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
    "MPL-2.0",
    "CC0-1.0",
    "0BSD",
]

[bans]
# Duplicate versions are a hygiene signal, not a merge blocker here.
multiple-versions = "warn"
wildcards = "allow"

[sources]
unknown-registry = "deny"
# cairn-* workspace deps are git-pinned by rev; allow git sources.
unknown-git = "allow"
```

- [ ] **Step 2: Run cargo-deny and tune until green**

Run: `cargo deny --manifest-path src-tauri/Cargo.toml check 2>&1 | tail -40`
Expected: `advisories ok`, `bans ok`, `licenses ok`, `sources ok`.
If a license is rejected: add the exact SPDX id printed (e.g. `error[rejected]: ... license = "Foo-1.0"`) to `licenses.allow` and re-run.
If an advisory fails on a transitive crate with no fix available: add its `RUSTSEC-XXXX-YYYY` id to `advisories.ignore` with a one-line `# reason` comment, and note it in the PR body. Prefer fixing over ignoring.

- [ ] **Step 3: Verify the aggregate verb fans out**

Run: `just deny-rust`
Expected: exit 0, same `... ok` lines.

- [ ] **Step 4: Commit**

```bash
git add deny.toml
git commit -m "build: add cargo-deny config for src-tauri supply-chain checks

Advisories + licenses + bans + sources, tuned to pass on the current
tree. git sources allowed (cairn-* deps are rev-pinned). Closes the
nightly-only audit gap for Rust on the T1 path (G8/G11).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 3: SHA-pin every `uses:` across all 8 workflows

**Files:**
- Modify: `.github/workflows/ci.yml`, `nightly.yml`, `coverage.yml`, `mutation-weekly.yml`, `auto-rerun-flaky.yml` (no `uses:` — skip), `auto-update-prs.yml` (no `uses:` — skip), `claude.yml`, `claude-review.yml`

**Context:** Use the SHA table at the top of this plan. Pin to the SAME version currently in use — no upgrades. Every pin gets a trailing `# vX.Y.Z` comment so Renovate/Dependabot can bump it. `auto-rerun-flaky.yml` and `auto-update-prs.yml` use no `uses:` (pure `gh` scripts) — confirm and skip.

- [ ] **Step 1: Replace each `uses:` tag with its pinned SHA + comment**

Apply these exact replacements (every occurrence — `checkout` appears multiple times per file):

```
uses: actions/checkout@v6              → uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
uses: pnpm/action-setup@v6             → uses: pnpm/action-setup@0e279bb959325dab635dd2c09392533439d90093 # v6.0.8
uses: actions/setup-node@v6            → uses: actions/setup-node@48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e # v6.4.0
uses: actions/upload-artifact@v7       → uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
uses: dtolnay/rust-toolchain@stable    → uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable channel
uses: Swatinem/rust-cache@v2           → uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
uses: taiki-e/install-action@v2        → uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
uses: anthropics/claude-code-action@beta → uses: anthropics/claude-code-action@28f83620103c48a57093dcc2837eec89e036bb9f # beta
```

Note `dtolnay/rust-toolchain@stable` keeps the `components:`/no-`components:` `with:` blocks unchanged; pinning the SHA does not change inputs.

- [ ] **Step 2: Verify zero mutable tags remain**

Run: `grep -rEn 'uses: [^ ]+@(v[0-9]+|stable|beta|main|master)( |$)' .github/workflows/`
Expected: NO output (every `uses:` is now `@<40-hex>`).

Run: `grep -rEc 'uses: [^ ]+@[0-9a-f]{40}' .github/workflows/`
Expected: nonzero counts in ci.yml, nightly.yml, coverage.yml, mutation-weekly.yml, claude.yml, claude-review.yml.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/
git commit -m "ci: pin all actions by commit SHA (G6)

Every uses: pinned to the SHA of its current version (no upgrades), with
a trailing version comment for Renovate. Closes the tag-move attack
surface across all workflows.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 4: Add `renovate.json`

**Files:**
- Create: `renovate.json`

**Context:** Dependabot already exists (`.github/dependabot.yml`) and bumps action *tags*. Renovate is added to bump the new SHA *pins* (it updates both the SHA and the `# vX.Y.Z` comment) and to serve as the workflow sync bot (Diagram 1). To avoid double-PRs with Dependabot on github-actions, Renovate owns github-actions SHA bumps; leave Dependabot's npm/cargo ecosystems as-is. Use the `helpers:pinGitHubActionDigests` preset so Renovate keeps actions digest-pinned.

- [ ] **Step 1: Write renovate.json**

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": [
    "config:recommended",
    "helpers:pinGitHubActionDigests",
    ":semanticCommits",
    ":semanticCommitScope(deps)"
  ],
  "timezone": "Europe/Paris",
  "schedule": ["before 06:00 on monday"],
  "labels": ["dependencies"],
  "enabledManagers": ["github-actions"],
  "github-actions": {
    "pinDigests": true
  },
  "packageRules": [
    {
      "description": "Group all GitHub Actions digest bumps into one PR.",
      "matchManagers": ["github-actions"],
      "groupName": "github-actions digests",
      "commitMessagePrefix": "ci:"
    }
  ]
}
```

> **Decision recorded:** `enabledManagers` is restricted to `github-actions` so Renovate does NOT race Dependabot on npm/cargo (those stay Dependabot-owned per `.github/dependabot.yml`). Renovate handles the SHA-pin digest bumps + comment refresh that Dependabot does not do as cleanly. The github-actions ecosystem in `dependabot.yml` may be left enabled (Dependabot bumps tags, Renovate re-pins to digest) — note in PR body that consolidating to one bot is a follow-up, not in scope.

- [ ] **Step 2: Validate the JSON**

Run: `node -e "JSON.parse(require('fs').readFileSync('renovate.json','utf8')); console.log('renovate.json valid')"`
Expected: `renovate.json valid`

(If `npx --yes --package renovate -- renovate-config-validator` is available offline it can be run, but JSON validity + schema URL is sufficient for this PR; Renovate validates on its first run.)

- [ ] **Step 3: Commit**

```bash
git add renovate.json
git commit -m "ci: add Renovate to bump SHA-pinned actions + sync workflow template (G7)

Renovate owns github-actions digest bumps (keeps the SHA pins + version
comments current and acts as the Diagram 1 sync bot); Dependabot keeps
npm/cargo.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 5: Add `timeout-minutes` to every job in `ci.yml` and `nightly.yml`

**Files:**
- Modify: `.github/workflows/ci.yml` (jobs: `web`, `e2e`, `tauri`, `ci-summary`; plus `changes` added in Task 6)
- Modify: `.github/workflows/nightly.yml` (jobs: `tauri-cross`, `e2e-full-browser`, `audit`, `nightly-summary`)

**Context:** Brief scopes timeouts to `ci.yml` + `nightly.yml` only (coverage/mutation/auto-rerun already have them). Place `timeout-minutes:` directly under `runs-on:` on each job. Values chosen generous enough not to trip on a cold cache, tight enough to kill a hang well short of the 6h default.

- [ ] **Step 1: Add timeouts in ci.yml**

```
web:        runs-on: ubuntu-latest  →  add  timeout-minutes: 15
e2e:        runs-on: ubuntu-latest  →  add  timeout-minutes: 20
tauri:      runs-on: ubuntu-latest  →  add  timeout-minutes: 30
ci-summary: runs-on: ubuntu-latest  →  add  timeout-minutes: 5
```

Example (web job):
```yaml
  web:
    name: web
    runs-on: ubuntu-latest
    timeout-minutes: 15
    defaults:
```

- [ ] **Step 2: Add timeouts in nightly.yml**

```
tauri-cross:      runs-on: ${{ matrix.os }}  →  add  timeout-minutes: 45
e2e-full-browser: runs-on: ubuntu-latest     →  add  timeout-minutes: 30
audit:            runs-on: ubuntu-latest     →  add  timeout-minutes: 15
nightly-summary:  runs-on: ubuntu-latest     →  add  timeout-minutes: 5
```

- [ ] **Step 3: Verify every ci.yml + nightly.yml job has a timeout**

Run: `grep -c 'timeout-minutes:' .github/workflows/ci.yml .github/workflows/nightly.yml`
Expected: `ci.yml:5` (web, e2e, tauri, ci-summary, changes — changes added in Task 6; before Task 6 this is 4) and `nightly.yml:4`.

Run (count jobs vs timeouts — should match after Task 6):
`grep -cE '^  [a-z].*:$' .github/workflows/nightly.yml` vs `grep -c 'timeout-minutes' .github/workflows/nightly.yml`

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/nightly.yml
git commit -m "ci: add timeout-minutes to every ci.yml and nightly.yml job (G12)

A hung cargo/Playwright run now fails in minutes instead of burning the
6h default.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 6: Changes-detection path filter (web vs rust), `ci-summary` green-on-skip

**Files:**
- Modify: `.github/workflows/ci.yml`

**Context — the correctness-critical task.** The brief's hard constraint: *the path filter must never make `ci-summary` pass when a real check failed.* Design:
1. A `changes` job runs `dorny/paths-filter` and outputs `web`/`rust` booleans.
2. `web`/`e2e` gate on `web` changes; `tauri` gates on `rust` changes — **only for `pull_request`**. On `push`/`merge_group`, the filter is bypassed and all jobs run (paths-filter has no reliable base outside PRs).
3. `changes` is added to `ci-summary`'s `needs`, so if `changes` *fails*, downstream jobs skip but `ci-summary` sees the failure and exits 1 — a broken filter can never silently green the gate.
4. `ci-summary`'s existing loop already treats `skipped` as success, so a legitimately skipped stack keeps it green.

- [ ] **Step 1: Add the `changes` job (after `permissions:`, before `jobs:` body's first job)**

Insert as the first job under `jobs:`:
```yaml
jobs:
  changes:
    name: changes
    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions:
      contents: read
      pull-requests: read
    outputs:
      web: ${{ steps.filter.outputs.web }}
      rust: ${{ steps.filter.outputs.rust }}
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: dorny/paths-filter@d1c1ffe0248fe513906c8e24db8ea791d46f8590 # v3.0.3
        id: filter
        with:
          filters: |
            web:
              - 'web/**'
              - '.github/workflows/ci.yml'
              - 'justfile'
            rust:
              - 'src-tauri/**'
              - '.github/workflows/ci.yml'
              - 'justfile'
              - 'deny.toml'
```

- [ ] **Step 2: Gate `web`, `e2e`, `tauri` on the filter (PR-only)**

Add `needs: changes` and the `if:` guard to each. The guard runs the job when the event is not a PR (push/merge_group → always run) OR the relevant stack changed:

```yaml
  web:
    name: web
    needs: changes
    if: ${{ github.event_name != 'pull_request' || needs.changes.outputs.web == 'true' }}
    runs-on: ubuntu-latest
    timeout-minutes: 15
    ...
  e2e:
    name: e2e
    needs: changes
    if: ${{ github.event_name != 'pull_request' || needs.changes.outputs.web == 'true' }}
    runs-on: ubuntu-latest
    timeout-minutes: 20
    ...
  tauri:
    name: tauri
    needs: changes
    if: ${{ github.event_name != 'pull_request' || needs.changes.outputs.rust == 'true' }}
    runs-on: ubuntu-latest
    timeout-minutes: 30
    ...
```

- [ ] **Step 3: Add `changes` to `ci-summary` needs**

```yaml
  ci-summary:
    name: ci-summary
    if: always()
    needs: [changes, web, e2e, tauri]
    runs-on: ubuntu-latest
    timeout-minutes: 5
    ...
```

The existing summary loop is unchanged — `success|skipped` pass, anything else (including a `failure` on `changes`) exits 1.

- [ ] **Step 4: Validate the YAML parses**

Run: `python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml','.github/workflows/nightly.yml']]; print('yaml ok')"`
Expected: `yaml ok`

- [ ] **Step 5: Reason-check the skip matrix (no command — inspect)**

Confirm by reading the edited `ci.yml`:
- A docs-only PR (no `web/`, no `src-tauri/`) → `web`/`e2e`/`tauri` all skip → `ci-summary` sees `[success(changes), skipped, skipped, skipped]` → green. ✅
- A `src-tauri/`-only PR → `web`/`e2e` skip, `tauri` runs → green iff tauri passes. ✅
- If `changes` itself errors → downstream skip, `ci-summary` needs includes `changes` → `failure` → exit 1. ✅
- Editing `ci.yml`/`justfile` flags BOTH stacks (the filter lists them under both) → full run. ✅

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add web/rust changes-detection path filter to the T1 gate (G9)

dorny/paths-filter gates web/e2e/tauri on the touched stack for PRs only
(push/merge_group still run everything). ci-summary depends on the
changes job, so a broken filter fails the gate instead of silently
greening it; skipped stacks keep it green.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 7: Wire `cargo build` + supply-chain checks into the T1 `tauri`/`web` jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

**Context:** Route the new checks through `just` (Task 1 verbs). `web` job gains `just deny-web` (pnpm audit). `tauri` job gains `cargo-deny` install + `just deny-rust`, `--locked` on `cargo test`, and `just build-rust` (the promoted T1 build, G10). cargo-deny is installed via the already-pinned `taiki-e/install-action`.

- [ ] **Step 1: Add `just deny-web` to the `web` job (after `pnpm build`)**

```yaml
      - run: pnpm build
      - name: Supply-chain audit (npm)
        run: just deny-web
```

(`just` is preinstalled on `ubuntu-latest` GitHub runners via `extractions/setup-just`? No — it is NOT preinstalled. Add a setup step.) Insert a `just` setup step near the top of BOTH the `web` and `tauri` jobs, right after `actions/checkout`:

```yaml
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: just
```

So the `web` job's checkout is followed by the `just` install, then the existing pnpm/node setup, then the run steps ending with `just deny-web`.

- [ ] **Step 2: Update the `tauri` job — install just + cargo-deny, --locked, deny-rust, build-rust**

The `tauri` job steps become (checkout → just + cargo-deny install → toolchain → cache → fmt/clippy → test --locked → deny-rust → build-rust):

```yaml
  tauri:
    name: tauri
    needs: changes
    if: ${{ github.event_name != 'pull_request' || needs.changes.outputs.rust == 'true' }}
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - name: Install WebKitGTK + build deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
            libayatana-appindicator3-dev librsvg2-dev build-essential cmake
      - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # stable channel
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          workspaces: src-tauri
      - uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912 # v2.81.10
        with:
          tool: just,cargo-deny
      - run: cargo fmt --manifest-path src-tauri/Cargo.toml --check
      - run: cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml
      - name: Supply-chain audit (cargo-deny)
        run: just deny-rust
      - name: Compile the Tauri shell (T1 link/bundle check)
        run: just build-rust
```

> `taiki-e/install-action` accepts a comma-separated `tool:` list — `just,cargo-deny` installs both in one step. (`cargo build` runs after `cargo test`; the rust-cache warms the build so it is mostly incremental.)

- [ ] **Step 3: Verify locally what can be verified**

Run: `just build-rust`
Expected: `cargo build --locked` succeeds (exit 0). If `--locked` errors with "Cargo.lock needs update", the lockfile is stale — run `cargo update --workspace --manifest-path src-tauri/Cargo.toml` is NOT desired (that's an upgrade); instead investigate why it's out of sync and fix the lockfile to match `Cargo.toml`, then re-run.

Run: `just deny-rust`
Expected: `... ok` (from Task 2).

- [ ] **Step 4: Validate YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml ok')"`
Expected: `yaml ok`

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: promote tauri build + supply-chain checks onto the T1 gate (G10/G8/G11)

tauri job: cargo test --locked, just deny-rust (cargo-deny), just
build-rust (cargo build --locked). web job: just deny-web (pnpm audit).
All routed through just so local matches CI. Link/bundle breaks and dep
advisories now fail on the PR, not in nightly.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 8: Update `audit/devops.md` checklist

**Files:**
- Modify: `audit/devops.md`

- [ ] **Step 1: Flip the boxes this PR closes**

Check (`- [x]`) the checklist items for: Pin every `uses:` by SHA (G6), Add `renovate.json` (G7), Add `timeout-minutes` (G12), Promote `tauri build` onto T1 (G10), Add changes-detection path filter (G9), Add cargo-deny + lockfile + audit to T1 (G8/G11). Leave G1/G2/G3/G4/G5/G13/G14/G15 unchecked. Append a one-line note to the justfile item noting a minimal subset was added by session 91 (full justfile still G1/session 90).

- [ ] **Step 2: Commit**

```bash
git add audit/devops.md
git commit -m "docs(audit): mark T1 gate-hardening checklist items done (G6/G7/G9/G10/G11/G8/G12)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 9: Verification (verification-before-completion)

**Context:** Capture REAL output. Local proves deny/build/grep; CI proves the path-filter skip + ci-summary green.

- [ ] **Step 1: Local evidence — capture and keep the output**

```bash
echo "=== no mutable-tag uses remain ===" && grep -rEn 'uses: [^ ]+@(v[0-9]+|stable|beta|main|master)( |$)' .github/workflows/ || echo "CLEAN"
echo "=== every uses is SHA-pinned ===" && grep -rEoh 'uses: [^ ]+@[0-9a-f]{40}' .github/workflows/ | sort -u
echo "=== timeouts present ===" && grep -nH 'timeout-minutes' .github/workflows/ci.yml .github/workflows/nightly.yml
echo "=== just deny (both stacks) ===" && just deny
echo "=== just build-rust ===" && just build-rust
```
Expected: `CLEAN`; SHA list matches the table; timeouts on all ci/nightly jobs; `just deny` and `just build-rust` exit 0.

- [ ] **Step 2: Push and open as draft, watch CI**

```bash
git push -u origin shanghai
gh pr create -R tau-rs/cairn-web-ui --base main --draft \
  --title "ci: T1 gate hardening — SHA pins, Renovate, timeouts, cargo build, path filter, cargo-deny" \
  --body "<see Task 10 body>"
gh run watch $(gh run list -R tau-rs/cairn-web-ui --branch shanghai --limit 1 --json databaseId --jq '.[0].databaseId') --exit-status
```
Expected: `ci-summary` green; tauri job shows `cargo build` + `cargo deny` passing; timeouts visible in the run.

- [ ] **Step 3: Prove the path filter skips correctly — evidence from a real run**

On the PR run, confirm in `gh run view` that for THIS PR (touches `.github/`, `justfile`, `deny.toml` → both stacks flagged) all stacks ran. To prove the *skip* path, note it in the PR body and (optionally) verify by reading the `changes` job outputs in the run log. A docs-only follow-up commit can demonstrate the skip if reviewers want it; do not fabricate the claim — only state what the run shows.

> If pushing to `tau-rs/cairn-web-ui` is not possible from this worktree (the remote here is the audit fork / origin differs), push to the configured origin and open the PR against the correct base; capture whatever CI the origin runs. Record the actual remote used in the PR/summary rather than asserting `tau-rs`.

- [ ] **Step 4: requesting-code-review**

Invoke the `superpowers:requesting-code-review` skill. Focus the reviewer on: (a) no pin left as a mutable tag, (b) the path filter cannot green `ci-summary` when a real check failed (the `changes ∈ needs` + PR-only guard), (c) SHA pins resolve to the same versions (no silent upgrade).

---

## Task 10: Finalize PR

- [ ] **Step 1: Mark ready, finalize body**

PR body must cite **G6, G7, G12, G10, G9, G8, G11**, list the deferred items (G1 full justfile/session 90, G2 lefthook, G3/G4 heavy+release/session 92, G5 SBOM, G13 composite actions, G14 OIDC, G15 cosign/SLSA), and note the session-90 justfile dependency (minimal subset shipped here). End with the Claude Code attribution line.

```bash
gh pr ready
```

- [ ] **Step 2: STOP — do not merge.** Report the PR URL and the captured verification evidence.

---

## Self-Review

- **Spec coverage:** G6→T3, G7→T4, G12→T5, G9→T6, G10/G8/G11→T2+T7, justfile dependency→T1, docs→T8, verification→T9, PR→T10. All brief sub-items (a)-(f) mapped. ✅
- **Constraint coverage:** "no silent upgrades" → SHA table pins current versions + re-resolve note (T3). "path filter never greens a failed check" → `changes ∈ ci-summary.needs` + PR-only guard (T6 step 3/5). "route through justfile, fan out to both stacks" → `just deny` = `deny-web` + `deny-rust` (T1). "one coherent change, no heavy tier" → heavy/release explicitly deferred. ✅
- **Type/name consistency:** verb names `deny`/`deny-web`/`deny-rust`/`build-rust` identical across T1, T7, T9. SHA values identical across the table, T3, T6, T7. ✅
- **Placeholder scan:** PR body content deferred to T10 (acceptable — it's prose, fully specified by the cite list). No code placeholders. ✅
