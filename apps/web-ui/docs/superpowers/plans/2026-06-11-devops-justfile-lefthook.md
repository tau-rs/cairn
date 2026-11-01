# justfile + lefthook + CI repoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give cairn-ui a single source of truth for dev verbs (`justfile`) that fans out to BOTH stacks (pnpm `web/` + cargo `src-tauri/`), a fast `lefthook` pre-commit hook, and repoint `ci.yml`'s `web`/`tauri` jobs at the same `just` verbs so local ≡ CI (closes G1, G2).

**Architecture:** A root `justfile` defines fan-out verbs (`fmt`, `lint`, `test`, `build`, `deny`, `ci`, `fix`) each delegating to stack-scoped recipes (`web-*` via `cd web && pnpm …`, `rust-*` via `cargo … --manifest-path src-tauri/Cargo.toml`). Two CI-exact aggregate recipes (`web-ci`, `rust-ci`) reproduce the current inlined command sequences verbatim; `ci.yml` calls those. `lefthook.yml` pre-commit runs the fast fan-out verbs only (`fmt`+`lint`+`test`). README documents `lefthook install`.

**Tech Stack:** just 1.x, lefthook 2.x, pnpm 10, cargo, GitHub Actions.

**Scope guard:** ONLY G1 + G2 + the CI repoint. NO SHA-pinning (G6), NO `deny.toml` (G8), NO release tier (G3/G4) — those are sessions 91/92. `just deny` keeps the verb but no-ops when `deny.toml` is absent.

---

## File structure

- Create: `justfile` (repo root) — canonical verbs, fan-out to both stacks.
- Create: `lefthook.yml` (repo root) — fast pre-commit.
- Modify: `.github/workflows/ci.yml` — `web` job → `just web-ci`, `tauri` job → `just rust-ci`; add `just` install step to both.
- Modify: `README.md` — add a "Local development" section (just verbs + `lefthook install`).
- Modify: `docs/ci.md` — note that CI calls the same `just` verbs (anti-drift).

## Exact CI commands to reproduce (the spec)

`web` job (`ci.yml:38-42`, run in `web/`):
`pnpm lint` → `pnpm format:check` → `pnpm typecheck` → `pnpm test` → `pnpm build`

`tauri` job (`ci.yml:88-90`, run at root):
`cargo fmt --manifest-path src-tauri/Cargo.toml --check` → `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → `cargo test --manifest-path src-tauri/Cargo.toml`

`just web-ci` and `just rust-ci` MUST equal these exactly. (just runs recipes from the justfile's directory = repo root, so `cd web && …` and `--manifest-path src-tauri/Cargo.toml` resolve correctly regardless of the job's `working-directory`.)

---

### Task 1: Write the justfile

**Files:** Create `justfile`

- [ ] **Step 1: Write the justfile** with fan-out verbs + CI-exact aggregates + graceful `deny` skip (see implementation below).
- [ ] **Step 2: Verify each verb runs green** locally:
  - `just fmt` (prettier --write + cargo fmt — no-op on clean tree)
  - `just lint` (eslint+tsc + clippy -Dwarnings)
  - `just test` (vitest run + cargo test)
  - `just web-ci` and `just rust-ci` (the exact CI sequences)
  - `just deny` (pnpm audit + rust skip message, no deny.toml)
  Expected: all exit 0 (except `deny`/`pnpm audit` may report advisories — that verb is not gating this session).
- [ ] **Step 3:** `git add justfile` (commit at end of cluster).

### Task 2: Write lefthook.yml

**Files:** Create `lefthook.yml`

- [ ] **Step 1:** pre-commit with `just fmt` (stage_fixed), `just lint`, `just test` — fast verbs ONLY, no e2e/stryker/matrix/build.
- [ ] **Step 2:** `lefthook install`; verify `.git/hooks/pre-commit` is written and points at lefthook.

### Task 3: Repoint ci.yml

**Files:** Modify `.github/workflows/ci.yml`

- [ ] **Step 1:** Add a `just` install step (`extractions/setup-just@v3`) to `web` and `tauri` jobs.
- [ ] **Step 2:** Replace the 5 inlined `pnpm …` steps in `web` with a single `- run: just web-ci` (keep `pnpm install --frozen-lockfile`).
- [ ] **Step 3:** Replace the 3 inlined `cargo …` steps in `tauri` with a single `- run: just rust-ci`.
- [ ] **Step 4:** Confirm `e2e`, `ci-summary` unchanged.

### Task 4: Document

**Files:** Modify `README.md`, `docs/ci.md`

- [ ] **Step 1:** README "Local development" section: prerequisites (just, lefthook), `lefthook install`, the verb table.
- [ ] **Step 2:** docs/ci.md: note web/tauri jobs call `just web-ci`/`just rust-ci` (single source of truth).

### Task 5: Verify + PR

- [ ] Run `just fmt`, `just lint`, `just test`, `just ci` — capture REAL green output.
- [ ] requesting-code-review on the diff.
- [ ] Commit (Co-Authored-By: Claude Fable 5), push, `gh pr create -R tau-rs/cairn-web-ui --base main`, cite G1+G2.
- [ ] Confirm repointed CI `web`/`tauri` jobs run green in Actions. STOP — no merge.
