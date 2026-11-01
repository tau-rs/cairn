#!/usr/bin/env bash
# One release train (monorepo spec D11): a v* tag ships the CLI, the daemon
# AND the desktop app, so the tag must equal the desktop app's own version
# or the Tauri bundles would carry a version the release name does not.
# Usage: check-desktop-version.sh <ref_name> [path/to/tauri.conf.json]
set -euo pipefail
ref="$1"
conf="${2:-apps/web-ui/src-tauri/tauri.conf.json}"
ver="$(jq -r .version "$conf")"
if [ "$ref" != "v$ver" ]; then
  echo "::error::tag $ref != desktop version v$ver ($conf) — bump tauri.conf.json in the release PR"
  exit 1
fi
echo "tag $ref matches desktop version $ver"
