#!/usr/bin/env bash
# Prints formula=true when a change touches Formula/, else formula=false. Never fails on a missing
# or all-zero "before" commit (a new branch or a tag push), where there is nothing to compare.
# Usage: formula-changed.sh <event-name> <base-ref> <before-sha> <after-sha>
set -uo pipefail

if [ $# -ne 4 ]; then
  echo "usage: formula-changed.sh <event-name> <base-ref> <before-sha> <after-sha>" >&2
  exit 2
fi
event=$1
base=$2
before=$3
after=$4

# the remote-tracking ref when there is one (a pull request checkout), else the name as given
remote() { if git rev-parse -q --verify "origin/$1^{commit}" >/dev/null 2>&1; then echo "origin/$1"; else echo "$1"; fi; }

range=""
case "$event" in
  pull_request) [ -n "$base" ] && range="$(remote "$base")...$after" ;;
  workflow_dispatch) range="$(remote main)...$after" ;;
  push)
    case "$before" in
      "" | 0000000000000000000000000000000000000000) ;;
      *) git cat-file -e "$before^{commit}" 2>/dev/null && range="$before..$after" ;;
    esac
    ;;
esac

if [ -n "$range" ] && [ -n "$(git diff --name-only "$range" -- Formula/ 2>/dev/null)" ]; then
  echo "formula=true"
else
  echo "formula=false"
fi
