#!/usr/bin/env bash
set -euo pipefail

# scripted_repo.sh: Generates a fixture git repository for testing drift detection and re-pinning.
# Usage: ./scripted_repo.sh <target-directory>

TARGET_DIR="${1:-}"
if [ -z "$TARGET_DIR" ]; then
    echo "Usage: $0 <target-directory>" >&2
    exit 1
fi

mkdir -p "$TARGET_DIR"
cd "$TARGET_DIR"

git init -b main
git config user.name "KB Test Double"
git config user.email "test@kb.internal"
git config commit.gpgsign false

# Commit 1 (Pinned Ref): Initial version
mkdir -p src
cat << 'EOF' > src/AuthHandler.java
package com.example;

public class AuthHandler {
    // Context setup
    public void configure() {
    }

    public boolean handleRequest(String token) {
        if (token == null) {
            return false;
        }
        return true;
    }

    public void cleanup() {
    }
}
EOF

git add src/AuthHandler.java
git commit -m "Initial commit with AuthHandler"
PINNED_COMMIT=$(git rev-parse HEAD)

# Branch 1: moved - 4 lines added above handleRequest, method itself unchanged
git checkout -b branch-moved
cat << 'EOF' > src/AuthHandler.java
package com.example;

import java.util.Map;
import java.util.HashMap;

public class AuthHandler {
    private Map<String, Object> cache = new HashMap<>();

    // Context setup
    public void configure() {
    }

    public boolean handleRequest(String token) {
        if (token == null) {
            return false;
        }
        return true;
    }

    public void cleanup() {
    }
}
EOF
git commit -am "Insert cache field and imports (moved)"
MOVED_COMMIT=$(git rev-parse HEAD)

# Branch 2: changed - method body modified
git checkout main
git checkout -b branch-changed
cat << 'EOF' > src/AuthHandler.java
package com.example;

public class AuthHandler {
    // Context setup
    public void configure() {
    }

    public boolean handleRequest(String token) {
        if (token == null || token.isEmpty()) {
            throw new IllegalArgumentException("Empty token");
        }
        return true;
    }

    public void cleanup() {
    }
}
EOF
git commit -am "Edit handleRequest validation (changed)"
CHANGED_COMMIT=$(git rev-parse HEAD)

# Branch 3: renamed - file renamed to TokenHandler.java
git checkout main
git checkout -b branch-renamed
git mv src/AuthHandler.java src/TokenHandler.java
git commit -m "Rename AuthHandler to TokenHandler"
RENAMED_COMMIT=$(git rev-parse HEAD)

# Branch 4: gone - method deleted
git checkout main
git checkout -b branch-gone
cat << 'EOF' > src/AuthHandler.java
package com.example;

public class AuthHandler {
    public void configure() {
    }

    public void cleanup() {
    }
}
EOF
git commit -am "Remove handleRequest method (gone)"
GONE_COMMIT=$(git rev-parse HEAD)

# Branch 5: symbol - heavy diff rewrite but method symbol remains
git checkout main
git checkout -b branch-symbol
cat << 'EOF' > src/AuthHandler.java
package com.example;

import java.util.Optional;

public class AuthHandler {
    /* Completely rewritten handler structure */
    private final String realm = "default";

    public Optional<Boolean> handleRequest(String token) {
        return Optional.ofNullable(token).map(t -> !t.isBlank());
    }
}
EOF
git commit -am "Rewrite handler completely (symbol fallback)"
SYMBOL_COMMIT=$(git rev-parse HEAD)

# Set origin/main pointing to branch-moved by default for standard tracking branch tests
git checkout main
# Create simulated remote tracking branch origin/main
git update-ref refs/remotes/origin/main "$MOVED_COMMIT"
git update-ref refs/remotes/origin/HEAD refs/remotes/origin/main

# Output metadata file for tests
cat << EOF > repo_manifest.json
{
  "pinned_commit": "$PINNED_COMMIT",
  "moved_commit": "$MOVED_COMMIT",
  "changed_commit": "$CHANGED_COMMIT",
  "renamed_commit": "$RENAMED_COMMIT",
  "gone_commit": "$GONE_COMMIT",
  "symbol_commit": "$SYMBOL_COMMIT",
  "pinned_line": 8,
  "moved_line": 15
}
EOF

echo "Scripted fixture repository generated at: $TARGET_DIR"
