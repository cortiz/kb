#!/usr/bin/env bash
# Reports a recurring problem without flooding: comments on the open issue carrying <label>, or opens
# one when there is none. Shared by the performance and audit workflows.
# Usage: track-issue.sh <label> <title> <body-file>
set -euo pipefail

if [ $# -ne 3 ] || [ ! -f "$3" ]; then
  echo "usage: track-issue.sh <label> <title> <body-file>" >&2
  exit 2
fi
label=$1
title=$2
body=$3

open=$(gh issue list --label "$label" --state open --json number --jq '.[0].number // empty')
if [ -n "$open" ]; then
  gh issue comment "$open" --body-file "$body"
else
  gh issue create --label "$label" --title "$title" --body-file "$body"
fi
