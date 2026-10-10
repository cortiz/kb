# Research & Technical Decisions: Actions and Code Links

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

This document records the architectural and design decisions for Milestone 7 (Actions and Code Links), resolving implementation details across console deep links, process execution, git-based code drift tracking, and TUI interactions.

---

## R1: Console Deep Link URL Schemes (Sumo Logic & CloudWatch)

### Decision
- **Sumo Logic**: Build search deep links using the deployment's base URL (`sumo.base_url`, e.g. `https://service.us2.sumologic.com`), formatting relative time ranges into millisecond epochs or relative range tokens:
  ```text
  {base_url}/ui/index.html#section/search/{from_epoch_ms},{to_epoch_ms},{percent_encoded_query}
  ```
  Relative ranges (`-15m`, `-2h`, `-7d`) are converted relative to current UTC time (e.g. `-15m` = `now - 900_000ms` to `now`).
- **CloudWatch Logs Insights**: Build deep links targeting the AWS CloudWatch console for the resolved region:
  ```text
  https://{region}.console.aws.amazon.com/cloudwatch/home?region={region}#logsV2:logs-insights$3FqueryDetail$3D~(end~0~start~'{range_seconds}'~timeType~'RELATIVE'~unit~'seconds'~editorString~'{encoded_query}'~source~(~'{log_group_1}'~'{log_group_2}'))
  ```
  AWS Console's `$3F` (`?`) and `$3D` (`=`) encoding with `~` delimiters is used.
- **URL Length Guard**: If the generated URL exceeds 8,000 characters (FR-011), kb logs a notice, copies the query to the clipboard, and opens the console URL stripped of the query string.

### Rationale
- Browser address bars and OS openers (e.g., `open`, `xdg-open`) reliably handle URLs under 8k characters.
- CloudWatch Logs Insights and Sumo web consoles directly execute queries preloaded via these deep link formats, fulfilling SC-001.

### Alternatives Considered
- *Sumo API / AWS SDK execution*: Rejected. Constitution principle III forbids servers/APIs and Constitution IX emphasizes simplicity. The goal is landing the engineer in the browser console, not running API queries.
- *Unencoded or raw hash strings*: Rejected because modern browsers truncate or misinterpret raw characters like quotes, brackets, and newlines in URI fragments.

---

## R2: Safe Command Execution, Terminal Suspension & Signal Handling

### Decision
- Process spawning is managed through the `Platform` trait (`run_interactive_command` / `run_command_plan`).
- In the TUI:
  1. Leave alternate screen mode (`LeaveAlternateScreen`) and disable terminal raw mode (`disable_raw_mode()`).
  2. Spawn the process with `<shell> -c <command>` using standard process I/O inheritance (`Stdio::inherit()`) and the resolved working directory (`cwd`).
  3. Measure wall-clock execution duration.
  4. Wait for the child process to complete. Signals like `SIGINT` (Ctrl-C) and `SIGTSTP` (Ctrl-Z) go to the child process group in the foreground.
  5. Upon termination, determine exit status: standard exit code, or `128 + signal` if terminated by a signal.
  6. Re-enable raw mode and re-enter alternate screen mode.
  7. If `run.pause_after = false` and exit code is 0, return immediately to the list. Otherwise, display the execution status line:
     `[Exit {code} in {duration}s] Press any key to return to kb...`
     and wait for a keypress.
- In the CLI:
  1. `kb run <id>` prints the resolved command to stderr when run with `--yes`.
  2. Runs foreground process directly, exiting with the child's exact exit code.

### Rationale
- Preserves full TTY interactivity for programs like `less`, `vim`, `ssh`, and `top`.
- Guarantees the terminal state is cleanly restored regardless of child exit code or fatal signals (Constitution V, SC-007).

### Alternatives Considered
- *Running inside a pseudo-terminal (PTY) in the TUI*: Rejected. Added complexity, risk of terminal resize glitches, and unnecessary when suspending ratatui is clean, reliable, and standard.
- *Capturing command stdout/stderr*: Rejected per FR-018. kb does not store run history or capture logs.

---

## R3: Destructive Command Detection & Multi-Tier Confirmation

### Decision
- Maintain a compiled set of built-in regex patterns for destructive operations:
  - `rm\s+(-[a-zA-Z]*r[a-zA-Z]*\s+|--recursive)`
  - `kubectl\s+delete`
  - `terraform\s+(destroy|apply)`
  - `helm\s+uninstall`
  - `aws\s+\S+\s+(delete|terminate|remove)-`
  - `git\s+push\s+.*(-f|--force)`
  - `git\s+reset\s+--hard`
  - `(?i)\b(DROP\s+(DATABASE|TABLE|SCHEMA)|TRUNCATE\s+TABLE|DELETE\s+FROM\s+\S+(?!\s+WHERE))\b`
  - `>\s*/dev/(sd[a-z]|nvme|vd[a-z])`
- Allow user additions in configuration (`[run.destructive_patterns]`), but forbid removing built-in rules (FR-019).
- Two-tier confirmation:
  - **Standard**: Requires pressing `y` in TUI/CLI prompt; `--yes` non-interactively.
  - **Destructive**: Requires typing `run` in full in TUI/CLI prompt; `--yes --force` non-interactively.

### Rationale
- Implements Constitution Principle VII ("Never surprising") and fulfills SC-002 across hostile commands.

### Alternatives Considered
- *Single-key confirmation for all commands*: Rejected. Accidental double-tap could trigger catastrophic commands like `rm -rf` or `terraform destroy`.

---

## R4: Secret Variable Isolation in Commands (`KB_SECRET_<NAME>`)

### Decision
- When generating a `RunPlan` for a `cmd` entry:
  - For each `secret` variable (e.g. `token`), compute an uppercase environment variable name `KB_SECRET_TOKEN` (variable names in M1 are `[a-z][a-z0-9_]*`).
  - Pass the secret value via child process environment (`cmd.env("KB_SECRET_TOKEN", secret_val)`).
  - Substitute the reference `"$KB_SECRET_TOKEN"` into the command string.
  - In confirmation dialogues and CLI output, display only the variable name (e.g. `env: KB_SECRET_TOKEN=[hidden]`), never the value.
  - Value is scrubbed/dropped immediately after child launch.

### Rationale
- Secrets never appear in command-line arguments (preventing visibility via `ps aux`, shell history, or process listings).
- Adheres strictly to Constitution Principle VI and satisfies SC-003.

### Alternatives Considered
- *Piping secret via stdin*: Rejected. Many commands accept credentials only via environment variables or flags, and standard input may be needed for the command payload.
- *Temporary files with chmod 600*: Rejected. Disk persistence risks credential leakage and requires cleanup handlers.

---

## R5: Local Git Clone Discovery, Normalization & Caching

### Decision
- Match `code.repo` (`owner/name`) to a local clone in order:
  1. Explicit config mappings: `[code.repos] "owner/name" = "~/repos/path"`.
  2. One-level deep scan of directories in `code.search_paths`:
     - Inspect subdirectories for `.git`.
     - Read remote URL via `git remote get-url origin` or `.git/config`.
     - Normalize SSH (`git@github.com:owner/name.git`) and HTTPS (`https://github.com/owner/name.git`) to canonical `host/owner/name`.
- Cache mappings in `.kb/clones.json` with metadata (`path`, `normalized_remote`, `tracking_branch`, `mtime`).
- Invalidate cache entry if the path ceases to exist or `.git` directory mtime changes.

### Rationale
- Fast lookup during startup and code navigation. Scanning `search_paths` on every action would introduce noticeable filesystem latency.
- Disposable cache lives in `.kb/`, conforming to Constitution Principle II.

### Alternatives Considered
- *Recursive directory walking*: Rejected. Recursively searching `~/code` can scan millions of files (e.g. `node_modules`, `target`), violating startup latency budgets.
- *Calling GitHub API for clone paths*: Rejected. Constitution Principle III forbids remote hosting APIs.

---

## R6: Web Permalinks & Editor URL Protocols

### Decision
- **Web Permalinks**:
  - Pinned commit SHA is always used (never mutable branch names).
  - GitHub: `https://github.com/{owner}/{repo}/blob/{sha}/{path}#L{line}` (or `#L{line}-L{end_line}`).
  - GitLab: `https://gitlab.com/{owner}/{repo}/-/blob/{sha}/{path}#L{line}` (or `#L{line}-{end_line}`).
  - Bitbucket: `https://bitbucket.org/{owner}/{repo}/src/{sha}/{path}#lines-{line}` (or `#lines-{line}:{end_line}`).
  - Custom: Configurable template `code.web_url` (with `{repo}`, `{ref}`, `{path}`, `{line}`, `{end_line}`).
- **Editor Schemes**:
  - VS Code: `vscode://file/{abs_path}:{line}`.
  - IntelliJ IDEA: `idea://open?file={abs_path}&line={line}`.
  - Opening executes through `Platform::open_url`. Over SSH without display, editor schemes are disabled with a diagnostic message.
- **Default Target**: If `code.default_open` is not configured and `--with` is not supplied, default to `web` (clarified in Session 2026-10-09).

### Rationale
- Commit-pinned web URLs guarantee permanence and shareability.
- URL schemes for VS Code and IntelliJ are standard across macOS and Linux desktop environments.

### Alternatives Considered
- *Invoking `code` and `idea` CLI binaries*: Rejected. CLI binaries are often not in `$PATH` (e.g., standard IntelliJ installs on macOS do not symlink `idea`), whereas OS URL schemes work out of the box.

---

## R7: Git Diff Mapping for Drift Detection & Symbol Fallback

### Decision
- Drift is calculated between pinned commit SHA $P$ and tracking branch tip $T$:
  ```bash
  git diff -M -U0 P T -- {path}
  ```
  - Parse hunk headers: `@@ -old_start,old_count +new_start,new_count @@`.
  - For pinned range $[L_{start}, L_{end}]$:
    - If a hunk modifies/overlaps $[L_{start}, L_{end}]$, mark as `changed`.
    - If hunks precede $L_{start}$ without overlap, adjust line numbers:
      $\Delta = \sum (new\_count - old\_count)$.
      If no hunks overlap and line shifted, mark as `moved` (new line $L_{start} + \Delta$).
    - If file was renamed (`rename from ...` / `rename to ...`), map line numbers and mark as `renamed`.
    - If file was deleted or lines deleted completely, mark as `gone`.
    - If file and line unchanged, mark as `current`.
- **Symbol Fallback**: If status would be `gone` or `changed`, and `code.symbol` is defined:
  - Search tracking branch tree: `git grep -n -F <symbol> T -- <path>`.
  - If located, propose new line and mark confidence as `symbol-matched` (vs `diff-mapped`).
- **Tracking Branch Priority**: Entry `code.branch` -> config mapping -> `origin/HEAD` -> `origin/main` -> `origin/master`.

### Rationale
- Using Git's native `-U0` diff algorithm is instantaneous (<50ms even on 100k commits) and eliminates external diff engine dependencies.
- Handles rename detection natively via `-M`.

### Alternatives Considered
- *Cloning libgit2 / git2-rs*: Rejected. Heavy C dependency, slow compilation, and violates Constitution Principle IX. Using `std::process::Command` with `git` binary is lightweight, robust, and fast.

---

## R8: Format-Preserving Re-pinning & Concurrency Safety

### Decision
- Re-pinning updates the entry file on disk:
  - Updates `ref = "<tracking_branch_sha>"`, `path = "<mapped_path>"`, `line = <mapped_line>`, optional `end_line = <mapped_end_line>`, and `updated = "<today>"`.
  - Uses M1's `set_field` format-preserving serializer to preserve all indentation, frontmatter comments, and body text byte-for-byte.
  - Verifies file mtime / content hash against when it was read. If modified externally, abort with concurrency conflict error (M6 FR-020).

### Rationale
- Satisfies SC-005: only the target frontmatter fields and `updated` change byte-for-byte.

### Alternatives Considered
- *Full frontmatter re-serialization*: Rejected because it discards user comments, reorganizes keys, and clobbers formatting.

---

## R9: Custom Actions Configuration & Pipeline Model

### Decision
- Defined in `config.toml` per entry type:
  ```toml
  [[actions.sql]]
  name = "psql"
  key = "p"
  command = "psql -d {{database}}"
  input = "stdin"  # optional, defaults to "stdin"; or "arg"
  ```
- Command string supports `{{field}}` interpolations referring to entry YAML fields (shell-escaped).
- If `input` is omitted, default to `stdin` (clarified in Session 2026-10-09).
  - `stdin`: Pipes resolved entry payload into command's standard input.
  - `arg`: Appends resolved entry payload as final argument.
- Key conflict check: If custom action `key` clashes with a built-in action key for that entry type, report warning and disable custom action.

### Rationale
- Extends kb to any CLI tool (`psql`, `sqlite3`, `claude`, `curl`) without hardcoding third-party integrations into the core.
- Conforms to Constitution Principle IX ("Simplicity first").

### Alternatives Considered
- *Dedicated SQL runner or database adapter plugin*: Rejected. Constitution Principle IV & IX require terminal-only simplicity without bespoke plugins.

---

## R10: TUI Integration: Action Menu (Alt-A), Code View (Screen 3) & Keymap

### Decision
- **Action Menu (Alt-A / F2)**:
  - Renders as a floating overlay modal on top of the entry list.
  - Lists all available actions (built-in + custom) with single-letter shortcuts and descriptive labels.
  - Pressing a letter runs the action; `Esc` dismisses the modal.
- **Code View (Screen 3)**:
  - Dedicated full-screen view for `code` entries.
  - Header displays title, repo, path:line, pinned ref.
  - Code panel displays preview lines (`git show <ref>:<path>`) with syntax highlighting via `syntect`.
  - Drift status badge prominently shown (`current`, `moved`, `changed`, `renamed`, `gone`, `unknown`).
  - Shortcut bar supports:
    - `Enter`: Open default target (`code.default_open`, defaults to `web`)
    - `g`: Open web permalink
    - `v`: Open in VS Code
    - `i`: Open in IntelliJ
    - `y`: Copy `path:line`
    - `Y`: Copy web permalink
    - `u`: Re-pin entry to current tracking branch
    - `F`: Git fetch and re-check drift
    - `Ctrl-E`: Edit entry file in editor
    - `Esc`: Return to main list
- **Enter Key Configuration (`tui.enter = "primary"`)**:
  - When set to `primary`, Enter triggers the type's primary open action:
    - `link`: Open in browser
    - `sumo`: Open Sumo deep link
    - `cw`: Open CloudWatch deep link
    - `code`: Open Code View (Screen 3)
    - `cmd`: Copies payload (Enter NEVER runs a command; FR-040)
    - `sql` / `prompt`: Copies payload

### Rationale
- Screen 3 provides rich context before opening code in desktop tools.
- Preserves the safety invariant that Enter never runs arbitrary commands.
