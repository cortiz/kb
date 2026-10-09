# Feature Specification: Variables

**Feature Branch**: `006-variables`
**Created**: 2026-10-09
**Status**: Draft
**Milestone**: M5 (spec number 006; spec 005 was added separately)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M4 (`004-platform-clipboard`)
**Input**: Turn `{{variables}}` from markers into a working feature: prompt for values, remember them, resolve the entry, and copy the result, from both the TUI (screen 2 of the design) and the CLI.

## Overview

M1 defined how variables are declared and validated. Until now, kb has copied payloads with `{{env}}` still in them. M5 adds **resolution**: kb collects a value for each variable, checks it, substitutes it safely for the place it's going (a shell command, a URL, a query), and hands the result to the clipboard.

The same resolution engine lives in the core and serves both the TUI modal and `kb use`, so a value means the same thing wherever it is entered.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Fill variables in the TUI and copy the result (Priority: P1)

Carlos selects "5xx by authorizer" and presses Enter. Instead of copying straight away, a small form opens: `env` (prod, stage, dev) and `minutes` (number). He picks `prod`, types `30`, watches the query update in the preview, and presses Enter. The resolved query is on his clipboard.

**Why this priority**: most saved queries are parameterized. Copying them with placeholders still in them means hand-editing every time, which is what kb is meant to remove.

**Independent Test**: in a pseudo-terminal, select a fixture entry with each variable kind, fill the form with keystrokes, and compare the clipboard (via the platform test double) with the expected resolved text.

**Acceptance Scenarios**:

1. **Given** the selected entry has at least one variable, **When** the user presses Enter, **Then** the variable form opens over the TUI instead of copying.
2. **Given** the form is open, **Then** it shows one field per variable in the order each variable first appears in the entry, each pre-filled per FR-010, and a live preview of the fully resolved text below the fields.
3. **Given** the user changes any field, **Then** the preview updates immediately, with substituted values highlighted.
4. **Given** all fields are valid, **When** the user presses Enter, **Then** the resolved text is copied, the values are remembered (except secrets), a use is recorded, and the TUI exits (or stays open, if the form was opened with Ctrl-Y).
5. **Given** a field is invalid (for example `abc` in a number field), **When** the user presses Enter, **Then** nothing is copied, the focus moves to the first invalid field, and an inline message explains what is expected.
6. **Given** the user presses Esc, **Then** the form closes and the list is shown again with the same selection. Values typed in the form are kept for the rest of the TUI session but not remembered.
7. **Given** the selected entry has no variables, **Then** Enter copies immediately, exactly as in M4.

---

### User Story 2 — Use a parameterized entry from the shell (Priority: P1)

Carlos runs `kb use 5xx --var env=prod --var minutes=30` in a script, or just `kb use 5xx` at the prompt and answers the questions.

**Why this priority**: constitution principle IV requires every TUI action to be scriptable.

**Acceptance Scenarios**:

1. **Given** `kb use <id|query>` in a terminal, **When** the entry has variables not supplied by flags, **Then** kb asks for each one on the terminal, one line at a time, showing the pre-filled value, kind and options; Enter accepts the pre-filled value.
2. **Given** every variable is supplied with `--var name=value`, **Then** kb asks nothing, resolves, copies and exits 0.
3. **Given** `--yes` (`-y`), **Then** kb accepts the pre-filled value for every variable not supplied by a flag and asks nothing. A variable with no pre-filled value is missing, and kb behaves as in scenario 4.
4. **Given** kb is not running in a terminal (or `--yes` is given) and a variable has no flag, no environment value, no remembered value and no default, **Then** kb copies nothing, lists the missing variables with their kinds on stderr, and exits 1.
5. **Given** `--print`, **Then** the resolved text goes to stdout instead of the clipboard, so `kb use 5xx --var env=prod --print | pbcopy` and `$(kb use tail-logs -y --print)` work.
6. **Given** `--var` names a variable the entry doesn't use, **Then** kb exits 2 and lists the variables the entry does use.
7. **Given** `kb vars <id|query>`, **Then** kb lists the entry's variables with kind, options, default, environment source, remembered value (never for secrets) and the value that would be pre-filled. `--json` is supported.

---

### User Story 3 — Secrets are used but never kept (Priority: P1)

A `cmd` entry needs an API token. The entry declares `token: { kind: secret, env: SUMO_TOKEN }`. When Carlos uses it, kb reads the token from his environment, or asks for it with masked input, and never writes it anywhere.

**Why this priority**: constitution principle VI. Remembering values must never become a way to store credentials.

**Acceptance Scenarios**:

1. **Given** a `secret` variable whose `env` is set, **Then** its value is taken from the environment and the field shows as `•••• (from $SUMO_TOKEN)`.
2. **Given** a `secret` variable with no environment value, **Then** the TUI field and the CLI prompt use masked input, and nothing typed is echoed.
3. **Given** a secret is resolved, **Then** its value never appears in the TUI preview, in `kb vars` output, in `.kb/state.json`, in error messages, or in any log. The preview shows `••••` in its place.
4. **Given** a secret was substituted into the copied text, **Then** the confirmation adds a short notice that the clipboard now holds a secret, because clipboard managers may keep a history.
5. **Given** `--var token=...` is passed on the command line for a secret, **Then** kb accepts it but warns that it may be saved in shell history, and suggests the `env` source instead.

---

### User Story 4 — Values land safely in commands and URLs (Priority: P1)

Carlos fills `branch` with `feature/x; rm -rf ~` in a `cmd` entry by mistake, or `query` with `a&b c` in a `link` entry. The command copies as one quoted argument, and the URL stays valid.

**Why this priority**: substituting raw text into a shell command is how a typo or a pasted value turns into damage. Safe defaults matter more than convenience here.

**Acceptance Scenarios**:

1. **Given** a `cmd` entry, **When** a value contains characters that are special to the entry's shell, **Then** it is quoted for that shell (`sh`, `bash`, `zsh` or `fish`) so it stays one argument.
2. **Given** a variable is used inside the `url` field of a `link` entry, **Then** its value is percent-encoded.
3. **Given** a variable declares `escape: none`, **Then** its value is substituted exactly as typed, and the TUI field shows a "raw" marker so the user knows.
4. **Given** `sql`, `sumo`, `cw` and `prompt` payloads, **Then** values are substituted as typed by default, since quoting rules there depend on where the variable sits; `escape: sql` (single-quoted SQL string literal) is available when wanted.

---

### User Story 5 — Suggestions from earlier use (Priority: P3)

Carlos has used `env=stage` in several entries this week. When a new entry also has an `env` variable, `stage` appears as a suggestion.

**Acceptance Scenarios**:

1. **Given** a `text` field has focus, **Then** up to five recently used values for any variable of the same name across the KB are shown below it as suggestions. Alt-↓ moves into the list, ↑/↓ then select one, Enter or Tab puts it in the field, and Esc or any typing closes the list. Until the list is entered, ↑/↓ move between fields as usual.
2. **Given** an `enum` field, **Then** suggestions are not shown; the options already cover it.

---

### Edge Cases

- **The same variable used several times**: one field, substituted everywhere.
- **Undeclared variables** (allowed by M1): treated as `text` with no default, shown with an "undeclared" marker, and listed as info by `kb lint` as before.
- **Escaped braces**: `\{{name}}` is output as the literal `{{name}}` and never prompted for.
- **A value containing `{{`**: substitution happens in a single pass, so a value is never itself expanded.
- **Newlines**: `text` values are single-line. Pasting text with a newline into a field keeps only the first line and shows a notice.
- **Empty values**: an empty `text` value is accepted only if the variable's default is the empty string. Otherwise the field is required.
- **Numbers**: accepted as integers or decimals in the forms `30`, `-5`, `0.25`; anything else is invalid. They are substituted as typed, not reformatted.
- **Many variables**: the form scrolls when there are more fields than fit, and keeps the preview visible.
- **Narrow terminal**: below 100 columns, the preview moves under the fields and is limited to 8 lines.
- **Remembered value no longer valid** (an option was removed from an enum, or a number was remembered for a field that became an enum): the remembered value is ignored and the default is used, without an error.
- **Entry edited while the form is open** (via Ctrl-E in another pane or an external editor): kb re-reads the entry when the form opens and again before copying. If the variables changed, it shows a notice and reopens the form.
- **`kb copy` on an entry with variables** (from M4): it now behaves as `kb use` with the same flags, so the user never gets unresolved placeholders by accident. `kb copy --raw` copies the unresolved payload as before.

## Requirements *(mandatory)*

### Resolution engine (core)

- **FR-001**: The core MUST provide a resolution engine that, for an entry, lists its variables (declared and undeclared) in order of first appearance, validates candidate values, and produces the resolved output.
- **FR-002**: Resolution MUST apply to the payload and to the string fields defined in M1 FR-009: `range`, `url`, `cwd`, `log_groups`. Notes are never resolved.
- **FR-003**: Substitution MUST be a single pass, MUST honor `\{{` as a literal `{{` (M1 FR-011), and MUST NOT expand values.
- **FR-004**: The engine MUST be the only place that substitutes values. The TUI and CLI MUST NOT implement substitution themselves.

### Validation

- **FR-005**: Each kind MUST be validated:
  - `text`: a single line; non-empty unless the default is `""`;
  - `number`: matches `-?\d+(\.\d+)?`;
  - `enum`: one of the declared `options`, compared exactly;
  - `secret`: non-empty, single line.
  A declared `default` that fails its own kind's check (for example a number default written `1e3` or `.5`, which lint accepts) is ignored when pre-filling, as if no default were declared; `kb vars` marks it `invalid, ignored`.
- **FR-006**: Validation errors MUST name the variable, state what was expected, and, for enums, list the options.

### Escaping

- **FR-007**: This milestone adds one optional field to variable declarations in `kb_format: 1`: `escape`, with values `auto` (default), `none`, `shell` and `sql`. M1's lint MUST accept it; unknown values are lint errors.
- **FR-008**: With `escape: auto`:
  - in a `cmd` payload and in `cwd`, values are quoted for the entry's `shell` when they contain any character special to it, and substituted unchanged otherwise. Quoting follows where the variable sits: unquoted in the template, the value is wrapped in quotes; inside single or double quotes of the template, it is escaped for that quoting instead of being wrapped again;
  - in `url`, values are percent-encoded;
  - in all other places, values are substituted unchanged.
- **FR-009**: `escape: shell` and `escape: sql` MUST apply that quoting wherever the variable appears. `escape: none` MUST substitute values unchanged everywhere, including in `cmd` payloads.

### Pre-filled values and memory

- **FR-010**: The pre-filled value for each variable MUST be chosen in this order:
  1. a `--var` flag (CLI only);
  2. the environment variable named by `env`, if set;
  3. the last value used for this variable **in this entry** (not for secrets);
  4. the declared `default`;
  5. empty.
  The TUI and `kb vars` MUST show where the pre-filled value came from.
- **FR-011**: After a successful copy or print, kb MUST remember the value of every non-secret variable for that entry in `.kb/state.json`. Remembering MUST be skipped with `--no-remember` or `vars.remember = false` in config. A value that looks like a credential (it matches kb's secret patterns) MUST NOT be remembered, and kb says so in one line; the copy itself is not affected.
- **FR-012**: kb MUST keep up to 20 recent values per variable name across the KB for suggestions (User Story 5), never including secrets.
- **FR-013**: Secret values MUST be held in memory only for the duration of one resolution and MUST NOT be written to disk, logs, the index, error messages, `--json` output or crash reports.
- **FR-014**: Remembered values for entries that no longer exist MUST be pruned along with their usage records (M2 FR-018), and MUST follow an entry across a rename.

### TUI (screen 2)

- **FR-015**: The variable form MUST match screen 2 of the design: the entry title and a field counter in the header; one field per variable with its name, kind, options or default, and where the pre-filled value came from; the live resolved preview; and a key bar.
- **FR-016**: Fields by kind:
  - `enum`: a horizontal choice; ←/→ to change;
  - `number` and `text`: a single-line input with standard line editing (as in M3 FR-011);
  - `secret`: masked input, showing only `••••`; values from the environment are not editable unless the user presses Ctrl-R to replace them.
- **FR-017**: Keys in the form:

  | Key | Action |
  |---|---|
  | Tab, ↓ / Shift-Tab, ↑ | next / previous field (↑/↓ choose a suggestion once the list has been entered with Alt-↓) |
  | ←, → | change an enum value; move the cursor in text fields |
  | Enter | copy the resolved text, if every field is valid |
  | Alt-Enter | print the resolved text to stdout and exit (same as `--print`) |
  | Ctrl-Z | reset the focused field to its pre-filled value |
  | Esc | close the form and return to the list |

- **FR-018**: In the main list, Alt-C MUST copy the unresolved payload without opening the form, for when the user wants the template itself. It records a use but remembers no values.
- **FR-019**: The preview MUST highlight each substituted value and show `••••` for secrets. Values quoted by escaping MUST be shown as they will be copied, quotes included.

### CLI

- **FR-020**: `kb use <id|query...>` MUST resolve the argument the same way as `kb copy` (M4 FR-020), including opening the picker on close matches when there is a terminal, then resolve variables and copy. When the picker is used, `--var` and `--yes` are ignored (kb says so on stderr before opening it) and the form collects the values; `--print`, `--method` and `--no-remember` still apply. Flags: `--var name=value` (repeatable), `--yes`/`-y`, `--print`, `--no-remember`, `--method` (from M4).
- **FR-021**: `kb copy` on an entry with variables MUST behave like `kb use`. `kb copy --raw` MUST copy the unresolved text.
- **FR-022**: `kb vars <id|query...>` with `--json`.
- **FR-023**: Interactive CLI prompts MUST read from and write to the controlling terminal (`/dev/tty`), so `kb use x --print > file` still prompts correctly.
- **FR-024**: Exit codes: `0` success; `1` missing values in non-interactive mode, not found, or ambiguous; `2` usage errors (unknown variable in `--var`, malformed `--var`); `130` cancelled.

### Key Entities

- **Variable**: from M1, plus `escape`, every location it appears in, and whether it was declared.
- **Binding**: variable name, value, source (flag, environment, remembered, default, typed), and whether it is secret.
- **ResolutionResult**: the resolved payload and fields, the bindings used, and the character ranges of each substitution (for highlighting).
- **VarMemory**: per-entry last values and per-name recent values, stored in `.kb/state.json`.

## Success Criteria *(mandatory)*

- **SC-001**: For an entry with two variables whose pre-filled values are right, going from the list to the resolved text on the clipboard takes one key press more than an entry without variables.
- **SC-002**: The preview updates within 16 ms of each keystroke for payloads up to 1,000 lines.
- **SC-003**: A test suite of hostile values (shell metacharacters, quotes of every kind, newlines, `{{`, Unicode, very long strings) produces correctly quoted output for every shell and for URLs, and never runs anything.
- **SC-004**: After a full test run that uses secrets through every path (environment, typed, `--var`), a search of `.kb/`, the config directory, the temp directory and all captured output finds no secret value.
- **SC-005**: Every `kb use` scenario that is valid interactively can be reproduced non-interactively with `--var` and `--yes`, producing byte-identical output.

## Out of Scope

- Running `cmd` entries and opening `link` entries after resolving (M7). This milestone stops at copy or print.
- Built-in variables such as `{{today}}` or `{{clipboard}}`.
- Variables computed from other variables, conditionals, or loops in templates.
- Reading secrets from a keychain or secret manager. `env` and typed input are the only sources.
- New variable kinds beyond the four from M1.
- Per-variable `min`, `max` or `pattern` constraints.

## Assumptions

- The order of first appearance is the right field order; users who want a different order can reorder their payload.
- Pre-filling from the environment before remembered values is the right order, because environment values usually reflect where the user is working right now.
- Spec 005 (added separately) doesn't change the M1 variable format, the M2 state file, or the M4 copy commands. If it does, the plan for this spec must reconcile with it.
