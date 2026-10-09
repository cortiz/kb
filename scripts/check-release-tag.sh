#!/usr/bin/env bash
# Validates a release before anything is built: the tag is vMAJOR.MINOR.PATCH[-pre-release], equals
# the workspace version in Cargo.toml, and points at a commit on main.
# Usage: check-release-tag.sh <tag>        a tag push (GITHUB_SHA is the tagged commit)
#        check-release-tag.sh --dry-run    a manual run: the tag is derived from Cargo.toml and the
#                                          "on main" check is skipped
# Prints tag=, version= and prerelease= lines (append them to $GITHUB_OUTPUT).
set -euo pipefail

fail() {
  echo "error: $1" >&2
  exit 1
}

usage() {
  echo "usage: check-release-tag.sh <tag> | --dry-run" >&2
  exit 2
}

[ $# -eq 1 ] || usage
dry=false
tag=""
case "$1" in
  --dry-run) dry=true ;;
  -*) usage ;;
  *) tag=$1 ;;
esac

# the version every workspace crate shares
versions=$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
print(" ".join(sorted({p["version"] for p in json.load(sys.stdin)["packages"]})))')
case "$versions" in
  "") fail "no crates found in the workspace" ;;
  *" "*) fail "the workspace crates have different versions: $versions" ;;
esac
version=$versions

if [ "$dry" = true ]; then
  tag="v$version"
fi

re='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'
[[ $tag =~ $re ]] || fail "\"$tag\" is not a version tag (expected vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-pre.release)"
[ "$tag" = "v$version" ] || fail "the tag is $tag but Cargo.toml says version $version"

if [ "$dry" = false ]; then
  sha=${GITHUB_SHA:-$(git rev-parse HEAD)}
  git merge-base --is-ancestor "$sha" origin/main ||
    fail "commit $sha is not on main (origin/main); releases only come from main"
fi

prerelease=false
case "$version" in *-*) prerelease=true ;; esac
echo "tag=$tag"
echo "version=$version"
echo "prerelease=$prerelease"
