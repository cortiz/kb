#!/usr/bin/env bash
# scripts/package.sh builds the release archive: fixed layout, fixed metadata, byte-identical on a
# second run, with a SHA256SUMS file (FR-016, US3.4).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/package.sh"
command -v python3 >/dev/null 2>&1 || { echo "skipped: needs python3"; exit 0; }

root="$TMP/root"
mkdir -p "$root/scripts" "$TMP/out"
echo "# kb" > "$root/README.md"
echo "MIT text" > "$root/LICENSE-MIT"
echo "Apache text" > "$root/LICENSE-APACHE"
printf '#!/bin/sh\necho kb-update\n' > "$root/scripts/update.sh"
printf '#!/bin/sh\necho kb 0.2.0\n' > "$TMP/kb"
chmod 600 "$TMP/kb"   # the archive must say 0755 regardless of the source mode
export KB_PACKAGE_ROOT="$root" SOURCE_DATE_EPOCH=1700000000

target=x86_64-unknown-linux-musl
archive="$TMP/out/kb-0.2.0-linux-x86_64.tar.gz"
assert_exit 0 "packages" -- "$script" 0.2.0 "$target" "$TMP/kb" "$TMP/out"
[ -f "$archive" ] || _fail "no archive at $archive"

listing() { # name mode uid gid mtime size for each member
  python3 - "$1" <<'F'
import sys, tarfile
with tarfile.open(sys.argv[1]) as t:
    for m in t.getmembers():
        print(m.name, oct(m.mode), m.uid, m.gid, m.mtime, m.uname, m.gname)
F
}
want="kb-0.2.0-linux-x86_64 0o755 0 0 1700000000  
kb-0.2.0-linux-x86_64/LICENSE-APACHE 0o644 0 0 1700000000  
kb-0.2.0-linux-x86_64/LICENSE-MIT 0o644 0 0 1700000000  
kb-0.2.0-linux-x86_64/README.md 0o644 0 0 1700000000  
kb-0.2.0-linux-x86_64/kb 0o755 0 0 1700000000  
kb-0.2.0-linux-x86_64/kb-update 0o755 0 0 1700000000  "
# the directory first, then files in byte order (capitals before lower case)
assert_eq "$want" "$(listing "$archive")" "layout and metadata"

# the gzip header carries no name (flags 00) and no time (mtime 00000000)
header=$(head -c 10 "$archive" | od -An -tx1 | tr -d ' \n')
case "$header" in 1f8b080000000000*) ;; *) _fail "gzip header has a name or a time: $header" ;; esac

# a second run, with different source mtimes, gives the same bytes
first=$(sha256 "$archive")
rm -f "$archive" "$TMP/out/SHA256SUMS"
touch -t 202001010000 "$TMP/kb" "$root/README.md" "$root/scripts/update.sh"
assert_exit 0 "packages again" -- "$script" 0.2.0 "$target" "$TMP/kb" "$TMP/out"
assert_eq "$first" "$(sha256 "$archive")" "byte-identical archives"

# SHA256SUMS: one line per archive, sorted by name, accepted by the checker
other=aarch64-apple-darwin
assert_exit 0 "packages a second target" -- "$script" 0.2.0 "$other" "$TMP/kb" "$TMP/out"
sums="$TMP/out/SHA256SUMS"
assert_eq 2 "$(wc -l < "$sums" | tr -d ' ')" "two lines"
assert_eq "$(LC_ALL=C sort -k2 "$sums")" "$(cat "$sums")" "sorted by file name"
case "$(head -1 "$sums")" in
  [0-9a-f]*"  kb-0.2.0-linux-x86_64.tar.gz") ;;
  *) _fail "line format: $(head -1 "$sums")" ;;
esac
if command -v sha256sum >/dev/null 2>&1; then check_sums="sha256sum -c"; else check_sums="shasum -a 256 -c"; fi
# shellcheck disable=SC2086
(cd "$TMP/out" && run $check_sums SHA256SUMS)
assert_eq 0 "$LAST_CODE" "the sums verify ($LAST_OUT)"

# packaging a target again replaces its line instead of adding another
assert_exit 0 "repackages" -- "$script" 0.2.0 "$other" "$TMP/kb" "$TMP/out"
assert_eq 2 "$(wc -l < "$sums" | tr -d ' ')" "still two lines"

# missing inputs are named
rm "$root/LICENSE-APACHE"
assert_exit 1 "a missing license" -- "$script" 0.2.0 "$target" "$TMP/kb" "$TMP/out"
assert_contains "LICENSE-APACHE" "$LAST_OUT" "the missing file is named"
echo "Apache text" > "$root/LICENSE-APACHE"
assert_exit 1 "a missing binary" -- "$script" 0.2.0 "$target" "$TMP/nope" "$TMP/out"
assert_contains "$TMP/nope" "$LAST_OUT" "the missing binary is named"
assert_exit 2 "usage" -- "$script" 0.2.0

finish
