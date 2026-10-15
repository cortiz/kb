#!/usr/bin/env bash
# Builds a release archive: kb-<version>-<platform>.tar.gz (the platform name comes from
# platform-name.sh, e.g. macos-apple-silicon for aarch64-apple-darwin) holding one top-level directory with the
# binary, README.md and the license files, and adds its checksum to SHA256SUMS in <outdir>.
# The archive is reproducible: members in a fixed order, owner 0, modes fixed, modification time
# SOURCE_DATE_EPOCH, and no name or time in the gzip header. Python's tarfile does the work so the
# result is the same on macOS and Linux.
# Usage: package.sh <version> <target> <path-to-kb> <outdir>
#        KB_PACKAGE_ROOT (default: the repository) is where README.md and the licenses are read.
set -euo pipefail

if [ $# -ne 4 ]; then
  echo "usage: package.sh <version> <target> <path-to-kb> <outdir>" >&2
  exit 2
fi
version=$1
target=$2
kb=$3
outdir=$4
root=${KB_PACKAGE_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}

for f in "$kb" "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE"; do
  [ -f "$f" ] || {
    echo "error: $f is missing" >&2
    exit 1
  }
done
mkdir -p "$outdir"
platform=$("$(dirname "$0")/platform-name.sh" "$target")
archive="kb-$version-$platform.tar.gz"

SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0} python3 - "$version" "$platform" "$kb" "$root" "$outdir/$archive" <<'PY'
import gzip, os, sys, tarfile

version, platform, kb, root, out = sys.argv[1:6]
epoch = int(os.environ["SOURCE_DATE_EPOCH"])
top = f"kb-{version}-{platform}"
files = sorted(
    [
        ("LICENSE-APACHE", f"{root}/LICENSE-APACHE", 0o644),
        ("LICENSE-MIT", f"{root}/LICENSE-MIT", 0o644),
        ("README.md", f"{root}/README.md", 0o644),
        ("kb", kb, 0o755),
    ]
)


def fix(info, mode):
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mode = mode
    info.mtime = epoch
    return info


with open(out, "wb") as raw:
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            d = tarfile.TarInfo(top + "/")
            d.type = tarfile.DIRTYPE
            tar.addfile(fix(d, 0o755))
            for name, src, mode in files:
                info = fix(tar.gettarinfo(src, arcname=f"{top}/{name}"), mode)
                with open(src, "rb") as f:
                    tar.addfile(info, f)
PY

# SHA256SUMS: this archive's line replaces any earlier one; lines stay sorted by file name
if command -v sha256sum >/dev/null 2>&1; then
  hash=$(sha256sum "$outdir/$archive" | cut -d' ' -f1)
else
  hash=$(shasum -a 256 "$outdir/$archive" | cut -d' ' -f1)
fi
sums="$outdir/SHA256SUMS"
tmp=$(mktemp)
{
  if [ -f "$sums" ]; then grep -v "  $archive\$" "$sums" || true; fi
  echo "$hash  $archive"
} | LC_ALL=C sort -k2 >"$tmp"
mv "$tmp" "$sums"
echo "packaged $outdir/$archive"
