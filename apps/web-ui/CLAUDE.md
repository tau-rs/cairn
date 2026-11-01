# cairn-web-ui — agent notes

## Parallel-workspace discipline ("who's driving")

This repo is worked on from **multiple Conductor workspaces in parallel**. That
is fine for *independent* tasks. It is **not** fine for a single feature that
produces one branch: on 2026-07-23 three workspaces independently executed the
same `graph-seam-upgrade-v3` plan and two were thrown away after PR #109 merged.

**Rule: for any single-branch feature that has a written plan
(`docs/superpowers/plans/*`), exactly ONE workspace owns it.**

The only state shared across workspaces is the **remote** — each workspace is
its own gitignored worktree, so a local file can't be a lock. Therefore the
feature branch (or its PR) on `origin` **is** the ownership token.

**Before you start executing a plan, run the pre-flight check:**

```sh
scripts/claim-plan.sh <plan-slug>      # e.g. graph-seam-upgrade
```

- **CLAIMED** (a matching branch or open PR exists) → another workspace is
  driving. **STOP and report** — do not start a parallel copy.
- **FREE** → you own it. **Plant your flag early**: create and push the feature
  branch (an empty commit is fine) so sibling workspaces see the claim before
  you've finished. Then record it in `.context/OWNERS.md` (local reminder only).

The equivalent by hand, if the script is unavailable:

```sh
git ls-remote --heads origin '*<slug>*'
gh pr list --state open --search '<slug>'
```

This applies **only** to a single plan that yields one branch. Genuinely
independent tasks across workspaces need no coordination.
