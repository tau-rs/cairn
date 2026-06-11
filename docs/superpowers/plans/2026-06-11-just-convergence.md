# `just` Convergence (D5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `just` verbs the single source of truth for cairn's build/test gate so lefthook (local) and `ci.yml` (CI) call the *same* command definitions and cannot silently drift (audit gap D5).

**Architecture:** Add a root `justfile` whose recipes are byte-equivalent to today's CI commands (caller supplies the environment — `CARGO_INCREMENTAL`, `CARGO_TARGET_DIR`). Rewrite `lefthook.yml` pre-commit/pre-push to call the verbs. Rewrite `ci.yml` `run:` steps to `run: just <verb>`. Add a `with-just` input to the `setup-rust` composite action so CI runners have `just`. This is a refactor toward one source of truth — **flag-for-flag identical to today's gate**, not a behavior change.

**Tech Stack:** `just` 1.x, cargo, cargo-nextest, cargo-deny, lefthook, GitHub Actions, `taiki-e/install-action`.

---

## Design decisions (resolved against tau's canonical template)

Tau's `justfile` is the canonical template the brief points to. Verified facts:

- `just fmt` is the **check** form (`cargo fmt --all -- --check`) — mirrors the CI `fmt` job, not the writing form.
- `just test *args` carries `--profile ci` and **forwards extra args** so lefthook can append `--target-dir`. Doctests are **not** folded into `test` (CI runs them as a separate job).
- `just deny` = `cargo deny --all-features check` (the action passes `--all-features` as the global flag; this is byte-for-byte what CI runs).
- Recipes carry **only** the cargo command; the caller supplies `CARGO_INCREMENTAL` / `CARGO_TARGET_DIR`. Never bake `CARGO_TARGET_DIR` into a recipe (it would clobber lefthook's per-command dirs).

Cairn-specific deltas from tau's template:
- `just lint` keeps `--locked` (cairn's CI clippy job uses `--locked`; tau's does not).
- Cairn's lefthook **pre-push** runs doc-tests + locked-check, and `ci.yml` has matching jobs — the exact duplication D5 flags. So cairn needs `doc-test` and `locked-check` verbs (tau left these raw); both `ci.yml` jobs *and* lefthook pre-push call them, eliminating the drift on both sides.
- Cairn has **no `[features]`** in any crate and **no xtask**, so `heavy` cannot do feature-powerset/xtask work. `heavy` = `ci` + a release build of the shipping binaries (the one T2 piece available with plain cargo today). The full T2 tier (powerset/fuzz/mutants/SBOM) is session 82.

### Verb → command table (each = today's exact gate)

| verb | command | mirrors |
|---|---|---|
| `fmt` | `cargo fmt --all -- --check` | `ci.yml` fmt job (`:38`), lefthook pre-commit fmt |
| `lint` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | `ci.yml` clippy job (`:51`), lefthook pre-commit clippy |
| `test *args` | `cargo nextest run --profile ci --workspace --all-targets {{args}}` | `ci.yml` test job (`:79`); lefthook pre-commit test (appends `--target-dir`) |
| `doc-test` | `cargo test --workspace --doc` | `ci.yml` doc-tests job (`:93`), lefthook pre-push doc-tests |
| `deny` | `cargo deny --all-features check` | `ci.yml` cargo-deny action (`:58-62`) — local parity only |
| `locked-check` | `cargo check --workspace --all-targets --locked` | `ci.yml` locked-check job (`:108`), lefthook pre-push locked-check |
| `ci` | `fmt lint test doc-test deny locked-check` | the full T1 local gate |
| `heavy` | `ci` + `cargo build --release -p cairn-cli -p cairn-daemon` | local approximation of T2 release build |
| `fix` | `cargo fmt --all` + `cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged` | local auto-fix |

**Intentional consequence:** lefthook pre-commit `test` switches from the default nextest profile (retries=2) to `--profile ci` (retries=0). This is the point of convergence — local == CI. Flake = signal, locally too.

---

## Task 1: Add the root `justfile`

**Files:**
- Create: `justfile`

- [ ] **Step 1:** Write `justfile` per the verb table above, with a header comment (adapted from tau) explaining that recipes carry only the cargo string and the caller supplies the environment.

- [ ] **Step 2:** Verify recipe list and dry-run parsing.

Run: `just --list`
Expected: lists `fmt lint test doc-test deny locked-check ci heavy fix` (+ `default`).

Run: `just --dry-run ci`
Expected: prints the six chained cargo commands, no errors.

- [ ] **Step 3:** Commit.

```bash
git add justfile
git commit -m "feat(devops): add root justfile (canonical verbs, D5)"
```

## Task 2: Route `lefthook.yml` through the verbs

**Files:**
- Modify: `lefthook.yml`

- [ ] **Step 1:** Rewrite each command's `run:` to call the verb while preserving the existing env isolation (per-command `CARGO_TARGET_DIR`, `CARGO_INCREMENTAL=0`, and the test command's `unset CARGO_TARGET_DIR GIT_*` + `--target-dir` append). Keep globs, `parallel: true`, and the pre-commit/pre-push split unchanged.
  - pre-commit fmt → `... just fmt`
  - pre-commit clippy → `... just lint`
  - pre-commit test → `bash -c 'unset ... && env CARGO_INCREMENTAL=0 just test --target-dir target/lefthook/test'`
  - pre-push doc-tests → `... just doc-test`
  - pre-push locked-check → `... just locked-check`

- [ ] **Step 2:** Verify hooks exercise the verbs.

Run: `lefthook run pre-commit`
Expected: runs fmt/lint/test via `just`, green.

Run: `lefthook run pre-push`
Expected: runs doc-test/locked-check via `just`, green.

- [ ] **Step 3:** Commit.

```bash
git add lefthook.yml
git commit -m "refactor(devops): route lefthook hooks through just verbs (D5)"
```

## Task 3: Route `ci.yml` run-steps through the verbs + install `just` in CI

**Files:**
- Modify: `.github/actions/setup-rust/action.yml` (add `with-just` input + install step)
- Modify: `.github/workflows/ci.yml` (5 run-steps → `just`; add `with-just: true` to those jobs)

- [ ] **Step 1:** Add a `with-just` boolean input (default `"false"`) to `setup-rust`, and an install step using `taiki-e/install-action@v2` with `tool: just`, gated on `inputs.with-just == 'true'`. Match cairn's existing **unpinned-tag** style (SHA-pinning is session 81).

- [ ] **Step 2:** In `ci.yml`, for the fmt, clippy, test, doc-tests, locked-check jobs: add `with-just: true` to the `setup-rust` `with:` block and change the `run:` line to the matching verb (`just fmt` / `just lint` / `just test` / `just doc-test` / `just locked-check`). Leave the `cargo-deny` job (action) and `ci-summary` untouched. Keep matrix/concurrency/permissions/env intact.

- [ ] **Step 3:** Verify YAML validity locally (the real check is the pushed CI run).

Run: `python3 -c "import yaml;[yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml','.github/actions/setup-rust/action.yml']];print('yaml ok')"`
Expected: `yaml ok`

- [ ] **Step 4:** Commit.

```bash
git add .github/actions/setup-rust/action.yml .github/workflows/ci.yml
git commit -m "refactor(devops): CI shells through just; install just via setup-rust (D5)"
```

## Task 4: Verification (verification-before-completion)

- [ ] **Step 1:** Run `just ci` locally and capture the real green output (requires cargo-deny installed; if absent, run each verb individually and note it).
- [ ] **Step 2:** Confirm `lefthook run pre-commit` and `lefthook run pre-push` are green via the verbs (captured in Task 2).
- [ ] **Step 3:** Push the branch and confirm `ci.yml` runs green now that every job shells through `just`. Paste the run conclusion.
- [ ] **Step 4:** `requesting-code-review` — confirm scope and that no verb dropped a flag vs the original `ci.yml`.

## Task 5: PR

- [ ] Open PR against `tau-rs/cairn:main`, cite D5, STOP (no merge — merge queue handles it).

---

## Self-review

- **Spec coverage:** justfile with fmt/lint/test/deny/ci/heavy/fix ✓ (+ doc-test/locked-check helpers required by cairn's pre-push); lefthook rewritten ✓; ci.yml run-steps → just ✓; matrix/setup-rust intact ✓; flag-for-flag verified in verb table ✓.
- **Placeholder scan:** none — every command is concrete.
- **Consistency:** verb names identical across justfile/lefthook/ci.yml. `just` install plumbing (Task 3) is the only scaffolding extension, required because runners lack `just`.
- **Out of scope (do not touch):** timeouts + SHA-pinning (session 81); heavy.yml / T2 tooling (session 82); cargo-deny job stays an action.
