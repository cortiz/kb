#!/usr/bin/env bash
# scripts/ci-local.sh runs the same checks as the pull request pipeline, in order, and names the
# first failing step (spec FR-024, SC-002).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
root=$(repo_root)

# a fake cargo that records its arguments and fails the subcommand named in FAKE_CARGO_FAIL
mkdir -p "$TMP/bin"
cat > "$TMP/bin/cargo" <<'F'
#!/bin/sh
echo "cargo $*" >> "$FAKE_CARGO_LOG"
echo "RUSTDOCFLAGS=${RUSTDOCFLAGS:-}" >> "$FAKE_CARGO_LOG"
if [ "$1" = "${FAKE_CARGO_FAIL:-none}" ]; then exit 1; fi
exit 0
F
chmod +x "$TMP/bin/cargo"
# a fake actionlint that proves optional tools run when present
cat > "$TMP/bin/actionlint" <<'F'
#!/bin/sh
echo "actionlint ran" >> "$FAKE_CARGO_LOG"
F
chmod +x "$TMP/bin/actionlint"

export FAKE_CARGO_LOG="$TMP/log"
export KB_CI_LOCAL_SKIP_SELF=1
PATH_FAKE="$TMP/bin:/usr/bin:/bin"

# --- all steps pass: order and arguments
: > "$FAKE_CARGO_LOG"
run env PATH="$PATH_FAKE" "$root/scripts/ci-local.sh"
assert_eq 0 "$LAST_CODE" "all green exits 0 ($LAST_OUT)"
log=$(cat "$FAKE_CARGO_LOG")
fmt=$(grep -n '^cargo fmt --all --check$' "$FAKE_CARGO_LOG" | cut -d: -f1)
clippy=$(grep -n '^cargo clippy --workspace --all-targets --locked -- -D warnings$' "$FAKE_CARGO_LOG" | cut -d: -f1)
tests=$(grep -n '^cargo test --workspace --locked$' "$FAKE_CARGO_LOG" | cut -d: -f1)
docs=$(grep -n '^cargo doc --workspace --no-deps --locked$' "$FAKE_CARGO_LOG" | cut -d: -f1)
for n in "$fmt" "$clippy" "$tests" "$docs"; do
  [ -n "$n" ] || _fail "a required cargo step is missing from: $log"
done
if [ -n "$fmt" ] && [ -n "$clippy" ] && [ -n "$tests" ] && [ -n "$docs" ]; then
  [ "$fmt" -lt "$clippy" ] && [ "$clippy" -lt "$tests" ] && [ "$tests" -lt "$docs" ] || _fail "steps out of order: $fmt $clippy $tests $docs"
fi
assert_contains "RUSTDOCFLAGS=-D warnings" "$log" "docs are built with warnings denied"
assert_contains "actionlint ran" "$log" "an installed optional tool runs"
assert_contains "skipped deny (not installed: cargo-deny)" "$LAST_OUT" "a missing optional tool is skipped, not failed"

# --- each cargo step failing stops the run and is named
for step in fmt clippy test doc; do
  case $step in doc) name=docs ;; test) name='test' ;; *) name=$step ;; esac
  : > "$FAKE_CARGO_LOG"
  run env PATH="$PATH_FAKE" FAKE_CARGO_FAIL=$step "$root/scripts/ci-local.sh"
  assert_eq 1 "$LAST_CODE" "$step failing exits 1"
  assert_contains "FAILED: $name" "$LAST_OUT" "$step failure is named"
done
# a failure stops later steps
: > "$FAKE_CARGO_LOG"
run env PATH="$PATH_FAKE" FAKE_CARGO_FAIL=fmt "$root/scripts/ci-local.sh"
assert_not_contains "cargo clippy" "$(cat "$FAKE_CARGO_LOG")" "nothing runs after the first failure"

finish
