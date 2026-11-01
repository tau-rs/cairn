#!/usr/bin/env bash
# DX3: verify the vendored TS contract (web/src/contract) is byte-identical to
# what sync-contract.sh regenerates from the engine commit recorded in
# source.ts. Catches hand-edits / partial syncs that S5's runtime validator
# would otherwise only surface at runtime. Run on a clean checkout (CI).
set -euo pipefail

ENGINE_REPO="${ENGINE_REPO:-https://github.com/tau-rs/cairn.git}"
COMMIT="$(grep -oE '[0-9a-f]{40}' web/src/contract/source.ts | head -1)"
[ -n "$COMMIT" ] || {
  echo "could not read CONTRACT_SOURCE_COMMIT from web/src/contract/source.ts"
  exit 1
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git clone --quiet "$ENGINE_REPO" "$TMP/cairn"
git -C "$TMP/cairn" checkout --quiet "$COMMIT"

scripts/sync-contract.sh "$TMP/cairn"

# Use `status --porcelain`, not `diff`: the engine can add a *new* binding file,
# which sync-contract.sh writes as an untracked file. `git diff` ignores
# untracked files and would pass green on that drift; porcelain reports
# untracked (`??`), modified, and deleted contract files alike.
DRIFT="$(git status --porcelain -- web/src/contract)"
if [ -n "$DRIFT" ]; then
  echo "::error::vendored contract drifted from engine @ $COMMIT — re-run scripts/sync-contract.sh and commit"
  echo "$DRIFT"
  exit 1
fi
echo "contract in sync with engine @ $COMMIT"
