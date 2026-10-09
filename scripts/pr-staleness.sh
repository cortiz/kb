#!/usr/bin/env bash
# Warns when a pull request is old or far behind its base: trunk-based development wants short-lived
# branches. Never fails the check; at worst it says it could not look.
# Usage: pr-staleness.sh <owner/repo> <pr-number>      (uses gh; KB_NOW_EPOCH overrides "now" in tests)
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: pr-staleness.sh <owner/repo> <pr-number>" >&2
  exit 2
fi
repo=$1
pr=$2
max_days=7
max_behind=50

# seconds since the epoch for an ISO 8601 UTC time, on GNU and BSD date
to_epoch() {
  date -u -d "$1" +%s 2>/dev/null || date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$1" +%s
}

if ! info=$(gh api "repos/$repo/pulls/$pr" --jq '[.created_at, .base.ref, .head.sha] | @tsv' 2>&1); then
  echo "note: could not check how old pull request $pr is: $info"
  exit 0
fi
IFS=$'\t' read -r created base sha <<<"$info"
behind=$(gh api "repos/$repo/compare/$base...$sha" --jq .behind_by 2>/dev/null) || behind=0
if ! created_epoch=$(to_epoch "$created" 2>/dev/null); then
  echo "note: could not read the creation time \"$created\""
  exit 0
fi
now=${KB_NOW_EPOCH:-$(date -u +%s)}

warnings=()
if [ $((now - created_epoch)) -gt $((max_days * 86400)) ]; then
  warnings+=("this pull request is more than $max_days days old; short-lived branches keep main releasable")
fi
if [ "${behind:-0}" -gt "$max_behind" ]; then
  warnings+=("this branch is more than $max_behind commits behind $base ($behind); update it from $base")
fi

if [ "${#warnings[@]}" -gt 0 ]; then
  for w in "${warnings[@]}"; do
    echo "::warning::$w"
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "- $w" >>"$GITHUB_STEP_SUMMARY"; fi
  done
fi
exit 0
