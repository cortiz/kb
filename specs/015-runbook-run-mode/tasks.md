# Tasks: Runbook Run Mode

**Input**: Design documents from `specs/015-runbook-run-mode/`
**Prerequisites**: `plan.md`, `spec.md`, `research.md`, `data-model.md`, `contracts/`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story mapping (`US1`, `US2`, `US3`, `US4`, `US5`, `US6`)

---

## Phase 1: Setup & Data Modeling

**Purpose**: Core domain types, storage layout, and serialization

- [X] T001 [P] Create `crates/kb-core/src/runbook/run/state.rs` with `Run`, `StepState`, `StepStatus`, `RunState`, `RunLock`, `StatusTransition`, `StepNote`, and `VarChange` data structures
- [X] T002 [P] Implement `crates/kb-core/src/runbook/run/fs.rs` for atomic read/write of `.kb/runs/<run-id>.json` via tempfile + rename, directory initialization, and `.gitignore` setup
- [X] T003 Implement advisory locking in `crates/kb-core/src/runbook/run/fs.rs` via `.kb/runs/<run-id>.lock` with PID, heartbeat timestamp, and stale lock reclamation

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Variable resolution hierarchy, state machine transitions, and configuration

- [X] T004 Implement multi-tier variable resolution (run variables -> step bindings -> step-only variables) in `crates/kb-core/src/runbook/run/resolve.rs`
- [X] T005 [P] Implement run state machine transitions and lifecycle helpers (`new_run`, `mark_step`, `reset_step`, `finish_run`, `abandon_run`) in `crates/kb-core/src/runbook/run/mod.rs`
- [X] T006 [P] Add run mode configuration settings `runbook.auto_done` and `runbook.order_warnings` in `crates/kb-core/src/config.rs`
- [X] T007 Register `Context::Run` and run mode commands (`step-action`, `copy-step`, `mark-done`, `mark-skipped`, `reset-step`, `add-run-note`, `edit-run-vars`, `finish-run`, `abandon-run`) in `crates/kb-core/src/keys/` and `crates/kb/src/tui/keys.rs`

**Checkpoint**: Core domain logic and persistence ready in `kb-core`.

---

## Phase 3: User Story 1 - Start a run and work through the steps (Priority: P1) 🎯 MVP

**Goal**: Carlos starts a run of a runbook, answers variables once, and steps through primary actions and manual marking in the TUI.

**Independent Test**: PTY test executes a runbook with cw and cmd steps; verifies initial variable prompt, step rendering, Enter executing primary actions, and `d` marking steps done.

### Tests for User Story 1 ⚠️

- [X] T008 [P] [US1] Integration tests in `crates/kb/tests/tui_run_mode_pty.rs` verifying run launch from list (`open`), variable pre-fill prompt, run screen header/step rendering, Enter primary action dispatch, and `d`/`s`/`u` status changes

### Implementation for User Story 1

- [X] T009 [US1] Implement TUI run screen view in `crates/kb/src/tui/run_view.rs` (left pane with step numbers, status icons, type badges, titles; right pane with resolved payload, Markdown notes, and bindings)
- [X] T010 [US1] Implement primary action dispatcher (`step-action`) in `crates/kb/src/tui/app.rs` for `sumo`/`cw` (open console), `link` (open URL), `code` (code view modal), `note` (full preview), `sql`/`prompt` (copy payload), and manual `do` steps
- [X] T011 [US1] Wire status transitions (`mark-done`, `mark-skipped`, `reset-step`, `finish-run`) in `crates/kb/src/tui/app.rs` with automatic cursor advancement to the next pending step
- [X] T012 [US1] Connect list view `open` command (Ctrl-O / `o`) to start run flow in `crates/kb/src/tui/app.rs`

**Checkpoint**: User Story 1 functional and independently testable in the TUI.

---

## Phase 4: User Story 2 - Commands and confirmations stay safe (Priority: P1)

**Goal**: Executing `cmd` steps suspends the TUI safely, runs the command with M7 confirmations and secret environment injection, auto-marks done on exit 0, and handles `confirm: true` and interruption.

**Independent Test**: PTY test verifies command suspension, destructive pattern confirmation, exit 0 auto-done, non-zero exit marking failed, and Ctrl-C marking interrupted.

### Tests for User Story 2 ⚠️

- [X] T013 [P] [US2] Integration tests in `crates/kb/tests/tui_run_mode_pty.rs` for `cmd` step execution, M7 confirmation, destructive warning, exit 0 auto-done, exit != 0 failure recording, and `confirm: true` confirmation modal

### Implementation for User Story 2

- [X] T014 [US2] Implement terminal mode suspension in `crates/kb/src/tui/app.rs` (leave alternate screen buffer, disable raw mode, run command in foreground with standard I/O, restore TUI on exit)
- [X] T015 [US2] Implement exit code evaluation, duration tracking, and auto-done behavior in `crates/kb/src/tui/app.rs` (exit 0 triggers auto-done if `runbook.auto_done = true`; non-zero triggers `failed` status)
- [X] T016 [US2] Implement `confirm: true` confirmation dialog requiring fixed `y` confirmation in `crates/kb/src/tui/app.rs`
- [X] T017 [US2] Enforce agent-review stronger confirmation (`run` for commands, `y` for opens) for unreviewed entries or unreviewed runbooks in `crates/kb/src/tui/app.rs`
- [X] T018 [US2] Implement SIGINT / Ctrl-C handling during command execution to mark step status as `interrupted`

**Checkpoint**: User Stories 1 and 2 deliver a safe, functional command-stepping MVP.

---

## Phase 5: User Story 4 - Several runs, resumable any time (Priority: P1)

**Goal**: Multiple runs of a runbook can exist concurrently, resumed seamlessly after closing the terminal, and managed via CLI.

**Independent Test**: Launch two runs of the same runbook with different variables, close and reopen TUI, verify run picker and state restoration; test CLI `runs`, `run`, and `abandon`.

### Tests for User Story 4 ⚠️

- [X] T019 [P] [US4] Integration tests in `crates/kb/tests/cli_runbook_run.rs` for `kb runbook runs` (human & json), `kb runbook run <id>`, `kb runbook abandon`, and stale lock reclamation
- [X] T020 [P] [US4] PTY test in `crates/kb/tests/tui_run_mode_pty.rs` verifying run picker modal when opening a runbook with active runs and read-only banner when run is locked

### Implementation for User Story 4

- [X] T021 [US4] Implement run picker modal in `crates/kb/src/tui/run_picker.rs` listing active runs (name, progress, last activity) with "Start a new run" option
- [X] T022 [US4] Implement CLI commands `kb runbook runs`, `kb runbook run`, and `kb runbook abandon` in `crates/kb/src/commands/runbook.rs`
- [X] T023 [US4] Implement read-only mode in `crates/kb/src/tui/app.rs` when a run is locked by another living process
- [X] T024 [US4] Add active runs indicator (`▶ N active runs`) in main list header and runbook preview in `crates/kb/src/tui/view.rs` and `crates/kb/src/tui/preview.rs`

**Checkpoint**: User Stories 1, 2, and 4 provide complete multi-run persistence and resume capabilities.

---

## Phase 6: User Story 3 - Jump around without losing track (Priority: P2)

**Goal**: Non-linear step navigation with warnings when acting on steps out of order.

**Independent Test**: Jump to step 4 while step 2 is pending; assert warning appears and requires `y` to continue; test disabling via config.

### Tests for User Story 3 ⚠️

- [X] T025 [P] [US3] PTY test in `crates/kb/tests/tui_run_mode_pty.rs` verifying out-of-order warning dialog when earlier required steps are pending, and bypassing warning when `runbook.order_warnings = false`

### Implementation for User Story 3

- [X] T026 [US3] Implement order validation in `crates/kb-core/src/runbook/run/mod.rs` detecting earlier pending/failed non-optional steps
- [X] T027 [US3] Implement out-of-order warning prompt in `crates/kb/src/tui/app.rs` requiring `y` to proceed unless `runbook.order_warnings = false`

---

## Phase 7: User Story 5 - The runbook changed since I started (Priority: P2)

**Goal**: Detect modifications to runbook or referenced entries between sessions, reconcile steps by ID, and flag unreviewed agent changes.

**Independent Test**: Modify runbook on disk (add step, reorder, change entry); resume run; assert banner appears, step statuses remain intact by ID, and agent badge is shown.

### Tests for User Story 5 ⚠️

- [X] T028 [P] [US5] Integration test in `crates/kb/tests/tui_run_mode_pty.rs` verifying runbook drift banner, step reconciliation by ID, and `changed since run start` indicators

### Implementation for User Story 5

- [X] T029 [US5] Implement runbook drift reconciliation in `crates/kb-core/src/runbook/run/mod.rs` (match by step ID, add new steps as pending, mark removed steps as removed)
- [X] T030 [US5] Implement entry change tracking in `crates/kb-core/src/runbook/run/mod.rs` comparing entry content hash against `entry_hash_at_action`
- [X] T031 [US5] Render drift banners, removed step sections, and `changed since run start` markers in `crates/kb/src/tui/run_view.rs`

---

## Phase 8: User Story 6 - Notes and variable changes during a run (Priority: P3)

**Goal**: Add timestamped notes to steps and modify run variables mid-run without altering entry definitions or everyday defaults.

**Independent Test**: Press `n` on step to add note; press `V` to edit run variables; verify notes and updated values persist in run file only.

### Tests for User Story 6 ⚠️

- [X] T032 [P] [US6] Integration test in `crates/kb/tests/tui_run_mode_pty.rs` for adding step notes (`n`), editing run variables (`V`), and prompting for unbound step variables

### Implementation for User Story 6

- [X] T033 [US6] Implement step note input prompt (`add-run-note`, `n`) in `crates/kb/src/tui/app.rs` and render notes in `crates/kb/src/tui/run_view.rs`
- [X] T034 [US6] Implement edit run variables flow (`edit-run-vars`, `V`) in `crates/kb/src/tui/app.rs` recording `VarChange` history
- [X] T035 [US6] Implement prompt for unbound step variables on first action in `crates/kb/src/tui/app.rs`, remembering values strictly in `step_state.step_vars`

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Secret leakage verification, terminal sanitization, performance benchmarks, and documentation

- [X] T036 [P] Secret leak test in `crates/kb/tests/run_secrets_scan.rs` scanning `.kb/runs/` to assert zero secrets in run JSON files across environment, typed, and flag inputs
- [X] T037 [P] Terminal safety check: verify all run text, step titles, notes, and banners pass through `sanitize_terminal` before rendering
- [X] T038 Performance benchmark: verify run state load/save $\le 5$ ms and listing 100 runs $\le 20$ ms in `crates/kb/tests/perf.rs`
- [X] T039 Execute quickstart validation scenarios from `specs/015-runbook-run-mode/quickstart.md`
- [X] T040 Update `README.md` and man pages documenting run mode commands and keybindings

---

## Dependencies & Completion Order

```text
Phase 1 (Setup & Models)
       │
       ▼
Phase 2 (Foundations & Keymaps)
       │
       ├────────────────────────────────┐
       ▼                                ▼
Phase 3 (US1: Core Run MVP)      Phase 5 (US4: Multi-run & CLI)
       │                                │
       ▼                                │
Phase 4 (US2: Safe Commands)            │
       │                                │
       ├────────────────────────────────┘
       │
       ├─────────────────┬─────────────────┐
       ▼                 ▼                 ▼
Phase 6 (US3: Order) Phase 7 (US5: Drift) Phase 8 (US6: Notes & Vars)
       │                 │                 │
       └─────────────────┼─────────────────┘
                         │
                         ▼
             Phase 9 (Polish & Security)
```
