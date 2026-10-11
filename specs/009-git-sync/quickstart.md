# Quickstart: Git Sync

This guide provides end-to-end verification scenarios to test and validate git-based synchronization, auto-commits, secret gating, and conflict resolution in `kb`.

---

## Prerequisites

1. Built `kb` binary in `$PATH` or via `cargo build --workspace`.
2. System `git` $\ge 2.28$ installed.
3. Configured git identity (`git config user.name "Test User"`, `git config user.email "test@example.com"`).

---

## Scenario 1: Automatic Commits on Authoring

Verify that writing, renaming, and deleting entries creates isolated, path-limited commits without altering other staged files.

```bash
# 1. Create a test KB repository
mkdir -p /tmp/kb-test && cd /tmp/kb-test
git init
echo "# My Notes" > README.md
git add README.md
git commit -m "initial commit"

# 2. Stage an unrelated file manually
echo "staged scratch note" > scratch.txt
git add scratch.txt

# 3. Add an entry using kb
kb add --id "cw/latency" --type cw --title "Latency P99" \
  --body "fields @timestamp, latency | filter latency > 100" \
  --tags "aws,p99"

# 4. Verify git log shows an atomic commit for the entry
git log -1 --pretty=format:"%s%n%b"
# Expected output:
# kb: add cw/latency

# 5. Verify scratch.txt is still staged and was NOT committed
git status --porcelain
# Expected: 'A  scratch.txt' is still staged; working tree is otherwise clean
```

---

## Scenario 2: Single-Command Sync Across Clones

Verify two machines (clones) sharing a bare remote sync cleanly with a linear rebase history.

```bash
# 1. Set up bare remote
git init --bare /tmp/bare-remote.git

# 2. Setup clone 1
cd /tmp/kb-test
git remote add origin /tmp/bare-remote.git
git push -u origin main

# 3. Setup clone 2
kb clone /tmp/bare-remote.git /tmp/kb-clone2

# 4. Make a change in clone 1
kb add --id "sumo/5xx" --type sumo --title "5xx errors" \
  --body "_sourceCategory=prod | count" --tags "prod"
kb sync

# 5. Make a separate change in clone 2 (different file)
cd /tmp/kb-clone2
kb add --id "sql/users" --type sql --title "User count" \
  --body "SELECT count(*) FROM users" --tags "db"

# 6. Run kb sync on clone 2
kb sync
# Expected summary:
# Remote added: 1 (sumo/5xx)
# Local pushed: 1 (sql/users)
# in sync with origin/main

# 7. Verify linear history
git log --oneline -n 4
```

---

## Scenario 3: Entry-by-Entry Conflict Resolution

Verify interactive conflict handling when both sides edit the same entry.

```bash
# 1. Edit the same entry in clone 1
cd /tmp/kb-test
# Edit cw/latency tags
kb edit cw/latency  # change tags to: [aws, p99, prod]
kb sync

# 2. Edit the same entry differently in clone 2
cd /tmp/kb-clone2
# Edit cw/latency tags
kb edit cw/latency  # change tags to: [aws, p99, lambda]

# 3. Trigger sync on clone 2
kb sync
# Expected:
# Halts at conflict screen:
# Conflict 1 of 1: cw/latency (both modified)
# Choices: [m] Mine  [t] Theirs  [e] Edit Merged  [b] Keep Both  [Esc] Abort

# 4. Press 'b' (Keep Both)
# Expected:
# Local version kept as cw/latency
# Remote version saved as cw/latency-conflict-<hostname>
# Sync completes and pushes successfully
```

---

## Scenario 4: Secret Protection Gate

Verify that secrets block commits and pushes before they leave the machine.

```bash
# 1. Hand-edit an entry with an AWS access key
cat << 'EOF' > cw/leak.md
---
kb_format: 1
id: cw/leak
title: Secret Leak Test
type: cw
tags: [test]
---
AKIAIOSFODNN7EXAMPLE
EOF

# 2. Attempt sync
kb sync
# Expected output:
# Error: secret finding in cw/leak.md:7: AWS access key detected
# Sync refused: no commits or pushes performed (Exit code: 1)

# 3. Clean up the leak
rm cw/leak.md
```

---

## Scenario 5: History Inspection & Restore

Verify entry history tracing through renames and restoring a previous revision.

```bash
# 1. Inspect history of cw/latency
kb history cw/latency
# Lists all commits touching cw/latency with author, date, and change category

# 2. Inspect diff of a specific commit
kb history cw/latency --diff HEAD~1

# 3. Restore revision
kb restore cw/latency --at HEAD~1
# File is restored to previous revision content and committed with:
# kb: restore cw/latency from <short-sha>
```

---

## Scenario 6: Unmanaged Dirty Files Guard

Verify that unstaged modifications to non-entry files prevent rebase divergence.

```bash
# 1. Modify unmanaged README.md
echo "temporary notes" >> README.md

# 2. Attempt kb sync
kb sync
# Expected output:
# Error: cannot sync because unmanaged files have unstaged changes:
#   - README.md
# Please commit or stash unmanaged changes before syncing. (Exit code: 1)
```
