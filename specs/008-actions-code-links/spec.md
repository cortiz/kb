# Feature Specification: Actions and Code Links

**Feature Branch**: `008-actions-code-links`
**Created**: 2026-10-09
**Status**: Draft
**Milestone**: M7 (merges the former M7 "Type actions" and M8 "Code links")
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M4 (`004-platform-clipboard`), M5 (`006-variables`), M6 (`007-authoring`)
**Input**: Give every entry type something to *do* beyond copying: open links and console deep links, run commands safely, and pipe queries into tools. Make code links first-class: open them in GitHub, VS Code or IntelliJ, show the code, detect when it has moved, and re-pin it (screen 3 of the design).

## Overview

Through M6, kb's only action is copy. That covers most daily use, but some entries are meant to be acted on directly: a runbook link should open, a Sumo query should land in Sumo with the time range set, a command should run, and a code link should open at the right line in the right tool.

Code links are the most demanding case. A link to `AuthorizerHandler.java:147` is only useful while line 147 is still the right place. This milestone pins each code link to a commit, previews the code from a local clone, maps the pinned line onto the current branch through git's own diff, and re-pins it when the user agrees.

All actions share one model: each entry type has a set of named actions, one of which is its **primary open action**. The TUI, the CLI and later the shell picker (M8) all list and run actions through the same core API.

## Clarifications

### Session 2026-10-09

- Q: What should be the default target when opening a code entry if `code.default_open` is not configured? (FR-002, FR-006) → A: `web` (open repository permalink in the default browser).
- Q: What should the input mode default to when a custom action definition omits the `input` field in configuration? (FR-034) → A: `stdin` (pipe entry payload into standard input).
- Q: What action should the Enter key perform while viewing a code entry in the code view (screen 3)? (FR-039) → A: Launch the default open target (configured `code.default_open`, or `web`).

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Open links and console deep links (Priority: P1)

Carlos selects his "Auth0 scopes dashboard" link and presses Ctrl-O: it opens in his browser. He selects "5xx by authorizer", fills `env` and `minutes`, presses Ctrl-O, and Sumo opens with the query and a 30-minute range already set. The same works for a CloudWatch Insights query, which opens in the right region with its log groups selected.

**Why this priority**: opening is the most common action after copy, and console deep links remove the paste-then-set-time-range routine in the middle of an incident.

**Independent Test**: with the platform opener replaced by the test double, run the open action on each fixture entry and compare the URL passed to the opener with the expected URL.

**Acceptance Scenarios**:

1. **Given** a `link` entry, **When** the user presses Ctrl-O or runs `kb open <id>`, **Then** its URL (with variables resolved and percent-encoded, per M5) opens in the default browser and a use is recorded.
2. **Given** a `sumo` entry and a configured Sumo deployment URL, **When** the open action runs, **Then** kb builds a Sumo search URL containing the resolved query and the resolved `range`, and opens it.
3. **Given** a `cw` entry with `region` and `log_groups`, **When** the open action runs, **Then** kb builds a CloudWatch Logs Insights console URL with the region, log groups, resolved query and range, and opens it.
4. **Given** a `sumo` entry but no Sumo deployment configured, **Then** kb explains which setting is missing and how to set it, and copies the query instead.
5. **Given** a `cw` entry with no `region`, **Then** kb uses `aws.default_region` from config, then `AWS_REGION`, then `AWS_DEFAULT_REGION`, and fails with a clear message if none is set.
6. **Given** the entry has variables, **Then** the M5 form opens first, and the open action runs on the resolved entry.
7. **Given** kb runs over SSH with no display (M4 FR-010), **Then** kb prints the URL and copies it instead of opening it.

---

### User Story 2 — Run a command safely (Priority: P1)

Carlos selects "Tail authorizer logs", fills `env=prod`, and presses Ctrl-G. kb shows the exact command it is about to run and asks him to confirm. He confirms, the TUI steps aside, the command runs in his terminal, and when it ends kb shows the exit code and returns to the list.

**Why this priority**: running saved commands is the reason many entries exist. Constitution principle VII requires that it is never surprising.

**Independent Test**: run fixture `cmd` entries through the test platform in a pseudo-terminal; check the confirmation text, the command line, working directory and environment received by the child process, and the reported exit code.

**Acceptance Scenarios**:

1. **Given** a `cmd` entry, **When** the user presses Ctrl-G or runs `kb run <id>`, **Then** kb resolves variables (M5), shows the full resolved command, its shell and working directory, and asks for confirmation (`y` to run, anything else to cancel).
2. **Given** the user confirms, **Then** the TUI suspends, the command runs in the foreground with the user's terminal as its input and output, and the entry's `shell` and `cwd` are used.
3. **Given** the command finishes, **Then** kb shows the exit code and duration, waits for a key press, and returns to the TUI with the same query and selection. From the CLI, `kb run` exits with the command's own exit code.
4. **Given** the resolved command matches a destructive pattern (FR-019), **Then** the confirmation shows a warning naming the pattern, and the user must type `run` in full instead of pressing `y`.
5. **Given** the command uses a `secret` variable, **Then** the secret is passed to the child process as an environment variable, the command refers to that variable, and the confirmation shows the variable name, never the value (FR-021).
6. **Given** `kb run <id> --yes`, **Then** kb prints the resolved command to stderr and runs it without prompting, unless it matches a destructive pattern, which also requires `--force`.
7. **Given** the user presses Ctrl-C while the command runs, **Then** the signal goes to the command, not to kb, and kb returns to the TUI when the command exits.

---

### User Story 3 — Open code where I work (Priority: P1)

Carlos selects "AuthorizerHandler.handleRequest" and presses Ctrl-O. The code view (screen 3) shows the repo, path, line and pinned ref, a few lines of code around the pinned line, and keys to open it in GitHub, VS Code or IntelliJ, or to copy `path:line`.

**Why this priority**: code links are only useful if they open in the tool where the user will act on them.

**Independent Test**: with a fixture git repository and a configured local clone, check the code view snapshot, and check the URL or command produced for each open target.

**Acceptance Scenarios**:

1. **Given** a `code` entry, **When** the user presses Ctrl-O, **Then** the code view opens, matching screen 3: the entry title and metadata; repo, path with line, pinned ref; the code around the pinned line from the pinned ref with that line highlighted; the drift status (User Story 4); and the open keys (Enter to open the default target, `g`, `v`, `i`, `y`, `Y`, `u`, `F`).
2. **Given** the code view, **When** the user presses `g`, **Then** the web permalink for the pinned ref and line (or line range) opens in the browser.
3. **Given** a local clone of the repo is known (FR-025), **When** the user presses `v` or `i`, **Then** the file opens at that line in VS Code or IntelliJ respectively, using the file in the local clone.
4. **Given** `y`, **Then** `path:line` is copied. **Given** `Y`, **Then** the web permalink is copied.
5. **Given** no local clone is known, **Then** the code snippet area says so, explains how to configure one, and the `v`/`i` keys are disabled; `g`, `y` and `Y` still work.
6. **Given** `code.default_open = "vscode" | "idea" | "web"` in config, **When** the user runs `kb open <code-id>`, **Then** that target is used. If unset, it defaults to `web`. `--with web|vscode|idea` overrides it.

---

### User Story 4 — Know when a code link has drifted, and re-pin it (Priority: P1)

Since the link was saved, `handleRequest` moved from line 147 to line 151 on `main`. The code view shows "moved: now at line 151 on main". Carlos presses `u`, confirms, and the entry now points at the current commit and line 151.

**Why this priority**: an out-of-date code link silently sends the user to the wrong place, which is worse than having no link.

**Independent Test**: a fixture repository with scripted history (lines inserted above, the function edited, the file renamed, the file deleted). For each case, check the detected status, the proposed line, and the file after re-pinning (byte-for-byte except for `ref`, `line`, `end_line`, `path` and `updated`).

**Acceptance Scenarios**:

1. **Given** a code entry and a local clone, **When** the code view opens, **Then** kb compares the pinned ref with the repo's tracking branch (FR-030) and shows one status:
   - **current**: the pinned lines are unchanged and at the same line on the branch;
   - **moved**: unchanged, but at a different line (shows the new line);
   - **changed**: the pinned lines were edited (shows a short diff);
   - **renamed**: the file moved to a new path (shows it);
   - **gone**: the file or the code was removed;
   - **unknown**: no local clone, the ref doesn't exist locally, or the branch can't be found (shows why).
2. **Given** the status is moved, changed or renamed, **When** the user presses `u`, **Then** kb shows the proposed new `ref`, `path` and `line` and asks to confirm; on confirm, only those fields and `updated` change in the file (M1 FR-017), and the index is refreshed.
3. **Given** `kb code check`, **Then** kb checks every code entry (or the given IDs) and prints one line per entry with its status, and `--json` gives the same as JSON. It exits 1 if any entry is changed, renamed or gone.
4. **Given** `kb code repin <id>` or `kb code repin --all`, **Then** kb re-pins each moved, changed or renamed entry after confirmation (or without, with `--yes`). Gone and unknown entries are never re-pinned and are listed at the end.
5. **Given** `--fetch` on `kb code check`, `kb code repin` or `F` in the code view, **Then** kb runs `git fetch` for the clone first. Without it, kb never touches the network.

---

### User Story 5 — One menu for every action (Priority: P2)

Carlos doesn't remember which keys apply to which type. He presses Alt-A and sees every action available for the selected entry, each with a key.

**Acceptance Scenarios**:

1. **Given** any entry, **When** the user presses Alt-A (or F2), **Then** an action menu lists every available action for that entry with its single-letter key, the primary action first; pressing the letter runs it, Esc closes the menu.
2. **Given** `kb actions <id>`, **Then** the same list is printed, with `--json` support.
3. **Given** a code entry, **Then** the action menu is the code view (User Story 3).

---

### User Story 6 — Pipe a query into my own tools (Priority: P3)

Carlos wants SQL entries for the `reporting` database to run in `psql` and prompts to go to `claude -p`. He adds custom actions in config.

**Acceptance Scenarios**:

1. **Given** a custom action defined in config for a type (FR-034), **Then** it appears in the action menu and in `kb actions`, under its own name and key.
2. **Given** the user runs it, **Then** kb resolves the entry, shows the command and how the payload is passed (stdin or argument), asks for confirmation as for `cmd` entries, and runs it the same way (User Story 2).
3. **Given** a custom action's command uses `{{field}}` placeholders for entry fields (for example `{{database}}`), **Then** they are filled from the entry, shell-quoted.

---

### Edge Cases

- **Console URL length**: very long queries can exceed browser URL limits. Above 8,000 characters kb warns, copies the query and opens the console without it.
- **`cw` with no log groups**: the console opens with the query and range, and the user picks log groups there.
- **Commands needing a TTY** (`less`, `vim`, `top`, `ssh`): work, because the TUI is fully suspended and the child gets the real terminal.
- **Command not found / shell not installed**: kb reports it after the run with exit code 127, as a shell would, and suggests checking `shell`.
- **`cwd` doesn't exist**: kb refuses to run and names the directory.
- **Long-running commands and Ctrl-Z**: job control works as in the user's shell; kb resumes when the command finishes.
- **Run over SSH**: works the same; nothing in running depends on the local platform.
- **Code: shallow clone or ref not present locally**: status is unknown with the hint to use `--fetch`.
- **Code: pinned to a branch name instead of a SHA**: kb resolves it to a SHA at check time, shows a warning that branch pins drift by design, and re-pinning always writes a SHA.
- **Code: repository has local uncommitted changes**: drift is computed against the tracking branch, not the working tree. The editor opens the working-tree file, which may differ; the code view says so when the file has local changes.
- **Code: `end_line` set**: the whole range is shown highlighted, and drift maps both ends.
- **Code: several local clones of the same repo**: the first one found in configured order is used; `kb doctor` lists duplicates.
- **Code: non-GitHub hosts**: GitLab and Bitbucket permalinks are built from the remote URL. Other hosts need a `web_url` template in config, otherwise `g` is disabled with an explanation.
- **IntelliJ / VS Code not installed**: the open fails with a message naming the URL scheme that wasn't handled; kb suggests `code.default_open`.

## Requirements *(mandatory)*

### Action model

- **FR-001**: The core MUST define, for each entry, a list of available actions, each with a stable name, a single-letter key, a label, whether it needs variables resolved first, and whether it needs confirmation.
- **FR-002**: Built-in actions per type:

  | Type | Actions (primary open in bold) |
  |---|---|
  | `sql` | copy, **custom actions only** (no built-in open) |
  | `sumo` | copy, **open in Sumo** |
  | `cw` | copy, **open in CloudWatch** |
  | `cmd` | copy, run |
  | `prompt` | copy |
  | `link` | copy URL, **open** |
  | `code` | copy `path:line`, copy permalink, **open (default target, defaults to web)**, open web, open VS Code, open IntelliJ, check, re-pin, fetch |

- **FR-003**: Every action that runs or opens something MUST record a use (M2 FR-017).
- **FR-004**: Actions MUST run on the resolved entry (M5). If variables need values, the M5 form or prompts run first.
- **FR-005**: The TUI, the CLI and later the shell picker MUST list and run actions only through the core API.

### Opening links and consoles

- **FR-006**: Opening MUST use the M4 opener, so it follows platform rules and the SSH fallback. When opening a `code` entry without `--with` and without `code.default_open` configured, the target MUST default to `web`.
- **FR-007**: Sumo deep links MUST use the configured `sumo.base_url` (the user's deployment, for example `https://service.us2.sumologic.com`) and include the resolved query and the resolved `range` as the search time range. The exact URL format is confirmed during planning against current Sumo behavior.
- **FR-008**: CloudWatch deep links MUST target the Logs Insights console for the resolved region and include the resolved query, `log_groups` and `range`, using the console's URL encoding. The exact format is confirmed during planning.
- **FR-009**: Region resolution for `cw`: the entry's `region`, then `aws.default_region` in config, then `AWS_REGION`, then `AWS_DEFAULT_REGION`.
- **FR-010**: A `range` value MUST be accepted in the forms `-15m`, `-2h`, `-7d` (relative to now). Invalid values MUST produce an error naming the field and the accepted forms.
- **FR-011**: URLs longer than 8,000 characters MUST NOT be opened as-is (see Edge Cases).

### Running commands

- **FR-012**: Running MUST show, before execution: the fully resolved command, the shell, the working directory, the secret environment variables that will be set (names only), and any destructive-pattern warning.
- **FR-013**: Confirmation MUST be required for every run in the TUI and in interactive CLI use. In non-interactive use, `--yes` counts as confirmation, and kb MUST print the resolved command to stderr before running it.
- **FR-014**: The command MUST run as `<shell> -c <command>` using the entry's `shell` (default `sh`), in the resolved `cwd` (with `~` expanded) or the user's current directory, with the user's environment.
- **FR-015**: While a command runs, kb MUST give it the terminal (leave the alternate screen, restore normal mode), forward signals to it, and restore the TUI afterwards (M3 FR-002).
- **FR-016**: After the command exits, the TUI MUST show the exit code and duration and wait for a key press (setting `run.pause_after = false` skips the wait for exit code 0).
- **FR-017**: `kb run` MUST exit with the child's exit code, or `128 + signal` if it was killed by a signal.
- **FR-018**: kb MUST NOT capture or store command output.
- **FR-019**: kb MUST check the resolved command against destructive patterns: `rm -r`/`-rf`, `kubectl delete`, `terraform destroy`, `terraform apply`, `helm uninstall`, `aws … delete-*`/`terminate-*`/`remove-*`, `git push --force`/`-f`, `git reset --hard`, `DROP`, `TRUNCATE`, `DELETE FROM` without `WHERE`, and redirection onto devices (`> /dev/sd…`). Users MAY add patterns in config (`run.destructive_patterns`) but MUST NOT remove the built-in ones.
- **FR-020**: Destructive commands MUST require typing `run` in the TUI and interactive CLI, and `--yes --force` non-interactively.
- **FR-021**: When running (not when copying), each `secret` variable in a `cmd` payload MUST be passed through the child's environment as `KB_SECRET_<NAME>`, and the substituted text MUST be a reference to that variable in the entry's shell syntax (for example `"$KB_SECRET_TOKEN"`). The secret value MUST NOT appear in the command line, the confirmation screen, or the process list.

### Code links: local clones

- **FR-022**: The `code.repo` field from M1 MUST be matched to a local clone by, in order: an explicit mapping in config (`[code.repos] "owner/name" = "~/code/name"`); then searching the directories in `code.search_paths` (default none) one level deep for a git work tree whose remote URL matches the repo.
- **FR-023**: Matching remote URLs MUST treat SSH and HTTPS forms of the same repo as equal (`git@github.com:owner/name.git` = `https://github.com/owner/name`).
- **FR-024**: Clone discovery results MUST be cached in `.kb/` and refreshed when a cached path no longer exists or no longer matches.
- **FR-025**: Git operations MUST use the `git` command line, read-only except for `git fetch` when explicitly requested. kb MUST NOT check out, modify or commit anything in the user's clones.

### Code links: opening and permalinks

- **FR-026**: Web permalinks MUST use the pinned `ref` (resolved to a full SHA) and the line or line range, in the format of the host detected from the clone's remote or from `code.repo`: GitHub, GitLab or Bitbucket. Other hosts MUST use a `web_url` template from config (with `{repo}`, `{ref}`, `{path}`, `{line}`, `{end_line}`) or report that no permalink can be built.
- **FR-027**: VS Code MUST be opened with the `vscode://file/<absolute path>:<line>` URL; IntelliJ with `idea://open?file=<absolute path>&line=<line>`. When `end_line` is set, `<line>` refers to the start line (desktop URL schemes navigate to start line; the full range is highlighted in the TUI Code View). Both MUST go through the M4 opener. Over SSH these targets MUST be disabled with an explanation.
- **FR-028**: The code view snippet MUST be read from the pinned ref with `git show <ref>:<path>`, showing by default 3 lines before and 8 after the pinned line (or the whole range), with syntax highlighting chosen by file extension.

### Code links: drift detection and re-pinning

- **FR-029**: Drift MUST be computed with git's own diff between the pinned ref and the tracking branch, with rename detection, by mapping the pinned line (and `end_line`) through the diff hunks. If `symbol` is set and the mapping fails, kb MUST try to locate the symbol by name in the new file as a fallback, and report a symbol-based result as lower confidence.
- **FR-030**: The tracking branch MUST be, in order: `code.branch` set on the entry (optional field added to `kb_format: 1`, as M5 did with `escape`), then the mapping in config, then the clone's remote default branch (`origin/HEAD`), then `origin/main`, then `origin/master`.
- **FR-031**: Statuses MUST be exactly: current, moved, changed, renamed, gone, unknown, each with the data defined in User Story 4.
- **FR-032**: Re-pinning MUST write `ref` as the full SHA of the tracking branch tip, plus the mapped `line`, `end_line` and `path`, and `updated`, using M1's format-preserving edit, and MUST refuse to write if the file changed on disk since it was read (M6 FR-020).
- **FR-033**: Drift checks for one entry MUST complete in 500 ms or less on a repository with 100,000 commits, without fetching.

### Custom actions

- **FR-034**: Config MAY define custom actions per type:
  ```toml
  [[actions.sql]]
  name = "psql"
  key = "p"
  command = "psql -d {{database}}"
  input = "stdin"   # optional, defaults to "stdin"; or "arg", which appends the payload as the last argument
  ```
  Placeholders refer to entry fields, are shell-quoted, and are filled after variables are resolved. The `input` field defaults to `stdin` if omitted. A placeholder for a field the entry doesn't have MUST disable the action for that entry with an explanation.
- **FR-035**: Custom actions MUST follow every rule for running commands (FR-012 to FR-021), including confirmation and destructive-pattern checks on the full command.
- **FR-036**: Custom action keys MUST NOT clash with built-in keys for that type; clashes MUST be reported at startup and the custom action disabled.

### TUI

- **FR-037**: New keys in the main list: Ctrl-O primary open (for `code`, the code view); Ctrl-G run (for `cmd`); Alt-A or F2 action menu. These MUST be added to the M3 KeyMap without clashing with existing bindings (Ctrl-N, Ctrl-R, Ctrl-X from M6; Ctrl-E, Ctrl-Y from M3).
  *Amended by spec 013: in the vim keymap's normal mode these are `o` open, `!` run, `m` or `.` action menu (the Ctrl and Alt keys keep working). The keys are defaults and can be changed in `[tui.keys.<context>]` (013 FR-018).*

- **FR-038**: The key bar MUST show only the actions available for the selected entry.
- **FR-039**: The code view (screen 3) MUST support: Enter or primary open key to launch the default open target (configured `code.default_open`, or `web`), `g` web, `v` VS Code, `i` IntelliJ, `y` copy `path:line`, `Y` copy permalink, `u` re-pin, `F` fetch then recheck, `Ctrl-E` edit the entry, Esc back to the list.
- **FR-040**: `tui.enter` from M4 MUST additionally accept `primary`, which makes Enter run the type's primary open action instead of copy. The default stays `copy`. Enter MUST never run a command.

### CLI

- **FR-041**: `kb open <id|query> [--with web|vscode|idea|console] [--var …] [-y]`.
- **FR-042**: `kb run <id|query> [--var …] [-y] [--force] [--action <name>]` (`--action` runs a custom action).
- **FR-043**: `kb actions <id|query> [--json]`.
- **FR-044**: `kb code check [ids…] [--fetch] [--json]`, `kb code repin <id>… | --all [--fetch] [-y]`, `kb code show <id>` (prints the snippet and status).
- **FR-045**: Argument resolution follows M4 FR-020. Exit codes: `0` success; `1` not found, ambiguous, action unavailable, or drift found by `kb code check`; `2` usage, config or I/O errors; `130` cancelled; `kb run` otherwise returns the child's exit code.

### Key Entities

- **Action**: name, key, label, kind (copy, open URL, open editor, run, custom, code operation), requirements (variables, confirmation, local clone, display).
- **RunPlan**: resolved command, shell, working directory, secret environment variable names, destructive warnings.
- **ConsoleLink**: target (Sumo, CloudWatch), URL, and whether the query was included or dropped for length.
- **LocalClone**: repo identity, path, remote URL, host kind, tracking branch.
- **DriftReport**: status, pinned ref and line, mapped path and line (or range), confidence (diff-mapped or symbol-matched), short diff, and the reason for unknown.

## Success Criteria *(mandatory)*

- **SC-001**: From the TUI, opening a parameterized Sumo or CloudWatch query in its console takes the variable form plus one key, and lands on a page where the query runs without edits, verified manually against a real Sumo deployment and AWS account.
- **SC-002**: In a test matrix of 30 hostile and destructive commands, every destructive one requires the stronger confirmation, and no command ever runs without a confirmation or `--yes`.
- **SC-003**: After a run that uses a secret, the secret value does not appear in the confirmation screen, captured `ps` output during the run, kb's stderr, or any file kb wrote.
- **SC-004**: On the scripted fixture repository, drift detection reports the correct status and line for 100% of cases mapped through the diff, and the symbol fallback finds the right location in at least 90% of the remaining cases.
- **SC-005**: Re-pinning changes only `ref`, `path`, `line`, `end_line` and `updated` in the file, byte for byte.
- **SC-006**: `kb code check` over 200 code entries in 10 repositories finishes in under 10 seconds without network access.
- **SC-007**: The terminal is restored correctly after every run in the test matrix, including commands killed by signals and commands that leave the terminal in raw mode.

## Out of Scope

- Inserting commands into the shell prompt instead of running them (M8, shell integration).
- Running queries against Sumo or CloudWatch APIs and showing results in kb.
- Recording command output or a run history.
- Browsing or searching repositories; code entries are created in M6 from pasted URLs or by hand.
- Creating clones automatically.
- Editors other than VS Code and IntelliJ through URL schemes; others can be used through a custom action.

## Assumptions

- The user has local clones of the repositories that matter to them, so local git is enough for previews and drift detection, and kb never needs a hosting provider's API or a token.
- Sumo and CloudWatch console deep-link formats are stable enough to depend on; they are checked during planning and covered by tests of the generated URL, plus a manual check per release.
- `y` as a confirmation is enough for ordinary commands, because the full resolved command is always on screen when it is asked for.

## Constitution Check notes

- **III. Local-only**: network access happens only through `git fetch` on explicit request and through URLs the user opens. No hosting APIs are called.
- **VI. Secrets**: FR-021 keeps secrets out of command lines when running.
- **VII. Keyboard-first and fast**: every action is keyboard-reachable, and running always shows the resolved command and requires confirmation (`--yes` is an explicit, logged confirmation in scripts).
- **IX. Simplicity**: custom actions are one small, general mechanism instead of per-tool integrations for databases or AI tools.
