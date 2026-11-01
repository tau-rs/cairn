#!/usr/bin/env bash
# Which UI stacks does a commit range touch? Used by ci.yml's `changes` job on
# merge_group events, where dorny/paths-filter has no reliable base
# (dorny/paths-filter#280). Prints exactly two lines: web=<bool> tauri=<bool>.
# Path lists mirror the pull_request filters in ci.yml — keep both in step.
# Usage: changed-stacks.sh <base-sha> <head-sha>
set -euo pipefail
web=false; tauri=false
while IFS= read -r f; do
  # Two independent matches: one path may belong to both stacks
  # (crates/cairn-contract/bindings/ is web-consumed and under crates/).
  case "$f" in
    apps/web-ui/web/*|crates/cairn-contract/bindings/*|apps/web-ui/justfile|.github/workflows/ci.yml|.github/scripts/changed-stacks.sh) web=true ;;
  esac
  case "$f" in
    apps/web-ui/src-tauri/*|apps/web-ui/deny.toml|crates/*|Cargo.lock|apps/web-ui/justfile|.github/workflows/ci.yml|.github/scripts/changed-stacks.sh) tauri=true ;;
  esac
done < <(git diff --name-only "$1" "$2")
echo "web=$web"
echo "tauri=$tauri"
