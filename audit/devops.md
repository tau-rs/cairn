# Cairn — DevOps & CI/CD Audit

This section audits cairn's CI/CD against the **canonical DevOps model** shared
verbatim across the four sibling projects (cairn, cairn-ui, tau, tau-ui). The
four repos deliberately share one model so they stay consistent; this document
presents that model tailored to cairn's **pure-Rust** workspace, measures the
current pipeline against it, and gives an ordered implementation checklist.

All citations are `path:line` against the `audit/design-security` worktree.

---

## 1. Current state

Cairn already has a thoughtful, well-commented CI setup — notably further along
than a greenfield repo. What follows is what it does today, what is already
good, and the concrete gaps versus the canonical model.

### What runs today

| Workflow | Trigger | Purpose |
|---|---|---|
| `ci.yml` | `push` main, `pull_request`, `merge_group` | The fast gate |
| `coverage.yml` | `pull_request`, `push` main | Coverage measurement (non-gating) |
| `claude-review.yml` | `pull_request` (gated by var) | AI PR review bot |
| `claude.yml` | issue/PR comments (`@claude`) | AI mention bot |
| `auto-rerun-flaky.yml` | `schedule` */10m, dispatch | Re-run flaky *Claude-review* runs only |
| `auto-update-prs.yml` | `push` main, schedule */30m, dispatch | Keep behind PRs up-to-date |

**The `ci.yml` fast gate** (`.github/workflows/ci.yml`):
- Jobs: `fmt` (`ci.yml:29`), `clippy` (`ci.yml:40`, `-D warnings` at `:51`),
  `cargo-deny` (`ci.yml:53`), `test` across a full
  linux/macos/windows matrix (`ci.yml:64-79`), `doc-tests` (`ci.yml:81`),
  `locked-check` doubling as the MSRV 1.88 build (`ci.yml:95-108`).
- A single **`ci-summary`** aggregator (`ci.yml:110-127`) that is green only
  when every needed job succeeded *or skipped* — this is the canonical
  "one required status check" pattern, already in place.
- `merge_group` trigger present (`ci.yml:7`) → merge-queue ready.
- `concurrency` with `cancel-in-progress` everywhere **except main**
  (`ci.yml:14-16`) — exactly the canonical nuance (don't cancel the
  cache-writing main run).
- Workflow-level `permissions: contents: read` (`ci.yml:25-26`).
- `cargo-deny` config (`deny.toml`) covers advisories + licenses + bans +
  sources (`deny.toml:8-56`).
- Caching via a thin local composite action `./.github/actions/setup-rust`
  (`setup-rust/action.yml`) wrapping `Swatinem/rust-cache` with
  **`save-if` only on main** (`setup-rust/action.yml:93-99`) — the canonical
  cache discipline.
- `.config/nextest.toml` sets `retries = 0` in the CI profile on purpose
  (flake = signal), with `auto-rerun-flaky.yml` structurally scoped to the
  Claude-review bot **only** so test jobs are never silently retried
  (`auto-rerun-flaky.yml:5-25, 73-75`).

**Dependency hygiene:** `.github/dependabot.yml` updates both `github-actions`
and `cargo` ecosystems (`dependabot.yml:11, 29`), grouped by crate family
(`dependabot.yml:42-54`).

### Already good (keep, do not regress)

- ✅ `ci-summary` single required check (`ci.yml:110-127`).
- ✅ `cargo-deny` advisories+licenses+bans+sources (`deny.toml`).
- ✅ `--locked` lockfile / MSRV-pin guard (`ci.yml:95-108`; mirrored in
  `lefthook.yml:34-37`).
- ✅ Least-privilege default `permissions: contents: read` on `ci.yml` and
  `coverage.yml` (`ci.yml:25`, `coverage.yml:21`).
- ✅ `Swatinem/rust-cache` with `save-if` on main (`setup-rust/action.yml:99`).
- ✅ Concurrency cancel-in-progress except on main (`ci.yml:14-16`).
- ✅ Merge-queue (`merge_group`) wired (`ci.yml:7`).
- ✅ Full OS matrix already on the `test` job (`ci.yml:67-70`).
- ✅ A local pre-commit/pre-push gate via `lefthook.yml` that mirrors CI verbs.
- ✅ Thin composite action for setup/caching (`setup-rust/action.yml`) — the
  "C" half of the canonical B+C model already exists.

### Gaps vs the canonical model

Each gap carries a priority (High / Medium / Low) and a one-line rationale.

| # | Gap | Priority | Rationale |
|---|---|---|---|
| D1 | **No `timeout-minutes` on any `ci.yml` job** (`ci.yml` has none; only `coverage.yml:28` and `auto-rerun-flaky.yml:56` set one). | **High** | A hung runner (network/sccache stall) burns the full 6h default and blocks the merge queue. Canonical: timeout on *every* job. |
| D2 | **All third-party actions pinned by mutable tag, not commit SHA.** `actions/checkout@v6` (`ci.yml:33`), `EmbarkStudios/cargo-deny-action@v2` (`:58`), `dtolnay/rust-toolchain@stable` (`setup-rust:45`), `rui314/setup-mold@v1` (`:52`), `mozilla-actions/sccache-action@v0.0.10` (`:58`), `taiki-e/install-action@v2` (`:88`), `Swatinem/rust-cache@v2` (`:93`), `actions/upload-artifact@v7` (`coverage.yml:66`), `anthropics/claude-code-action@beta` (`claude*.yml`). | **High** | A moving tag means an upstream compromise/retag executes in CI with repo permissions. Canonical: SHA-pin all third-party actions + bot to bump them. |
| D3 | **No T2 HEAVY tier** — there is no `v*`-tag / `workflow_dispatch` workflow at all. No OS-matrix-on-release (the matrix lives in the PR gate instead), no separate MSRV job beyond the lock guard, no feature-powerset, no fuzz, no mutants, no coverage-on-release, no GitHub Release build. | **High** | This is the user's "heavy lifting on feature release". Today nothing runs on a tag; releases are unverified beyond the PR gate. |
| D4 | **No supply-chain artifacts.** No SBOM (cyclonedx), no provenance. | **Medium** | Canonical T2 core = SBOM (cyclonedx); cosign + SLSA are optional phase-2. Cairn ships a CLI + daemon binary with no bill-of-materials. |
| D5 | **No `justfile`** — local DX and CI invoke cargo directly via separate `lefthook.yml` runs (`lefthook.yml:18-37`) and separate `ci.yml` `run:` lines. The two can drift. | **Medium** | Canonical: `just` verbs are the single source of truth so "passes locally" and "passes in CI" cannot diverge. Cairn has no xtask, so the justfile delegates straight to cargo. |
| D6 | **No anti-drift sync mechanism across the 4 repos.** Dependabot bumps deps but nothing keeps the *workflow templates* aligned. | **Medium** | Canonical B+C: a sync bot opens reviewable per-repo PRs when the canonical template changes; drift becomes a visible PR, not silent rot. |
| D7 | **No T3 scheduled drift-catchers for code health.** The two schedules that exist (`auto-rerun-flaky.yml:28`, `auto-update-prs.yml:47`) are housekeeping, not nightly-fuzz / weekly-mutants / dependency-review. | **Low** | Canonical T3 = slow drift-catchers that must not block release. Lower priority once T2 exists. |
| D8 | **No OIDC / cloud auth posture documented.** Not currently needed (no registry/cloud push), but a future SBOM/cosign step should use OIDC, not long-lived secrets. | **Low** | Forward-looking; only bites when T2 supply-chain lands. |
| D9 | **`clippy.toml` is empty** (`clippy.toml:1`) and `rustfmt.toml` pins only `edition` (`rustfmt.toml:1`). | **Low** | Not a CI gap per se, but the shared model expects identical lint config across the 4 repos; worth aligning when the template syncs. |

**Gap count by priority: 3 High (D1, D2, D3), 3 Medium (D4, D5, D6), 3 Low
(D7, D8, D9).**

> Note: cairn's PR gate is actually *richer* than the canonical T1 in one
> respect — it already runs the full OS matrix on every PR. The canonical model
> puts the OS matrix in T2 (release) to keep T1 under ~10 min. Cairn can keep
> the linux-only fast path on PRs and move the macos/windows matrix to T2, or
> keep it as a justified intentional variation (the B+C model explicitly allows
> a repo to decline a sync to preserve such a variation).

---

## 2. Target model (canonical model applied to pure-Rust cairn)

The model is **B+C anti-drift + `just` local DX + a tiered pipeline + cross-cutting
hardening**. Applied to cairn:

- **T0 — local** (`lefthook` + `just`): fmt, lint, fast unit tests on staged
  files. Seconds. (cairn already has the lefthook half.)
- **T1 — PR / merge_group fast gate** (target < 10 min): paths-filter
  changes-detection → `fmt` + `clippy -D warnings` → unit + doc tests →
  `cargo-deny` → lockfile `--locked` → build → **`ci-summary`** single required
  check (green if all pass *or* all skip — handles docs-only PRs). Concurrency
  cancel-in-progress except main; `timeout-minutes` on every job. (cairn has
  most of this; gaps are timeouts D1 and changes-detection.)
- **T2 — HEAVY**, on `push` of a `v*` tag + `workflow_dispatch` (the "heavy
  lifting on feature release"): full OS matrix, MSRV check, **feature-powerset**,
  fuzz, mutation testing, coverage, e2e/conformance, and supply-chain — **SBOM
  (cyclonedx) as core**, **cosign signing + SLSA provenance as
  recommended-optional phase-2** — then a release build → GitHub Release.
- **T3 — scheduled** (nightly/weekly): slow drift-catchers that must NOT block
  release — nightly fuzz, weekly mutants, dependency review.

**Considered and rejected:** a central reusable-workflow repo called at runtime
via `workflow_call` with a moving tag. Rejected because one change there can
turn all four repos red at once and adds debugging indirection — too much blast
radius. Cairn instead owns its full self-contained workflow files.

### Diagram 1 — Anti-drift "B+C"

```
   canonical template (in one of the 4 repos / a docs repo)
                         │
                         │  sync bot (Renovate file-sync /
                         │  repo-file-sync-action / multi-gitter)
                         │  opens a REVIEWABLE PR per repo
        ┌────────────────┼────────────────┬────────────────┐
        ▼                ▼                ▼                ▼
   ┌─────────┐      ┌─────────┐     ┌─────────┐      ┌─────────┐
   │  cairn  │      │ cairn-ui│     │   tau   │      │  tau-ui │
   │ ci.yml  │      │ ci.yml  │     │ ci.yml  │      │ ci.yml  │  ← FULL, self-contained
   │ heavy.yml│     │ heavy.yml│    │ heavy.yml│     │ heavy.yml│    (debuggable locally,
   └────┬────┘      └────┬────┘     └────┬────┘      └────┬────┘     no runtime SPOF)
        │                │                │                │
        └────────────────┴───────┬────────┴────────────────┘
                                  ▼
                 thin composite actions, SHA-pinned
                 (setup-rust, cache strategy)  ← cairn already has setup-rust

   A change to the template ⇒ a VISIBLE open PR in each repo you review.
   Any repo MAY decline a sync to keep a justified variation.
   No central runtime workflow ⇒ one change can't turn all repos red at once.
```

### Diagram 2 — Tiered pipeline

```
 TRIGGER                       TIER   JOBS                                   BUDGET
 ───────────────────────────   ────   ───────────────────────────────────   ──────
 commit (pre-commit/push)      T0     fmt · lint · fast unit (staged)        seconds
   via lefthook + just          │     [cairn: lefthook.yml ✓]
                                 ▼
 pull_request / merge_group     T1     changes-detect → fmt → clippy -Dwarn  <10 min
                                 │     → unit+doc tests → cargo-deny →
                                 │     --locked → build
                                 │            │
                                 │            ▼
                                 │     ci-summary  ◀── THE one required check
                                 │     (green if all pass OR all skip)
                                 ▼     [cairn: ci.yml ✓ ; add timeouts + changes-detect]
 push tag v*  / workflow_dispatch T2    OS matrix · MSRV · feature-powerset ·  minutes-hours
   ("heavy lifting on release")  │     fuzz · mutants · coverage · e2e ·
                                 │     SBOM(cyclonedx, core) · [cosign+SLSA
                                 │     phase-2] · release build → GH Release
                                 ▼     [cairn: MISSING — add heavy.yml]
 schedule (nightly / weekly)     T3    nightly fuzz · weekly mutants ·         off critical
                                       dependency-review (NON-blocking)        path
                                       [cairn: only housekeeping crons today]
```

### Diagram 3 — `just` as one source of truth

```
   developer's terminal                 GitHub Actions
   ────────────────────                 ──────────────
     $ just ci                            run: just ci
          │                                   │
          └─────────────┬─────────────────────┘
                        ▼
                   ┌──────────┐
                   │ justfile │   identical verbs in all 4 repos:
                   │          │   fmt · lint · test · deny · ci · heavy · fix
                   └────┬─────┘
                        │  cairn has NO xtask ⇒ delegate straight to cargo
                        ▼
        cargo fmt / cargo clippy / cargo nextest / cargo-deny / cargo check --locked
        (lefthook pre-commit and CI BOTH call the same `just` verbs ⇒
         "passes locally" and "passes in CI" cannot diverge)
```

### Diagram 4 — cairn-specific: canonical blocks ON vs OFF (pure Rust)

```
   CANONICAL BUILDING BLOCK            cairn (pure Rust)
   ────────────────────────            ─────────────────
   fmt / clippy -D warnings            ON   (ci.yml:29,40)
   unit + doc tests                    ON   (ci.yml:64,81)
   cargo-deny                          ON   (ci.yml:53 / deny.toml)
   lockfile --locked / MSRV pin        ON   (ci.yml:95)
   ci-summary single check             ON   (ci.yml:110)
   rust-cache save-if main             ON   (setup-rust:99)
   merge_group / concurrency           ON   (ci.yml:7,14)
   OS matrix                           ON   (ci.yml:67 — currently in T1)
   ── below: OFF, to add ──
   osv-scan (JS only)                  OFF  — N/A, no JavaScript in cairn
   npm cache                           OFF  — N/A, no npm
   per-job timeout-minutes             OFF  — ADD (D1)
   SHA-pinned actions                  OFF  — ADD (D2)
   heavy.yml (v* tag / dispatch)       OFF  — ADD (D3)
     ├─ feature-powerset               OFF  — ADD (cairn has feature seams)
     ├─ fuzz (cargo-fuzz)              OFF  — ADD (NotePath / plugin parsing)
     ├─ mutants (cargo-mutants)        OFF  — ADD
     ├─ MSRV as its own job            OFF  — ADD (today only via --locked)
     └─ SBOM (cargo-cyclonedx)         OFF  — ADD (D4, core)
   cosign + SLSA provenance            OFF  — phase-2 optional
   justfile                            OFF  — ADD (D5)
   cross-repo sync bot                 OFF  — ADD (D6)
```

---

## 3. Anti-drift & local DX (as they apply to cairn)

**B+C.** Cairn already owns full, self-contained workflow files and already has
the "C" half — the thin `setup-rust` composite action
(`setup-rust/action.yml`). What is missing is (a) **SHA-pinning** those
actions (D2) and (b) a **sync bot** to keep cairn's `ci.yml` / `heavy.yml`
aligned with the canonical template across the four repos (D6). The model
explicitly keeps each `ci.yml` debuggable locally with no runtime dependency on
a shared workflow repo; cairn already satisfies that. Drift is surfaced as a
reviewable per-repo PR, and cairn may decline a sync to keep a justified
variation (e.g. its richer T1 OS matrix). Phase-2 (only if sync PRs get
tedious): a projen-style YAML generator with a CI check that fails when
`synth` output ≠ committed files.

**`just` local DX.** Cairn has **no xtask**, so the justfile delegates straight
to cargo. The verbs are identical across all four repos:

| verb | cairn delegates to |
|---|---|
| `just fmt` | `cargo fmt --all` |
| `just lint` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `just test` | `cargo nextest run --workspace --all-targets` (+ `cargo test --doc`) |
| `just deny` | `cargo deny check` |
| `just ci` | the full T1 gate (fmt + lint + test + deny + `cargo check --locked`) |
| `just heavy` | the T2 set (matrix/MSRV/powerset/fuzz/mutants/SBOM as available locally) |
| `just fix` | `cargo fmt --all` + `cargo clippy --fix` |

Then `lefthook.yml` and `ci.yml` both call the **same** `just` verbs, so the
two cannot diverge. Today `lefthook.yml:18-37` and `ci.yml` duplicate the raw
cargo invocations — converging them onto `just` is the point of D5.

**Git hooks stay lightweight.** Pre-commit runs ONLY the fast `just` verbs
(fmt, lint, fast staged tests) — seconds, never blocking. NO heavy or
container-based checks belong in git hooks. Heavy correctness work runs in the
T2 `v*`-tag heavy CI tier and T3 schedules, never on `git commit` / `git push`.
A pre-push hook, if present, runs at most a fast `just ci` subset. Rationale:
pushes must stay fast; a slow pre-push gate just relocates CI latency onto the
developer and gets bypassed with `--no-verify` anyway.

---

## 4. Implementation checklist (ordered)

A future session can execute these top-to-bottom. Priority in brackets.

**Phase A — hardening the existing gate (fast, high value)**
- [ ] **[High, D1]** Add `timeout-minutes` to every job in `ci.yml` (e.g. 15
      for `test`, 10 for `fmt`/`clippy`/`doc-tests`/`locked-check`, 5 for
      `cargo-deny`/`ci-summary`). Match the pattern already in
      `coverage.yml:28`.
- [ ] **[High, D2]** Pin all third-party actions by commit SHA with a
      `# vX.Y.Z` trailing comment, in `ci.yml`, `coverage.yml`, `claude.yml`,
      `claude-review.yml`, `auto-*.yml`, and `setup-rust/action.yml`. Targets:
      `actions/checkout`, `EmbarkStudios/cargo-deny-action`,
      `dtolnay/rust-toolchain`, `rui314/setup-mold`,
      `mozilla-actions/sccache-action`, `taiki-e/install-action`,
      `Swatinem/rust-cache`, `actions/upload-artifact`,
      `anthropics/claude-code-action`. Confirm `dependabot.yml` (already
      present) keeps SHAs bumped — Dependabot updates SHA pins in place.

**Phase B — `just` as the single source of truth**
- [ ] **[Medium, D5]** Add a `justfile` at repo root exposing `fmt`, `lint`,
      `test`, `deny`, `ci`, `heavy`, `fix`, each delegating straight to cargo
      (no xtask in cairn). Mirror the exact flags already in `ci.yml`.
- [ ] **[Medium, D5]** Rewrite `lefthook.yml` pre-commit/pre-push commands to
      call `just fmt` / `just lint` / `just test` / `just ci` instead of raw
      cargo, so local and CI share one definition. Keep pre-commit lightweight
      (fast `just` verbs only — fmt, lint, fast staged tests); no
      heavy/container checks in hooks. A pre-push hook runs at most a fast
      `just ci` subset; heavy gates stay in the T2/T3 CI tiers.
- [ ] **[Medium, D5]** Change `ci.yml` `run:` steps to `run: just <verb>` (keep
      the matrix/setup-rust scaffolding) so CI invokes the same verbs.

**Phase C — the T2 HEAVY tier (the "release heavy lifting")**
- [ ] **[High, D3]** Add `.github/workflows/heavy.yml` triggered on
      `push: tags: ['v*']` + `workflow_dispatch`, `permissions: contents: read`
      (elevate per-job only where needed for the Release), concurrency group,
      `timeout-minutes` on every job.
- [ ] **[High, D3]** In `heavy.yml`: full OS matrix (linux/macos/windows) — and
      decide whether to *remove* the OS matrix from `ci.yml` T1 to keep the PR
      gate < 10 min, or keep it as a documented intentional variation.
- [ ] **[High, D3]** Add an explicit **MSRV** job (`cargo +1.88 check
      --workspace --locked`) distinct from the lock guard.
- [ ] **[High, D3]** Add a **feature-powerset** job (`cargo hack --feature-powerset
      check`) — cairn has feature seams worth exercising.
- [ ] **[Medium, D3]** Add **fuzz** (`cargo-fuzz`) targeting the parse/validation
      surfaces flagged in `security.md` (e.g. `NotePath`, plugin manifest TOML).
- [ ] **[Medium, D3]** Add **mutation testing** (`cargo-mutants`) over the
      workspace.
- [ ] **[Medium, D3]** Add **coverage** on release by reusing the existing
      `coverage.yml` logic (cargo-llvm-cov) as a `heavy.yml` job.
- [ ] **[Medium, D4]** Add **SBOM** generation (`cargo-cyclonedx`) and attach the
      SBOM as a release artifact — this is canonical T2 **core**.
- [ ] **[High, D3]** Add the **release build → GitHub Release** step (build the
      `cairn-cli` / `cairn-daemon` binaries, upload with the SBOM).
- [ ] **[Low, D8 / phase-2]** *(optional phase-2)* Add **cosign** signing +
      **SLSA provenance** attestation using **OIDC** (`id-token: write`), not
      long-lived secrets. Mark clearly as optional.

**Phase D — T3 scheduled drift-catchers**
- [ ] **[Low, D7]** Add a nightly `schedule` job (or `scheduled.yml`) for fuzz,
      and a weekly job for `cargo-mutants` + dependency-review — non-blocking,
      off the release path.

**Phase E — cross-repo anti-drift**
- [ ] **[Medium, D6]** Add a sync mechanism (Renovate file-sync /
      `BetaHuhn/repo-file-sync-action` / multi-gitter) that opens a reviewable
      PR in cairn when the canonical `ci.yml` / `heavy.yml` / `justfile`
      template changes. Cairn already runs Dependabot for *dependency* bumps;
      this adds *template* alignment.
- [ ] **[Low, D9]** Align `clippy.toml` (`clippy.toml:1`, currently empty) and
      `rustfmt.toml` (`rustfmt.toml:1`) with the shared cross-repo lint config
      when the template syncs.
- [ ] **[Low / phase-2]** *(optional)* Add a projen-style YAML generator + a CI
      `synth`-diff check, only if the sync PRs become tedious.

---

### Summary

Cairn's pipeline is solid on the **T1 fast-gate** axis — `ci-summary`,
`cargo-deny`, `--locked`/MSRV, least-privilege permissions, main-only cache
save, merge-queue, and a lefthook local gate are all already in place. The work
toward the unified model is concentrated in three High-priority moves
(per-job timeouts, SHA-pinned actions, a `v*`-tag HEAVY tier), three
Medium-priority moves (SBOM, `justfile`, cross-repo sync), and three Low
follow-ups (T3 drift-catchers, OIDC posture, lint-config alignment) —
**9 gaps total**, with cosign/SLSA explicitly kept as optional phase-2.
