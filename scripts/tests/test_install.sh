#!/usr/bin/env bash
# Tests install.sh and scripts/update.sh
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
new_tmp

install_script="$(repo_root)/install.sh"
update_script="$(repo_root)/scripts/update.sh"

assert_exit 0 "install help" -- "$install_script" --help
assert_exit 0 "update help" -- "$update_script" --help

# Detect host platform string to match install.sh detection
os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin)
    case "$arch" in
      arm64) platform="macos-apple-silicon" ;;
      x86_64) platform="macos-intel" ;;
    esac
    ;;
  Linux)
    case "$arch" in
      x86_64) platform="linux-x86_64" ;;
      aarch64|arm64) platform="linux-arm64" ;;
    esac
    ;;
esac

echo "Running installer test for platform: $platform"

# Create a mock release archive
mock_root="$TMP/mock_pkg"
mkdir -p "$mock_root" "$TMP/dist"
echo "# Mock binary" > "$mock_root/kb"
cat << 'BIN' > "$mock_root/kb"
#!/bin/sh
echo "kb 0.9.0"
BIN
chmod 755 "$mock_root/kb"

archive_name="kb-0.9.0-${platform}.tar.gz"
tar -czf "$TMP/dist/$archive_name" -C "$mock_root" kb

if command -v sha256sum >/dev/null 2>&1; then
  hash=$(sha256sum "$TMP/dist/$archive_name" | cut -d' ' -f1)
else
  hash=$(shasum -a 256 "$TMP/dist/$archive_name" | cut -d' ' -f1)
fi
echo "$hash  $archive_name" > "$TMP/dist/SHA256SUMS"

# Test 1: Install to custom bin-dir with no PATH modification
bin_dir="$TMP/test_bin"
export KB_DOWNLOAD_URL="file://$TMP/dist/$archive_name"
export KB_CHECKSUMS_URL="file://$TMP/dist/SHA256SUMS"

assert_exit 0 "install runs" -- "$install_script" -b "$bin_dir" -v v0.9.0 --no-modify-path -y
[ -x "$bin_dir/kb" ] || _fail "kb binary not executable at $bin_dir/kb"
[ -x "$bin_dir/kb-update" ] || _fail "kb-update not executable at $bin_dir/kb-update"
out=$("$bin_dir/kb" --version)
assert_eq "kb 0.9.0" "$out" "installed binary outputs correct version"

# Test 2: PATH configuration in shell rc file
fake_home="$TMP/home"
mkdir -p "$fake_home"
touch "$fake_home/.bashrc"
export HOME="$fake_home" SHELL="/bin/bash"

assert_exit 0 "install with PATH config" -- "$install_script" -b "$bin_dir" -v v0.9.0 -y
grep -q "$bin_dir" "$fake_home/.bashrc" || _fail "PATH export missing in .bashrc"

# Idempotency check: running again shouldn't duplicate
assert_exit 0 "install idempotent" -- "$install_script" -b "$bin_dir" -v v0.9.0 -y
count=$(grep -c "$bin_dir" "$fake_home/.bashrc")
assert_eq "1" "$count" "PATH export line not duplicated"

# Test 3: Update script
# Create a v0.9.1 mock archive
cat << 'BIN' > "$mock_root/kb"
#!/bin/sh
echo "kb 0.9.1"
BIN
archive_name_v2="kb-0.9.1-${platform}.tar.gz"
tar -czf "$TMP/dist/$archive_name_v2" -C "$mock_root" kb

if command -v sha256sum >/dev/null 2>&1; then
  hash2=$(sha256sum "$TMP/dist/$archive_name_v2" | cut -d' ' -f1)
else
  hash2=$(shasum -a 256 "$TMP/dist/$archive_name_v2" | cut -d' ' -f1)
fi
echo "$hash2  $archive_name_v2" >> "$TMP/dist/SHA256SUMS"

export KB_DOWNLOAD_URL="file://$TMP/dist/$archive_name_v2"
assert_exit 0 "update runs" -- "$update_script" -b "$bin_dir" -v v0.9.1 -f
out_updated=$("$bin_dir/kb" --version)
assert_eq "kb 0.9.1" "$out_updated" "updated binary outputs new version"

echo "All installer and updater tests passed!"
