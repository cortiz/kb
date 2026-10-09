#!/usr/bin/env bash
# Runs the pull request checks on this machine, in the same order as the CI workflow.
# Optional tools (cargo-deny, actionlint, shellcheck) are skipped with a note when not installed.
# Set KB_CI_LOCAL_SKIP_SELF=1 to skip the checks of the scripts themselves (used by their tests).
set -euo pipefail

cd "$(dirname "$0")/.."

step() {
  local name=$1
  shift
  echo "==> $name"
  if "$@"; then
    echo "ok   $name"
  else
    echo "FAILED: $name" >&2
    exit 1
  fi
}

optional() {
  local name=$1 tool=$2
  shift 2
  if command -v "$tool" >/dev/null 2>&1; then
    step "$name" "$@"
  else
    echo "skipped $name (not installed: $tool)"
  fi
}

step fmt cargo fmt --all --check
step clippy cargo clippy --workspace --all-targets --locked -- -D warnings
step test cargo test --workspace --locked
step docs env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
optional deny cargo-deny cargo deny check
optional actionlint actionlint actionlint
optional shellcheck shellcheck shellcheck -x --source-path=SCRIPTDIR scripts/*.sh scripts/tests/*.sh
if [ -z "${KB_CI_LOCAL_SKIP_SELF:-}" ]; then
  step scripts scripts/tests/run.sh
  if [ -x scripts/check-pins.sh ]; then step pins scripts/check-pins.sh; fi
fi
echo "all checks passed"
