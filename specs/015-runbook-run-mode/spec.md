# Feature Specification: Runbook Run Mode

**Feature Branch**: `015-runbook-run-mode`
**Created**: 2026-10-15
**Status**: Draft
**Milestone**: R2 (second runbook milestone; run records and CLI stepping are R3 / spec 016)
**Depends on**: R1 (`011-runbook-format`), M3 (`003-tui-core`), M4 (`004-platform-clipboard`), M5 (`006-variables`), M7 (`008-actions-code-links`), MCP review flow (`012-mcp-server`), keymap (`013-vim-keymap`), notes (`014-markdown-notes`)
**Input**: Let the user **execute** a runbook from the TUI: answer its variables once, act on each step in place (run the command, open the console, copy the query), mark steps done, skipped or failed, respect confirmation steps, keep several runs going at once, and resume any of them after closing the terminal. Run state is local to the machine.

## Clarifications

### Session 2026-10-15

- Q: Where is run state stored? → A: Locally only, in `.kb/runs/` (git-ignored, per machine). Runs can't be resumed on another machine.
- Q: What does Enter do on a step? → A: The step's primary action (run a command with the M7 confirmation, open a console or link, open the code view, show a note); `y` copies instead.
- Q: Must steps be done in order? → A: No. Any step can be acted on, done or skipped; kb warns when the user moves past an unfinished required or confirm step.
- Q: How many active runs per runbook? → A: Several (for example stage and prod deploys at the same time). Each run is named by its variable values and start time.
- Q: How should process locking (FR-020) for active runs be implemented? → A: Advisory file locking with a sidecar `.kb/runs/<run-id>.lock` containing PID and timestamp, automatically cleared on process exit or when the PID is dead.
- Q: When a running command step is cancelled via Ctrl-C (SIGINT), how should its step status be recorded? → A: Mark the step as `interrupted` if cancelled by Ctrl-C/SIGINT, preserving it for retry without counting as a normal exit failure.
- Q: When resuming a run that requires secret variables, how should secrets be acquired? → A: Read from environment variables if set; only open the variable input prompt if an active secret variable has no value in the environment.

## Overview

R1 made runbooks writable, searchable and shareable as checklists. R2 makes them **runnable**. Opening a runbook starts (or resumes) a run: kb asks for the runbook's variables once, then shows the steps on the left and the current step on the right, with its query or command already resolved. One key does the step's natural action; another marks it done. Progress is saved after every change, so closing the terminal mid-incident loses nothing.

Everything a step does goes through existing paths: M5 resolves variables, M7 runs commands (with its confirmation, destructive-pattern and secret rules) and opens consoles, spec 012's review rules add the stronger confirmation for agent-changed entries, and spec 013's keymap makes every run-mode key configurable. Run mode adds sequencing and memory on top, not a second way to execute things.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Start a run and work through the steps (Priority: P1)

Carlos selects "Deploy authorizer to prod" and presses Ctrl-O (`o` in vim normal mode). A form asks for `env` and `version` once. The run screen opens: four steps on the left, the first one selected and its Sumo query resolved on the right. He presses Enter: Sumo opens with the query and a 30-minute range. He presses `d` to mark it done; the selection moves to the deploy step.

**Why this priority**: this is the milestone: from a checklist to something executed.

**Independent Test**: PTY tests against a fixture KB with runbooks covering every step kind; send keys; assert on the run screen snapshot, the opener and runner test doubles (URLs, commands, working directories), and the saved run file after each action.

**Acceptance Scenarios**:

1. **Given** a runbook is selected in the list, **When** the user runs the `open` command (Ctrl-O / `o`), **Then** kb starts a new run (or offers to resume one, User Story 4), asks for the runbook variables with the M5 form, and opens the run screen.
2. **Given** the run screen, **Then** it shows: a header with the runbook title, the run name (variable values and start time), progress (`2/4 done · 1 skipped`), and elapsed time; on the left, the numbered steps with their status icon, type badge and title; on the right, the selected step: its title, note (Markdown, spec 014), and either the referenced entry resolved with the run's variables and step bindings, or the manual instruction.
3. **Given** a step is selected, **When** the user presses Enter, **Then** its primary action runs (FR-013).
4. **Given** a step, **When** the user presses `d`, **Then** it is marked done (with confirmation for `confirm` steps, User Story 2), the time is recorded, and the selection moves to the next step that is not done or skipped.
5. **Given** a step, **When** the user presses `s`, **Then** it is marked skipped, and `u` returns it to pending.
6. **Given** every non-optional step is done or skipped, **Then** the header shows `complete`, and `F` (finish) closes the run, keeping it as finished for R3.

---

### User Story 2 — Commands and confirmations stay safe (Priority: P1)

Step 2 deploys to prod. Carlos presses Enter: kb shows the resolved command with M7's confirmation. He confirms; the TUI steps aside, the deploy runs in his terminal, exits 0, and the step is marked done automatically. Later, an unrelated step fails with exit code 1 and is marked failed.

**Why this priority**: run mode makes running commands faster; it must not make it less careful.

**Acceptance Scenarios**:

1. **Given** a `cmd` step, **When** the user presses Enter, **Then** kb runs it exactly through M7: resolved command, shell and directory shown, confirmation required, destructive-pattern rules, secrets passed through the environment (M7 FR-021).
2. **Given** the command exits 0, **Then** the step is marked done automatically (config `runbook.auto_done = true`, default). **Given** a non-zero exit, **Then** the step is marked failed with the exit code, and stays selected.
3. **Given** a `confirm: true` step, **When** the user marks it done (by `d`, or by an automatic exit-0 success), **Then** kb asks "Confirm '<title>' is complete?" and needs `y`. The answer is fixed and can't be rebound (spec 013 FR-024).
4. **Given** a step whose entry is unreviewed (spec 012), or a run of an unreviewed runbook, **Then** acting on it requires the stronger confirmation (typing `run` for commands, `y` for opens), as everywhere else in kb.
5. **Given** a command step is running when kb is killed or the terminal closes, **Then** on resume the step shows `interrupted`, not done, and the user decides what to do.

---

### User Story 3 — Jump around without losing track (Priority: P2)

During an incident Carlos jumps straight to step 4 (roll back). kb warns that step 2 (confirm) isn't done yet and asks whether to continue.

**Acceptance Scenarios**:

1. **Given** the user acts on or marks done a step while an earlier non-optional step is still pending or failed, **Then** kb shows a one-line warning naming the earliest such step and asks to continue (`y`).
2. **Given** `runbook.order_warnings = false`, **Then** no warning is shown.
3. **Given** optional steps, **Then** they never trigger the warning and don't block `complete`.

---

### User Story 4 — Several runs, resumable any time (Priority: P1)

Carlos has a stage deploy and a prod deploy of the same runbook in progress. He closes his terminal, reopens kb the next morning, opens the runbook, and picks the prod run from a list; every step is where he left it.

**Why this priority**: incidents and deploys outlast terminal sessions, and parallel environments are common.

**Acceptance Scenarios**:

1. **Given** a runbook has active runs, **When** the user opens it, **Then** kb lists them (name, progress, last activity) with "Start a new run" as the last option.
2. **Given** a resumed run, **Then** step statuses, timestamps, notes, non-secret variable values and failed exit codes are exactly as they were. Secret variables are taken from their environment source or asked again (FR-022).
3. **Given** active runs exist anywhere in the KB, **Then** the main list header shows `▶ N active runs`, and the runbook's preview lists its active runs.
4. **Given** `kb runbook runs`, **Then** kb lists active runs (and with `--all`, finished and abandoned ones), with `--json`. `kb runbook run <id|query>` opens the TUI on that runbook's run picker. `kb runbook abandon <run-id>` marks a run abandoned.
5. **Given** two kb processes open the same run, **Then** the second one opens it read-only and says so, until the first one closes it.

---

### User Story 5 — The runbook changed since I started (Priority: P2)

Carlos started a run yesterday. Overnight an agent added a step and edited a query a step uses. On resume, kb shows what changed and flags the edited entry as unreviewed.

**Acceptance Scenarios**:

1. **Given** the runbook file changed since the run started, **Then** a banner says so, and steps are matched by step id (R1): existing steps keep their status, new steps appear as pending, and removed steps are listed in a collapsed "removed since start" section with their recorded status.
2. **Given** steps without explicit ids changed title (so their derived id changed), **Then** they are treated as removed plus new, and the banner suggests adding explicit ids (R1 lint info).
3. **Given** a referenced entry changed since the step was last acted on, **Then** the step shows `changed since run start`, and if the change was made by an agent and not yet reviewed, the `agent` badge and the stronger confirmation apply.
4. **Given** the runbook variables changed (a variable added or removed), **Then** kb asks only for the new variables.

---

### User Story 6 — Notes and variable changes during a run (Priority: P3)

**Acceptance Scenarios**:

1. **Given** a step, **When** the user presses `n`, **Then** a one-line input adds a timestamped note to that step in the run (not to the runbook file). Notes appear under the step and are kept for R3 records.
2. **Given** `V` (edit run variables), **Then** the M5 form opens with the run's current values; changes apply to later actions only, and the change is recorded with a timestamp.
3. **Given** a step entry has variables not bound by the runbook (R1 FR-010), **Then** the M5 form asks for them the first time the step is acted on, and their non-secret values are remembered **in this run** (not in the entry's own remembered values, M5 FR-011), so prod values used in a run never become pre-fills for everyday use.

---

### Edge Cases

- **Manual (`do`) steps**: Enter marks them done (with confirmation if `confirm`); there is nothing to run.
- **Note steps** (spec 014): Enter opens the note in full preview; Esc returns to the run.
- **Code steps**: Enter opens the M7 code view; marking done is manual.
- **SQL and prompt steps**: they have no primary open action (M7 FR-002), so Enter copies the resolved text, and the step shows `copied` until marked done.
- **Open and copy actions** never mark a step done automatically; the step shows `opened` or `copied` (in progress) until `d`.
- **Missing or hidden entries**: a step whose entry no longer exists shows `missing` and can only be skipped or marked done manually; its R1 lint error is shown.
- **Over SSH**: opens fall back to printing and copying the URL (M4 FR-010), as everywhere else.
- **Very long runs** (days): elapsed time switches to days and hours; nothing expires automatically.
- **Disk full or `.kb/` read-only**: kb shows that progress can't be saved and keeps the run in memory, warning before exit.
- **Run state file corrupted**: kb keeps a copy as `.bak`, shows the run as `unreadable`, and offers to abandon it; it never deletes it silently.
- **Runbook deleted** while it has active runs: runs remain listed as `runbook missing` and can be abandoned or exported later (R3).
- **Keys**: the run screen has no text input outside the note prompt, so single-letter keys work in both the emacs and vim keymaps, as in the code view.

## Requirements *(mandatory)*

### Runs

- **FR-001**: A **run** MUST be created from a runbook with a run id (`<runbook-id-slug>-<UTC timestamp>`), a display name (runbook title, the values of up to three non-secret enum or short text variables, and the start time), the runbook's content hash at start, and the answered variables.
- **FR-002**: A runbook MAY have any number of active runs. Runs have a state: `active`, `finished` or `abandoned`.
- **FR-003**: Starting a run MUST ask for runbook variables with the M5 form and pre-fill rules (M5 FR-010), using the runbook's own remembered values.
- **FR-004**: Starting a run of an unreviewed runbook (spec 012 FR-028) MUST require the stronger confirmation before the variable form opens.

### Step state

- **FR-005**: Each step in a run MUST have a status: `pending`, `opened`, `copied`, `running`, `done`, `skipped`, `failed`, `interrupted`, plus `removed` for steps no longer in the runbook (User Story 5). Status changes MUST record a timestamp.
- **FR-006**: Steps MUST be tracked by step id (R1 FR-004), so reordering the runbook never moves statuses to the wrong step.
- **FR-007**: `failed` MUST record the exit code (or signal) and duration of the command (M7 FR-017); kb MUST NOT capture command output (M7 FR-018).
- **FR-008**: A run is **complete** when every non-optional step is `done` or `skipped`. Completing doesn't finish the run; `finish-run` does.

### Ordering

- **FR-009**: Any step MAY be acted on or marked at any time.
- **FR-010**: When the user acts on or marks done a step while an earlier non-optional step is `pending`, `failed` or `interrupted`, kb MUST warn once per step and ask to continue, unless `runbook.order_warnings = false`.

### Confirmations

- **FR-011**: Marking a `confirm: true` step done MUST require `y` to "Confirm '<title>' is complete?", including when done is triggered automatically by a successful command.
- **FR-012**: Every action MUST keep the confirmations of the underlying path: M7 run confirmation and destructive patterns, and spec 012's stronger confirmation for unreviewed entries. Run mode MUST NOT add any way to skip them.

### Step actions

- **FR-013**: The primary action (Enter) MUST be, by the referenced entry's type: `cmd` → run (M7); `sumo`, `cw` → open console (M7); `link` → open; `code` → code view (M7); `note` → full preview; `sql`, `prompt` → copy resolved text; manual `do` step → mark done.
- **FR-014**: `copy-step` (`y`) MUST copy the resolved payload (or URL, or `path:line`) of any entry step, through the M4 clipboard.
- **FR-015**: Actions MUST resolve variables with: run variables (runbook variables), then step bindings (R1 FR-008), then step-only variables answered in this run (User Story 6, scenario 3), using the R1 binding table (R1 FR-011).
- **FR-016**: After a `cmd` step exits 0, the step MUST be marked done when `runbook.auto_done = true` (default), subject to FR-011; non-zero exits MUST mark it failed.
- **FR-017**: Every action MUST record a use of the step entry and of the runbook (M2 FR-017).

### Persistence

- **FR-018**: Run state MUST be stored locally as one file per run in `.kb/runs/<run-id>.json` (git-ignored, never committed, never synced), versioned, and written atomically after every change.
- **FR-019**: A run file MUST contain: run id, runbook id, runbook content hash at start and at last resume, state, display name, start/finish times, non-secret variable values and their change history, step-only variable values, per-step status history, exit codes, durations, and run notes.
- **FR-020**: While a process has a run open for writing, it MUST hold a lock; other processes MUST open that run read-only (User Story 4, scenario 5). Stale locks from dead processes MUST be detected and released.
- **FR-021**: A step in `running` state when the run is reopened without a live process MUST become `interrupted`.
- **FR-022**: Secret values MUST NEVER be written to run files (constitution VI). On resume, secrets come from their `env` source or are asked again.
- **FR-023**: Finished and abandoned runs MUST be kept (for R3) until removed by `kb runbook runs --prune [--older-than <days>]`; nothing is deleted automatically in this milestone.

### Runbook changes

- **FR-024**: On resume, kb MUST compare the runbook's current content hash with the run's last-seen hash and apply User Story 5: match by step id, add new steps as pending, keep removed steps with their history under `removed since start`, and ask only for new variables.
- **FR-025**: For each entry step, kb MUST record the entry's content hash when the step was last acted on, and show `changed since run start` when it differs.

### Run screen (TUI)

- **FR-026**: The run screen MUST show the layout in User Story 1, scenario 2, with status icons readable without color (`○` pending, `◐` opened/copied, `▶` running, `●` done, `⊘` skipped, `✕` failed, `!` interrupted), the `agent` badge for unreviewed entries, and `changed since run start` markers.
- **FR-027**: The right pane MUST reuse existing renderers: entry previews (M3), resolved payloads with substitutions highlighted (M5), Markdown for notes and step text (spec 014), with all text sanitized (spec 014 FR-019, FR-020).
- **FR-028**: New keymap context `run` (spec 013 FR-019) with these commands and defaults (same in both keymaps, since the run screen has no text input):

  | Command | Default |
  |---|---|
  | `step-action` | Enter |
  | `copy-step` | `y` |
  | `mark-done` | `d` |
  | `mark-skipped` | `s` |
  | `reset-step` | `u` |
  | `add-run-note` | `n` |
  | `edit-run-vars` | `V` |
  | `finish-run` | `F` |
  | `abandon-run` | `A` |
  | `down` / `up` | ↓ `j` / ↑ `k` |
  | `scroll-preview-down` / `-up` | Shift-↓ `J` / Shift-↑ `K` |
  | `help` | `?` |
  | `close` | Esc (back to the list; the run stays active) |

  All MUST be configurable, appear in `kb keys`, help and the key bar (spec 013 FR-027, FR-028), and pass startup validation.
- **FR-029**: `abandon-run` and `finish-run` before completion MUST ask for confirmation (`y`).
- **FR-030**: Opening a runbook (the `open` command) MUST start or resume a run (User Story 4, scenario 1). Enter (`pick`) on a runbook in the list keeps copying its checklist (R1 FR-033).
- **FR-031**: The main list header MUST show the number of active runs when there are any, and runbook previews MUST list their active runs.

### CLI

- **FR-032**: `kb runbook run <id|query> [--new]` opens the TUI on that runbook (run picker, or a new run with `--new`); `kb runbook runs [<runbook>] [--all] [--json] [--prune [--older-than <days>]]`; `kb runbook abandon <run-id> [--yes]`. Step-by-step CLI control is R3.
- **FR-033**: Exit codes follow kb's conventions: `0` success, `1` not found or refused, `2` usage or I/O errors, `130` cancelled.

### Key Entities

- **Run**: id, runbook id, display name, state, variables, timestamps, runbook hashes, step states, notes.
- **StepState**: step id, status history, exit codes and durations, entry hash at last action, step-only variable values, notes.
- **RunLock**: process id and start time, for read-only detection and stale-lock cleanup.

## Success Criteria *(mandatory)*

- **SC-001**: A four-step deploy runbook can be run end to end from the TUI with one variable form, one Enter and one `d` per step (plus M7's confirmation for the command), and no typing of queries or commands.
- **SC-002**: Killing kb at any point during a run (including mid-command) and reopening it restores every step's status exactly, with interrupted commands shown as `interrupted`.
- **SC-003**: No test path writes a secret value into `.kb/runs/`, verified by scanning the folder after runs that use secrets from the environment, typed input and `--var`.
- **SC-004**: In a fixture where an agent edits a step's command between start and resume, acting on that step always requires the stronger confirmation until the change is reviewed.
- **SC-005**: Two concurrent runs of the same runbook with different variables never mix statuses or values.
- **SC-006**: Reordering, adding and removing steps (with explicit ids) between sessions never moves a status to the wrong step.
- **SC-007**: Every run-mode key can be rebound in config and works in both keymaps.

## Out of Scope

- Run records, timelines and Markdown export; CLI stepping (`kb runbook start | next | status`) — R3, spec 016.
- Syncing runs between machines or sharing them with others.
- Automated checks, polling, waiting and conditions — R4.
- Running steps in parallel or in the background; commands run one at a time in the foreground (M7).
- Capturing command output.
- Running runbooks through MCP; agents can still read runbooks and resolve steps (R1).

## Assumptions

- Local-only run state is acceptable because a run is usually carried out by one person on one machine; R3's export covers sharing.
- Auto-marking successful commands as done matches how deploy steps are used; users who disagree can turn it off.
- Free order with warnings fits incident work better than strict order, where steps are often skipped or done out of sequence.

## Constitution Check notes

- **I. Plain files / II. Disposable state**: run files live in `.kb/`, next to usage and remembered values; deleting them loses run progress only, never entries.
- **III. Local-only**: no network beyond what step actions already do (opening URLs the user chose).
- **IV. One core**: run state, ordering rules and resolution live in `kb-core`; the TUI and CLI only drive them.
- **VI. Secrets**: secrets never reach run files (FR-022); commands keep M7's environment-variable handling.
- **VII. No surprises**: every step action keeps its existing confirmation; run mode adds confirmation for `confirm` steps and adds none that can be bypassed.
- **IX. Simplicity**: no syncing, no automation, no parallel execution in this milestone.
