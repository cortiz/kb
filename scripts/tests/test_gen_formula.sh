#!/usr/bin/env bash
# scripts/gen-formula.sh renders the Homebrew formula from a release's SHA256SUMS (FR-027, FR-029).
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp
script="$(repo_root)/scripts/gen-formula.sh"

h() { printf '%s' "$1" | head -c 1 | awk -v c="$1" 'BEGIN{ORS=""} {for(i=0;i<64;i++) print c}'; }
a=$(h a); b=$(h b); c=$(h c); d=$(h d)
cat > "$TMP/SUMS" <<S
$a  kb-0.2.0-aarch64-apple-darwin.tar.gz
$c  kb-0.2.0-aarch64-unknown-linux-musl.tar.gz
$b  kb-0.2.0-x86_64-apple-darwin.tar.gz
$d  kb-0.2.0-x86_64-unknown-linux-musl.tar.gz
S

assert_exit 0 "renders" -- "$script" v0.2.0 "$TMP/SUMS" "$TMP/kb.rb"
f=$(cat "$TMP/kb.rb")
assert_contains 'version "0.2.0"' "$f" "version"
assert_contains 'license any_of: ["MIT", "Apache-2.0"]' "$f" "license"
assert_eq 4 "$(grep -c '^ *url ' "$TMP/kb.rb")" "four url lines"
assert_eq 4 "$(grep -c '^ *sha256 ' "$TMP/kb.rb")" "four sha256 lines"
assert_not_contains "@" "$f" "no placeholder left"
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  assert_contains "https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-$t.tar.gz" "$f" "url for $t"
done
# each hash sits right under the url of its own target
check_pair() { # <target> <hash>
  local got
  got=$(grep -A1 "kb-0.2.0-$1.tar.gz" "$TMP/kb.rb" | grep sha256 | sed 's/.*"\(.*\)".*/\1/')
  assert_eq "$2" "$got" "sha256 of $1"
}
check_pair aarch64-apple-darwin "$a"
check_pair x86_64-apple-darwin "$b"
check_pair aarch64-unknown-linux-musl "$c"
check_pair x86_64-unknown-linux-musl "$d"
# the platform blocks pair OS and CPU with the right archive
block() { awk -v os="$1" -v cpu="$2" '$0 ~ "on_"os" do"{o=1} o&&$0 ~ "on_"cpu" do"{c=1} c&&/url /{print; exit}' "$TMP/kb.rb"; }
assert_contains "aarch64-apple-darwin" "$(block macos arm)" "macOS arm"
assert_contains "x86_64-apple-darwin" "$(block macos intel)" "macOS intel"
assert_contains "aarch64-unknown-linux-musl" "$(block linux arm)" "Linux arm"
assert_contains "x86_64-unknown-linux-musl" "$(block linux intel)" "Linux intel"
assert_contains 'bin.install "kb"' "$f" "installs the binary"
assert_contains "test do" "$f" "has a test block"
assert_contains "kb --version" "$f" "tests --version"

# a missing checksum names its target
grep -v x86_64-apple-darwin "$TMP/SUMS" > "$TMP/SUMS2"
run "$script" v0.2.0 "$TMP/SUMS2" "$TMP/out2.rb"
assert_eq 1 "$LAST_CODE" "missing checksum fails"
assert_contains "x86_64-apple-darwin" "$LAST_OUT" "and names the target"

# stable releases only
run "$script" v0.2.0-rc.1 "$TMP/SUMS" "$TMP/out3.rb"
assert_eq 1 "$LAST_CODE" "pre-release fails"
assert_contains "stable releases only" "$LAST_OUT" "with the reason"

assert_exit 2 "no arguments" -- "$script"

if command -v ruby >/dev/null 2>&1; then
  run ruby -c "$TMP/kb.rb"
  assert_eq 0 "$LAST_CODE" "ruby -c accepts the formula: $LAST_OUT"
fi
finish
