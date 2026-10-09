#!/usr/bin/env bash
# Checks that a pull request title is a Conventional Commit, because the squash commit on main takes
# the title and the release notes are grouped by its type.
# Usage: check-pr-title.sh "<title>"     The title is only ever read from $1, never evaluated.
set -euo pipefail

title=${1-}
types="feat|fix|docs|test|refactor|perf|build|ci|chore|revert"
re="^($types)(\\([a-z0-9._/-]+\\))?!?: [^[:space:]].*\$"

fail() {
  echo "error: $1" >&2
  echo "expected type(scope): subject, with type one of feat, fix, docs, test, refactor, perf, build, ci, chore, revert" >&2
  echo "examples: feat(copy): add --method    fix: handle an empty config    chore(release): 0.2.0" >&2
  exit 1
}

[ -n "$title" ] || fail "the title is empty"
[ "${#title}" -le 100 ] || fail "the title is ${#title} characters; keep it to 100"
case "$title" in
  *.) fail "the title ends with a period" ;;
esac
[[ $title =~ $re ]] || fail "\"$title\" is not a Conventional Commit title"
echo "ok: $title"
