#!/usr/bin/env bash
# The release binary may grow by less than 100 KiB over a base release (constitution IX: simplicity
# first; spec 014 SC-005 for Markdown notes).
#
# Usage: check-binary-size.sh [BASE_REF]       (default: v0.2.0)
#   builds `kb` in release mode at BASE_REF (in a throwaway worktree) and at the current checkout,
#   prints both sizes and fails (exit 1) when the growth is KB_MAX_GROWTH_BYTES or more
#   (default 102400).
#
# For tests and for builds done elsewhere: KB_SIZE_BASE_BIN and KB_SIZE_NEW_BIN name two binaries
# to compare instead of building anything.
set -euo pipefail

base_ref=${1:-v0.2.0}
max=${KB_MAX_GROWTH_BYTES:-102400}

size_of() { wc -c < "$1" | tr -d '[:space:]'; }

if [ -n "${KB_SIZE_BASE_BIN:-}" ] && [ -n "${KB_SIZE_NEW_BIN:-}" ]; then
  base_bin=$KB_SIZE_BASE_BIN
  new_bin=$KB_SIZE_NEW_BIN
else
  root=$(git rev-parse --show-toplevel)
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/kbsize.XXXXXX")
  trap 'git -C "$root" worktree remove --force "$tmp/base" >/dev/null 2>&1 || true; rm -rf "$tmp"' EXIT
  git -C "$root" worktree add --detach "$tmp/base" "$base_ref" >/dev/null
  echo "building $base_ref ..." >&2
  (cd "$tmp/base" && CARGO_TARGET_DIR="$tmp/target-base" cargo build --release --locked -p kb -q)
  echo "building the current checkout ..." >&2
  (cd "$root" && cargo build --release --locked -p kb -q)
  base_bin=$tmp/target-base/release/kb
  new_bin=$root/target/release/kb
fi

base=$(size_of "$base_bin")
new=$(size_of "$new_bin")
growth=$((new - base))
echo "base ($base_ref): $base bytes"
echo "now:  $new bytes"
echo "growth: $growth bytes (limit: under $max)"
if [ "$growth" -ge "$max" ]; then
  echo "the release binary grew by $growth bytes; the limit is under $max" >&2
  exit 1
fi
