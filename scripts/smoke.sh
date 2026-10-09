#!/usr/bin/env bash
# The release smoke test: a freshly built kb must start, create a KB, lint it, search it and pass its
# own doctor, all in a throw-away directory (never the user's KB or home).
# Usage: smoke.sh [--static] <path-to-kb> <expected-version> [<runner-prefix>...]
#   --static          also require that the binary is statically linked (Linux musl builds)
#   runner-prefix     a command that runs the binary, e.g. `qemu-aarch64` or `arch -x86_64`
set -euo pipefail

static=false
if [ "${1-}" = "--static" ]; then
  static=true
  shift
fi
if [ $# -lt 2 ]; then
  echo "usage: smoke.sh [--static] <path-to-kb> <expected-version> [<runner-prefix>...]" >&2
  exit 2
fi
kb=$1
expected=$2
shift 2
prefix=("$@")

fail() {
  echo "error: $1" >&2
  exit 1
}

[ -f "$kb" ] || fail "$kb does not exist"
kb="$(cd "$(dirname "$kb")" && pwd)/$(basename "$kb")"

if [ "$static" = true ]; then
  described=$(file "$kb")
  case "$described" in
    *"statically linked"* | *"static-pie linked"*) ;;
    *) fail "$kb is not statically linked: $described" ;;
  esac
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/kbsmoke.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
for name in $(env | sed -n 's/^\(KB_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$name"; done
export HOME="$tmp/home" XDG_CONFIG_HOME="$tmp/config" KB_ROOT="$tmp/kb"
mkdir -p "$HOME"

# bash 3.2 (macOS) rejects an empty array under `set -u`, hence the +"" form
kbrun() { ${prefix[@]+"${prefix[@]}"} "$kb" "$@"; }

version_out=$(kbrun --version) || fail "kb --version failed"
[ "$version_out" = "kb $expected" ] || fail "kb --version printed \"$version_out\", expected \"kb $expected\""

kbrun init >/dev/null || fail "kb init failed"
kbrun lint --strict --quiet >/dev/null || fail "kb lint --strict failed on the KB that kb init made"
found=$(kbrun search lambda --ids) || fail "kb search lambda failed"
[ -n "$found" ] || fail "kb search lambda found no entry in the example KB"
doctor=$(kbrun doctor --json) || fail "kb doctor --json exited non-zero"
printf '%s' "$doctor" | python3 -c 'import json, sys; sys.exit(0 if json.load(sys.stdin).get("ok") is True else 1)' ||
  fail "kb doctor reported a failed check"

echo "smoke: ok ($version_out)"
