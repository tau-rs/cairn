# cairn-web-ui — DevOps & CI/CD Audit

Scope: GitHub Actions, local developer experience (DX), and supply-chain
posture for the Tauri app (React/TypeScript `web/` frontend + Rust `src-tauri/`
shell embedding `web/dist`). This section adds the **canonical DevOps model**
shared verbatim across the four sibling repos (`cairn`, `cairn-ui`, `tau`,
`tau-ui`) and applies it to cairn-ui's two-stack reality.

The model is deliberately identical in *shape* across all four repos so they
never drift apart; the *jobs* differ by stack. cairn-ui lights up the most rows
because it is the only repo that is both web **and** Rust **and** ships a native
Tauri binary.

---

## 1. Current state

### What runs today (with evidence)

| Workflow | Trigger | Jobs | Notes |
|----------|---------|------|-------|
| `ci.yml` | push `main`, `pull_request`, `merge_group` (`.github/workflows/ci.yml:3-8`) | `web`, `e2e`, `tauri`, `ci-summary` | The PR fast gate. |
| `coverage.yml` | `pull_request`, push `main` (`coverage.yml:7-10`) | `vitest coverage` | Measurement, non-gating. |
| `nightly.yml` | `schedule` 03:00 + dispatch (`nightly.yml:7-10`) | `tauri-cross` (3-OS matrix), `e2e-full-browser` (3 browsers), `audit` (pnpm+cargo) | Drift-catcher, non-gating. |
| `mutation-weekly.yml` | `schedule` Mon 04:00 + dispatch (`mutation-weekly.yml:7-10`) | Stryker mutation testing | Heavy, weekly, non-gating. |
| `auto-rerun-flaky.yml` | cron */10 + dispatch | bounded auto-rerun of known-flaky jobs | Ops glue. |
| `auto-update-prs.yml` | push `main` + cron */30 + dispatch | updates behind PR branches | Ops glue, pairs with the protect-main concurrency policy. |
| `claude-review.yml` / `claude.yml` | PR / mention | AI review + `@claude` bot | Gated, fork-safe. |

### The PR fast gate (`ci.yml`) in detail

- Triggers include **`merge_group`** already (`ci.yml:7-8`) — merge queue is wired.
- Concurrency cancels superseded runs **except on `main`** (`ci.yml:13-15`).
- Top-level least-privilege `permissions: contents: read` (`ci.yml:17-18`).
- `web` job: `pnpm install --frozen-lockfile` → `lint` → `format:check` →
  `typecheck` → `test` → `build` (`ci.yml:37-42`). Scripts resolve to eslint,
  prettier, `tsc --noEmit`, `vitest run`, `tsc -b && vite build`
  (`web/package.json:13-17`).
- `tauri` job: `cargo fmt --check` → `cargo clippy -- -D warnings` →
  `cargo test`, all `--manifest-path src-tauri/Cargo.toml` (`ci.yml:88-90`),
  with `Swatinem/rust-cache` (`ci.yml:85-87`).
- `e2e` job: Playwright **chromium only** on PR (`ci.yml:60-62`); full 3-browser
  matrix is deferred to nightly (`nightly.yml:55-77`).
- `ci-summary` aggregator is the intended single required status check, green
  iff every job succeeded-or-skipped (`ci.yml:92-109`). Good pattern.

### What's good (keep)

- Single `ci-summary` required check already exists (`ci.yml:92-109`).
- `merge_group` already wired (`ci.yml:7-8`); concurrency protects `main`.
- Least-privilege `permissions:` on every workflow (e.g. `ci.yml:17-18`,
  `nightly.yml:16-17`, `coverage.yml:16-17`).
- Clippy is `-D warnings` (`ci.yml:89`); fmt + typecheck + format:check enforced.
- Tauri native build is already exercised cross-OS in nightly
  (`nightly.yml:20-53`) — most pure-Rust repos lack this.
- e2e (Playwright) and mutation testing (Stryker) already exist
  (`ci.yml:44-70`, `mutation-weekly.yml`).
- Dependency audit (`pnpm audit` + `cargo audit`) exists in nightly
  (`nightly.yml:87-108`).
- `Swatinem/rust-cache` + pnpm cache used throughout.

### Gaps vs the canonical model

| # | Gap | Evidence | Priority |
|---|-----|----------|----------|
| G1 | **No `justfile`** — no single source of truth for verbs; CI calls pnpm/cargo directly. | absent at repo root; `ci.yml:37-42`, `88-90` inline the commands | High |
| G2 | **No `lefthook`** — nothing runs fmt/lint/fast-test pre-commit; "passes locally" can diverge from CI. | no `lefthook.yml` anywhere | High |
| G3 | **No `v*`-tag HEAVY tier.** The "heavy lifting on release" lives in scattered `schedule`-only workflows (nightly, mutation-weekly) with no release trigger and no GitHub Release output. | `nightly.yml:7-10`, `mutation-weekly.yml:7-10` are `schedule`+dispatch only; no `push: tags: v*` anywhere | High |
| G4 | **No signed Tauri release artifacts / GitHub Release.** `bundle.active: true, targets: "all"` is configured but never invoked by CI; no updater signing. | `src-tauri/tauri.conf.json:26-39`; no release job | High |
| G5 | **No SBOM** for npm or cargo (CycloneDX). | no `cyclonedx`/`sbom` reference in `.github/` | Medium |
| G6 | **Actions pinned by mutable tag, not commit SHA.** Every `uses:` is `@v4`/`@stable`/`@beta`. | `ci.yml:28,29,32`, `nightly.yml:103-104`, `claude*.yml @beta` | High |
| G7 | **No Renovate/Dependabot** to bump SHA-pinned actions or sync the workflow template. | no `renovate.json`/`dependabot.yml` | High |
| G8 | **`cargo-deny` absent** (licenses/bans/advisories DB). Only `cargo audit` runs, nightly-only. | no `deny.toml`; `nightly.yml:103-108` | Medium |
| G9 | **No changes-detection / path filter** splitting web vs Rust. Every PR runs the full web + Rust + e2e set even for a one-file change in the other stack. | `ci.yml` jobs run unconditionally | Medium |
| G10 | **`tauri build` (`cargo build`) not on the PR gate** — only `cargo test`. A break that only surfaces at link/bundle time escapes to nightly. | `ci.yml:88-90` (no build); build is `nightly.yml:52-53` | Medium |
| G11 | **No supply-chain hardening on the PR gate**: no osv-scan / `npm audit` / lockfile `--locked` gate. `--frozen-lockfile` covers pnpm but Rust never runs `--locked`. | audit is nightly-only (`nightly.yml:87-108`) | Medium |
| G12 | **No `timeout-minutes` on `ci.yml`/`nightly.yml` jobs.** Only coverage (15), mutation (60), auto-rerun (10) set them. A hung Playwright/cargo build can burn the full 6h default. | `grep timeout-minutes`: only `coverage.yml:23`, `mutation-weekly.yml:23`, `auto-rerun-flaky.yml:40` | Medium |
| G13 | **No thin composite actions** for the repeated setup blocks (pnpm+node, rust toolchain+cache) — the same ~10 lines are copy-pasted across 4 workflows. | repeated in `ci.yml`, `nightly.yml`, `coverage.yml`, `mutation-weekly.yml` | Low |
| G14 | **No OIDC** posture documented for any future registry/cloud auth (currently none, but the release tier will need it). | n/a | Low |
| G15 | **cosign signing / SLSA provenance** absent (acceptable — phase-2 optional). | n/a | Low |

**Gap count by priority: High = 5 (G1, G2, G3, G4, G6, G7 → see note), Medium = 6, Low = 3.**
(Counting distinct rows: High = G1, G2, G3, G4, G6, G7 = **6**; Medium = G5, G8, G9, G10, G11, G12 = **6**; Low = G13, G14, G15 = **3**. Total **15**.)

---

## 2. Target model (canonical, applied to Tauri cairn-ui)

The model has four pillars: **B+C anti-drift**, **`just` as the universal
wrapper**, a **tiered pipeline** of identical shape, and **cross-cutting
hardening**.

### Pillar 1 — Anti-drift "B+C"

- **B (self-contained files):** each repo owns its FULL workflow YAML. No
  runtime `workflow_call` into a central repo, so one bad edit can never turn
  all four repos red at once, and any `ci.yml` is debuggable locally.
- **C (thin SHA-pinned composite actions):** a few SMALL composite actions for
  the *stable atomic steps only* — `setup-node` (pnpm+node+cache), `setup-rust`
  (toolchain+rust-cache) — pinned by commit SHA.
- A **sync bot** (Renovate, or `BetaHuhn/repo-file-sync-action`, or
  `multi-gitter`) opens a PR in each repo when the canonical template changes.
  Drift becomes a **visible, reviewable PR per repo**, not silent rot. A repo
  may intentionally decline a sync to keep a justified variation.
- **Phase-2 (optional):** a projen-style generator that *synthesizes* the YAML,
  with a CI check failing when `synth` output ≠ committed files. Adopt only if
  sync PRs become tedious.
- **Explicitly rejected:** a central reusable-workflow repo called at runtime
  via `workflow_call` with a moving tag — too much blast radius and debugging
  indirection.

```
DIAGRAM 1 — Anti-drift "B+C": self-contained files + synced template + thin composite actions

   canonical template (one source)
            │
   sync bot │ opens a REVIEWABLE PR per repo  (Renovate / repo-file-sync / multi-gitter)
            ▼
 ┌───────────────┬───────────────┬───────────────┬───────────────┐
 │   cairn       │   cairn-ui    │     tau        │    tau-ui     │
 │ ci.yml (FULL) │ ci.yml (FULL) │ ci.yml (FULL)  │ ci.yml (FULL) │   ← each self-contained,
 │   …           │   …           │   …            │   …            │     debuggable locally,
 └──────┬────────┴──────┬────────┴──────┬─────────┴──────┬────────┘     NO runtime SPOF
        │               │               │                │
        └───────────────┴───────┬───────┴────────────────┘
                                 ▼
              thin composite actions (SHA-pinned)
              .github/actions/setup-node   .github/actions/setup-rust
                                 ▲
                       Renovate bumps the SHAs

   drift = a visible OPEN PR you review per-repo, not silent rot.
   rejected: central reusable workflow @moving-tag called at runtime (blast radius).
```

### Pillar 2 — Tiered pipeline (same shape in all four repos)

```
DIAGRAM 2 — Tiered pipeline: TRIGGER → TIER → jobs → budget

 TRIGGER                         TIER                         BUDGET
 ───────────────────────────────────────────────────────────────────
 git commit / push (local) ───►  T0  lefthook + just         seconds
                                     fmt · lint · fast unit (staged)

 pull_request / merge_group ──►  T1  FAST GATE                < 10 min
                                     changes-detection (web│rust)
                                     → lint (eslint+tsc, clippy -Dwarn)
                                     → unit (vitest + cargo test)
                                     → deny + osv/npm audit
                                     → lockfile (pnpm --frozen, cargo --locked)
                                     → web build + tauri build
                                     → ci-summary  ◄── THE single required check
                                       (green if all pass OR all skip)

 push tag v* / workflow_dispatch ► T2 HEAVY                  minutes-hours
                                     OS matrix linux/macos/windows
                                     e2e (Playwright 3 browsers)
                                     mutation (Stryker) · coverage
                                     SBOM (cyclonedx npm + cargo)  ◄ core
                                     [cosign + SLSA provenance]    ◄ phase-2
                                     signed Tauri artifacts → GitHub Release

 schedule (nightly/weekly) ────►  T3  DRIFT-CATCHERS          non-gating
                                     nightly cross-OS · mutation · dep-review
                                     (a red T3 is a SIGNAL, never a merge gate)
```

### Pillar 3 — `just` as one source of truth (fans out to BOTH stacks)

```
DIAGRAM 3 — `just` verbs fan out to npm AND cargo (Tauri repo)

                 ┌──────────── justfile ────────────┐
                 │  identical verbs in all 4 repos  │
                 └──────────────────────────────────┘
   verb          ▼ web/ (pnpm)                  ▼ src-tauri/ (cargo)
   ─────────────────────────────────────────────────────────────────
   just fmt   →  prettier --write          +   cargo fmt
   just lint  →  eslint . + tsc --noEmit    +   cargo clippy -D warnings
   just test  →  vitest run                 +   cargo test
   just deny  →  pnpm audit                  +   cargo deny check
   just ci    →  build + lint + test (web)  +   build + clippy + test (rust)
   just heavy →  e2e (playwright) + stryker  +   OS matrix + SBOM
   just fix   →  eslint --fix + prettier    +   cargo fmt + clippy --fix

   lefthook (pre-commit)  ─┐
                            ├─►  invoke the SAME `just` verbs
   CI jobs (T1)           ─┘     ⇒ "passes locally" ≡ "passes in CI"
```

### Pillar 4 — cairn-ui lights up the most rows

```
DIAGRAM 4 — canonical building blocks: which rows cairn-ui turns ON

   building block            cairn   tau   cairn-ui (THIS)   tau-ui
   ───────────────────────────────────────────────────────────────
   cargo fmt/clippy/test       ✔      ✔        ✔ (src-tauri)    ✔
   eslint + tsc + vitest       —      —        ✔ (web/)         ✔
   web build (vite)            —      —        ✔                ✔
   tauri native build          —      —        ✔ (T1 + matrix)  ✔
   e2e (Playwright)            —      —        ✔                ✔
   mutation (Stryker)          *      *        ✔                ✔
   SBOM npm + cargo            cargo  cargo    ✔ BOTH           ✔
   signed Tauri release        —      —        ✔                ✔
   ───────────────────────────────────────────────────────────────
   ⇒ cairn-ui is the densest matrix: web ∪ Rust ∪ tauri ∪ e2e.
```

### Cross-cutting hardening (applies everywhere)

- Least-privilege `permissions:` per workflow, default `contents: read`
  (already true — keep).
- ALL third-party actions pinned by **commit SHA**, Renovate to bump.
- **OIDC** for any future registry/cloud auth instead of long-lived secrets
  (the release tier's signing/publish step).
- `Swatinem/rust-cache` save-only-on-`main`; pnpm cache (mostly in place).
- `timeout-minutes` on **every** job.
- `merge_group` merge queue (already wired, `ci.yml:7-8`).
- Concurrency groups cancel superseded PR runs (already wired, `ci.yml:13-15`).

---

## 3. Anti-drift & local DX, for cairn-ui specifically

- The **`justfile`** is where the canonical model bites hardest here, because
  cairn-ui is the only sibling whose verbs must fan out to **both** pnpm and
  cargo (Diagram 3). `just lint` = `eslint . && tsc --noEmit` **and**
  `cargo clippy -- -D warnings`; `just test` = `vitest run` **and**
  `cargo test`; `just deny` = `pnpm audit` **and** `cargo deny check`. CI's
  `web` and `tauri` jobs (`ci.yml:37-42`, `88-90`) then call `just …` instead of
  inlining commands, so the local and CI definitions cannot diverge.
- **lefthook is currently absent** and is the single biggest local-DX gap.
  Adding `lefthook.yml` with a pre-commit hook that runs `just fmt` + `just lint`
  + fast `vitest`/`cargo test` on staged files closes the "passes locally vs
  passes in CI" gap before a push ever happens.
- **Git hooks stay lightweight.** When lefthook lands here, pre-commit runs
  **only** the fast `just` verbs — `fmt`, `lint`, and fast staged tests for
  **both** stacks (eslint/tsc/vitest + clippy/`cargo test`) — seconds, never
  blocking. No heavy or container-based checks belong in git hooks. Heavy work
  (full OS matrix, tauri build, e2e, mutation) runs in the T2 `v*`-tag heavy CI
  tier and the T3 schedules — never on `git commit`/`git push`. A pre-push hook,
  if present, runs at most a fast `just ci` subset.
- **B+C** for cairn-ui: keep the four workflow files self-contained, extract the
  repeated pnpm+node and rust-toolchain+cache blocks (copy-pasted across
  `ci.yml`, `nightly.yml`, `coverage.yml`, `mutation-weekly.yml`) into two
  SHA-pinned composite actions, and let Renovate + a sync bot keep them aligned
  with the other three repos via reviewable PRs.

---

## 4. Implementation checklist (ordered)

Execute roughly top-to-bottom; each item notes **priority** and a one-line
rationale. cosign/SLSA stay optional phase-2.

- [x] **Add `justfile` with the canonical verbs** (`fmt`, `lint`, `test`,
  `deny`, `ci`, `heavy`, `fix`) fanning out to eslint/tsc/prettier/vitest
  **and** cargo fmt/clippy/test/deny. **High** — single source of truth; CI
  cannot diverge from local. *(G1)* — *canonical justfile landed in PR #40; the
  T1-gate-hardening PR activated `rust-deny` (via `deny.toml`), gate-hardened
  `web-deny` (`--audit-level=high`) and `rust-ci` (`--locked` test + `cargo
  build`).*
- [x] **Add `lefthook.yml`** pre-commit running `just fmt` + `just lint` + fast
  tests on staged files; document `lefthook install`. Pre-commit = fast `just`
  verbs only; no heavy/container checks in hooks. **High** — closes the
  local↔CI divergence gap; currently nothing runs pre-commit. *(G2)* — *landed
  in PR #40.*
- [x] **Repoint `ci.yml` `web`/`tauri` jobs at `just` verbs** instead of inline
  pnpm/cargo commands. **High** — makes Diagram 3 real. *(G1)* — *PR #40
  (`web-ci`/`rust-ci`); this PR adds the `web-deny`/`rust-deny` gate steps.*
- [x] **Pin every `uses:` by commit SHA** (drop `@v4`/`@stable`/`@beta`) across
  all 8 workflows. **High** — supply-chain integrity; mutable tags are a
  tag-move attack surface. *(G6)*
- [x] **Add `renovate.json`** to bump the pinned SHAs and act as the workflow
  sync bot. **High** — without it, SHA pins rot and the template can't sync.
  *(G7)* — *scoped to `github-actions` digests; Dependabot keeps npm/cargo.*
- [x] **Add `timeout-minutes` to every job in `ci.yml` and `nightly.yml`.**
  **Medium** — a hung cargo/Playwright run currently burns the 6h default. *(G12)*
- [x] **Promote `tauri build` (`cargo build`) onto the T1 gate** (single-OS) so
  link/bundle breaks fail on the PR, not in nightly. **Medium**. *(G10)* — *via
  `just build-rust` (`cargo build --locked`) on the `tauri` job.*
- [x] **Add changes-detection path filter** (web vs Rust) gating the `web`/
  `tauri`/`e2e` jobs; `ci-summary` stays green when a stack is skipped.
  **Medium** — keeps the <10 min budget on single-stack PRs. *(G9)* — *PR-only
  filter; push/merge_group run the full gate; `changes` ∈ `ci-summary` needs so
  a broken filter fails the gate, never a silent green.*
- [x] **Add `cargo-deny` (+ `deny.toml`)** and a lockfile/`--locked` +
  osv/`npm audit` step to the T1 gate. **Medium** — moves supply-chain checks
  off nightly-only onto the merge gate. *(G8, G11)* — *`just deny-rust`
  (cargo-deny) + `just deny-web` (pnpm audit) + `cargo test --locked`.*
- [ ] **Create `heavy.yml` triggered on `push: tags: v*` + `workflow_dispatch`;
  fold in the cross-OS matrix, 3-browser e2e, Stryker, and coverage.** Keep
  `nightly.yml`/`mutation-weekly.yml` schedules as T3 drift-catchers (or call
  the same composite jobs). **High** — there is no release-driven heavy tier
  today; "heavy lifting on feature release" has no trigger. *(G3)*
- [ ] **Add SBOM generation (CycloneDX for pnpm + cargo)** to `heavy.yml` as a
  core release artifact. **Medium** — supply-chain transparency for shipped
  binaries. *(G5)*
- [ ] **Build + bundle signed Tauri artifacts and attach to a GitHub Release**
  in `heavy.yml` (`bundle.active` is already configured,
  `src-tauri/tauri.conf.json:26-39`). Wire the Tauri updater signing key.
  **High** — the app is bundlable but never released by CI. *(G4)*
- [ ] **Extract thin composite actions** `setup-node` and `setup-rust` from the
  copy-pasted setup blocks; SHA-pin and reference them everywhere. **Low** —
  DRY + the "C" half of B+C. *(G13)*
- [ ] **Document OIDC** for the release/publish step instead of long-lived
  secrets. **Low** — future-proofs registry/cloud auth. *(G14)*
- [ ] **(Phase-2, optional)** Add cosign signing + SLSA provenance to the
  release artifacts. **Low** — defense-in-depth once core SBOM ships. *(G15)*
- [ ] **(Phase-2, optional)** Adopt a projen-style YAML generator with a
  `synth`-diff CI check if sync PRs become tedious. **Low**. *(Diagram 1)*

---

*Cross-refs: the disabled CSP (`src-tauri/tauri.conf.json:22-24`) is tracked in
`security.md` (S1) and is independent of this section; SBOM/signing here would
complement that webview hardening at the supply-chain layer.*
