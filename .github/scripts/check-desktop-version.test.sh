#!/usr/bin/env bash
# Tests for check-desktop-version.sh. Run: bash .github/scripts/check-desktop-version.test.sh
set -euo pipefail
SCRIPT="$(cd "$(dirname "$0")" && pwd)/check-desktop-version.sh"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
CONF="$TMP/tauri.conf.json"
printf '{ "productName": "cairn", "version": "0.2.0" }\n' > "$CONF"
fail=0
case_() { # <name> <ref> <expected-exit>
  local rc=0; bash "$SCRIPT" "$2" "$CONF" >/dev/null 2>&1 || rc=$?
  if [ "$rc" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: exit $rc, want $3"; fail=1; fi
}
case_ matching-tag  v0.2.0  0
case_ other-version v0.3.0  1
case_ missing-v     0.2.0   1
case_ prerelease    v0.2.0-rc1 1
exit $fail
