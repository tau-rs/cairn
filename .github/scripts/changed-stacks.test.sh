#!/usr/bin/env bash
# Tests for changed-stacks.sh: builds a throwaway repo, commits files per case,
# and asserts the two output lines. Run: bash .github/scripts/changed-stacks.test.sh
set -euo pipefail
SCRIPT="$(cd "$(dirname "$0")" && pwd)/changed-stacks.sh"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
cd "$TMP"
git init -q && git config user.email t@t && git config user.name t
echo base > base.txt && git add . && git commit -qm base
fail=0
# case <name> <expected-web> <expected-tauri> <files...>  (one commit per file)
case_() {
  local name=$1 ew=$2 et=$3; shift 3
  local base; base=$(git rev-parse HEAD)
  for f in "$@"; do mkdir -p "$(dirname "$f")"; echo x >> "$f"; git add -A; git commit -qm "$f"; done
  local out; out=$(bash "$SCRIPT" "$base" "$(git rev-parse HEAD)")
  if [ "$out" = "web=$ew"$'\n'"tauri=$et" ]; then echo "ok   $name"; else echo "FAIL $name: got [$out] want [web=$ew tauri=$et]"; fail=1; fi
}
case_ web-only   true  false apps/web-ui/web/src/x.ts
case_ rust-only  false true  crates/cairn-domain/src/x.rs
case_ bindings   true  true  crates/cairn-contract/bindings/X.ts
case_ docs-only  false false README.md
case_ batched    true  true  apps/web-ui/web/src/y.ts crates/cairn-app/src/y.rs
case_ justfile   true  true  apps/web-ui/justfile
case_ lockfile   false true  Cargo.lock
exit $fail
