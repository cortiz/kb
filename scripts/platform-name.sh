#!/usr/bin/env bash
# Prints the name release archives use for a Rust target triple, so the files on the releases page
# read as machines rather than triples. An unknown target is printed unchanged.
# Usage: platform-name.sh <target>
set -euo pipefail

[ $# -eq 1 ] || {
  echo "usage: platform-name.sh <target>" >&2
  exit 2
}
case "$1" in
  aarch64-apple-darwin) echo macos-apple-silicon ;;
  x86_64-apple-darwin) echo macos-intel ;;
  x86_64-unknown-linux-musl) echo linux-x86_64 ;;
  aarch64-unknown-linux-musl) echo linux-arm64 ;;
  *) echo "$1" ;;
esac
