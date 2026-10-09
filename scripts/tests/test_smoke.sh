#!/usr/bin/env bash
# scripts/smoke.sh runs the five-step smoke test against a built kb in a throw-away directory
# (FR-015), optionally through a runner prefix (qemu, Rosetta) and with a static-link check.
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/smoke.sh"
command -v python3 >/dev/null 2>&1 || { echo "skipped: needs python3"; exit 0; }

# a fake kb: answers the five commands, records where it ran, and can fail one step
cat > "$TMP/kb" <<'F'
#!/bin/sh
echo "args=$* HOME=$HOME KB_ROOT=${KB_ROOT:-} XDG=${XDG_CONFIG_HOME:-}" >> "$FAKE_LOG"
step=none
case "$1" in
  --version) step=version; [ "$FAKE_FAIL" = version ] || echo "kb ${FAKE_VERSION:-0.2.0}" ;;
  init) step=init; [ "$FAKE_FAIL" = init ] || echo "created" ;;
  lint) step=lint ;;
  search) step=search; [ "$FAKE_FAIL" = search-empty ] || echo "cw/example-lambda-errors" ;;
  doctor) step=doctor
    if [ "$FAKE_FAIL" = doctor-notok ]; then echo '{"ok":false,"checks":[]}'; else echo '{"ok":true,"checks":[]}'; fi ;;
esac
[ "$FAKE_FAIL" != "$step" ] || exit 1
exit 0
F
chmod +x "$TMP/kb"
export FAKE_LOG="$TMP/log" FAKE_FAIL=none
# a fake "runner prefix" that proves it wraps every invocation
cat > "$TMP/prefix" <<'F'
#!/bin/sh
echo "prefixed" >> "$FAKE_LOG"
exec "$@"
F
chmod +x "$TMP/prefix"

: > "$FAKE_LOG"
assert_exit 0 "all five steps pass" -- "$script" "$TMP/kb" 0.2.0
assert_contains "smoke: ok" "$LAST_OUT" "says ok"
log=$(cat "$FAKE_LOG")
for want in "args=--version" "args=init" "args=lint --strict --quiet" "args=search lambda --ids" "args=doctor --json"; do
  assert_contains "$want" "$log" "ran [$want]"
done
# a fresh temp HOME and KB_ROOT, never the real ones
home_seen=$(grep -o 'HOME=[^ ]*' "$FAKE_LOG" | sort -u)
assert_eq 1 "$(echo "$home_seen" | wc -l | tr -d ' ')" "one HOME for all steps"
case "$home_seen" in "HOME=$HOME") _fail "ran with the real HOME" ;; esac
case "$log" in *"KB_ROOT=$HOME/kb"*) _fail "ran against the real KB" ;; esac
assert_contains "KB_ROOT=" "$log" "KB_ROOT was set"
assert_not_contains "KB_ROOT= " "$log" "KB_ROOT was not empty"

# each failure is named and exits 1
check_failure() { # <FAKE_FAIL value> <text that must appear>
  : > "$FAKE_LOG"
  FAKE_FAIL=$1 run "$script" "$TMP/kb" 0.2.0
  assert_eq 1 "$LAST_CODE" "[$1] exits 1"
  assert_contains "$2" "$LAST_OUT" "[$1] is named"
}
check_failure version "kb --version"
check_failure init "kb init"
check_failure lint "kb lint"
check_failure search "kb search"
check_failure search-empty "no entry"
check_failure doctor "kb doctor"
check_failure doctor-notok "kb doctor"
FAKE_VERSION=0.9.9 run "$script" "$TMP/kb" 0.2.0
assert_eq 1 "$LAST_CODE" "a wrong version exits 1"
assert_contains "0.9.9" "$LAST_OUT" "the version found is shown"
assert_contains "0.2.0" "$LAST_OUT" "and the one expected"

# the runner prefix wraps every invocation (five steps)
: > "$FAKE_LOG"
assert_exit 0 "with a prefix" -- "$script" "$TMP/kb" 0.2.0 "$TMP/prefix"
assert_eq 5 "$(grep -c prefixed "$FAKE_LOG")" "the prefix ran five times"

# --static needs `file` to say statically linked
cat > "$TMP/file" <<'F'
#!/bin/sh
echo "$1: ELF 64-bit LSB executable, ${FAKE_LINK:-statically linked}, stripped"
F
chmod +x "$TMP/file"
assert_exit 0 "static binary" -- env PATH="$TMP:$PATH" "$script" --static "$TMP/kb" 0.2.0
FAKE_LINK="dynamically linked, interpreter /lib/ld.so" run env PATH="$TMP:$PATH" "$script" --static "$TMP/kb" 0.2.0
assert_eq 1 "$LAST_CODE" "a dynamically linked binary fails --static"
assert_contains "not statically linked" "$LAST_OUT" "and says why"
FAKE_LINK="static-pie linked" run env PATH="$TMP:$PATH" "$script" --static "$TMP/kb" 0.2.0
assert_eq 0 "$LAST_CODE" "static-pie counts as static"

assert_exit 2 "usage: no arguments" -- "$script"
assert_exit 2 "usage: missing version" -- "$script" "$TMP/kb"

finish
