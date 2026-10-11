# Research: Git Sync

This document records the architectural and technical decisions for Milestone 9 (`009-git-sync`), keeping the KB in a git repository and in sync across machines with no custom server.

---

## R1: Git CLI Delegation & Execution Architecture

### Context
Constitution principle IX (Simplicity) and FR-001 mandate that kb must use the `git` command line installed on the user's system rather than embedding an in-process git library like `libgit2` or `git2-rs`. It must use the user's credentials, SSH agent, GPG signing, and hooks without storing or managing secrets.

### Decision
1. **Core Abstraction (`GitRunner`)**:
   - `kb-core` defines a pure trait `GitRunner` with methods for running git subcommands in a given working directory.
   - For production, `kb` implements `SystemGitRunner` utilizing `std::process::Command` targeting the system `git` binary.
   - For unit testing in `kb-core`, `FakeGitRunner` allows deterministic, scripted in-memory git responses without disk or network I/O.
2. **Execution Modes**:
   - **Batch Mode (Default)**: Used for status queries, diffs, log inspections, and background fetches. Standard streams are captured, `GIT_TERMINAL_PROMPT=0` is set, and input is closed (`stdin(null)`).
   - **Interactive Mode**: Used for operations that may prompt the user (credential entry, SSH passphrase, commit signing with pinentry, or interactive conflict resolution). If running inside the TUI, ratatui leaves the alternate screen and raw mode (M3 FR-002), hands the terminal to the git process, and restores the TUI when done.
3. **Environment Scrubbing & Isolation**:
   - All operations pass `-C <kb_root>` (or set `current_dir` to the KB root).
   - `GIT_DIR` and `GIT_WORK_TREE` environment variables are cleared so executing kb from inside a parent or child repository never leaks outer git contexts.
4. **Minimum Git Version**:
   - Minimum supported Git version is **Git 2.28+** (released July 2020), which supports `init.defaultBranch`, `--only` pathspec commits, and stable rebase mechanics. `kb sync` checks `git --version` at startup and exits with code 2 if git is missing or older than 2.28.

### Alternatives Considered
- *Embedding `git2-rs` (libgit2)*: Rejected. `libgit2` requires linking against OpenSSL/libssh2, does not support SSH agent or custom credential helpers consistently across macOS and Linux, lacks support for modern commit signing tools, and violates Constitution Principle IX.
- *Running a background sync daemon*: Rejected. Violates Constitution Principle III (Local-only, no server/daemon) and the project's no-daemon architectural rule.

---

## R2: Path-Limited Atomic Auto-Commits

### Context
User Story 1 and FR-005 require that when `git.auto_commit = true` (default when the KB root is a git repository), every write path (`add`, `edit`, `mv`, `rm`, `repin`, `restore`) creates exactly one commit containing *only* the touched entry file(s), without staging or committing other staged or unstaged changes in the repository.

### Decision
1. **Commit Mechanism**:
   - When an entry file is created or edited: `git commit --only <relative_path> -m "<message>"`.
   - When an entry file is renamed: `git commit --only <old_rel_path> <new_rel_path> -m "<message>"`.
   - When an entry file is deleted: `git commit --only <relative_path> -m "<message>"`.
   - `git commit --only <paths>` bypasses the git index for all other files, creating a commit strictly from the specified working-tree paths, leaving all other staged or unstaged modifications exactly as they were.
2. **Commit Message Formatting**:
   - Template follows `git.commit_template` (default: `kb: {action} {id}`).
   - Recognized actions: `add`, `edit`, `mv`, `rm`, `repin`, `restore`, `sync`, `import`.
   - Examples: `kb: add cw/authorizer-p99`, `kb: mv old/id -> new/id`, `kb: repin code/handle-request`.
   - Operations touching multiple entries list them in the commit body.
3. **Identity Verification & Failure Handling**:
   - If `git config user.name` or `git config user.email` is unset, `git commit` fails. kb leaves the file modified on disk, skips the commit without failing the save operation, and displays a one-time message explaining how to configure `user.name` and `user.email`. The changes remain on disk and are picked up during the next `kb sync`.
4. **Commit Signing Support**:
   - If `git config commit.gpgsign` is `true`, git may invoke GPG/SSH pinentry. When running within the TUI, ratatui suspends raw mode and alternate screen, gives the terminal to git, and resumes cleanly (FR-008).

### Alternatives Considered
- *Staging with `git add <file>` followed by `git commit`*: Rejected. If the user had previously staged other files manually, `git add` and `git commit` would include those unrelated staged files in kb's commit.
- *Temporary index via `GIT_INDEX_FILE`*: More complex than `git commit --only`. `git commit --only` is the native Git mechanism designed specifically for single-file commits without altering index state.

---

## R3: Single-Command Sync Workflow (`kb sync`)

### Context
User Story 2 and FR-009 define the 1-command sync workflow: Carlos runs `kb sync` on his laptop; kb commits local hand edits, pulls remote changes, pushes local commits, refreshes the index, and summarizes changes.

### Decision
1. **Workflow Pipeline**:
   The sync pipeline executes in strict sequential order, aborting on failure without partial corruption:
   - **Step 1: Sanity & Cleanliness Check**: Verify KB root is inside a git repo with an upstream branch configured. Verify that no unmanaged, non-entry files (such as `README.md`) have unstaged or staged modifications; if dirty unmanaged files exist, refuse to sync with exit code 1 and prompt the user to commit or stash them (clarified in Session 2026-10-10).
   - **Step 2: Secret Gate (Local Changes)**: Run secret checks (M1 FR-013) on all uncommitted entry files. If any secret finding exists without `kb:allow-secret`, abort immediately with exit code 1.
   - **Step 3: Commit Hand Edits**: If any `.md` entry files have been added, modified, renamed, or deleted outside kb, commit them in a single batch commit: `kb: sync N entries` (or `--message`).
   - **Step 4: Record Pre-Sync State**: Record the current `HEAD` commit SHA to enable exact rollback (`--abort`).
   - **Step 5: Fetch**: Run `git fetch <remote> <branch>` with timeout `sync.timeout` (default 30s).
   - **Step 6: Integration**:
     - Check remote status:
       - If local is already up to date with remote: skip integration and push; print `already in sync`.
       - If remote is ahead and local has no new commits: fast-forward (`git merge --ff-only <remote>/<branch>`).
       - If both local and remote have commits: integrate using `sync.strategy` (default: `rebase`, alternative: `merge`).
     - If conflicts occur, pause pipeline and transition to Conflict Resolution (R4).
   - **Step 7: Secret Gate (History Before Push)**: Traverse all commits to be pushed (`@{u}..HEAD`). If any commit introduced an unallowed secret, refuse to push (R5).
   - **Step 8: Push & Concurrency Retry**: Run `git push <remote> <branch>`. If the push is rejected because the remote moved concurrently:
     - Automatically run `git fetch` + integrate once more (FR-011).
     - Attempt push again. If rejected a second time, stop with the repository in a clean, synced-up-to-fetch state and exit 1 asking the user to retry.
   - **Step 9: State Migration & Index Refresh**: Detect renames between pre-sync and post-sync commits (`git diff -M --summary`) to migrate usage statistics and remembered variable values (FR-021). Incrementally reindex modified/added entries in SQLite.
   - **Step 10: Summary Report**: Display a structured report of entries added, changed, renamed, deleted on each side, warnings, secret allowances, and the final sync status (`in sync with origin/main`).
2. **Dry Run (`--dry-run`)**: Runs fetch and computes the sync plan (entries to commit, commits to pull, commits to push) without making commits, rebasing, or pushing.

### Alternatives Considered
- *Defaulting to `git merge` instead of `git rebase`*: Rejected. Rebase is the standard default for personal knowledge bases because it keeps history clean and linear, eliminating unnecessary merge commits. Merge remains an opt-in strategy via `sync.strategy = "merge"`.
- *Automatic stashing of unmanaged files*: Rejected during clarification (Session 2026-10-10). Automatic `git stash` and `git stash pop` on non-entry files risks stash pop merge conflicts on files kb doesn't manage.

---

## R4: Entry-by-Entry Conflict Resolution

### Context
User Story 3, FR-014–FR-017, and clarification Session 2026-10-10 define how conflicts are presented and resolved entry by entry without exposing users to a raw git rebase shell.

### Decision
1. **Conflict Discovery**:
   When `git rebase` or `git merge` produces conflicts, kb parses unmerged paths from `git status --porcelain` to identify conflicting entry files and their conflict kind:
   - `both_modified`: Both sides edited the same entry file.
   - `modified_vs_deleted`: One side modified, the other side deleted.
   - `deleted_vs_modified`: Local deleted, remote modified.
   - `both_added`: Both sides added an entry with the same path/ID.
   - `renamed_differently`: File renamed to different paths on each side.
2. **Resolution Options**:
   - `m` (Mine): Keep local version.
   - `t` (Theirs): Keep remote version.
   - `e` (Edit Merged): Open user's configured editor (`$EDITOR` / `$VISUAL`) with conflict markers or side-by-side template. On save, validate with M1 lint & secret checks. If invalid, offer to re-edit, save anyway, or discard (as in M6).
   - `b` (Keep Both): Keep local version as `<id>`, save remote version as `<id>-conflict-<short-hostname>.md`. If `<id>-conflict-<short-hostname>.md` already exists, prompt the user interactively in the TUI/CLI for a unique ID before saving (clarified in Session 2026-10-10).
   - `d` (Delete): In edit/delete conflicts, choose whether to delete the entry or keep the edited version.
   - `q` / `Esc` (Abort): Cleanly abort integration and restore repository to pre-sync commit.
3. **Rollback & Interrupted Sync**:
   - Before starting integration, kb records `pre_sync_commit` and in-progress state in `.kb/sync-state.json`.
   - On explicit abort (`Esc`, `q`, Ctrl-C) or failure, kb runs `git rebase --abort` (or `git reset --hard <pre_sync_commit>`) and restores the exact pre-sync working tree.
   - If the terminal crashes or process is killed mid-conflict, subsequent kb commands detect `.kb/sync-state.json`, report "sync in progress", and direct the user to `kb sync --continue` or `kb sync --abort`.
4. **TUI Conflict Screen**:
   - Side-by-side view if terminal $\ge 120$ columns; vertically stacked view if $< 120$ columns.
   - Highlights diff lines and itemizes frontmatter changes field by field.

### Alternatives Considered
- *Dropping user into standard git rebase shell*: Rejected. Constitution requires kb to be a self-contained terminal tool. Half-finished git rebases cause user confusion and risk accidental data loss.
- *Three-way automatic merging of frontmatter*: Rejected. Operational snippets are concise and critical; auto-merging YAML fields silently risks syntax errors or logical corruption. Explicit entry-by-entry user choice is safer.

---

## R5: Two-Stage Secret Scanning Gate

### Context
Constitution principle VI ("Secrets Are Never Stored") and FR-018–FR-020 mandate that no secret can ever leave the machine via commit or push.

### Decision
1. **Stage 1: Pre-Commit Gate (Local Files)**:
   - Before any commit created by kb (auto-commit, hand-edit commit in `kb sync`, `kb sync setup`, or `kb restore`), all entries included in the commit are scanned with M1 secret detection rules (`check_text`).
   - If any finding lacks the `kb:allow-secret` inline directive, the commit is blocked, every finding is listed with file and line, and kb exits 1.
2. **Stage 2: Pre-Push Gate (Commit History Traversal)**:
   - Before `git push`, kb determines the set of commits about to be pushed:
     - With upstream: `git rev-list @{u}..HEAD`.
     - Initial push without upstream: all commits in current branch history (`git rev-list HEAD`).
   - For every commit in the range, kb inspects modified `.md` files using `git show <commit>:<file>`.
   - If any historical commit contains an unallowed secret, the push is refused, even if the secret was deleted in a subsequent commit. kb reports the commit hash, file, and line, and provides instructions on how to rewrite local history using standard git tools (`git reset` or `git rebase -i`).
3. **Pre-Commit Hook Installation (`kb git install-hooks`)**:
   - Installs `.git/hooks/pre-commit` running `kb lint --staged --secrets-only`.
   - Chaining: If a pre-commit hook already exists, kb wraps the existing hook or appends to it so both hooks run. It never overwrites existing user hooks.
   - `kb git uninstall-hooks` cleanly removes only kb's hook block.

### Alternatives Considered
- *Only scanning the latest version of files before push*: Rejected. A user might commit a token by accident and later remove it in a second commit. Pushing would still leak the token in git history.
- *Automatically rewriting git history to erase secrets*: Rejected. Out of scope and dangerous. kb explains how to use standard git commands to fix history.

---

## R6: Cross-Machine Rename Tracking & Disposable Index Refresh

### Context
When entries are renamed on another machine and pulled into the local KB, usage statistics (`.kb/state.json`) and remembered variable values must follow the renamed entry (FR-021). The search index (`.kb/index.db`) must be refreshed from files without persisting derived index data in git (FR-022, Constitution II).

### Decision
1. **Rename Detection**:
   - After integration completes, kb runs `git diff -M -C --name-status <pre_sync_commit> HEAD`.
   - Paths matching `R<score> <old_path> <new_path>` are identified as remote renames.
   - The entry ID is extracted from relative paths: `old_id` $\to$ `new_id`.
2. **State Migration**:
   - In `.kb/state.json`, usage frequency counts, last used timestamps, and remembered variable values for `old_id` are migrated to `new_id`.
3. **Disposable Search Index Refresh**:
   - Modified, added, and renamed entry files are updated in SQLite (`.kb/index.db`).
   - `.kb/` remains git-ignored via `.kb/.gitignore` (`*`) and root `.gitignore`.

### Alternatives Considered
- *Syncing `.kb/state.json` via git*: Rejected. Usage statistics differ legitimately between machines (e.g. work vs personal). Constitution II requires that `.kb/` stays out of git.

---

## R7: Setup & Clone Workflows

### Context
User Story 5, FR-023–FR-024 provide idempotent setup commands for existing KBs and onboarding second machines.

### Decision
1. **`kb sync setup <remote-url> [--branch <name>]`**:
   - If not a git repo: initializes git (`git init`).
   - Ensures `.gitignore` exists and contains `.kb/`.
   - Scans all existing entries for secrets; aborts if any unallowed secrets are found.
   - Creates initial commit: `kb: initial import`.
   - Configures remote: `git remote add origin <remote-url>` (or updates `origin` if already present).
   - Configures upstream tracking and pushes: `git push -u origin <branch>` where `<branch>` is the user-specified branch, or active local branch (`git branch --show-current`), falling back to `main` if unborn (clarified in Session 2026-10-10).
2. **`kb clone <remote-url> [path]`**:
   - Target directory defaults to `~/kb`.
   - Clones repo via `git clone <remote-url> <target_path>`.
   - Verifies repo is not empty; if empty, exits with hint to run `kb sync setup` on the machine with entries.
   - Builds initial search index (`kb reindex`).
   - If path is not the default `~/kb`, offers to update `root = "..."` in `~/.config/kb/config.toml`.
   - Offers to run `kb git install-hooks`.
3. **`kb init --git`**:
   - Extends M1 `kb init` to automatically run `git init` and commit initial template files if git is available.

---

## R8: Offline Status & Background Fetch

### Context
User Story 6 and FR-025–FR-026 define how sync status is monitored without unexpected network calls.

### Decision
1. **Offline Status (`kb sync status`)**:
   - Runs locally without network:
     - Branch name (`git rev-parse --abbrev-ref HEAD`).
     - Upstream tracking branch (`git rev-parse --abbrev-ref @{u}`).
     - Commits ahead (`git rev-list --count @{u}..HEAD`).
     - Commits behind (`git rev-list --count HEAD..@{u}`).
     - Uncommitted modified/added entries from `git status --porcelain`.
     - In-progress sync / conflict state from `.kb/sync-state.json`.
     - Last fetch timestamp from `.git/FETCH_HEAD` modification time.
   - `--fetch` flag triggers an explicit fetch before computing status.
   - `--json` outputs machine-readable JSON status.
2. **Background Fetch in TUI (`sync.fetch_on_start`)**:
   - Disabled by default (opt-in via config: `sync.fetch_on_start = true`).
   - When enabled, spawns a background worker thread on TUI launch running `git fetch` with:
     - `GIT_TERMINAL_PROMPT=0`
     - SSH batch mode (`-o BatchMode=yes`)
     - Background timeout (`sync.timeout`, default 5s)
   - Guarantees TUI first paint is not delayed (M3 SC-001).
   - On completion, dispatches an event to the TUI event loop to refresh the header indicator.

---

## R9: Entry History & Safe Restore

### Context
User Story 7 and FR-027 define how users inspect an entry's historical revisions and restore past content.

### Decision
1. **History Listing (`kb history <id> [--json]`)**:
   - Locates entry file path and runs `git log --follow --format=... -- <file_path>` to follow renames.
   - Parses commits and extracts change summaries: diff stats comparing frontmatter fields, payload, tags, and variables.
2. **Diff Inspection (`kb history <id> --diff <rev>`)**:
   - Runs `git diff <rev>^..<rev> -- <file_path>` (or `git show <rev>:<file_path>`).
   - Outputs diff with syntax highlighting in TUI or terminal pager.
3. **Restoring Previous Version (`kb restore <id> --at <rev>`)**:
   - Retrieves historical entry text via `git show <rev>:<file_path>`.
   - Validates content with M1 lint and secret detection.
   - Writes to entry file via standard authoring path (`write_entry`).
   - Creates auto-commit: `kb: restore {id} from {short-rev}`.

---

## R10: TUI Sync Integration & Keybindings

### Context
FR-028–FR-030 specify how sync status, sync triggers, history inspection, and conflict resolution integrate into the ratatui interface.

### Decision
1. **Header Indicator**:
   - Shows badge in list view header:
     - In sync: `git ✓ in sync (10m ago)`
     - Ahead: `git ↑2`
     - Behind: `git ↓3`
     - Dirty: `git ● 4 changed`
     - Conflict: `git ⚠ conflict`
2. **Keybindings**:
   - `Alt-S`: Trigger `kb sync`. Header shows spinner; on completion, key bar shows sync summary. If conflicts arise, opens Conflict Screen.
   - `Alt-H`: Open History overlay for currently selected entry.
3. **Conflict Screen**:
   - Keybindings: `m` mine, `t` theirs, `e` edit merged, `b` keep both (prompts for ID on collision), `d` delete, `↑`/`↓` navigate, `Esc` abort with confirmation.
