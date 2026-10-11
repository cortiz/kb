# Configuration Contracts: Git Sync

This document specifies the configuration keys and defaults under `~/.config/kb/config.toml` for Milestone 9 (`009-git-sync`).

---

## 1. `[git]` Table

Controls automatic commit behavior on local authoring operations (FR-005, FR-006).

```toml
[git]
# Automatically commit entries created, edited, moved, removed, or repinned through kb.
# Default: true if the KB root is inside a git working tree, false otherwise.
auto_commit = true

# Template used for generating commit messages.
# Placeholders:
#   {action} - add, edit, mv, rm, repin, restore, sync, import
#   {id}     - canonical entry ID (or "old/id -> new/id" for mv)
# Default: "kb: {action} {id}"
commit_template = "kb: {action} {id}"
```

---

## 2. `[sync]` Table

Controls remote synchronization behavior, network timeouts, and background polling (FR-010, FR-026).

```toml
[sync]
# Integration strategy used by `kb sync` when both local and remote have new commits.
# Options: "rebase" (keeps history linear), "merge" (creates merge commits)
# Default: "rebase"
strategy = "rebase"

# Foreground network timeout (in seconds) for fetch and push operations.
# Default: 30
timeout = 30

# Background fetch on TUI startup.
# When enabled, the TUI starts a non-blocking background fetch on open and updates
# the header sync status once finished without prompting for credentials.
# Default: false
fetch_on_start = false
```
