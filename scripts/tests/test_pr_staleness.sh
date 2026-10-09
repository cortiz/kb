#!/usr/bin/env bash
# scripts/pr-staleness.sh warns about old or far-behind pull requests and never fails (FR-034).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/pr-staleness.sh"

# a fake gh: answers the two API calls from FAKE_CREATED / FAKE_BEHIND, or fails
mkdir -p "$TMP/bin"
cat > "$TMP/bin/gh" <<'F'
#!/bin/sh
echo "gh $*" >> "$FAKE_GH_LOG"
[ -z "${FAKE_GH_FAIL:-}" ] || { echo "gh: HTTP 500" >&2; exit 1; }
case "$2" in
  repos/o/r/pulls/7) printf '%s\tmain\tabc123\n' "$FAKE_CREATED" ;;
  repos/o/r/compare/main...abc123) echo "$FAKE_BEHIND" ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
F
chmod +x "$TMP/bin/gh"
export FAKE_GH_LOG="$TMP/gh.log"
export KB_NOW_EPOCH=1791590400   # 2026-10-10T00:00:00Z

stale() { # <created> <behind> [env...]
  local created=$1 behind=$2
  shift 2
  : > "$TMP/summary"
  run env PATH="$TMP/bin:$PATH" FAKE_CREATED="$created" FAKE_BEHIND="$behind" GITHUB_STEP_SUMMARY="$TMP/summary" "$@" "$script" o/r 7
}

stale 2026-10-07T00:00:00Z 10
assert_eq 0 "$LAST_CODE" "3 days, 10 behind: exit 0"
assert_not_contains "warning" "$LAST_OUT" "3 days, 10 behind: no warning"
assert_eq "" "$(cat "$TMP/summary")" "3 days, 10 behind: nothing in the summary"

stale 2026-10-03T00:00:00Z 50
assert_not_contains "::warning" "$LAST_OUT" "exactly 7 days and exactly 50 behind are still fine"

stale 2026-10-02T23:59:59Z 0
assert_eq 0 "$LAST_CODE" "just over 7 days: exit 0"
assert_contains "::warning" "$LAST_OUT" "just over 7 days warns"
assert_contains "7 days" "$LAST_OUT" "the age warning names the limit"
assert_contains "7 days" "$(cat "$TMP/summary")" "the warning is in the job summary"

stale 2026-10-09T00:00:00Z 51
assert_contains "50 commits" "$LAST_OUT" "51 commits behind warns"
assert_not_contains "7 days" "$LAST_OUT" "and only about the commits"

stale 2026-09-01T00:00:00Z 400
assert_contains "7 days" "$LAST_OUT" "both warnings: age"
assert_contains "50 commits" "$LAST_OUT" "both warnings: commits"

# gh failing never fails the check
: > "$TMP/summary"
run env PATH="$TMP/bin:$PATH" FAKE_GH_FAIL=1 FAKE_CREATED=x FAKE_BEHIND=0 GITHUB_STEP_SUMMARY="$TMP/summary" "$script" o/r 7
assert_eq 0 "$LAST_CODE" "a failing gh still exits 0"
assert_contains "could not check" "$LAST_OUT" "and says it could not check"

# usage errors are the only failure
assert_exit 2 "no arguments" -- "$script"

finish
