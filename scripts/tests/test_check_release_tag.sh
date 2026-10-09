#!/usr/bin/env bash
# scripts/check-release-tag.sh: tag syntax, tag == workspace version, commit on main (FR-011, FR-012).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/check-release-tag.sh"

command -v python3 >/dev/null 2>&1 || { echo "skipped: needs python3"; exit 0; }

# a fake workspace with one crate, a bare "origin" and a main branch pushed to it
mkdir -p "$TMP/work/crates/x/src"
cd "$TMP/work" || exit 1
cat > Cargo.toml <<'F'
[workspace]
members = ["crates/x"]
resolver = "3"

[workspace.package]
version = "0.2.0"
edition = "2024"
F
cat > crates/x/Cargo.toml <<'F'
[package]
name = "x"
version.workspace = true
edition.workspace = true
F
: > crates/x/src/lib.rs
git init -q -b main . && git config user.email t@t && git config user.name t
git add -A && git commit -q -m "chore: start"
git init -q --bare "$TMP/origin.git"
git remote add origin "$TMP/origin.git" && git push -q origin main
main_sha=$(git rev-parse HEAD)

check() { run env GITHUB_SHA="${SHA:-$main_sha}" "$script" "$@"; }

# --- the happy path
check v0.2.0
assert_eq 0 "$LAST_CODE" "v0.2.0 on main ($LAST_OUT)"
assert_contains "tag=v0.2.0" "$LAST_OUT" "prints the tag"
assert_contains "version=0.2.0" "$LAST_OUT" "prints the version"
assert_contains "prerelease=false" "$LAST_OUT" "a plain version is not a pre-release"

# --- a tag that disagrees with Cargo.toml names both
check v0.3.0
assert_eq 1 "$LAST_CODE" "v0.3.0 against 0.2.0"
assert_contains "v0.3.0" "$LAST_OUT" "names the tag"
assert_contains "0.2.0" "$LAST_OUT" "names the manifest version"

# --- syntax: only vMAJOR.MINOR.PATCH[-pre-release]
for bad in latest v1 v1.2 release-3 v1.2.3abc v01.2.3 1.2.3 v1.2.3- "v1.2.3 " v; do
  check "$bad"
  assert_eq 1 "$LAST_CODE" "rejects the tag [$bad]"
  assert_contains "not a version tag" "$LAST_OUT" "explains [$bad]"
done
check
assert_eq 2 "$LAST_CODE" "no argument is a usage error"

# --- a pre-release tag with a matching manifest
sed -i.bak 's/^version = "0.2.0"/version = "0.2.0-rc.1"/' Cargo.toml && rm -f Cargo.toml.bak
git commit -q -am "chore(release): 0.2.0-rc.1" && git push -q origin main
rc_sha=$(git rev-parse HEAD)
SHA=$rc_sha check v0.2.0-rc.1
assert_eq 0 "$LAST_CODE" "v0.2.0-rc.1 ($LAST_OUT)"
assert_contains "prerelease=true" "$LAST_OUT" "a suffix makes it a pre-release"
assert_contains "version=0.2.0-rc.1" "$LAST_OUT" "the version keeps its suffix"

# --- a commit that is not on main
git checkout -q -b side
git commit -q --allow-empty -m "chore: side"
side_sha=$(git rev-parse HEAD)
SHA=$side_sha check v0.2.0-rc.1
assert_eq 1 "$LAST_CODE" "a tag on a commit that is not on main"
assert_contains "not on main" "$LAST_OUT" "says so"

# --- dry run: version from Cargo.toml, tag derived, main check skipped
run env GITHUB_SHA="$side_sha" "$script" --dry-run
assert_eq 0 "$LAST_CODE" "--dry-run on a side branch ($LAST_OUT)"
assert_contains "tag=v0.2.0-rc.1" "$LAST_OUT" "the tag is derived from the version"
assert_contains "prerelease=true" "$LAST_OUT" "and so is the pre-release flag"

# --- crates that disagree on the version
git checkout -q main
sed -i.bak 's/^version.workspace = true/version = "9.9.9"/' crates/x/Cargo.toml && rm -f crates/x/Cargo.toml.bak
SHA=$rc_sha check v0.2.0-rc.1
assert_eq 1 "$LAST_CODE" "workspace crates with different versions"
assert_contains "9.9.9" "$LAST_OUT" "the disagreement is shown"

finish
