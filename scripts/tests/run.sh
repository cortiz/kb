#!/usr/bin/env bash
# Runs every scripts/tests/test_*.sh in its own shell. Exit 1 if any fails.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
failed=0
count=0
for t in "$here"/test_*.sh; do
  [ -e "$t" ] || continue
  count=$((count + 1))
  name=$(basename "$t")
  if out=$(bash "$t" 2>&1); then
    case "$out" in
      skipped:*) echo "skip  $name  ${out#skipped: }" ;;
      *) echo "ok    $name" ;;
    esac
  else
    failed=$((failed + 1))
    echo "FAIL  $name"
    printf '%s\n' "$out" | sed 's/^/      /'
  fi
done
echo "$count script test file(s), $failed failed"
[ "$failed" -eq 0 ]
