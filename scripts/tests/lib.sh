#!/usr/bin/env bash
# Helpers for the script tests. Source this file, call the asserts, end with `finish`.
# Written for bash 3.2 (macOS /bin/bash) as well as bash 5.

FAILS=0
LAST_OUT=""
LAST_CODE=0

_fail() {
  echo "  FAIL: $*" >&2
  FAILS=$((FAILS + 1))
}

# assert_eq <want> <got> <message>
assert_eq() {
  if [ "$1" != "$2" ]; then _fail "$3: want [$1] got [$2]"; fi
}

# assert_contains <needle> <text> <message>
assert_contains() {
  case "$2" in
    *"$1"*) ;;
    *) _fail "$3: [$1] not found in [$2]" ;;
  esac
}

# assert_not_contains <needle> <text> <message>
assert_not_contains() {
  case "$2" in
    *"$1"*) _fail "$3: [$1] unexpectedly found in [$2]" ;;
  esac
}

# run <command...>: run it, capture stdout+stderr in LAST_OUT and the exit code in LAST_CODE
run() {
  LAST_CODE=0
  LAST_OUT=$("$@" 2>&1) || LAST_CODE=$?
}

# assert_exit <code> <message> -- <command...>   (also fills LAST_OUT / LAST_CODE)
assert_exit() {
  local want=$1 msg=$2
  shift 3
  run "$@"
  if [ "$LAST_CODE" != "$want" ]; then
    _fail "$msg: exit $LAST_CODE, wanted $want; output: $LAST_OUT"
  fi
}

# new_tmp: make a temporary directory in TMP, removed when the test ends
new_tmp() {
  TMP=$(mktemp -d "${TMPDIR:-/tmp}/kbtest.XXXXXX")
  trap 'rm -rf "$TMP"' EXIT
}

# repo_root: the repository root, from the location of this file
repo_root() {
  (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
}

# sha256 <file>: hash of a file on macOS (shasum) and Linux (sha256sum)
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

finish() {
  if [ "$FAILS" -ne 0 ]; then
    echo "  $FAILS assertion(s) failed" >&2
    exit 1
  fi
  exit 0
}
