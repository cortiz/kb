#!/usr/bin/env bash
set -euo pipefail

# scripted_sync_repos.sh: Generates a fixture bare remote and two local clones for sync testing.
# Usage: ./scripted_sync_repos.sh <target-base-directory>

BASE_DIR="${1:-}"
if [ -z "$BASE_DIR" ]; then
    echo "Usage: $0 <target-base-directory>" >&2
    exit 1
fi

mkdir -p "$BASE_DIR"
REMOTE_DIR="$BASE_DIR/remote.git"
CLONE1_DIR="$BASE_DIR/clone1"
CLONE2_DIR="$BASE_DIR/clone2"

# 1. Initialize bare remote
git init --bare -b main "$REMOTE_DIR"

# 2. Setup clone1 and create initial entries
git clone "$REMOTE_DIR" "$CLONE1_DIR"
cd "$CLONE1_DIR"
git config user.name "Carlos Ortiz"
git config user.email "carlos@example.com"
git config commit.gpgsign false

echo ".kb/" > .gitignore
mkdir -p cw sumo sql
cat << 'EOF' > cw/latency.md
---
kb_format: 1
id: cw/latency
title: P99 Latency Insights
type: cw
tags: [latency, p99]
---
fields @timestamp, @message | filter latency > 100
EOF

cat << 'EOF' > sumo/errors.md
---
kb_format: 1
id: sumo/errors
title: 5xx Error Spike
type: sumo
tags: [errors, prod]
---
_sourceCategory=prod/api | parse "error: *" as err | count by err
EOF

git add .gitignore cw/latency.md sumo/errors.md
git commit -m "kb: initial import"
git push -u origin main

# 3. Setup clone2
git clone "$REMOTE_DIR" "$CLONE2_DIR"
cd "$CLONE2_DIR"
git config user.name "Carlos Ortiz (Laptop)"
git config user.email "carlos@example.com"
git config commit.gpgsign false

echo "Generated test sync environment at $BASE_DIR"
