# Monorepo: bring cairn-web-ui into cairn — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `tau-rs/cairn-web-ui` into this repo under `apps/web-ui/` with history, replace its git-rev engine pins with path deps, read ts-rs bindings in-tree, and retire the drift machinery.

**Architecture:** Three sequential PRs, each green and revertable alone. PR 1 is the mechanical import plus CI and Dependabot merge; the UI still builds from its vendored contract. PR 2 switches the web app to the engine's committed bindings and adds a bindings-current guard to engine CI. PR 3 resolves workflow overlap, merges the release train, updates local tooling, and closes the ledger.

**Tech Stack:** git subtree, Cargo path deps across an excluded workspace, Vite/TS path alias, GitHub Actions (dorny/paths-filter on PRs, `git diff` on merge groups), Dependabot, `just`, lefthook.

**Spec:** `docs/superpowers/specs/2026-11-01-monorepo-web-ui-design.md`
**Umbrella issue:** tau-rs/cairn#244

## Global Constraints

- Target directory is exactly `apps/web-ui/`; its `web/` and `src-tauri/` stay siblings (spec D1).
- Import is `git subtree add --prefix=apps/web-ui https://github.com/tau-rs/cairn-web-ui.git main` (D2).
- PR 1 merges as a merge commit: ruleset `main-merge-queue` (id `17547535`) is set to `merge_method=MERGE` before PR 1 is queued and back to `SQUASH` right after it merges. PR 2 and PR 3 squash (D2).
- `apps/web-ui/src-tauri` is listed in the root `Cargo.toml` `exclude`, never in `members` (D3).
- Six path deps, exactly: cairn-domain, cairn-app, cairn-infra, cairn-contract, cairn-service, cairn-ports, each `{ path = "../../../crates/<name>" }` (D3).
- Zero copies of generated bindings outside `crates/cairn-contract/bindings/` after PR 2 (D4).
- Bindings-current check is `cargo test -p cairn-contract` then `git diff --exit-code -- crates/cairn-contract/bindings` (D5).
- One `ci.yml`, one `ci-summary`; no new required check name (D6).
- Merge-group filtering uses `git diff --name-only <merge_group.base_sha> <merge_group.head_sha>`; `push` to `main` runs everything (D6).
- Renovate and `auto-update-prs.yml` are not imported (D7).
- Engine wins on every workflow overlap (D8).
- One release train: tag `v*` must equal `version` in `apps/web-ui/src-tauri/tauri.conf.json` or the release job fails (D11).
- Every PR body carries `Closes #<child issue>`; conventional commits, imperative, scoped.
- Engine CI jobs are not modified except `ci.yml` (new jobs, summary needs) and `coverage.yml` / `heavy.yml` additions named below.

## Review Focus

1. A merge group that batches a web-only PR with a Rust-only PR: `base_sha..head_sha` spans both, so both stacks must run. Pinned by Task 4's script test with a multi-commit range.
2. A PR that changes `crates/cairn-contract/src` without regenerating bindings: must fail the bindings-current job, not pass silently. Pinned by Task 9's negative test (stale binding edited by hand).
3. Root `rustfmt.toml` and `clippy.toml` now apply to `src-tauri` (both tools walk up directories): the UI's `rust-ci` may go red on formatting it never saw. Pinned by Task 3 running `just -f apps/web-ui/justfile rust-ci` before commit.
4. `cargo deny check sources` for src-tauri after the path switch: `unknown-git` flips to `deny` and must still pass. Pinned by Task 3.
5. Windows checkout of PR 2: the TS alias must be a forward-slash relative path in `tsconfig.json` and Vite `resolve.alias`, resolved with `path.resolve`, not a string concatenation; and `server.fs.allow` must include the bindings directory or the dev server returns 403 for `pluginValues.ts`. Pinned by Task 7's `pnpm typecheck && pnpm build`, a local `pnpm dev` smoke, and the e2e job on the PR.

---

## Phase 1 — PR 1: import, path deps, CI and Dependabot merge

Branch: `monorepo/import-web-ui`. Closes child issue A.

### Task 1: Ledger: child issues, follow-ons, pre-import freeze of the UI repo

**Files:** none in-tree.

**Interfaces:**
- Produces: issue numbers A (PR 1), B (PR 2), C (PR 3), and three follow-on issue numbers, recorded in `.context/monorepo-issues.md` for the later tasks.

- [ ] **Step 1: File three child issues on tau-rs/cairn**, each labelled `status:ready`, assigned to `LEBOCQTitouan`, body starting with `Part of #244` and one paragraph naming the PR's scope from the spec's Staging section. Titles:
  - A: `monorepo PR 1: subtree-import cairn-web-ui into apps/web-ui, path deps, CI + Dependabot merge`
  - B: `monorepo PR 2: web app reads ts-rs bindings in-tree; delete drift machinery; bindings-current CI guard`
  - C: `monorepo PR 3: workflow overlap, one release train, lefthook/README, ADR 0015, archive cairn-web-ui`

- [ ] **Step 2: File three follow-on issues on tau-rs/cairn**, labelled `status:gated` with reason "after #244":
  - `Fold apps/web-ui/src-tauri into the engine Cargo workspace once engine CI can skip it cheaply`
  - `Unify graph-viz and apps/web-ui/web JS tooling under a pnpm workspace`
  - `coverage.yml: upload both stacks under codecov flags`

- [ ] **Step 3: Resolve the UI repo's two human PRs so the import is a clean snapshot of `main`.**
  Run: `gh pr view 173 -R tau-rs/cairn-web-ui --json mergeable,statusCheckRollup -q '{m:.mergeable, s:[.statusCheckRollup[].conclusion]}'`
  If mergeable and all conclusions are SUCCESS: `gh pr merge 173 -R tau-rs/cairn-web-ui --squash`. Otherwise close it with the comment `Superseded by the monorepo move (tau-rs/cairn#244); roadmap will be reconciled in apps/web-ui/docs after the import.`
  Close #174 with the comment `Not imported: auto-update-prs.yml is dropped by tau-rs/cairn#244 (spec D7).`

- [ ] **Step 4: Record the numbers.** Write `.context/monorepo-issues.md` with one line per issue: `A=#..`, `B=#..`, `C=#..`, `F1=#..`, `F2=#..`, `F3=#..`.

- [ ] **Step 5: Create and push the claim branch.**
  ```bash
  git checkout -b monorepo/import-web-ui
  git commit --allow-empty -m "chore(monorepo): claim PR 1 lane (#A)"
  git push -u origin monorepo/import-web-ui
  ```

### Task 2: Subtree import

**Files:**
- Create: `apps/web-ui/**` (entire UI repo at its `main` HEAD)
- Create: `docs/superpowers/specs/2026-11-01-monorepo-web-ui-design.md` and this plan (already on disk; commit them here)

**Interfaces:**
- Produces: `apps/web-ui/web/`, `apps/web-ui/src-tauri/`, `apps/web-ui/justfile`, `apps/web-ui/.github/workflows/*`, `apps/web-ui/scripts/*`.

- [ ] **Step 1: Commit the spec and plan.**
  ```bash
  git add docs/superpowers/specs/2026-11-01-monorepo-web-ui-design.md docs/superpowers/plans/2026-11-01-monorepo-web-ui.md
  git commit -m "docs(monorepo): design + plan for importing cairn-web-ui (#A)"
  ```

- [ ] **Step 2: Record the UI HEAD you import.**
  Run: `git ls-remote https://github.com/tau-rs/cairn-web-ui.git refs/heads/main`
  Save the SHA; it goes in the PR body.

- [ ] **Step 3: Import.**
  ```bash
  git subtree add --prefix=apps/web-ui https://github.com/tau-rs/cairn-web-ui.git main
  ```
  Expected: one merge commit titled `Add 'apps/web-ui/' from commit '<sha>'`.

- [ ] **Step 4: Verify history and layout.**
  Run: `git log --oneline -- apps/web-ui | wc -l` → expected ≥ 842.
  Run: `ls apps/web-ui` → expected to list `web src-tauri justfile docs audit scripts CLAUDE.md README.md deny.toml lefthook.yml renovate.json .github .gitignore`.
  Run: `git status --porcelain` → expected empty.

### Task 3: Path dependencies, workspace exclude, deny sources, format

**Files:**
- Modify: `apps/web-ui/src-tauri/Cargo.toml` (the six `cairn-*` lines)
- Modify: `apps/web-ui/src-tauri/Cargo.lock` (regenerated)
- Modify: `Cargo.toml` root, `exclude` array
- Modify: `apps/web-ui/deny.toml` `[sources]`

**Interfaces:**
- Produces: a Tauri crate that resolves every `cairn-*` crate from `crates/` by path.

- [ ] **Step 1: Add the exclude.** In root `Cargo.toml`, change `exclude = ["fuzz"]` to `exclude = ["fuzz", "apps/web-ui/src-tauri"]`.

- [ ] **Step 2: Rewrite the six pins** in `apps/web-ui/src-tauri/Cargo.toml` to `{ path = "../../../crates/<name>" }`, keeping the six names and their order. Delete the comment in `apps/web-ui/.github/dependabot.yml` that says the cairn crates are git-pinned (the whole file is replaced in Task 5; skip if that is easier).

- [ ] **Step 3: Regenerate the UI lockfile and prove the source.**
  Run: `cargo update --manifest-path apps/web-ui/src-tauri/Cargo.toml -p cairn-domain -p cairn-app -p cairn-infra -p cairn-contract -p cairn-service -p cairn-ports`
  Run: `cargo metadata --manifest-path apps/web-ui/src-tauri/Cargo.toml --format-version 1 | jq -r '.packages[] | select(.name|startswith("cairn-")) | "\(.name) \(.source)"'`
  Expected: six lines, every `source` is `null`.

- [ ] **Step 4: Flip deny sources.** In `apps/web-ui/deny.toml`, set `unknown-git = "deny"` and replace the comment with `# All cairn-* deps are path deps inside the monorepo; no git sources remain.`
  Run: `cargo deny --manifest-path apps/web-ui/src-tauri/Cargo.toml check sources`
  Expected: `sources ok`.

- [ ] **Step 5: Run the UI's own Rust gate from the monorepo root** so root `rustfmt.toml` and `clippy.toml` are in effect.
  Run: `just -f apps/web-ui/justfile rust-ci`
  Expected: exit 0. If `cargo fmt --check` fails, run `just -f apps/web-ui/justfile rust-fmt` and include the result in this commit.

- [ ] **Step 6: Confirm the engine workspace is untouched.**
  Run: `cargo metadata --format-version 1 | jq '.workspace_members | length'` → expected 14.
  Run: `just lint` → exit 0.

- [ ] **Step 7: Commit.**
  ```bash
  git add Cargo.toml apps/web-ui/src-tauri/Cargo.toml apps/web-ui/src-tauri/Cargo.lock apps/web-ui/deny.toml apps/web-ui/src-tauri/src
  git commit -m "build(web-ui): depend on engine crates by path; exclude src-tauri from engine workspace (#A)"
  ```

### Task 4: CI merge: `changes` filter script, web/e2e/tauri jobs, summary

**Files:**
- Create: `.github/scripts/changed-stacks.sh`
- Create: `.github/scripts/changed-stacks.test.sh`
- Modify: `.github/workflows/ci.yml` (add `changes`, `web`, `e2e`, `tauri`; extend `ci-summary.needs`)
- Delete: `apps/web-ui/.github/workflows/ci.yml`

**Interfaces:**
- Produces: `changed-stacks.sh <base> <head>` prints exactly two lines `web=true|false` and `tauri=true|false` to stdout. Path lists (spec D6):
  - web: `apps/web-ui/web/`, `apps/web-ui/justfile`, `crates/cairn-contract/bindings/`, `.github/workflows/ci.yml`, `.github/scripts/changed-stacks.sh`
  - tauri: `apps/web-ui/src-tauri/`, `apps/web-ui/justfile`, `apps/web-ui/deny.toml`, `crates/`, `Cargo.lock`, `.github/workflows/ci.yml`, `.github/scripts/changed-stacks.sh`

- [ ] **Step 1: Write the failing script test** `.github/scripts/changed-stacks.test.sh` (bash, `set -euo pipefail`). It builds a throwaway git repo in `mktemp -d`, commits a baseline, then for each case commits the listed files and asserts the two output lines:
  - `web-only`: touch `apps/web-ui/web/src/x.ts` → `web=true` `tauri=false`
  - `rust-only`: touch `crates/cairn-domain/src/x.rs` → `web=false` `tauri=true`
  - `bindings`: touch `crates/cairn-contract/bindings/X.ts` → `web=true` `tauri=true`
  - `docs-only`: touch `README.md` → `web=false` `tauri=false`
  - `batched`: two commits, one web-only then one rust-only; range base..head → `web=true` `tauri=true`
  Run: `bash .github/scripts/changed-stacks.test.sh` → expected FAIL with `changed-stacks.sh: No such file`.

- [ ] **Step 2: Implement `.github/scripts/changed-stacks.sh`.** `git diff --name-only "$1" "$2"` piped through one `case` per path prefix; print the two lines. Make it executable.

- [ ] **Step 3: Run the test.** Run: `bash .github/scripts/changed-stacks.test.sh` → expected `ok` for all five cases, exit 0.

- [ ] **Step 4: Add the `changes` job** to `.github/workflows/ci.yml`, modelled on the UI's job but with: on `pull_request` use `dorny/paths-filter@d1c1ffe0248fe513906c8e24db8ea791d46f8590 # v3.0.3` with the two path lists above (glob form, `**`); on `merge_group` run `bash .github/scripts/changed-stacks.sh "${{ github.event.merge_group.base_sha }}" "${{ github.event.merge_group.head_sha }}"` with `fetch-depth: 0` on checkout and write its lines to `$GITHUB_OUTPUT`; on `push` set both `true`. Outputs `web` and `tauri`.

- [ ] **Step 5: Add `web`, `e2e`, `tauri` jobs** copied from `apps/web-ui/.github/workflows/ci.yml` with these edits only:
  - `if:` on each: `needs.changes.outputs.<stack> == 'true'` (no event special-casing; `changes` already handles it).
  - `working-directory: web` → `working-directory: apps/web-ui/web`; `cache-dependency-path: apps/web-ui/web/pnpm-lock.yaml`; artifact paths prefixed `apps/web-ui/`.
  - `just web-ci`, `just web-deny`, `just rust-ci`, `just rust-deny` → `just -f apps/web-ui/justfile <verb>` run from the repo root.
  - `tauri` job uses `./.github/actions/setup-rust` with `shared-key: tauri`, `components: rustfmt, clippy`, `with-just: true`, then installs `cargo-deny` via `taiki-e/install-action` as the engine's other jobs do; keep the WebKitGTK apt step verbatim. `setup-rust` already runs `Swatinem/rust-cache`, so the UI's separate cache step is dropped.
  - Drop the UI's `contract-drift` job entirely (PR 2 removes what it checked; until then the vendored copy is unchanged and the engine's own check is PR 2's).
  Pin every new action by the SHA the engine already uses where the same action appears (`actions/checkout`, `actions/upload-artifact`, `taiki-e/install-action`); for `pnpm/action-setup` and `actions/setup-node` keep the UI's SHAs.

- [ ] **Step 6: Extend `ci-summary.needs`** to `[fmt, clippy, cargo-deny, test, doc-tests, locked-check, neural, changes, web, e2e, tauri]`.

- [ ] **Step 7: Delete `apps/web-ui/.github/workflows/ci.yml`** (nested workflows never run; leaving it invites edits to a dead file).

- [ ] **Step 8: Lint the workflow.** Run: `actionlint .github/workflows/ci.yml` if installed, else `python3 -c 'import yaml,sys; yaml.safe_load(open(".github/workflows/ci.yml"))'` → exit 0.

- [ ] **Step 9: Commit.**
  ```bash
  git add .github/scripts/changed-stacks.sh .github/scripts/changed-stacks.test.sh .github/workflows/ci.yml apps/web-ui/.github/workflows/ci.yml
  git commit -m "ci(monorepo): gate web/e2e/tauri in ci.yml with per-stack path filters (#A)"
  ```

### Task 5: Dependabot merge

**Files:**
- Modify: `.github/dependabot.yml`
- Delete: `apps/web-ui/.github/dependabot.yml`, `apps/web-ui/renovate.json`

- [ ] **Step 1: Create the missing label.** Run: `gh label create javascript -R tau-rs/cairn --color F1E05A --description "npm / web dependencies"`.

- [ ] **Step 2: Append the UI's two entries** to `.github/dependabot.yml` verbatim, changing only `directory: /web` → `/apps/web-ui/web` and `directory: /src-tauri` → `/apps/web-ui/src-tauri`, and replacing the cargo entry's leading comment with `# The Tauri shell. cairn-* crates are path deps and never bumped here.`

- [ ] **Step 3: Delete** `apps/web-ui/.github/dependabot.yml` and `apps/web-ui/renovate.json`.

- [ ] **Step 4: Validate.** Run: `python3 -c 'import yaml; d=yaml.safe_load(open(".github/dependabot.yml")); print(len(d["updates"]))'` → expected `4`.

- [ ] **Step 5: Commit.**
  ```bash
  git add .github/dependabot.yml apps/web-ui/.github/dependabot.yml apps/web-ui/renovate.json
  git commit -m "chore(deps): fold web-ui npm + cargo Dependabot entries into the root config (#A)"
  ```

### Task 6: PR 1: open, flip merge method, babysit, flip back

- [ ] **Step 1: Push and open the PR** with `gh pr create --base main --title "feat(monorepo): import cairn-web-ui as apps/web-ui with path deps (#A)"`. Body: plain-language summary from the spec, the imported UI SHA from Task 2, a Mermaid `flowchart LR` of the three PRs, `Closes #A`, and the line `Merge as a merge commit: history import (spec D2).` End with the attribution line from the session reminder.

- [ ] **Step 2: Wait for CI green** (background watcher: `gh pr checks <n> --watch` in a background Bash). Confirm in the run that `web`, `e2e`, and `tauri` all executed (not skipped), since the PR touches both stacks.

- [ ] **Step 3: Confirm with the user, then flip the ruleset.**
  ```bash
  gh api repos/tau-rs/cairn/rulesets/17547535 > /tmp/ruleset.json
  jq '.rules |= map(if .type=="merge_queue" then .parameters.merge_method="MERGE" else . end)' /tmp/ruleset.json > /tmp/ruleset-merge.json
  gh api -X PUT repos/tau-rs/cairn/rulesets/17547535 --input /tmp/ruleset-merge.json
  ```
  Verify: `gh api repos/tau-rs/cairn/rulesets/17547535 --jq '.rules[]|select(.type=="merge_queue")|.parameters.merge_method'` → `MERGE`.

- [ ] **Step 4: Queue the merge.** Run: `gh pr merge <n> --merge --auto`. Wait for merge.

- [ ] **Step 5: Flip back.** Same as Step 3 with `SQUASH`; verify output `SQUASH`.

- [ ] **Step 6: Verify history on main.** Run: `git fetch origin && git log --oneline origin/main -- apps/web-ui | wc -l` → ≥ 842.

---

## Phase 2 — PR 2: in-tree bindings, drift machinery deleted, bindings guard

Branch: `monorepo/in-tree-bindings` from updated `main`. Closes child issue B.

### Task 7: Alias the engine bindings into the web app; delete the vendored copy

**Files:**
- Modify: `apps/web-ui/web/tsconfig.json` (`compilerOptions.paths`)
- Modify: `apps/web-ui/web/vite.config.ts` (`resolve.alias`)
- Modify: `apps/web-ui/web/src/contract/index.ts` (barrel re-exports)
- Modify: the 14 files listed below (import specifiers)
- Delete: every file under `apps/web-ui/web/src/contract/` except `index.ts`
- Modify: `apps/web-ui/web/.prettierignore` (remove the `src/contract/` block)

**Interfaces:**
- Produces: alias `@cairn-contract/*` → `../../../crates/cairn-contract/bindings/*` (relative to `apps/web-ui/web/`).

- [ ] **Step 1: Claim.** `git checkout main && git pull && git checkout -b monorepo/in-tree-bindings && git commit --allow-empty -m "chore(monorepo): claim PR 2 lane (#B)" && git push -u origin monorepo/in-tree-bindings`.

- [ ] **Step 2: Write the failing typecheck.** Delete `apps/web-ui/web/src/contract/source.ts` and all generated `.ts` files and the `serde_json/` dir under `apps/web-ui/web/src/contract/`, keeping `index.ts` only.
  Run: `cd apps/web-ui/web && pnpm install --frozen-lockfile && pnpm typecheck` → expected FAIL, `Cannot find module './Command'` and friends.

- [ ] **Step 3: Add the alias.** In `tsconfig.json` add `"baseUrl": "."` and `"paths": { "@cairn-contract/*": ["../../../crates/cairn-contract/bindings/*"] }`. In `vite.config.ts` add `resolve: { alias: { "@cairn-contract": path.resolve(__dirname, "../../../crates/cairn-contract/bindings") } }` using `fileURLToPath(new URL(".", import.meta.url))` for `__dirname` (ESM). Vitest inherits this from the same config. Also add `server: { fs: { allow: [".", bindingsDir] } }` with the same resolved path: Vite's dev server refuses files outside the project root by default, and `pluginValues.ts` is a value import that must be served in `pnpm dev` (which the Playwright e2e job uses).

- [ ] **Step 4: Rewrite the barrel.** Every line of `index.ts` becomes `export type { X } from "@cairn-contract/X";`. Add the three names imported directly elsewhere if absent: `Revision`, `PluginWidget`, and `export type { JsonValue } from "@cairn-contract/serde_json/JsonValue";`. If `index.ts` re-exports from `./pluginValues`, point it at `@cairn-contract/pluginValues` without the `type` keyword (it exports runtime arrays).

- [ ] **Step 5: Rewrite the 14 direct importers** to the alias. Exact files under `apps/web-ui/web/src/`:
  `components/StatusBar.tsx`, `components/plugins/WidgetView.tsx`, `components/plugins/WidgetView.test.tsx`, `components/plugins/pluginCommands.ts`, `components/plugins/pluginCommands.test.ts`, `components/history/HistoryList.tsx`, `components/history/HistoryList.test.tsx`, `components/history/groupRevisions.ts`, `components/history/groupRevisions.test.ts`, `components/history/versionSummary.ts`, `client/pluginContributions.ts`, `client/pluginBrokerHost.ts`, `client/pluginBroker.ts`, `store/store.ts`.
  Replacement: `"../contract/Revision"` and `"../../contract/Revision"` → `"@cairn-contract/Revision"`; same for `PluginWidget`; `".../contract/serde_json/JsonValue"` → `"@cairn-contract/serde_json/JsonValue"`. Imports of the bare barrel (`"../contract"`, `"../../contract"`) stay.

- [ ] **Step 6: Remove the prettier exclusion block** for `src/contract/` from `.prettierignore` (the bindings now live outside `web/`, so prettier never reaches them). Confirm eslint's `ignores` needs no change.

- [ ] **Step 7: Verify the gate.** Run from `apps/web-ui/web`: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build` → all exit 0. Then `pnpm dev --port 5273 --strictPort` in the background and `curl -s -o /dev/null -w '%{http_code}' http://localhost:5273/@fs/$(cd ../../.. && pwd)/crates/cairn-contract/bindings/pluginValues.ts` → `200`; stop the server.
  Run: `grep -rn "CONTRACT_SOURCE_COMMIT" apps/web-ui --include=*.ts --include=*.tsx --include=*.sh --include=*.md --include=*.yml` → expected: hits only in `scripts/` and `docs/` (handled in Task 8).

- [ ] **Step 8: Commit.**
  ```bash
  git add apps/web-ui/web
  git commit -m "refactor(web-ui): import ts-rs bindings from the engine crate via @cairn-contract alias (#B)"
  ```

### Task 8: Delete the drift machinery and its documentation

**Files:**
- Delete: `apps/web-ui/scripts/sync-contract.sh`, `apps/web-ui/scripts/check-contract-drift.sh`
- Modify: `apps/web-ui/docs/ci.md` (remove the `contract-drift` bullet and the DX3 paragraph), `apps/web-ui/README.md` (any sync instructions), `apps/web-ui/justfile` (any recipe calling the scripts)

- [ ] **Step 1: Find every reference.** Run: `grep -rln "sync-contract\|check-contract-drift\|contract-drift\|CONTRACT_SOURCE_COMMIT" apps/web-ui --exclude-dir=node_modules --exclude-dir=docs/superpowers` → list.

- [ ] **Step 2: Delete the two scripts; edit each listed file** to drop the reference. In `docs/ci.md` replace the removed bullet with one line: `Contract drift is impossible by construction: the web app imports the engine's committed bindings via the @cairn-contract alias, and engine CI's bindings-current job keeps that directory in step with the Rust source.` Files under `apps/web-ui/docs/superpowers/` are historical and are not edited.

- [ ] **Step 3: Verify.** Re-run the Step 1 grep → expected empty.

- [ ] **Step 4: Commit.**
  ```bash
  git add -A apps/web-ui/scripts apps/web-ui/docs/ci.md apps/web-ui/README.md apps/web-ui/justfile
  git commit -m "chore(web-ui): remove contract sync + drift scripts; bindings are read in-tree (#B)"
  ```

### Task 9: Engine CI: bindings-current guard

**Files:**
- Modify: `justfile` root (new recipe `bindings-check`)
- Modify: `.github/workflows/ci.yml` (new job `bindings-current`; `ci-summary.needs`)
- Modify: `.github/scripts/changed-stacks.sh` only if Task 4's web list lacks `crates/cairn-contract/bindings/` (it should not)

**Interfaces:**
- Produces: `just bindings-check` exits non-zero when committed bindings differ from what the Rust source generates.

- [ ] **Step 1: Negative test.** In `crates/cairn-contract/src/lib.rs` temporarily add a field `pub extra: u8` to `GraphEdge` (the Rust source now disagrees with the committed `GraphEdge.ts`). Run: `cargo test -p cairn-contract >/dev/null; git diff --exit-code -- crates/cairn-contract/bindings; echo "exit=$?"` → expected `exit=1` with `GraphEdge.ts` in the diff (ts-rs regenerated it). Then `git checkout -- crates/cairn-contract` to restore both. Note that hand-editing a binding file is not the failure mode: the test overwrites it, so the diff would be empty; only Rust-side drift is caught, which is the case that matters.

- [ ] **Step 2: Add the recipe** to the root `justfile`:
  ```
  # Committed ts-rs bindings must match the Rust source — mirrors the `bindings-current` CI job.
  bindings-check:
      cargo test -p cairn-contract
      git diff --exit-code -- crates/cairn-contract/bindings
  ```
  Add `bindings-check` to the `ci:` recipe's dependency list.

- [ ] **Step 3: Add the job** `bindings-current` to `ci.yml` using `./.github/actions/setup-rust` with `shared-key: linux`, `with-just: true`, step `just bindings-check`. Add it to `ci-summary.needs`. It runs unconditionally like the other engine jobs.

- [ ] **Step 4: Verify positive.** Run: `just bindings-check` → exit 0.

- [ ] **Step 5: Commit.**
  ```bash
  git add justfile .github/workflows/ci.yml
  git commit -m "ci(contract): fail when committed ts-rs bindings lag the Rust source (#B)"
  ```

### Task 10: PR 2: open and babysit

- [ ] **Step 1: Open** with title `refactor(web-ui): read engine bindings in-tree; retire contract drift machinery (#B)`, body with plain-language summary, a 3-row before/after table (vendored copy / commit stamp / drift job → alias / none / bindings-current), `Closes #B`, attribution.
- [ ] **Step 2: Babysit to merge** (squash, auto-merge). Confirm `web`, `e2e`, `bindings-current` ran green.

---

## Phase 3 — PR 3: workflow overlap, release train, tooling, ledger, ADR

Branch: `monorepo/tooling-and-ledger` from updated `main`. Closes child issue C and #244.

### Task 11: Scheduled and automation workflows (D8, minus heavy)

**Files:**
- Create: `.github/workflows/web-nightly.yml` (from `apps/web-ui/.github/workflows/nightly.yml`)
- Create: `.github/workflows/web-mutation-weekly.yml` (from `mutation-weekly.yml`)
- Create: `.github/workflows/dependabot-automerge.yml` (from the UI's, unchanged)
- Modify: `.github/workflows/coverage.yml` (add job `vitest-coverage`)
- Delete: `apps/web-ui/.github/` entirely after the moves

- [ ] **Step 1: Claim** branch `monorepo/tooling-and-ledger` as in Task 7 Step 1 with `(#C)`.

- [ ] **Step 2: Move nightly and mutation** with these path rewrites only: `working-directory: web` → `apps/web-ui/web`; `cache-dependency-path: web/pnpm-lock.yaml` → `apps/web-ui/web/pnpm-lock.yaml`; `--manifest-path src-tauri/Cargo.toml` → `apps/web-ui/src-tauri/Cargo.toml`; `--file src-tauri/Cargo.lock` → `apps/web-ui/src-tauri/Cargo.lock`; `Swatinem/rust-cache` `workspaces: src-tauri` → `apps/web-ui/src-tauri`; artifact `path:` entries prefixed `apps/web-ui/`. Rename `name:` to `web-nightly` and `web-mutation-weekly`; `concurrency.group` likewise.

- [ ] **Step 3: Copy `dependabot-automerge.yml`** unchanged.

- [ ] **Step 4: Add `vitest-coverage` job** to engine `coverage.yml`: the UI's `coverage` job body with the Task 11 Step 2 path rewrites and artifact name `lcov-web-${{ github.sha }}`.

- [ ] **Step 5: Delete `apps/web-ui/.github/`** (remaining files: `claude.yml`, `auto-rerun-flaky.yml`, `auto-update-prs.yml`, `heavy.yml` is consumed by Task 12 first; do this step after Task 12's copy).

- [ ] **Step 6: Validate YAML** for each new/edited file as in Task 4 Step 8.

- [ ] **Step 7: Commit** `ci(monorepo): move web nightly, mutation, automerge, coverage into root workflows (#C)`.

### Task 12: Heavy tier and one release train (D11)

**Files:**
- Modify: `.github/workflows/heavy.yml` (add `tauri-bundle`, `web-sbom`, `tag-matches-desktop-version`; extend `release-publish`)
- Create: `.github/scripts/check-desktop-version.sh`
- Create: `.github/scripts/check-desktop-version.test.sh`

**Interfaces:**
- Produces: `check-desktop-version.sh <ref_name>` exits 0 iff `<ref_name>` equals `v` + `.version` from `apps/web-ui/src-tauri/tauri.conf.json`.

- [ ] **Step 1: Write the failing test** `.github/scripts/check-desktop-version.test.sh`: three cases against a temp `tauri.conf.json` with `"version": "0.2.0"` (pass the path via a second argument): `v0.2.0` → exit 0; `v0.3.0` → exit 1; `0.2.0` (no `v`) → exit 1.
  Run → expected FAIL, script missing.

- [ ] **Step 2: Implement** with `jq -r .version "$2"` and a string compare; print `tag $1 != desktop version v$ver` on mismatch. Run the test → `ok` ×3.

- [ ] **Step 3: Add jobs to `heavy.yml`:**
  - `tag-matches-desktop-version`: `if: startsWith(github.ref, 'refs/tags/v')`, runs the script with `${{ github.ref_name }}` and the conf path.
  - `tauri-bundle`: the UI's `build-bundle` matrix job with Task 11 path rewrites; `projectPath: apps/web-ui/src-tauri`; artifact name `tauri-bundle-${{ matrix.os }}`.
  - `web-sbom`: the UI's `sbom` job's pnpm half only (the engine's `sbom` job already covers cargo; the Tauri crate's cargo SBOM is added to the engine `sbom` job by one extra `cargo cyclonedx --manifest-path apps/web-ui/src-tauri/Cargo.toml` line). Artifact name `sbom-web-${{ github.sha }}`.
  - `release-publish.needs` gains `tag-matches-desktop-version, tauri-bundle, web-sbom`; its "Stage release assets" step also copies `artifacts/tauri-bundle-*/**` files and `artifacts/sbom-web-*/**` into `release/`. Keep `draft` unset (engine policy: published, prerelease on hyphen).

- [ ] **Step 4: Validate YAML; commit** `ci(release): one release train — desktop bundles + web SBOM ship with the engine tag (#C)`.

- [ ] **Step 5: Now run Task 11 Step 5** (delete `apps/web-ui/.github/`) and commit `chore(web-ui): drop nested .github; workflows live at the root (#C)`.

### Task 13: Local tooling and READMEs (D9)

**Files:**
- Modify: `lefthook.yml` root (add `web-fmt`, `web-lint`, `web-test` to `pre-commit`)
- Delete: `apps/web-ui/lefthook.yml`
- Modify: `apps/web-ui/README.md`, `README.md`

- [ ] **Step 1: Root lefthook.** Under `pre-commit.commands` add three commands, each `glob: "apps/web-ui/web/**"`, running `just -f apps/web-ui/justfile web-fmt` (with `stage_fixed: true`), `web-lint`, `web-test`. Delete `apps/web-ui/lefthook.yml`.
  Verify: `lefthook run pre-commit --files apps/web-ui/web/src/App.tsx` → the three web commands execute; `lefthook run pre-commit --files crates/cairn-domain/src/lib.rs` → they are skipped.

- [ ] **Step 2: UI README.** Replace the badge with the engine's CI badge URL, replace the "Local development" clone instructions with `cd apps/web-ui` from a cairn checkout, add one line under the title: `Lives in the cairn monorepo since 2026-11 (tau-rs/cairn#244); the former standalone repo is archived.`

- [ ] **Step 3: Engine README.** After the Status paragraph add one paragraph: what `apps/web-ui/` is, that it links the engine in-process (Tauri) or over the daemon, and `just -f apps/web-ui/justfile` as the entry point.

- [ ] **Step 4: Commit** `docs(monorepo): root lefthook runs web verbs; READMEs point at apps/web-ui (#C)`.

### Task 14: ADR 0015

**Files:**
- Create: `docs/decisions/0015-monorepo-web-ui.md`

- [ ] **Step 1: Write the ADR** in the format of `docs/decisions/0014-crdt-collaboration-model.md` (read it for headings). Content: context (two seams, four drift incidents), decision (D1–D11 as one-line each with the why), consequences (two lockfiles, heavier merge-queue only when stacks change, one release train with the tag=version rule), and the rollback (revert the PR 1 merge commit with `-m 1`).

- [ ] **Step 2: Commit** `docs(adr): 0015 cairn-web-ui joins the monorepo (#C)`.

### Task 15: PR 3, ledger close-out, archive

- [ ] **Step 1: Open PR 3** titled `chore(monorepo): workflow overlap, one release train, tooling, ADR 0015 (#C)`, body with the D8 table from the spec, `Closes #C`, `Closes #244`, attribution. Babysit to merge (squash).

- [ ] **Step 2: Close UI issues with evidence.** On tau-rs/cairn-web-ui: close #180 with `Re-pinning is no longer a thing: the engine is a path dependency and the bindings are read in-tree. See tau-rs/cairn#244 and ADR 0015.`; close #181 with `Both seams were removed rather than policed: tau-rs/cairn#244, spec D3/D4/D5. Rule chosen: equality by construction (one commit).`

- [ ] **Step 3: Close the Dependabot PRs** on the UI repo: `gh pr list -R tau-rs/cairn-web-ui --author app/dependabot --json number -q '.[].number' | xargs -I{} gh pr close {} -R tau-rs/cairn-web-ui -c "Repo moved to tau-rs/cairn/apps/web-ui; Dependabot now runs there."`

- [ ] **Step 4: Archive.** Push one last commit to the UI repo's `main` replacing `README.md` with a three-line pointer to `https://github.com/tau-rs/cairn/tree/main/apps/web-ui` (via a PR if the ruleset requires it), set the description `Moved to tau-rs/cairn/apps/web-ui`, then `gh repo archive tau-rs/cairn-web-ui -y`. Confirm with the user before this step: it is the one action in the plan that is outward-facing and not trivially reversible.

- [ ] **Step 5: Verify ledger.** `gh issue view 244 -R tau-rs/cairn --json state -q .state` → `CLOSED`; the three follow-on issues remain open and `status:gated` is replaced by `status:ready`.
