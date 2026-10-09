#!/usr/bin/env bash
# Renders the Homebrew formula for a stable release from .github/formula.rb.tmpl and the release's
# SHA256SUMS. Formulas are for stable releases only.
# Usage: gen-formula.sh <tag> <SHA256SUMS> <out-file>
set -euo pipefail

if [ $# -ne 3 ]; then
  echo "usage: gen-formula.sh <tag> <SHA256SUMS> <out-file>" >&2
  exit 2
fi
tag=$1
sums=$2
out=$3
root=$(cd "$(dirname "$0")/.." && pwd)

if ! [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: $tag is not a stable release tag; formulas are for stable releases only" >&2
  exit 1
fi
version=${tag#v}

text=$(cat "$root/.github/formula.rb.tmpl")
text=${text//@VERSION@/$version}
text=${text//@TAG@/$tag}
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  sum=$(awk -v f="kb-$version-$target.tar.gz" '$2 == f { print $1 }' "$sums")
  if ! [[ $sum =~ ^[0-9a-f]{64}$ ]]; then
    echo "error: no checksum for $target in $sums" >&2
    exit 1
  fi
  key=$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')
  text=${text//@SHA_${key}@/$sum}
done
printf '%s\n' "$text" > "$out"
