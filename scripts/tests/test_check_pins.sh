#!/usr/bin/env bash
# scripts/check-pins.sh requires every external action to be pinned to a commit hash with a version
# comment (FR-022).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/check-pins.sh"
h40=3d3c42e5aac5ba805825da76410c181273ba90b1

wf() { # <name> <uses line...>
  local name=$1
  shift
  mkdir -p "$TMP/wf"
  { echo "jobs:"; echo "  j:"; echo "    steps:"; for l in "$@"; do echo "      - $l"; done; } > "$TMP/wf/$name.yml"
}
check() { rm -rf "$TMP/wf"; wf t "$@"; run "$script" "$TMP/wf"; }

check "uses: actions/checkout@$h40 # v7.0.1"
assert_eq 0 "$LAST_CODE" "hash plus version comment passes"
check "uses: owner/repo/sub/path@$h40 # v1.2.3"
assert_eq 0 "$LAST_CODE" "action in a subpath passes"
check "uses: ./.github/workflows/ci.yml"
assert_eq 0 "$LAST_CODE" "local reference passes"
check "uses: docker://alpine:3"
assert_eq 0 "$LAST_CODE" "docker reference passes"
check "run: echo uses: nothing"
assert_eq 0 "$LAST_CODE" "the word uses in a run line is not an action"

check "uses: actions/checkout@v4"
assert_eq 1 "$LAST_CODE" "a tag fails"
assert_contains "t.yml:4" "$LAST_OUT" "and names file:line"
check "uses: actions/checkout@main"
assert_eq 1 "$LAST_CODE" "a branch fails"
check "uses: actions/checkout@${h40%?} # v1"
assert_eq 1 "$LAST_CODE" "39 characters fail"
check "uses: actions/checkout@${h40}0 # v1"
assert_eq 1 "$LAST_CODE" "41 characters fail"
check "uses: actions/checkout@$h40"
assert_eq 1 "$LAST_CODE" "no version comment fails"
check "uses: actions/checkout@$h40 # latest"
assert_eq 1 "$LAST_CODE" "a comment that is not a version fails"

# a good and a bad line: only the bad one is named
rm -rf "$TMP/wf"; wf t "uses: actions/checkout@$h40 # v7.0.1" "uses: foo/bar@v1"
run "$script" "$TMP/wf"
assert_eq 1 "$LAST_CODE" "mixed file fails"
assert_contains "t.yml:5" "$LAST_OUT" "mixed file names the bad line"
assert_not_contains "t.yml:4" "$LAST_OUT" "mixed file leaves the good line alone"

assert_exit 2 "a missing directory" -- "$script" "$TMP/nope"

finish
