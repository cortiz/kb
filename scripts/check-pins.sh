#!/usr/bin/env bash
# Every external action a workflow uses must be pinned to a full commit hash, followed by a version
# comment (`# v1.2.3`) so a human can read it and Dependabot can update it. Local (./) and docker://
# references are exempt.
# Usage: check-pins.sh [dir]     (default: .github/workflows)
set -euo pipefail

dir=${1:-.github/workflows}
if [ ! -d "$dir" ]; then
  echo "usage: check-pins.sh [dir]: $dir is not a directory" >&2
  exit 2
fi

use_re='^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]+([^[:space:]#]+)[[:space:]]*(#[[:space:]]*(.*))?$'
pin_re='^[A-Za-z0-9._-]+/[A-Za-z0-9._/-]+@[0-9a-f]{40}$'
ver_re='^v[0-9]+(\.[0-9]+)*([-+][0-9A-Za-z.-]+)?$'
bad=0
shopt -s nullglob
for f in "$dir"/*.yml "$dir"/*.yaml; do
  n=0
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    [[ $line =~ $use_re ]] || continue
    ref=${BASH_REMATCH[2]}
    comment=${BASH_REMATCH[4]-}
    case "$ref" in ./* | docker://*) continue ;; esac
    if ! [[ $ref =~ $pin_re ]]; then
      echo "$f:$n: $ref is not pinned to a 40-character commit hash" >&2
      bad=1
    elif ! [[ $comment =~ $ver_re ]]; then
      echo "$f:$n: $ref needs a version comment such as '# v1.2.3'" >&2
      bad=1
    fi
  done < "$f"
done
exit "$bad"
