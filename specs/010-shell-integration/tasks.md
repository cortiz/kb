# Tasks: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Ready for Implementation  

---

## Dependencies & Execution Order

```
Phase 1: Setup
    │
    ▼
Phase 2: Foundational (Settings & Shell Dialects)
    │
    ├───────────────────────┬───────────────────────┐
    ▼                       ▼                       ▼
Phase 3: US1 (Picker)   Phase 4: US3 (Secrets)   Phase 5: US2 (Setup/Init)
    │                       │                       │
    └───────────────────────┴───────────────────────┤
                                                    ▼
                                            Phase 6: US4 (Scripted Pick)
                                                    │
                                                    ▼
                                            Phase 7: US5 (Tab Completion)
                                                    │
                                                    ▼
                                            Phase 8: US6 (Save Last Cmd)
                                                    │
                                                    ▼
                                            Phase 9: Polish & Performance
```

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Test fixtures, mock shells, and module scaffolding.

- [x] T001 Create shell module scaffolding in `crates/kb-core/src/shell/{mod,dialects,completions,templates}.rs` and export `pub mod shell;` in `crates/kb-core/src/lib.rs`.
- [x] T002 [P] Create CLI command scaffolds in `crates/kb/src/commands/{pick,shell,complete}.rs`.
- [x] T003 [P] Create TUI inline picker scaffolds in `crates/kb/src/tui/{inline_picker,inline_form}.rs`.
- [x] T004 [P] Create pseudo-terminal integration test harness in `crates/kb/tests/shell_integration_pty.rs`.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Configuration schema and shell dialect abstractions required by all user stories.

- [x] T005 [P] Write failing tests for `[widget]` configuration additions in `crates/kb-core/src/config.rs`: verify `widget.key` (`\ek`), `widget.save_key` (`\eK`), `widget.default_type` (`cmd`), `widget.buffer` (`query`), `widget.height` (`40%`), `widget.preview` (`true`).
- [x] T006 Implement configuration schema additions for `[widget]` in `crates/kb-core/src/config.rs`. Makes T005 pass.
- [x] T007 [P] Write failing tests for `ShellDialect` syntax, quoting, and environment variable reference rules (`"$VAR"` in zsh, bash, fish) in `crates/kb-core/src/shell/dialects.rs`.
- [x] T008 Implement `ShellDialect` enum, syntax rules, and escaping in `crates/kb-core/src/shell/dialects.rs`. Makes T007 pass.

**Checkpoint**: Foundation ready — core dialect and configuration primitives are green.

---

## Phase 3: User Story 1 — Pull a command into the prompt (Priority: P1) 🎯 MVP

**Goal**: Pressing Alt-K opens an inline compact picker below the prompt, filtered to `cmd` entries. Selecting an entry resolves variables and places the command directly on the prompt line without executing it.

**Independent Test**: In CI, run real zsh, bash, and fish sessions in a pseudo-terminal; press the key binding; verify picker appears below prompt, resolves an entry, places it on the command line with cursor at the end, does not execute, and restores prompt lines cleanly.

### Tests for User Story 1

- [x] T009 [P] [US1] Write failing unit tests for normal-screen cursor positioning, viewport height clamping (min 10 lines, max 40%, fallback full-screen if $< 8$), and line-clearing teardown in `crates/kb/src/tui/inline_picker.rs`.
- [x] T010 [P] [US1] Write failing unit tests for inline variable form lifecycle and keyboard events in `crates/kb/src/tui/inline_form.rs`.
- [x] T011 [P] [US1] Write failing PTY integration test in `crates/kb/tests/shell_integration_pty.rs` verifying prompt cursor position preservation, zero auto-execution, and screen restoration after pick.

### Implementation for User Story 1

- [x] T012 [US1] Implement inline compact picker rendering directly on `/dev/tty` without alternate screen buffer in `crates/kb/src/tui/inline_picker.rs`.
- [x] T013 [US1] Implement inline variable input form in `crates/kb/src/tui/inline_form.rs`.
- [x] T014 [US1] Implement `kb pick --widget` handling with JSON IPC protocol output in `crates/kb/src/commands/pick.rs`.
- [x] T015 [US1] Implement `BufferMode` (`Query` vs `Insert`) and cursor offset calculation in `crates/kb/src/commands/pick.rs`. Makes T009-T011 pass.

**Checkpoint**: User Story 1 (MVP) complete — commands can be searched and inserted into prompts cleanly.

---

## Phase 4: User Story 3 — Secrets never land in history (Priority: P1)

**Goal**: Secret variables are substituted on the command line as references to environment variables (`"$VAR"`), preventing plaintext secrets from ever being stored in shell history. Secrets lacking an `env` source block insertion.

**Independent Test**: Resolve command entries with secret variables having `env` sources; verify output contains `$ENV_VAR` references across bash, zsh, and fish syntax; verify secrets lacking an `env` source fail insertion with exit code 1.

### Tests for User Story 3

- [x] T016 [P] [US3] Write failing tests for `ResolutionTarget::ShellWidget(dialect)` in `crates/kb-core/src/varres/mod.rs`: verifying secret variables with `env` attribute resolve to `"$VAR"` references and secrets without `env` return error.

### Implementation for User Story 3

- [x] T017 [US3] Implement `ResolutionTarget::ShellWidget` resolution in `crates/kb-core/src/varres/mod.rs`. Makes T016 pass.
- [x] T018 [US3] Wire secret variable reference resolution and refusal gating (exit code 1 with message) into `crates/kb/src/tui/inline_form.rs` and `crates/kb/src/commands/pick.rs`.

**Checkpoint**: Secrets are never inserted in plain text onto the command line.

---

## Phase 5: User Story 2 — Easy setup in each shell (Priority: P1)

**Goal**: One line in `.zshrc` / `.bashrc` / `config.fish` sets up widgets, keybindings, and completions. `kb shell install`/`uninstall` safely manages marked startup blocks.

**Independent Test**: Run `kb shell init` across zsh, bash 4+, bash 3.2 fallback, and fish; verify syntax validity. Test `kb shell install` and `kb shell uninstall` for idempotency and clean removal.

### Tests for User Story 2

- [x] T019 [P] [US2] Write failing tests for shell init script generation across zsh, bash 4+, bash 3.2 fallback, and fish in `crates/kb-core/src/shell/templates.rs`.
- [x] T020 Implement shell init script generator `generate_shell_init` in `crates/kb-core/src/shell/templates.rs`. Makes T019 pass.
- [x] T021 [P] [US2] Write failing integration tests for `kb shell install` and `kb shell uninstall` in `crates/kb/tests/shell_install.rs`: verifying marked block creation, idempotency, and clean removal.
- [x] T022 [US2] Implement `kb shell init <shell>`, `kb shell install`, and `kb shell uninstall` in `crates/kb/src/commands/shell.rs`. Makes T021 pass.
- [x] T023 [US2] Update `kb doctor` in `crates/kb/src/commands/doctor.rs` to report active shell, widget load status, key binding, and clashing key bindings.

**Checkpoint**: Shell setup is seamless and safe across all supported shells.

---

## Phase 6: User Story 4 — Pick from scripts and other tools (Priority: P2)

**Goal**: `kb pick` can be used standalone in scripts and tmux popups with custom printing modes, heights, and filters.

**Independent Test**: Run `kb pick` with various arguments in piped contexts with `/dev/tty`; verify stdout receives only the picked payload, `--tmux` uses tmux popups when available, and cancellations return exit code 130.

### Tests for User Story 4

- [x] T024 [P] [US4] Write failing CLI tests for `kb pick [query...] [--type] [--tag] [--height] [--no-preview] [--print <mode>]` in `crates/kb/tests/cli_pick.rs`.

### Implementation for User Story 4

- [x] T025 [US4] Implement standalone CLI `kb pick` options, printing modes (`resolved`, `raw`, `id`, `payload`), and query filtering in `crates/kb/src/commands/pick.rs`. Makes T024 pass.
- [x] T026 [US4] Implement `--tmux` popup execution (`tmux display-popup -E`) in `crates/kb/src/commands/pick.rs`.

**Checkpoint**: `kb pick` composes cleanly in shell scripts and tmux bindings.

---

## Phase 7: User Story 5 — Tab completion knows the KB (Priority: P2)

**Goal**: Tab completion offers entry IDs with titles, tags, types, and variable names in $\le 50$ ms without rebuilding the index.

**Independent Test**: Benchmark `kb __complete` queries across 10,000 entries; verify results return in $\le 50$ ms; verify static and dynamic completions for all shells.

### Tests for User Story 5

- [x] T027 [P] [US5] Write failing tests for fast SQLite index queries in `crates/kb-core/src/shell/completions.rs`: `complete_entries` (<50ms), `complete_tags`, `complete_types`, `complete_vars`, `complete_var_values`.

### Implementation for User Story 5

- [x] T028 [US5] Implement completion query helpers in `crates/kb-core/src/shell/completions.rs`. Makes T027 pass.
- [x] T029 [US5] Implement `kb __complete` hidden CLI subcommand in `crates/kb/src/commands/complete.rs`.
- [x] T030 [US5] Implement `kb completions <shell>` generation using `clap_complete` and custom dynamic completion hooks in `crates/kb/src/commands/shell.rs`.

**Checkpoint**: Rich shell tab completions are fast and accurate.

---

## Phase 8: User Story 6 — Save the command I just ran (Priority: P3)

**Goal**: Pressing Alt-Shift-K (or running `kb-save-last`) reads the previous command from shell history and opens `kb add --type cmd` pre-filled.

**Independent Test**: Test `kb-save-last` across shells with dummy shell history; verify last command extraction into `kb add --type cmd` and secret detection gating.

### Tests for User Story 6

- [x] T031 [P] [US6] Write failing tests for `kb-save-last` function definition in shell templates in `crates/kb-core/src/shell/templates.rs`.

### Implementation for User Story 6

- [x] T032 [US6] Implement `kb-save-last` shell hook (using `fc -ln -1` in zsh/bash and `history --max=1` in fish) piping previous command to `kb add --type cmd --stdin` in `crates/kb-core/src/shell/templates.rs`. Makes T031 pass.

**Checkpoint**: Operational snippets can be captured directly from shell history.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Performance validation, terminal edge cases, and documentation.

- [x] T033 [P] Add benchmark tests for picker startup ($\le 80$ms) and dynamic completion latency ($\le 50$ms) in `tests/perf.rs`.
- [x] T034 [P] Validate edge cases across Starship/Powerlevel10k prompts, Vi editing mode, and small terminals in `crates/kb/tests/shell_integration_pty.rs`.
- [x] T035 Update `README.md` and user documentation with shell integration instructions, key bindings, and configuration guide.
- [x] T036 Run end-to-end quickstart validation scenarios from `specs/010-shell-integration/quickstart.md`.
