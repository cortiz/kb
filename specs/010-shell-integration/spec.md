# Feature Specification: Shell Integration

**Feature Branch**: `010-shell-integration`  
**Created**: 2026-10-10  
**Status**: Draft  
**Milestone**: M8 (spec number 010)  
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M4 (`004-platform-clipboard`), M5 (`006-variables`), M7 (`008-actions-code-links`). Independent of M6 and M9.  
**Input**: Bring kb into the shell prompt: a key binding in zsh, bash and fish that opens a compact picker under the prompt and puts the resolved command on the command line, ready to edit and run; plus `kb pick` for scripts, shell completions with entry IDs and tags, and a quick way to save the command just run.

## Overview

The full-screen TUI is the right place to browse and manage the KB. For commands, though, the natural destination is the command line itself: find "Tail authorizer logs", fill `env=prod`, and see the exact command in the prompt, where it can be tweaked and run with Enter like anything the user typed. This replaces the launcher the desktop app would have offered, without leaving the terminal.

Three principles shape this milestone:

1. **Insert, never execute.** The widget only edits the command line. The user always presses Enter themselves, so the command shows up and runs exactly as if they had typed it.
2. **Secrets stay out of shell history.** A resolved command placed on the command line ends up in history once run, so secrets are inserted as references to environment variables, never as values.
3. **One engine.** The picker uses the same search (M2), variable resolution (M5) and action model (M7) as the TUI and CLI.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Pull a command into the prompt (Priority: P1)

At a zsh prompt, Carlos presses Alt-K. A compact picker opens below the prompt, already filtered to commands. He types `tail auth`, presses Enter, fills `env=prod` in the inline form, and the prompt now reads `aws logs tail /aws/lambda/authorizer-prod --follow`, with the cursor at the end. He adds `--since 5m` and presses Enter.

**Why this priority**: this is the shell-native way to use command entries, and the primary purpose of the milestone.

**Independent Test**: in CI, start real zsh, bash and fish sessions in a pseudo-terminal with the widget loaded and a fixture KB; send the key binding, keystrokes and Enter; check the command line contents, the cursor position, that nothing was executed, and that the screen is restored.

**Acceptance Scenarios**:

1. **Given** the widget is loaded and the command line is empty, **When** the user presses the widget key, **Then** the picker opens below the prompt, using at most the configured height (default 40% of the terminal, at least 10 lines), filtered to `cmd` entries.
2. **Given** the user picks an entry without variables, **Then** the picker closes, its lines are cleared, the prompt is redrawn, and the resolved command is on the command line with the cursor at its end. Nothing is executed.
3. **Given** the entry has variables, **Then** an inline variable form (FR-013) appears in place of the list; Enter on the form inserts the resolved command.
4. **Given** the user presses Esc or Ctrl-C in the picker, **Then** the picker closes and the command line is exactly as it was, cursor included.
5. **Given** the command line already has text, **Then** that text becomes the initial query, and picking an entry replaces it (setting `widget.buffer = "insert"` instead inserts the result at the cursor and starts with an empty query).
6. **Given** a use is inserted, **Then** a use is recorded and non-secret variable values are remembered, as in M5.

---

### User Story 2 — Easy setup in each shell (Priority: P1)

Carlos adds one line to his `.zshrc` and the widget, completions and helper functions are all set up.

**Why this priority**: a shell integration that is fiddly to install doesn't get used.

**Independent Test**: run `kb shell init` and `kb shell install`/`uninstall` across zsh, bash, and fish configurations; verify syntax validity, idempotent installation block modification, clean uninstall, and doctor diagnostics.

**Acceptance Scenarios**:

1. **Given** `kb shell init zsh` (or `bash`, `fish`), **Then** kb prints shell code that defines the widget, binds the key, and loads completions, meant to be used as `eval "$(kb shell init zsh)"` (or `kb shell init fish | source`).
2. **Given** `kb shell install`, **Then** kb detects the current shell, shows the exact line it will add and the file it will add it to, and asks for confirmation. On confirmation it appends the line inside a marked block. Running it again changes nothing.
3. **Given** `kb shell uninstall`, **Then** kb removes only its marked block.
4. **Given** the key is set with `KB_WIDGET_KEY` before the init line, or with `widget.key` in config, **Then** that key is bound instead of the default.
5. **Given** `kb doctor` (M4), **Then** it also reports which shell is running, whether the widget is loaded in it, which key is bound, and whether that key is already used by something else.

---

### User Story 3 — Secrets never land in history (Priority: P1)

An entry needs `token` (`kind: secret`, `env: SUMO_TOKEN`). When inserted into the prompt, the command reads `curl -H "Authorization: Bearer $SUMO_TOKEN" …`, never the token itself.

**Why this priority**: constitution principle VI. Shell history files are persistent, often synced, and readable by any tool the user runs.

**Independent Test**: verify variable resolution in widget mode replaces `secret` variables having `env` sources with `$ENV_VAR` references across bash, zsh, and fish syntax, and blocks insertion with code 1 if a secret variable lacks an `env` source.

**Acceptance Scenarios**:

1. **Given** a `secret` variable with an `env` source, **When** the command is inserted, **Then** the secret is replaced by a reference to that environment variable in the current shell's syntax (`"$SUMO_TOKEN"` in bash and zsh, `"$SUMO_TOKEN"` in fish), and the inline form shows it as `from $SUMO_TOKEN` without asking for a value.
2. **Given** the environment variable is not set in the shell, **Then** the form warns that the command will fail unless it is set, and still inserts the reference.
3. **Given** a `secret` variable with no `env` source, **Then** the picker refuses to insert, explains why, and offers to run the entry with `kb run` instead (M7 FR-021), which keeps the secret out of the command line.

---

### User Story 4 — Pick from scripts and other tools (Priority: P2)

Carlos uses `kb pick` in his own scripts and tmux bindings: `query=$(kb pick --type sumo)`.

**Why this priority**: enables unix composition, tmux popups, and non-widget script workflows.

**Independent Test**: run `kb pick` with various arguments in scripted/piped contexts with `/dev/tty` interaction; verify stdout receives only the picked payload, `--tmux` uses tmux popups when available, and cancellations return exit code 130.

**Acceptance Scenarios**:

1. **Given** `kb pick [query…]`, **Then** the compact picker opens on the terminal, and on pick the resolved text (per M3's `--print` modes) is written to stdout and kb exits 0. Esc exits 130 with no output.
2. **Given** `--type`, `--tag` and the M2 query syntax, **Then** results are filtered the same way as `kb search`.
3. **Given** `--height <lines|percent>`, `--no-preview` and `--print <mode>`, **Then** they control the picker's size, preview and output.
4. **Given** `--tmux`, **When** running inside tmux 3.2 or later, **Then** the picker opens in a tmux popup over the current pane instead of under the prompt.
5. **Given** stdout is redirected and stdin is not a terminal, **Then** the picker still draws on and reads from the controlling terminal (`/dev/tty`).

---

### User Story 5 — Tab completion knows the KB (Priority: P2)

**Why this priority**: tab completion saves typing and prevents syntax errors when invoking `kb` subcommands.

**Independent Test**: execute `kb __complete <kind> [prefix]` and generated completion scripts for entry IDs, tags, types, and variables; verify latency budget (<50ms) and accuracy.

**Acceptance Scenarios**:

1. **Given** completions are loaded, **When** the user types `kb copy 5x<Tab>`, **Then** matching entry IDs are offered, with titles as descriptions where the shell supports them (zsh, fish).
2. **Given** `kb search tag:<Tab>` or `--tag <Tab>`, **Then** known tags are offered; `type:<Tab>` offers the seven types.
3. **Given** `kb use <id> --var <Tab>`, **Then** the entry's variable names are offered as `name=`, and for enum variables, `name=<Tab>` offers its options.
4. **Given** subcommands and flags, **Then** they complete as generated from the CLI definition.
5. **Given** `kb completions <shell>`, **Then** the completion script alone is printed, for users who manage completions separately.

---

### User Story 6 — Save the command I just ran (Priority: P3)

Carlos runs a long `aws` command that finally works. He presses Alt-Shift-K (or runs `kb-save-last`), and the add form (M6) opens with that command as the body and type `cmd`.

**Why this priority**: captures operational knowledge in the moment with zero copy-pasting friction.

**Independent Test**: test `kb-save-last` across shells with dummy shell history; verify last command extraction into `kb add --type cmd` and secret detection gating.

**Acceptance Scenarios**:

1. **Given** the save-last binding or function, **Then** kb takes the previous command from the shell's history and opens `kb add --type cmd` with it as the body.
2. **Given** the previous command contains a value that looks like a secret, **Then** M6's secret checks apply, and the form highlights it before anything can be saved.
3. **Given** the shell is configured not to record history, **Then** kb explains it has nothing to save.

---

### Edge Cases

- **Prompt at the bottom of the screen**: the picker scrolls the terminal up just enough to fit, and restores the prompt in place afterwards.
- **Multi-line prompts and right prompts** (Powerlevel10k, Starship): the shell redraws the prompt after the widget returns, so it stays correct.
- **Multi-line commands**: inserted as-is; in zsh and fish the command line becomes multi-line; in bash, newlines are inserted as typed continuation lines.
- **Command written for another shell**: if the entry's `shell` (M1) differs from the current shell, the picker shows a warning such as `written for fish` next to the entry, and inserts it unchanged.
- **Entries other than `cmd`**: with the type filter cleared (Tab), any entry can be picked; its pick output (M3) is inserted. For `sql`, `sumo`, `cw` and `prompt`, inserting into a command line is rarely what the user wants, so the picker shows a hint to use Ctrl-Y (copy) instead, which copies and leaves the command line unchanged.
- **bash 3.2** (the macOS default): `bind -x` cannot edit the command line there. The init script detects this and falls back to adding the resolved command to history, so pressing ↑ brings it up, and tells the user once to install a newer bash for direct insertion.
- **Vi mode** in zsh and bash: the widget is bound in both insert and command keymaps; the shell returns to insert mode after insertion.
- **Key already bound**: `kb shell init` does not override an existing binding for the chosen key unless `KB_WIDGET_FORCE=1` is set, and `kb doctor` reports the clash.
- **SSH and tmux**: the picker works the same; nothing in it depends on the local platform. The tmux popup needs tmux 3.2+.
- **Very small terminals**: below 8 available lines, the picker uses the full screen (alternate screen) instead of inline mode.
- **KB not found or empty**: the widget prints a one-line message below the prompt and leaves the command line unchanged.
- **Slow first run** (index rebuild): the picker opens immediately with a "refreshing" indicator, as in M3 FR-003.

## Requirements *(mandatory)*

### Compact picker

- **FR-001**: kb MUST provide an inline picker mode that draws in the normal screen below the cursor (not the alternate screen), uses at most the configured height, and on exit clears exactly the lines it used and returns the cursor to where it was.
- **FR-002**: The picker MUST reuse M3's search line, result list, type chips and matching highlights, in a compact layout: one line per result (type badge, title, tags), and an optional preview on the right when the terminal is at least 120 columns wide (toggle with Alt-P).
- **FR-003**: The picker MUST read keys from and draw to the controlling terminal (`/dev/tty`), so it works when stdout is captured by a shell widget or a pipe.
- **FR-004**: Keys in the picker follow M3 where they overlap: typing, ↑/↓, Ctrl-P/Ctrl-N, Tab/Shift-Tab (type filter), Enter (pick), Ctrl-Y (copy and close without inserting), Alt-P (preview), Esc and Ctrl-C (cancel).
- **FR-005**: The first frame MUST appear within 80 ms of the key press on a KB of 10,000 entries, and typing MUST meet M3's keystroke-to-results budget.

### Widgets

- **FR-006**: `kb shell init <zsh|bash|fish>` MUST print code that:
  - defines a widget function that runs `kb pick` in widget mode with the current command line and cursor position;
  - puts the result on the command line (FR-007) and asks the shell to redraw the prompt;
  - binds it to the configured key (default Alt-K) in every relevant keymap, unless already bound (see Edge Cases);
  - binds the save-last function (default Alt-Shift-K);
  - loads completions.
- **FR-007**: Insertion rules:
  - command line empty: insert the result, cursor at its end;
  - command line not empty and `widget.buffer = "query"` (default): use the command line as the initial query, replace it with the result;
  - `widget.buffer = "insert"`: start with an empty query, insert the result at the cursor.
- **FR-008**: The widget MUST NEVER execute the command, accept the line, or press Enter on the user's behalf.
- **FR-009**: Supported shells and versions: zsh 5.0+ (ZLE widget); bash 4.0+ (`bind -x` with `READLINE_LINE` and `READLINE_POINT`), with the bash 3.2 history fallback described in Edge Cases; fish 3.0+ (`bind` with `commandline`).
- **FR-010**: Widget mode MUST default the type filter to `cmd` (`widget.default_type`, which also accepts `all`), and MUST start with the command line text as the query when FR-007 says so.
- **FR-011**: `kb shell install`/`uninstall` MUST edit only the user's shell startup file for the detected shell (`~/.zshrc`, `~/.bashrc` or `~/.bash_profile` on macOS, `~/.config/fish/config.fish`), only after confirmation, using a block delimited by `# >>> kb >>>` and `# <<< kb <<<` markers. It MUST be idempotent and MUST NOT touch anything outside the block.

### Variables and secrets

- **FR-012**: Picking an entry with variables MUST use the M5 resolution engine and pre-fill rules (M5 FR-010), including remembering values on insertion (M5 FR-011).
- **FR-013**: The inline variable form MUST show one variable per line (name, kind, value) and a one-line preview of the resolved command, within the picker's height, with the same keys as the M5 form (Tab, ←/→ for enums, Enter, Esc).
- **FR-014**: When the target is the command line (widget mode), each `secret` variable MUST be resolved to a reference to its `env` variable in the current shell's syntax, quoted so it stays one argument. A secret without an `env` source MUST block insertion (User Story 3, scenario 3).
- **FR-015**: Values substituted into `cmd` payloads MUST be quoted for the **entry's** shell (M5 FR-008), and the picker MUST warn when that differs from the current shell (Edge Cases).

### Completions

- **FR-016**: Static completions for subcommands and flags MUST be generated from the CLI definition for zsh, bash and fish, and printed by `kb completions <shell>`.
- **FR-017**: Dynamic completions MUST be served by a hidden `kb __complete <kind> [prefix]` command for: entry IDs (with titles), tags, types, variable names for a given entry, and enum options for a given variable.
- **FR-018**: A dynamic completion call MUST return within 50 ms on a KB of 10,000 entries with a warm index, and MUST never trigger a full index rebuild (it uses the index as-is and may be slightly stale).

### Save last command

- **FR-019**: The init code MUST define `kb-save-last` (and the Alt-Shift-K binding) that reads the previous command from the shell's own history mechanism (`fc -ln -1` in zsh and bash, `history --max=1` in fish) and runs `kb add --type cmd --stdin` with it.

### CLI

- **FR-020**: Commands: `kb pick [query…] [--type] [--tag] [--height] [--no-preview] [--print <mode>] [--tmux]`; `kb shell init <shell>`; `kb shell install`; `kb shell uninstall`; `kb completions <shell>`.
- **FR-021**: Widget mode MUST be reachable only through the init code (for example a hidden `kb pick --widget --buffer <text> --cursor <n> --shell <name>`), and MUST output a small, documented format that the init code parses (the new command line and cursor position), so the shell side stays minimal.
- **FR-022**: Exit codes: `0` pick, `130` cancelled, `1` nothing to pick (empty KB, no matches on exit) or insertion refused (secret without `env`), `2` usage or I/O errors.

### Key Entities

- **WidgetRequest**: shell, current command line, cursor position, buffer mode, type filter.
- **WidgetResult**: new command line, new cursor position, or "unchanged" with an optional one-line message.
- **ShellDialect**: name, version, quoting rules, environment-variable reference syntax, history access, binding syntax.

## Success Criteria *(mandatory)*

- **SC-001**: From an empty prompt, putting a parameterized command (two variables with correct pre-filled values) on the command line takes the widget key, a few characters of search, and two Enter presses.
- **SC-002**: Automated pseudo-terminal tests pass for zsh 5.8+, bash 5.x, bash 3.2 (fallback), and fish 3.x on macOS and Linux, inside and outside tmux, in emacs and vi editing modes.
- **SC-003**: In the test matrix, no widget action ever executes a command, and Esc always leaves the command line byte-for-byte as it was.
- **SC-004**: After inserting and running commands that use secrets in every shell, the shell's history file contains only environment-variable references, never a secret value.
- **SC-005**: The picker's lines are fully cleared after every exit path in the test matrix, including when the prompt was on the last line of the screen.
- **SC-006**: `kb shell install` followed by opening a new shell gives a working widget and completions with no other step, in all three shells.

## Out of Scope

- PowerShell, Nushell, Elvish and Xonsh (PowerShell belongs to native Windows support, M11).
- A global, system-wide hotkey. Users who want one can bind a system shortcut to open a small terminal window running `kb pick`.
- Running commands from the widget (use `kb run`, M7).
- Replacing or wrapping the shell's own history search (Ctrl-R).
- Bulk import from shell history; only the last command can be saved.

## Assumptions

- Alt-K is free in default zsh, bash and fish configurations and memorable ("**k**b"). Users with a conflicting binding can choose another key.
- Most users run zsh or bash 4+ on Linux and zsh on macOS, so the bash 3.2 fallback serves a minority.
- Inserting environment-variable references for secrets is acceptable, because a user with a secret in an environment variable already has it available to the commands they run.

## Constitution Check notes

- **III. Local-only**: no network access is involved.
- **IV. Terminal only, one core**: the picker and widgets use the M2, M5 and M7 core APIs; the shell code only passes the command line in and puts the result back.
- **VI. Secrets**: FR-014 keeps secrets out of command lines and history.
- **VII. Keyboard-first and fast**: FR-005 and FR-018 set the latency budgets; insertion never runs anything, so nothing happens without the user pressing Enter.
- **I. Plain files**: `kb shell install` writes outside the KB root, to the user's shell startup file. This is limited to a marked block, done only after explicit confirmation, and reversible with `kb shell uninstall`. It is recorded here as a justified exception.
