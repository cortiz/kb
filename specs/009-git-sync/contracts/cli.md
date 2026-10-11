# CLI Contracts: Git Sync

This document specifies the command-line interfaces, options, JSON payloads, and exit codes introduced or modified in Milestone 9 (`009-git-sync`).

---

## 1. Commands

### 1.1 `kb sync`

Executes the primary sync pipeline: validates secrets, commits local hand edits, fetches from remote, integrates changes, resolves conflicts, pushes local commits, migrates renames, and refreshes the index (FR-009).

```text
kb sync [OPTIONS]

OPTIONS:
    --dry-run           Fetch and report what would be committed, pulled, and pushed without modifying the repository
    --continue          Resume conflict resolution after an interrupted sync
    --abort             Abort an in-progress sync and restore the repository to its pre-sync commit
    --no-push           Fetch and integrate remote changes without pushing local commits
    -m, --message <MSG> Custom commit message for uncommitted hand edits (default: "kb: sync N entries")
    --json              Output structured sync summary in JSON format
```

**JSON Output Format (`--json`)**:
```json
{
  "status": "success",
  "local_added": 1,
  "local_modified": 2,
  "local_deleted": 0,
  "remote_added": 3,
  "remote_modified": 0,
  "remote_deleted": 1,
  "conflicts_resolved": 1,
  "warnings": [],
  "allowed_secrets": [],
  "unmanaged_files": [],
  "new_status": "in sync with origin/main"
}
```

---

### 1.2 `kb sync status`

Reports git synchronization status without triggering network access by default (FR-025).

```text
kb sync status [OPTIONS]

OPTIONS:
    --fetch             Fetch from remote before computing status
    --json              Output status in JSON format
```

**JSON Output Format (`--json`)**:
```json
{
  "is_git": true,
  "branch": "main",
  "upstream": "origin/main",
  "ahead": 2,
  "behind": 0,
  "uncommitted": [
    {
      "id": "cw/authorizer-p99",
      "rel_path": "cw/authorizer-p99.md",
      "status": "modified"
    }
  ],
  "unmanaged_changes": [],
  "last_fetch": 1775836800,
  "in_progress": null
}
```

---

### 1.3 `kb sync setup`

Configures git synchronization for an existing knowledge base (FR-023).

```text
kb sync setup <REMOTE_URL> [OPTIONS]

ARGS:
    <REMOTE_URL>        Git remote URL (SSH or HTTPS, e.g. git@github.com:user/kb.git)

OPTIONS:
    -b, --branch <NAME> Branch name to track and push (default: active local branch, or main if unborn)
```

**Behavior**:
- Initializes a git repository if not already present (`git init`).
- Ensures `.gitignore` excludes `.kb/`.
- Scans all entries for secrets; aborts before first commit if any unallowed secrets are found.
- Creates initial commit: `kb: initial import`.
- Adds remote `origin` and pushes with upstream tracking (`git push -u origin <branch>`).

---

### 1.4 `kb clone`

Clones an existing knowledge base repository to a new machine and sets up the local environment (FR-023).

```text
kb clone <REMOTE_URL> [PATH]

ARGS:
    <REMOTE_URL>        Git remote URL
    [PATH]              Destination directory (default: ~/kb)
```

**Behavior**:
- Clones the repository into the target directory.
- Rebuilds disposable search index (`kb reindex`).
- If target directory differs from standard `~/kb`, offers to update `root` in user configuration.
- Prompts to install pre-commit secret hooks (`kb git install-hooks`).

---

### 1.5 `kb git install-hooks` & `kb git uninstall-hooks`

Manages git hooks to protect against accidental secret leakage (FR-020).

```text
kb git install-hooks
kb git uninstall-hooks
```

**Behavior**:
- `install-hooks`: Installs or chains a pre-commit hook in `.git/hooks/pre-commit` running `kb lint --staged --secrets-only`. Chains existing user hooks without overwriting them.
- `uninstall-hooks`: Removes only the hook block installed by kb, leaving other chained hooks intact.

---

### 1.6 `kb history`

Inspects historical revisions of an entry (FR-027).

```text
kb history <ID> [OPTIONS]

ARGS:
    <ID>                Entry ID or search query resolving to a single entry

OPTIONS:
    --diff <REV>        Show diff for the specified commit revision
    --json              Output commit history list in JSON format
```

**JSON Output Format (`--json`)**:
```json
{
  "id": "sumo/5xx-authorizer",
  "commits": [
    {
      "hash": "a1b2c3d4e5f6...",
      "short_hash": "a1b2c3d",
      "author": "Carlos Ortiz <carlos@example.com>",
      "date": "2026-10-10T12:00:00Z",
      "message": "kb: edit sumo/5xx-authorizer",
      "change_categories": ["payload", "tags"]
    }
  ]
}
```

---

### 1.7 `kb restore`

Restores an entry's content from a historical git commit (FR-027).

```text
kb restore <ID> --at <REV>

ARGS:
    <ID>                Entry ID
OPTIONS:
    --at <REV>          Commit hash or git revision reference (required)
```

**Behavior**:
- Reads entry content at specified revision (`git show <rev>:<path>`).
- Validates content with M1 lint & secret detection.
- Writes to file on disk and creates an auto-commit: `kb: restore <id> from <short-rev>`.

---

### 1.8 `kb init --git`

Extends `kb init` (M1) to initialize a git repository and create the first commit (FR-024).

```text
kb init [PATH] [--git]
```

---

## 2. Exit Codes

In accordance with FR-032:

| Exit Code | Meaning |
|---|---|
| `0` | Success or already in sync |
| `1` | Blocked by unallowed secrets, unresolved conflicts in non-interactive mode, push rejected after retry, unmanaged dirty files blocking integration, or KB is not a git repository |
| `2` | Command usage error, Git CLI missing/outdated (< 2.28), or I/O error |
| `130` | Interrupted/Cancelled by user (Esc, `q`, Ctrl-C); repository restored to pre-sync state |
