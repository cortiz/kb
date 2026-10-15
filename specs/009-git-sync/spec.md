# Feature Specification: Git Sync

**Feature Branch**: `009-git-sync`
**Created**: 2026-10-10
**Status**: Draft
**Milestone**: M9
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M6 (`007-authoring`), M7 (`008-actions-code-links`). Independent of M8 (shell integration).
**Input**: Keep the KB in a git repository and in sync across machines with no server of kb's own: commit changes made through kb automatically, sync with one command, resolve conflicts entry by entry, never push a secret, and show an entry's history.

## Overview

The KB is plain files (principle I), so git already provides history, backup and sync between machines through any remote the user controls (a private GitHub repo, a self-hosted server, a USB drive). This milestone makes that the normal way to use kb, without the user having to remember git commands.

kb stays a thin layer over the user's own `git`: it uses their identity, credentials, signing and hooks, and never stores credentials. Three rules shape everything here:

1. **No data loss.** A sync either completes or leaves the repository exactly as it was before, with conflicts presented for the user to decide.
2. **No secrets leave the machine.** Every commit and push made by kb passes the M1 secret checks first.
3. **No surprise network access.** kb talks to the remote only on `kb sync`, or when the user turns on a configured sync point (principle III).

## Clarifications

### Session 2026-10-10

- Q: How should `kb sync` handle unmanaged, non-entry files (such as `README.md` or custom scripts) that have unstaged modifications before integration? (FR-010, FR-013) → A: Refuse to sync, exit with code 1, list the unmanaged modified files, and prompt the user to commit or stash them first.
- Q: When resolving a conflict with "keep both" (`b`), how should kb determine the new filename if `<id>-conflict-<short-hostname>.md` already exists on disk? (FR-015) → A: Prompt the user interactively in the conflict screen (TUI) or CLI prompt to provide a unique ID before saving.
- Q: When running `kb sync setup <remote-url>` without the `--branch` flag, what branch should kb configure as the upstream tracking branch? (FR-023) → A: Use the active local branch name (`git branch --show-current`), falling back to `main` if the repository was just initialized and HEAD is unborn.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Every change made through kb is committed (Priority: P1)

Carlos adds an entry, renames another and re-pins a code link. Each change appears in `git log` as its own commit, with a clear message, without him running git.

**Why this priority**: if commits depend on the user remembering, history and sync silently fall behind. Automatic commits are what make the rest of the milestone trustworthy.

**Independent Test**: in a temporary KB that is a git repository, run every write path from M6 and M7, and after each one check the new commit's message, author and exact set of files.

**Acceptance Scenarios**:

1. **Given** the KB root is a git work tree and `git.auto_commit` is on (the default), **When** kb writes, renames or deletes an entry (add, edit, mv, rm, re-pin, "save anyway"), **Then** kb creates one commit containing only the files that operation touched.
2. **Given** the user has other changes staged or unstaged, **Then** kb's commit includes none of them and leaves the index and working tree otherwise exactly as they were.
3. **Given** the commit message template (default `kb: {action} {id}`), **Then** messages read like `kb: add cw/authorizer-p99-latency`, `kb: mv old/id -> new/id`, `kb: repin code/handle-request`.
4. **Given** the user's git has no identity configured, **Then** kb still writes the file, skips the commit, and shows once how to set `user.name` and `user.email`.
5. **Given** commit signing is configured and needs interaction (for example a passphrase prompt), **Then** kb suspends the TUI while committing so the prompt can use the terminal, and resumes afterwards.
6. **Given** the KB is not a git repository, **Then** writes work as before, and no git command is run.

---

### User Story 2 — Sync with one command (Priority: P1)

On his laptop Carlos runs `kb sync`. kb commits his hand edits, pulls the entries he added on his work machine, pushes his own, and reports what changed.

**Why this priority**: this is the multi-machine workflow the milestone exists for.

**Independent Test**: two clones of a fixture KB sharing a bare remote; make changes on each, sync both, and check that both end with identical entries and a linear history.

**Acceptance Scenarios**:

1. **Given** `kb sync`, **Then** kb in order: checks secrets in all changed entries (FR-018); commits hand-edited entries (one commit, `kb: sync N entries`); fetches; integrates remote changes (FR-010); pushes; refreshes the index; and prints a summary: entries added, changed, renamed and deleted on each side, and the new state (`in sync with origin/main`).
2. **Given** nothing changed on either side, **Then** kb prints `already in sync` and exits 0 after one fetch.
3. **Given** the remote has changes and the local KB has none, **Then** kb fast-forwards and does not push.
4. **Given** the push is rejected because the remote moved during the sync, **Then** kb fetches and integrates again once, then pushes. If it is rejected again, kb stops with the repository in a clean, synced-up-to-the-last-fetch state and asks the user to retry.
5. **Given** the TUI is open, **When** the user presses Alt-S, **Then** the same sync runs with progress in the header and the summary in the key bar. If it needs the user (credentials, conflicts), the TUI hands over the terminal or opens the conflict screen.
6. **Given** `kb sync --dry-run`, **Then** kb fetches and reports what would be committed, pulled and pushed, and changes nothing.

---

### User Story 3 — Resolve conflicts entry by entry (Priority: P1)

Carlos edited the same Sumo query on both machines. `kb sync` stops at that entry and shows both versions side by side. He keeps his version for the payload but takes the other machine's tags by editing a merged version.

**Why this priority**: without a clear conflict flow, users are left inside a half-finished git rebase, which is exactly the situation kb should never create.

**Independent Test**: scripted two-clone scenarios for every conflict kind (both edited, edited vs deleted, both renamed differently, both added the same ID); check each resolution choice produces the expected files and that aborting restores the pre-sync state byte for byte.

**Acceptance Scenarios**:

1. **Given** integration produces conflicts, **Then** kb lists each conflicting entry with the kind of conflict and opens the first one in the conflict screen (TUI) or prompts for it (CLI).
2. **Given** a conflict where both sides edited an entry, **Then** the user can choose: **m**ine, **t**heirs, **e**dit a merged version (opens the editor with both versions marked), or keep **b**oth (theirs is saved as `<id>-conflict-<short-hostname>`; if that ID already exists, kb prompts interactively for a unique ID).
3. **Given** an edit/delete conflict, **Then** the choices are: keep the edited entry, or delete it.
4. **Given** every choice yields a valid entry, **Then** kb continues the integration, commits, and pushes as in User Story 2. A merged version that fails lint is sent back to the editor, with the same choices as M6.
5. **Given** the user quits during conflict resolution (Esc on the list, `q` in the CLI, or Ctrl-C), **Then** kb aborts the integration and the repository returns to exactly its pre-sync state; the local commits made in step 1 remain.
6. **Given** `kb sync --continue` or `kb sync --abort` after an interrupted sync (for example a closed terminal), **Then** kb resumes resolving the remaining conflicts or aborts cleanly.

---

### User Story 4 — Never push a secret (Priority: P1)

Carlos hand-edits an entry and pastes a real token into it. `kb sync` refuses to commit or push, names the file and line, and tells him how to fix it.

**Why this priority**: constitution principle VI. A KB synced through git is one push away from leaking whatever it contains.

**Independent Test**: in a test git repository, attempt commits and pushes with fixture secrets across all write paths (hand edits, kb commands, hooks); verify every secret blocks commit/push, reports file and line, and allows only `kb:allow-secret`.

**Acceptance Scenarios**:

1. **Given** any entry to be committed has a secret finding (M1 FR-013), **Then** kb commits nothing, pushes nothing, lists every finding, and exits 1.
2. **Given** local commits that are not yet pushed contain a secret anywhere in their history (for example committed by hand and later removed), **Then** kb refuses to push and explains that the secret is still in history and how to remove it.
3. **Given** `kb git install-hooks`, **Then** kb installs a pre-commit hook that runs the same secret check on staged entries, so commits made outside kb are protected too. Existing hooks are kept and chained, never overwritten.
4. **Given** a finding marked with `kb:allow-secret` (M1), **Then** it does not block, but the sync summary lists it.

---

### User Story 5 — Set up sync and add a second machine (Priority: P2)

Carlos has used kb locally for a week and now wants it on his work laptop.

**Why this priority**: onboard new users and second devices to git-based sync safely with idempotent commands.

**Independent Test**: run `kb sync setup` on a non-git directory and an existing git repo with bare remote, and `kb clone` into a new target path; verify `.gitignore`, initial commit, remote tracking, and index building.

**Acceptance Scenarios**:

1. **Given** `kb sync setup <remote-url> [--branch <name>]` on a KB that is not a git repository, **Then** kb initializes git, makes sure `.gitignore` excludes `.kb/`, commits all entries that pass the secret check (`kb: initial import`), adds the remote, and pushes with upstream tracking using the specified branch or the active local branch (falling back to `main` if unborn).
2. **Given** the KB is already a git repository without a remote, **Then** kb only adds the remote and pushes.
3. **Given** entries with secret findings exist during setup, **Then** setup stops before the first commit and lists them.
4. **Given** `kb clone <remote-url> [path]` on another machine, **Then** kb clones the repository (default `~/kb`), builds the index, offers to set `root` in config if the path isn't the default, and offers to install the hooks.
5. **Given** the remote is empty, **Then** `kb clone` fails with a hint to run `kb sync setup` on the machine that has the entries.

---

### User Story 6 — See sync state at a glance (Priority: P2)

Carlos wants to know if his local knowledge base is up to date or has pending changes without triggering unintended network activity.

**Why this priority**: visibility into sync health prevents surprise divergence and supports offline confidence.

**Independent Test**: verify sync status output in CLI (`kb sync status` with and without `--fetch`/`--json`) and TUI header indicators across clean, ahead, behind, modified, and conflicted states.

**Acceptance Scenarios**:

1. **Given** the TUI is open in a KB with a remote, **Then** the header shows the sync state: `git ✓ in sync`, `↑2` (local commits not pushed), `↓3` (remote commits not pulled, as of the last fetch), `● 4 changed` (uncommitted hand edits), or `⚠ conflict`, plus the time of the last fetch.
2. **Given** `kb sync status`, **Then** kb prints the same information, the remote and branch, and the list of uncommitted entries, without network access. `--fetch` fetches first. `--json` is supported.
3. **Given** `sync.fetch_on_start = true` in config, **Then** the TUI starts a fetch in the background when it opens and updates the header when it finishes, without delaying the first frame and without prompting for credentials.

---

### User Story 7 — History of an entry (Priority: P3)

Carlos wants to see when a snippet changed, compare against an earlier revision, and restore previous versions if needed.

**Why this priority**: plain-file git history should be accessible directly within kb workflows without dropping to raw git commands.

**Independent Test**: commit multiple revisions of an entry including renames, verify `kb history` traces through renames, check diff display, and verify `kb restore` writes the historical revision through lint and auto-commit.

**Acceptance Scenarios**:

1. **Given** `kb history <id>`, **Then** kb lists the commits that touched the entry, following renames, with date, author, message and a short summary of what changed (payload, tags, variables, other).
2. **Given** `kb history <id> --diff <rev>`, **Then** kb shows the diff of the entry at that revision.
3. **Given** `kb restore <id> --at <rev>`, **Then** kb writes the entry's content from that revision through the normal save path (lint, secret check, commit `kb: restore <id> from <short-rev>`).
4. **Given** the TUI, **When** the user presses Alt-H on an entry, **Then** a history overlay lists the same commits, and Enter shows the diff for one of them.

---

### Edge Cases

- **`git` not installed or too old**: sync commands explain what's needed (minimum version decided in the plan) and exit 2; everything else in kb works.
- **No upstream branch**: `kb sync` explains and offers `kb sync setup`.
- **Detached HEAD or a rebase/merge already in progress that kb didn't start**: kb refuses to sync, explains the state, and changes nothing.
- **Non-entry files changed by hand** (README, scripts): kb never commits them. If non-entry files have unstaged or staged modifications when `kb sync` runs, kb refuses to sync, lists them, and exits with code 1 without modifying the working tree, prompting the user to commit or stash them. The sync summary lists unmanaged files as "not managed by kb".
- **Entries with non-secret lint errors**: committed and synced (they are the user's files), with a warning in the summary.
- **`.kb/` not ignored**: kb adds the ignore line to `.gitignore` in its own commit before the first commit it makes, and never commits anything under `.kb/`.
- **A remote rename** (an entry renamed on the other machine): usage and remembered values follow the rename using git's rename information (FR-021).
- **Entry deleted remotely while open in the TUI**: the selection moves to the nearest result, as in M3.
- **Credentials prompts**: allowed for foreground `kb sync`; background fetches never prompt and simply report "fetch needs credentials" in the header.
- **Network down or slow**: fetch and push time out (`sync.timeout`, default 30 seconds for foreground, 5 seconds for background), and kb reports the failure without changing local state.
- **Large KBs**: sync time is dominated by git; kb adds no per-entry network round trips.
- **Line endings**: kb follows the repository's git configuration and never rewrites line endings on its own.
- **Case-only renames on case-insensitive file systems** (macOS): handled through `git mv` as in M6, with a two-step rename when needed.

## Requirements *(mandatory)*

### Git integration

- **FR-001**: kb MUST use the `git` command line installed on the system, with the user's configuration, identity, credential helpers, SSH agent, signing and hooks. kb MUST NOT store, read or request credentials itself.
- **FR-002**: kb MUST detect whether the KB root is inside a git work tree, and which branch and upstream it tracks.
- **FR-003**: All git operations MUST run with the KB root as the working directory and MUST NOT affect other repositories.
- **FR-004**: kb MUST NOT commit, stage, or modify any file outside the set of entry files, `.gitignore`, and hook files the user asked for.

### Automatic commits

- **FR-005**: With `git.auto_commit = true` (default when the KB is a git repository), every kb operation that writes, renames or deletes entries MUST create exactly one commit containing exactly the touched files, using a path-limited commit that leaves other staged changes staged and other unstaged changes unstaged.
- **FR-006**: Commit messages MUST follow `git.commit_template` (default `kb: {action} {id}`), with actions `add`, `edit`, `mv`, `rm`, `repin`, `restore`, `sync`, `import`. Operations touching several entries MUST list them in the message body.
- **FR-007**: If committing fails (no identity, a hook rejects it, signing fails), the file change MUST stay on disk, kb MUST report the reason, and the next `kb sync` MUST pick the change up.
- **FR-008**: When commit signing or hooks may need the terminal, kb MUST suspend the TUI for the commit (M3 FR-002).

### Sync

- **FR-009**: `kb sync` MUST run these steps in order and stop at the first failure without leaving a partial state: secret check → commit hand edits → fetch → integrate → (resolve conflicts) → push → refresh index → summary.
- **FR-010**: Integration MUST use `sync.strategy`: `rebase` (default, keeps history linear) or `merge`. If any unmanaged/non-entry files have unstaged or staged changes, `kb sync` MUST refuse to integrate, list the files, and exit 1 so the working tree is never partially modified or stash-conflicted. Before integrating, kb MUST record the pre-sync commit so `--abort` and quitting can restore it exactly.
- **FR-011**: A rejected push MUST trigger one automatic fetch-integrate-push retry (User Story 2, scenario 4).
- **FR-012**: `kb sync` MUST support `--dry-run`, `--continue`, `--abort`, `--no-push` (fetch and integrate only), and `--message` (message for the hand-edit commit).
- **FR-013**: Hand-edited entries MUST be detected as any added, modified, deleted or renamed `.md` file under the KB root that git reports as changed, excluding hidden directories (M1 FR-001).

### Conflicts

- **FR-014**: Conflicts MUST be presented per entry, with the kinds: both modified, modified vs deleted, deleted vs modified, both added, renamed differently on each side.
- **FR-015**: Resolution choices MUST be as in User Story 3. "Keep both" MUST save the other side's version under a new ID with a `-conflict-<short-hostname>` suffix and a note in its body naming the original. If `<id>-conflict-<short-hostname>` already exists, kb MUST prompt the user interactively in the conflict screen (TUI) or CLI prompt for a unique ID before saving. Every resolved entry MUST pass the same lint and secret checks as any save (M6 FR-017).
- **FR-016**: The conflict screen in the TUI MUST show both versions side by side (stacked below 120 columns), with differing lines highlighted and frontmatter differences listed field by field.
- **FR-017**: Quitting at any point during conflict resolution MUST restore the repository to the pre-sync commit with the working tree unchanged from before integration. An interrupted sync MUST be detectable on the next kb command, which MUST point to `--continue` and `--abort`.

### Secret gate

- **FR-018**: Before any commit made by kb, every entry in that commit MUST pass the M1 secret checks; any finding without `kb:allow-secret` MUST block the commit.
- **FR-019**: Before any push, kb MUST scan the entry content of every commit that would be pushed (not only the latest version), and MUST refuse to push if any of them contains a secret finding, naming the commit, file and line.
- **FR-020**: `kb git install-hooks` MUST install a pre-commit hook running `kb lint --staged --secrets-only`, chaining any existing hook (and any configured `core.hooksPath`) instead of replacing it. `kb git uninstall-hooks` MUST remove only what kb added.

### State and index after sync

- **FR-021**: After integrating, kb MUST use git's rename detection between the pre-sync and post-sync commits to carry usage and remembered variable values across renames made on the other machine (extending M2 FR-018 and M6 FR-022), then refresh the index.
- **FR-022**: `.kb/` (index, usage, remembered values, clone cache) MUST stay per machine and never be committed. Usage statistics are intentionally not synced.

### Setup and clone

- **FR-023**: `kb sync setup <remote-url> [--branch <name>]` and `kb clone <remote-url> [path]` MUST behave as in User Story 5, and MUST be safe to re-run (doing only what is missing). When `--branch` is omitted in `kb sync setup`, kb MUST default to the active local branch (`git branch --show-current`), or `main` if HEAD is unborn.
- **FR-024**: `kb init` (M1) MUST gain `--git`, which initializes a repository and makes the first commit.

### Status

- **FR-025**: The core MUST provide sync status without network access: branch, upstream, commits ahead and behind as of the last fetch, uncommitted entries, conflict or interrupted-sync state, and last fetch time.
- **FR-026**: Background fetches (`sync.fetch_on_start`) MUST never prompt (`GIT_TERMINAL_PROMPT=0` and an SSH batch mode), MUST respect the background timeout, and MUST never integrate or push.

### History

- **FR-027**: `kb history <id> [--diff <rev>] [--json]` and `kb restore <id> --at <rev>` as in User Story 7. History MUST follow renames.

### TUI

- **FR-028**: The M3 header MUST show the sync status from FR-025 when the KB is a git repository.
- **FR-029**: New keys in the main list: Alt-S sync, Alt-H history of the selected entry. They MUST be added to the M3 KeyMap without clashes.
  *Amended by spec 013: in the vim keymap's normal mode these are `s` sync and `H` history (Alt-S and Alt-H keep working). The keys are defaults and can be changed in `[tui.keys.<context>]` (013 FR-018).*

- **FR-030**: The conflict screen keys: `m` mine, `t` theirs, `e` edit merged, `b` keep both, `d` delete (edit/delete conflicts), ↑/↓ or `n`/`p` between conflicts, Esc abort (with confirmation).

### CLI and exit codes

- **FR-031**: Commands: `kb sync`, `kb sync status`, `kb sync setup`, `kb clone`, `kb git install-hooks`, `kb git uninstall-hooks`, `kb history`, `kb restore`, all with `--json` where they produce data.
- **FR-032**: Exit codes: `0` success or already in sync; `1` blocked by secrets, conflicts left unresolved in non-interactive mode, push rejected twice, or not a git repository; `2` usage, git missing, or I/O errors; `130` cancelled (repository restored).

### Key Entities

- **SyncStatus**: branch, upstream, ahead, behind, uncommitted entries, last fetch time, in-progress operation.
- **SyncPlan**: entries to commit, commits to pull and push, and the pre-sync commit for rollback.
- **Conflict**: entry ID (or IDs, for renames), kind, the two versions, and the base version when available.
- **Resolution**: the choice per conflict and the resulting file content.
- **SyncSummary**: per side, entries added, changed, renamed, deleted; warnings; secret findings; unmanaged files changed.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a scripted two-machine test with 200 random operations on each side, followed by syncs and scripted conflict resolutions, both clones end with identical entries and no content is lost: every version written on either side is either present or was explicitly discarded by a resolution choice.
- **SC-002**: In 100 runs that interrupt a sync at every step (killed process, closed terminal, network loss), the repository is always either fully synced or exactly at its pre-sync state, and the next command reports any interrupted sync.
- **SC-003**: No test path (auto-commit, `kb sync`, `kb sync setup`, hooks) can commit or push a fixture secret; history containing a secret is always caught before push.
- **SC-004**: Auto-commit adds 100 ms or less to a save on a repository with 10,000 entries and 5,000 commits.
- **SC-005**: With `fetch_on_start` on and the network unavailable, the TUI's first frame is not delayed (M3 SC-001 still holds).
- **SC-006**: A second machine goes from nothing to a working, synced KB with `kb clone <url>` in one command and under one minute for a KB of 1,000 entries over a normal connection.

## Out of Scope

- A kb-hosted sync service, or syncing without git.
- Syncing usage statistics and remembered values between machines.
- Encrypting the repository or individual entries.
- Branch workflows (pull requests, review) for the KB.
- Automatic periodic sync in the background (would require a daemon; principle III and the no-daemon rule).
- Removing secrets from already-pushed history (kb explains the standard tools instead).

## Assumptions

- The user owns or controls a private remote and has working git credentials for it.
- A linear history (rebase) is the better default for a personal KB, since merges add noise and conflicts are rare.
- Conflicts are rare in practice (one person, a few machines), so per-entry interactive resolution is enough; no three-way merge of frontmatter fields is attempted automatically.
- Usage statistics differ legitimately between machines (work vs personal), so keeping them local is a feature, not a gap.

## Constitution Check notes

- **I. Plain files**: git operates on the entry files as they are; no export or conversion step.
- **II. Disposable index**: `.kb/` stays out of git; after a pull the index is refreshed from the files.
- **III. Local-only**: network access happens only on `kb sync`, `kb clone`, `kb sync setup`, explicit `--fetch`, and the opt-in `fetch_on_start`; kb runs no daemon.
- **VI. Secrets**: two gates (before commit, before push), plus an optional hook for commits made outside kb.
- **VII. Keyboard-First and Fast**: sync shortcuts (Alt-S, Alt-H, conflict resolution keys) and performance budget for auto-commit (≤ 100 ms) and TUI startup.
- **VIII. Test-First Core**: scripted multi-clone sync scenarios, secret gating test suites, and rollback verification.
- **IX. Simplicity**: kb delegates to the user's git rather than embedding a git implementation, and adds only the conflict flow on top.
