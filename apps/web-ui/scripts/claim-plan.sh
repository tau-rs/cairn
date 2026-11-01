#!/usr/bin/env bash
# Pre-flight ownership check for a single-branch feature plan.
# The remote is the only thing shared across Conductor workspaces, so the
# feature branch (or its PR) on `origin` IS the ownership token. Run this
# BEFORE executing any plan: if a matching branch/PR already exists, another
# workspace is driving — stand down instead of duplicating the work.
#
# Usage: scripts/claim-plan.sh <plan-slug>
#   e.g. scripts/claim-plan.sh graph-seam-upgrade
#
# Exit 0 = FREE (no claim found), exit 1 = CLAIMED (stand down).
# See CLAUDE.md § "Parallel-workspace discipline".
set -euo pipefail

SLUG="${1:-}"
if [[ -z "$SLUG" ]]; then
  echo "usage: scripts/claim-plan.sh <plan-slug>" >&2
  exit 2
fi

claimed=0

echo "== Remote branches matching *${SLUG}* =="
branches="$(git ls-remote --heads origin "*${SLUG}*" || true)"
if [[ -n "$branches" ]]; then
  echo "$branches"
  claimed=1
else
  echo "  (none)"
fi

echo "== Open PRs matching '${SLUG}' =="
if command -v gh >/dev/null 2>&1; then
  prs="$(gh pr list --state open --search "$SLUG" \
          --json number,title,headRefName,author \
          --template '{{range .}}  #{{.number}} {{.headRefName}} — {{.title}} (@{{.author.login}}){{"\n"}}{{end}}' 2>/dev/null || true)"
  if [[ -n "$prs" ]]; then
    echo "$prs"
    claimed=1
  else
    echo "  (none)"
  fi
else
  echo "  gh not available — skipped (check PRs manually)"
fi

echo
if [[ "$claimed" -eq 1 ]]; then
  echo "CLAIMED → another workspace is driving '${SLUG}'. STAND DOWN and report."
  exit 1
else
  echo "FREE → '${SLUG}' is unclaimed. Plant your flag: push the feature branch early."
  exit 0
fi
