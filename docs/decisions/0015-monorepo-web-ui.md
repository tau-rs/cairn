# ADR-0015: cairn-web-ui joins the cairn repository as `apps/web-ui`

**Status:** Accepted
**Date:** 2026-11-01
**Tracking:** [#244](https://github.com/tau-rs/cairn/issues/244) · spec `docs/superpowers/specs/2026-11-01-monorepo-web-ui-design.md`

## Context

The engine (this repo) and its web/desktop UI (`tau-rs/cairn-web-ui`) lived in two
repositories. The UI depended on the engine in two places that had to name the
same engine commit: a vendored copy of the ts-rs TypeScript contract stamped with
a source commit, and six `cairn-*` git-rev pins in the Tauri shell's `Cargo.toml`.
Nothing enforced that they agreed. The UI tracker recorded the seams drifting at
least four times (cairn-web-ui #172, #176, #177, #180) and an open proposal (#181)
to add CI machinery just to detect it.

Analogy: the UI carried a photocopy of the engine's API with a note saying which
edition it was copied from. The monorepo hands it the original.

## Decision

1. **`apps/web-ui/` is the home.** cairn ships several faces (CLI, daemon, MCP
   server, UI); `apps/` says so in `ls`. The UI's `web/` and `src-tauri/` stay
   siblings so its own tooling keeps working.
2. **Imported with `git subtree add`, merged as a merge commit.** All 842 UI
   commits are reachable (`git log <merge>^2`), so blame and the SHAs cited in
   the UI's handoffs still resolve. The merge-queue ruleset was flipped to
   `MERGE` for that one PR and back to `SQUASH` after.
3. **The Tauri shell stays its own Cargo workspace**, excluded from the engine's
   (like `fuzz/`), depending on `crates/` by path. Engine jobs never compile
   Tauri or need WebKitGTK; the drift seam is still gone because path deps have
   no revision.
4. **The web app reads the engine's committed bindings directly** through a
   `@cairn-contract/*` alias (tsconfig `paths` + Vite `resolve.alias`, with
   `server.fs.allow` for the one runtime import). No vendored copy, no stamp,
   no sync script.
5. **Engine CI gains `bindings-current`** (`just bindings-check`): regenerate
   the bindings via the crate's tests and fail on any diff or untracked file.
   That is the last way the contract could lag the Rust source.
6. **One `ci.yml`, one `ci-summary`.** A `changes` job gates the `web`, `e2e`
   and `tauri` jobs per stack: dorny/paths-filter on pull requests,
   `.github/scripts/changed-stacks.sh` (base..head diff, same path lists) in the
   merge queue, everything on for pushes to `main`. No new required check.
7. **One Dependabot config**; the UI's Renovate (action digests) and
   `auto-update-prs.yml` are not imported.
8. **Engine workflows win on overlap** (claude, auto-rerun-flaky); UI-only ones
   move to the root as `web-nightly`, `web-mutation-weekly`,
   `dependabot-automerge`, and a `vitest-coverage` job in `coverage.yml`.
9. **Nested `justfile` and `CLAUDE.md` stay; the root `lefthook.yml`** runs the
   web `fmt`/`lint`/`test` verbs for commits touching `apps/web-ui/web/`.
10. **Ledger moves with the code:** cairn-web-ui #180/#181 are closed by this
    change, its Dependabot PRs closed, the repository archived with a pointer.
11. **One release train.** A `v*` tag builds CLI, daemon, desktop bundles and
    both SBOMs into one GitHub Release; `check-desktop-version.sh` fails the
    run unless the tag equals `tauri.conf.json`'s version.

## Consequences

- Engine ↔ UI drift is impossible by construction: there is one commit.
- Two `Cargo.lock` and two `deny.toml` files remain (one per workspace);
  transitive versions may differ. Folding the Tauri crate into the engine
  workspace is a follow-on (#248).
- Merge-queue runs grow only when a stack actually changed; a Rust-only PR
  never pays for pnpm or a Tauri build.
- Releasing the desktop app means bumping `tauri.conf.json` in the same PR as
  the tag; the heavy tier refuses otherwise.
- The import PR also had to repair gates that were already red on both sides
  (RustSec and npm advisories newer than the last green runs); those
  acceptances carry their exit conditions in the two `deny.toml` files.

## Rollback

Revert the PR 1 merge commit with `git revert -m 1 <merge>`; PR 2 and PR 3 are
ordinary squash commits and revert in reverse order.
