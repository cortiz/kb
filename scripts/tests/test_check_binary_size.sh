#!/usr/bin/env bash
# scripts/check-binary-size.sh compares the sizes of two release binaries.
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/check-binary-size.sh"

bytes() { head -c "$2" /dev/zero > "$TMP/$1"; }
compare() { KB_SIZE_BASE_BIN="$TMP/base" KB_SIZE_NEW_BIN="$TMP/new" run "$script" "$@"; }

bytes base 1000000
bytes new 1000000
compare
assert_eq 0 "$LAST_CODE" "no growth passes"
assert_contains "growth: 0 bytes" "$LAST_OUT" "and says so"

bytes new 1102399
compare
assert_eq 0 "$LAST_CODE" "one byte under the limit passes"
assert_contains "growth: 102399 bytes" "$LAST_OUT" "and reports the growth"

bytes new 1102400
compare
assert_eq 1 "$LAST_CODE" "the limit itself fails"
assert_contains "grew by 102400 bytes" "$LAST_OUT" "and names the growth"

bytes new 900000
compare
assert_eq 0 "$LAST_CODE" "a smaller binary passes"

bytes new 1050000
KB_MAX_GROWTH_BYTES=40000 KB_SIZE_BASE_BIN="$TMP/base" KB_SIZE_NEW_BIN="$TMP/new" run "$script" v0.2.0
assert_eq 1 "$LAST_CODE" "the limit can be changed"
finish
