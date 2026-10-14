---
description: "Task list for M9: Git Sync"
---

# Tasks: Git Sync

**Input**: Design documents from `specs/009-git-sync/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII: `kb-core` git operations, auto-commit generation, sync pipeline, conflict resolution, secret gating, and history inspection follow red-green development: test tasks are written and verified failing before their implementation tasks. Glue (CLI commands, interactive conflict prompts, TUI overlays) is tested using `FakeGitRunner`, `FakePlatform`, and scripted integration test repositories.

**Organization**: Tasks are structured by phase and user story: Setup → Foundation → US1 (Auto-commit) → US4 (Secret gate) → US2 (1-command sync) → US3 (Conflict resolution) → US5 (Setup & clone) → US6 (Status & background fetch) → US7 (History & restore) → Polish.

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Parallelizable (different files, no dependency on incomplete tasks)
- **[Story]**: `[US1]` to `[US7]` for user story phases; omitted for Setup, Foundation, and Polish phases

## Path Conventions

- Core: `crates/kb-core/src/` (`git/`, `sync/`, `history.rs`, `config.rs`, `author/fsops.rs`)
- Binary: `crates/kb/src/` (`commands/`, `platform/`, `tui/`)
- Integration tests: `tests/` (`sync_two_clones.rs`, `sync_conflicts.rs`, `auto_commit_isolation.rs`, `hook_installation.rs`, `cli_clone_setup.rs`, `cli_sync_status.rs`, `cli_history.rs`)
- Fixtures: `tests/fixtures/git_sync/`

---

## Phase 1: Setup

**Purpose**: Test fixtures, mock repositories, and module scaffolding.

- [x] T001 Create fixture git repositories generator in `tests/fixtures/git_sync/scripted_sync_repos.sh`: script creating a bare remote and two local test repositories with initial commits, branches, and sample entries.
- [x] T002 [P] Create secret test fixtures in `tests/fixtures/git_sync/secret_entries/`: sample entries with AWS keys, GitHub tokens, private keys, and entries containing `kb:allow-secret`.
- [x] T003 [P] Create empty module scaffolds in `crates/kb-core/src/git/{mod,commit,status,secrets}.rs`, `crates/kb-core/src/sync/{mod,pipeline,conflict,state}.rs`, `crates/kb-core/src/history.rs`, and declare modules in `crates/kb-core/src/lib.rs`.
- [x] T004 [P] Create empty CLI command scaffolds in `crates/kb/src/commands/{sync,clone,hooks,history}.rs` and empty TUI scaffolds in `crates/kb/src/tui/{conflict_view,history_view}.rs`.

**Checkpoint**: `cargo build --workspace` passes; existing test suite remains green.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Configuration schema, GitRunner abstraction, and Git repository inspection primitives required by all user stories.

### Tests first (must fail)

- [x] T005 [P] Write failing tests for `[git]` and `[sync]` configuration additions in `crates/kb-core/src/config.rs`: parse `git.auto_commit` (default true if git repo), `git.commit_template` (default `kb: {action} {id}`), `sync.strategy` (`rebase` or `merge`), `sync.timeout` (default 30s), and `sync.fetch_on_start` (default false).
- [x] T006 [P] Write failing tests for `GitRunner` trait, `FakeGitRunner`, and Git version detection (requiring Git $\ge 2.28$) in `crates/kb-core/src/git/mod.rs`.
- [x] T007 [P] Write failing tests for `GitStatus` parsing and dirty unmanaged non-entry file detection in `crates/kb-core/src/git/status.rs`.

### Implementation

- [x] T008 Implement configuration additions for `[git]` and `[sync]` in `crates/kb-core/src/config.rs`. Makes T005 pass.
- [x] T009 Implement `GitRunner` trait, `FakeGitRunner`, command execution helpers, and Git version verification in `crates/kb-core/src/git/mod.rs`. Makes T006 pass.
- [x] T010 Implement `GitStatus` parsing (`branch`, `upstream`, `ahead`, `behind`, `uncommitted`, `unmanaged_changes`) in `crates/kb-core/src/git/status.rs`. Makes T007 pass.
- [x] T011 Implement `SystemGitRunner` and interactive terminal suspension execution in `crates/kb/src/platform/unix.rs` and `crates/kb/src/platform/fake.rs`.

**Checkpoint**: `cargo test --workspace` passes; all foundational unit tests pass.

---

## Phase 3: User Story 1 — Every change made through kb is committed (Priority: P1) 🎯 MVP

**Goal**: Every write path (`add`, `edit`, `mv`, `rm`, `repin`, `restore`) creates an isolated, path-limited commit (`git commit --only <paths>`), leaving other staged or unstaged changes untouched.

**Independent Test**: In a temporary git repository with unrelated staged and unstaged files, execute write operations and verify the resulting commit contains only the touched entry file and has the expected message.

### Tests for User Story 1

- [x] T012 [P] [US1] Write failing tests for `auto_commit_operation` in `crates/kb-core/src/git/commit.rs`: test path-limited commits with `git commit --only <paths>`, template interpolation (`kb: {action} {id}`), missing identity (`user.name`/`user.email`) skipping without error, and commit signing config detection.
- [x] T013 [P] [US1] Write failing integration tests for auto-commit isolation in `tests/auto_commit_isolation.rs`: test that creating, editing, renaming, and removing entries via kb commits only touched files while other staged and unstaged files remain untouched in git index and working tree.

### Implementation for User Story 1

- [x] T014 [US1] Implement `auto_commit_operation` in `crates/kb-core/src/git/commit.rs`. Makes T012 pass.
- [x] T015 [US1] Integrate `auto_commit_operation` into authoring save paths: `crates/kb-core/src/author/fsops.rs` (`write_entry`, `rename_entry`, `remove_entry`) and `crates/kb/src/author.rs`.
- [x] T016 [US1] Integrate `auto_commit_operation` into code re-pin path in `crates/kb-core/src/code/repin.rs` and `crates/kb/src/commands.rs`.
- [x] T017 [US1] Add identity check notification and TUI suspension for commit signing in `crates/kb/src/commands.rs` and `crates/kb/src/tui/app.rs`. Makes T013 pass.

**Checkpoint**: Auto-commits occur cleanly on all write paths; unrelated index/worktree modifications are preserved.

---

## Phase 4: User Story 4 — Never push a secret (Priority: P1)

**Goal**: Enforce pre-commit and pre-push secret scanning, preventing credentials from leaving the machine, with hook management.

**Independent Test**: Attempt commits and pushes containing fixture secrets across hand edits and commits; verify secret findings block execution, report file and line, and allow only `kb:allow-secret`.

### Tests for User Story 4

- [x] T018 [P] [US4] Write failing tests for pre-commit entry secret scanner in `crates/kb-core/src/git/secrets.rs`: verify blocking findings without `kb:allow-secret`, and allowing marked entries.
- [x] T019 [P] [US4] Write failing tests for historical commit scanner in `crates/kb-core/src/git/secrets.rs`: verify traversing `@{u}..HEAD` detects secrets introduced earlier in history even if deleted in latest commit.
- [x] T020 [P] [US4] Write failing integration tests for hook installation and chaining in `tests/hook_installation.rs`: verify installing, chaining existing user hooks, and uninstalling hooks cleanly.

### Implementation for User Story 4

- [x] T021 [US4] Implement `SecretGate::check_entries` in `crates/kb-core/src/git/secrets.rs`. Makes T018 pass.
- [x] T022 [US4] Implement `SecretGate::check_unpushed_history` in `crates/kb-core/src/git/secrets.rs` using `git rev-list` and `git show`. Makes T019 pass.
- [x] T023 [US4] Implement `kb git install-hooks` and `kb git uninstall-hooks` CLI commands in `crates/kb/src/commands/hooks.rs`. Makes T020 pass.

**Checkpoint**: No secret can be committed or pushed; pre-commit hooks chain cleanly.

---

## Phase 5: User Story 2 — Sync with one command (Priority: P1)

**Goal**: `kb sync` commits hand edits, fetches remote, integrates via rebase/merge, pushes with automatic retry on concurrent remote moves, migrates renames, and refreshes the search index.

**Independent Test**: Two clones sharing a bare remote; make independent changes in each, sync both, and verify both end with identical entries and a linear history.

### Tests for User Story 2

- [x] T024 [P] [US2] Write failing tests for `SyncEngine::plan` and hand-edit batch committing in `crates/kb-core/src/sync/pipeline.rs`: verify hand-edit commit message `kb: sync N entries`, pre-sync commit recording, and unmanaged dirty file guard.
- [x] T025 [P] [US2] Write failing tests for push rejection and automatic retry in `crates/kb-core/src/sync/pipeline.rs`: verify 1 retry on rejected push, and stopping cleanly if rejected twice.
- [x] T026 [P] [US2] Write failing integration tests for two-clone sync in `tests/sync_two_clones.rs`: test bare remote sharing, linear rebase history, `--dry-run`, and `--no-push`.

### Implementation for User Story 2

- [x] T027 [US2] Implement hand-edit scanning, dirty unmanaged file validation, and pre-sync commit recording in `crates/kb-core/src/sync/pipeline.rs`. Makes T024 pass.
- [x] T028 [US2] Implement fetch, rebase integration, pre-push secret check, push with concurrency retry, rename migration, and index refresh in `crates/kb-core/src/sync/pipeline.rs`. Makes T025 pass.
- [x] T029 [US2] Implement `kb sync` CLI subcommand with `--dry-run`, `--no-push`, `--message`, `--json` in `crates/kb/src/commands/sync.rs`. Makes T026 pass.
- [x] T030 [US2] Wire `Alt-S` keybinding in `crates/kb/src/tui/app.rs` to run foreground sync and report summary in status bar.

**Checkpoint**: `kb sync` synchronizes multi-clone repositories in one command with clean linear rebase history.

---

## Phase 6: User Story 3 — Resolve conflicts entry by entry (Priority: P1)

**Goal**: Detect integration conflicts per entry and provide interactive resolution choices (mine, theirs, edit merged, keep both with collision prompting, delete) with clean rollback on abort.

**Independent Test**: Scripted two-clone conflicts covering edit/edit, edit/del, and keep-both; verify resolution choices produce expected files and abort restores pre-sync state byte-for-byte.

### Tests for User Story 3

- [x] T031 [P] [US3] Write failing tests for conflict porcelain status parsing in `crates/kb-core/src/sync/conflict.rs`: test `both_modified`, `modified_vs_deleted`, `deleted_vs_modified`, `both_added`, and `renamed_differently`.
- [x] T032 [P] [US3] Write failing tests for conflict resolution choices in `crates/kb-core/src/sync/conflict.rs`: test mine, theirs, edit merged, delete, and keep both with interactive ID collision prompt.
- [x] T033 [P] [US3] Write failing tests for rollback and sync resumption in `crates/kb-core/src/sync/state.rs`: test restoring pre-sync commit on abort, and persisting `.kb/sync-state.json`.
- [x] T034 [P] [US3] Write failing integration tests for conflict resolution and abort in `tests/sync_conflicts.rs`: test scripted resolutions, invalid merged text lint loop, non-interactive exit 1, and `kb sync --abort` / `--continue`.

### Implementation for User Story 3

- [x] T035 [US3] Implement conflict parsing and `apply_resolution` in `crates/kb-core/src/sync/conflict.rs`. Makes T031 and T032 pass.
- [x] T036 [US3] Implement `SyncInProgressState` persistence, rollback (`git reset --hard`), and `--continue`/`--abort` in `crates/kb-core/src/sync/state.rs` and `pipeline.rs`. Makes T033 pass.
- [x] T037 [US3] Implement CLI interactive conflict resolution loop in `crates/kb/src/commands/sync.rs`. Makes T034 pass.
- [x] T038 [US3] Implement TUI Conflict Screen (`crates/kb/src/tui/conflict_view.rs`) supporting side-by-side ($\ge 120$) and stacked ($< 120$) diffs, frontmatter comparison, and keys `m`, `t`, `e`, `b`, `d`, `n`/`p`, `Esc`.

**Checkpoint**: Entry-by-entry conflict resolution works in both CLI and TUI; aborting restores pre-sync repository byte-for-byte.

---

## Phase 7: User Story 5 — Set up sync and add a second machine (Priority: P2)

**Goal**: Onboard existing KBs and new machines with `kb sync setup <remote-url> [--branch <name>]`, `kb clone <url> [path]`, and `kb init --git`.

**Independent Test**: Run `kb sync setup` on a non-git directory and an existing git repo with bare remote, and `kb clone` into a new target path; verify `.gitignore`, initial commit, remote tracking, and index building.

### Tests for User Story 5

- [x] T039 [P] [US5] Write failing tests for `kb sync setup` logic in `crates/kb-core/src/sync/mod.rs`: test git init, `.gitignore` creation with `.kb/`, initial secret scan, initial commit, remote setup, and active branch default resolution.
- [x] T040 [P] [US5] Write failing integration tests for `kb clone` and `kb init --git` in `tests/cli_clone_setup.rs`: test cloning from bare remote, building disposable index, hook prompt, and empty remote error.

### Implementation for User Story 5

- [x] T041 [US5] Implement `kb sync setup` subcommand in `crates/kb/src/commands/sync.rs`. Makes T039 pass.
- [x] T042 [US5] Implement `kb clone` subcommand in `crates/kb/src/commands/clone.rs`. Makes T040 pass.
- [x] T043 [US5] Extend `kb init` in `crates/kb/src/commands/init.rs` with `--git` flag to initialize git and make the first commit (FR-024).

**Checkpoint**: Setup and cloning workflows are fully operational and idempotent.

---

## Phase 8: User Story 6 — See sync state at a glance (Priority: P2)

**Goal**: Offline sync status reporting via `kb sync status` and persistent TUI header indicators with background fetch support.

**Independent Test**: Verify sync status output in CLI (`kb sync status` with and without `--fetch`/`--json`) and TUI header indicators across clean, ahead, behind, modified, and conflicted states.

### Tests for User Story 6

- [x] T044 [P] [US6] Write failing tests for `kb sync status` data resolution in `crates/kb-core/src/git/status.rs`: test ahead, behind, uncommitted, last fetch timestamp from `FETCH_HEAD`, and JSON serialization.
- [x] T045 [P] [US6] Write failing integration tests for `kb sync status` CLI in `tests/cli_sync_status.rs`: test `--json`, `--fetch`, and offline behavior without network calls.

### Implementation for User Story 6

- [x] T046 [US6] Implement `kb sync status` CLI subcommand in `crates/kb/src/commands/sync.rs`. Makes T044 and T045 pass.
- [x] T047 [US6] Implement TUI header sync indicator rendering in `crates/kb/src/tui/header.rs` (badges: in sync, ahead, behind, dirty, conflict, fetch auth).
- [x] T048 [US6] Implement background fetch worker thread and event dispatch in `crates/kb/src/tui/app.rs` respecting `sync.fetch_on_start`, 5s timeout, batch mode, and non-blocking startup.

**Checkpoint**: TUI header displays live sync status badges; offline CLI queries complete $\le 50$ ms.

---

## Phase 9: User Story 7 — History of an entry (Priority: P3)

**Goal**: Trace entry revisions across renames with `kb history <id>`, inspect diffs, restore past versions with `kb restore <id> --at <rev>`, and view history in TUI (`Alt-H`).

**Independent Test**: Commit multiple revisions of an entry including renames, verify `kb history` traces through renames, check diff display, and verify `kb restore` writes historical revision through lint and auto-commit.

### Tests for User Story 7

- [x] T049 [P] [US7] Write failing tests for `HistoryEngine` in `crates/kb-core/src/history.rs`: test `git log --follow`, change categorization (payload, tags, vars, frontmatter), diff generation, and reading content at revision.
- [x] T050 [P] [US7] Write failing integration tests for `kb history` and `kb restore` in `tests/cli_history.rs`: test JSON output, diff display, restore through lint and auto-commit.

### Implementation for User Story 7

- [x] T051 [US7] Implement `HistoryEngine` in `crates/kb-core/src/history.rs`. Makes T049 pass.
- [x] T052 [US7] Implement `kb history` and `kb restore` CLI subcommands in `crates/kb/src/commands/history.rs`. Makes T050 pass.
- [x] T053 [US7] Implement TUI History overlay modal in `crates/kb/src/tui/history_view.rs` and wire `Alt-H` keybinding in `crates/kb/src/tui/app.rs`.

**Checkpoint**: Entry revisions can be inspected, compared, and restored from both CLI and TUI.

---

## Phase 10: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, end-to-end quickstart validation, performance benchmarking, and compiler lint checks.

- [x] T054 [P] Update user documentation in `README.md` and man pages for all new sync commands, configuration keys, and TUI shortcuts.
- [x] T055 Performance benchmark in `benches/auto_commit.rs`: verify auto-commit adds $\le 100$ ms on 10,000-entry repository (SC-004).
- [x] T056 Run end-to-end scenarios from `quickstart.md` in integration test harness (`tests/quickstart_scenarios.rs`).
- [x] T057 Code cleanup, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories.
- **User Story 1 (Phase 3 - Auto-commit)**: Depends on Foundational completion. MVP milestone!
- **User Story 4 (Phase 4 - Secret gate)**: Depends on Foundational completion. Blocks US2 integration.
- **User Story 2 (Phase 5 - 1-command sync)**: Depends on US1 (auto-commit) and US4 (secret gate).
- **User Story 3 (Phase 6 - Conflict resolution)**: Depends on US2 (sync pipeline).
- **User Story 5 (Phase 7 - Setup & clone)**: Depends on US4 (secret gate) and US2 (sync pipeline).
- **User Story 6 (Phase 8 - Status & fetch)**: Depends on Foundational and US2.
- **User Story 7 (Phase 9 - History & restore)**: Depends on US1 (auto-commit on restore) and Foundational.
- **Polish (Phase 10)**: Depends on all user stories being complete.

---

## Implementation Strategy

### MVP First (User Story 1 Only)
1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1 (Auto-commit)
4. **VALIDATE**: Run `tests/auto_commit_isolation.rs`

### Incremental Delivery
1. Add User Story 4 (Secret Gate) → Validate `tests/hook_installation.rs`
2. Add User Story 2 (1-Command Sync) → Validate `tests/sync_two_clones.rs`
3. Add User Story 3 (Conflicts) → Validate `tests/sync_conflicts.rs`
4. Add User Story 5 (Setup & Clone) → Validate `tests/cli_clone_setup.rs`
5. Add User Story 6 (Status) → Validate `tests/cli_sync_status.rs`
6. Add User Story 7 (History) → Validate `tests/cli_history.rs`
7. Final polish & performance benchmarks
