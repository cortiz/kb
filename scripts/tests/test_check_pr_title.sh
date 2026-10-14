#!/usr/bin/env bash
# scripts/check-pr-title.sh accepts Conventional Commit titles and rejects the rest (FR-031).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/check-pr-title.sh"

ok_titles=(
  "feat: x"
  "fix(core): y"
  "docs(spec)!: z"
  "chore(release): 0.2.0"
  "revert: a"
  "feat(scope.with-dots/slash): b"
  "perf(search): faster typo expansion"
  "ci: replace the CI workflow"
  "refactor!: drop the old API"
  "build(deps): bump the cargo group"
)
for t in "${ok_titles[@]}"; do
  assert_exit 0 "accepts [$t]" -- "$script" "$t"
done

bad_titles=(
  "Update stuff"
  "feat:"
  "feat: "
  "feat(): x"
  "Feat: x"
  "feat : x"
  "wip: x"
  "feat(Scope): x"
  "feat(scope) x"
  "fix: ends with a period."
  ""
)
for t in "${bad_titles[@]}"; do
  assert_exit 1 "rejects [$t]" -- "$script" "$t"
  assert_contains "type(scope): subject" "$LAST_OUT" "the message for [$t] names the expected form"
done

long=$(printf 'a%.0s' $(seq 1 94))
assert_exit 0 "accepts a 100-character title" -- "$script" "feat: $long"
long=$(printf 'a%.0s' $(seq 1 95))
assert_exit 1 "rejects a 101-character title" -- "$script" "feat: $long"

assert_exit 1 "no argument is an error" -- "$script"
assert_contains "feat, fix, docs, test, refactor, perf, build, ci, chore, revert" "$LAST_OUT" "the message lists the allowed types"

# a hostile title is only ever data (the single quotes are deliberate)
# shellcheck disable=SC2016
assert_exit 1 "a command substitution in a title" -- "$script" 'Update $(touch '"$TMP"'/pwned) and `touch '"$TMP"'/pwned2`'
if [ -e "$TMP/pwned" ] || [ -e "$TMP/pwned2" ]; then
  _fail "the title was executed"
fi
# shellcheck disable=SC2016
assert_exit 0 "a valid title that contains shell syntax" -- "$script" 'feat(x): handle $(echo) and `ticks` literally'
[ ! -e "$TMP/pwned" ] || _fail "the title was executed"

finish
