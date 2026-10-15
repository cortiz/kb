#!/bin/sh
# Re-records the README demo: scripts/demo/record.sh [path/to/kb]   (needs python3 and agg)
# Plays scripts/demo/storyboard.py against a throwaway KB and writes demo.cast and demo.gif.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
kb=${1:-$repo/target/release/kb}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/home"
"$here/make-kb.sh" "$work/home/kb"
python3 "$here/drive.py" "$kb" "$work/home/kb" "$repo/demo.cast"
agg --quiet --theme monokai --font-size 15 --speed 1.25 --idle-time-limit 2.5 --last-frame-duration 4 \
  "$repo/demo.cast" "$repo/demo.gif"
ls -l "$repo/demo.cast" "$repo/demo.gif"
