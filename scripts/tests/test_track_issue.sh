#!/usr/bin/env bash
# scripts/track-issue.sh comments on the open issue with a label, or opens one (FR-008, FR-021).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/track-issue.sh"

# a fake gh: `issue list` answers FAKE_OPEN (an issue number or empty); everything is logged
mkdir -p "$TMP/bin"
cat > "$TMP/bin/gh" <<'F'
#!/bin/sh
echo "gh $*" >> "$FAKE_GH_LOG"
[ -z "${FAKE_GH_FAIL:-}" ] || { echo "gh: HTTP 500" >&2; exit 1; }
case "$2" in
  list) echo "${FAKE_OPEN:-}" ;;
  create) echo "https://github.com/o/r/issues/99" ;;
  comment) ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
F
chmod +x "$TMP/bin/gh"
export FAKE_GH_LOG="$TMP/gh.log"
echo "body text" > "$TMP/body.md"

track() { : > "$FAKE_GH_LOG"; run env PATH="$TMP/bin:$PATH" "$@" "$script" perf "Perf regression" "$TMP/body.md"; }

track FAKE_OPEN=
assert_eq 0 "$LAST_CODE" "no open issue: exit 0"
assert_eq 1 "$(grep -c 'issue create' "$FAKE_GH_LOG")" "no open issue: one create"
assert_contains "--label perf" "$(cat "$FAKE_GH_LOG")" "create carries the label"
assert_contains "--title Perf regression" "$(cat "$FAKE_GH_LOG")" "create carries the title"
assert_contains "--body-file $TMP/body.md" "$(cat "$FAKE_GH_LOG")" "create carries the body file"
assert_not_contains "issue comment" "$(cat "$FAKE_GH_LOG")" "no open issue: no comment"

track FAKE_OPEN=12
assert_eq 0 "$LAST_CODE" "open issue 12: exit 0"
assert_contains "issue comment 12" "$(cat "$FAKE_GH_LOG")" "comments on issue 12"
assert_not_contains "issue create" "$(cat "$FAKE_GH_LOG")" "never creates a second issue"

track FAKE_GH_FAIL=1
assert_eq 1 "$LAST_CODE" "a failing gh fails the script"

assert_exit 2 "missing arguments" -- "$script" perf
assert_exit 2 "missing body file" -- "$script" perf title "$TMP/nope.md"

finish
