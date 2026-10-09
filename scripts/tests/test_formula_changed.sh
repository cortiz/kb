#!/usr/bin/env bash
# scripts/formula-changed.sh says whether a change touches Formula/ (FR-029).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/formula-changed.sh"

repo="$TMP/repo"
git init -q -b main "$repo"
git -C "$repo" config user.email t@t
git -C "$repo" config user.name t
echo a > "$repo/a"
git -C "$repo" add -A && git -C "$repo" commit -q -m one
base=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" update-ref refs/remotes/origin/main HEAD
mkdir "$repo/Formula" && echo r > "$repo/Formula/kb.rb"
git -C "$repo" add -A && git -C "$repo" commit -q -m formula
with=$(git -C "$repo" rev-parse HEAD)
echo b >> "$repo/a"
git -C "$repo" commit -q -am other
without=$(git -C "$repo" rev-parse HEAD)
zeros=0000000000000000000000000000000000000000

# the script runs with the repository as its working directory
in_repo() { LAST_OUT=$(cd "$repo" && "$script" "$@" 2>&1); LAST_CODE=$?; }

in_repo pull_request main "" "$with"
assert_eq "formula=true" "$LAST_OUT" "pull request touching Formula/"
in_repo pull_request main "" "$without"
assert_eq "formula=true" "$LAST_OUT" "pull request whose range includes the formula commit"
in_repo pull_request "$base" "" "$base"
assert_eq "formula=false" "$LAST_OUT" "pull request with no change"
in_repo push "" "$base" "$with"
assert_eq "formula=true" "$LAST_OUT" "push range touching Formula/"
in_repo push "" "$with" "$without"
assert_eq "formula=false" "$LAST_OUT" "push range not touching Formula/"
in_repo push "" "$zeros" "$without"
assert_eq 0 "$LAST_CODE" "a new branch or tag (zero before) does not fail"
assert_eq "formula=false" "$LAST_OUT" "and reports false"
in_repo push "" "" "$without"
assert_eq "formula=false" "$LAST_OUT" "an empty before reports false"
in_repo workflow_dispatch "" "" "$with"
assert_eq "formula=true" "$LAST_OUT" "dispatch compares with origin/main"
in_repo workflow_call "" "" "$with"
assert_eq "formula=false" "$LAST_OUT" "an event we do not know is false"
in_repo push "" "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" "$without"
assert_eq 0 "$LAST_CODE" "an unknown before commit does not fail"
assert_eq "formula=false" "$LAST_OUT" "and reports false"

assert_exit 2 "no arguments" -- "$script"

finish
