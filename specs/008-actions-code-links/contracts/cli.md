# Contract: CLI Commands and Schemas

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](../spec.md)

This contract defines the CLI interface, arguments, outputs, JSON formats, and exit codes for actions and code links.

---

## 1. `kb open`

Opens an entry's primary link, console deep link, or code target.

### Usage
```bash
kb open <id|query> [--with <web|vscode|idea|console>] [--var <name=value>...] [-y|--yes]
```

### Options
- `--with <target>`: Override default open target. For `code`: `web`, `vscode`, `idea`. For `sumo`/`cw`: `console`.
- `--var <name=value>`: Supply variable values (repeatable).
- `-y`, `--yes`: Non-interactive; accepts defaults for unspecified variables.

### Behavior
- Resolves `<id|query>` via M4/M5 search logic.
- If variables are needed, prompts interactively (or consumes `--var`).
- Records a use (M2 FR-017).
- If over SSH without display: prints URL and copies to clipboard instead of opening.
- For `code` entries: if `code.default_open` is unset and `--with` is omitted, defaults to `web`.

---

## 2. `kb run`

Safely executes a `cmd` entry or configured custom action.

### Usage
```bash
kb run <id|query> [--var <name=value>...] [-y|--yes] [--force] [--action <name>]
```

### Options
- `--action <name>`: Execute named custom action instead of default `run`.
- `--var <name=value>`: Supply variable values.
- `-y`, `--yes`: Skip confirmation for standard commands; prints resolved command to stderr before running.
- `--force`: Required in addition to `--yes` when running destructive commands non-interactively.

### Behavior & Confirmation
- Interactive prompt:
  - Standard command: shows command, cwd, shell, secret variable names; prompts `Run this command? [y/N]: `.
  - Destructive command: shows warning and pattern matched; prompts `Type 'run' to execute: `.
- Spawns child process in foreground with terminal attached.
- Exits with child process exit code, or `128 + signal` if killed by signal.

---

## 3. `kb actions`

Lists available actions for an entry.

### Usage
```bash
kb actions <id|query> [--json]
```

### JSON Output Schema
```json
{
  "id": "cw/authorizer-p99-latency",
  "type": "cw",
  "title": "Authorizer p99 Latency",
  "primary_action": "open-cw",
  "actions": [
    {
      "id": "open-cw",
      "key": "o",
      "name": "Open in CloudWatch",
      "label": "Open CloudWatch Logs Insights console with preloaded query",
      "kind": "OpenUrl",
      "is_primary": true,
      "needs_vars": true,
      "needs_confirm": false,
      "available": true
    },
    {
      "id": "copy",
      "key": "c",
      "name": "Copy",
      "label": "Copy resolved query to clipboard",
      "kind": "CopyPayload",
      "is_primary": false,
      "needs_vars": true,
      "needs_confirm": false,
      "available": true
    }
  ]
}
```

---

## 4. `kb code check`

Inspects code links for drift against repository tracking branches.

### Usage
```bash
kb code check [ids...] [--fetch] [--json]
```

### Behavior
- Inspects specified entries, or all `code` entries in KB if no IDs provided.
- `--fetch`: Runs `git fetch` for local clones before checking drift. Without `--fetch`, no network calls are made.
- Prints one line per entry:
  ```text
  code/auth-handler: moved (now at line 151 on origin/main)
  code/user-service: current (origin/main)
  code/legacy-router: changed (+4 -2 on origin/main)
  ```
- Exits with `1` if any checked entry has status `changed`, `renamed`, or `gone`. Exits `0` if all are `current` or `moved`.

### JSON Output Schema
```json
[
  {
    "id": "code/auth-handler",
    "repo": "cortiz/kb",
    "status": "moved",
    "pinned_ref": "3a4b5c6",
    "pinned_path": "crates/kb/src/main.rs",
    "pinned_line": 147,
    "proposed_line": 151,
    "tracking_branch": "origin/main",
    "confidence": "DiffMapped"
  }
]
```

---

## 5. `kb code repin`

Updates code entries to point to the current tracking branch tip commit.

### Usage
```bash
kb code repin <id>... | --all [--fetch] [-y|--yes]
```

### Behavior
- Re-pins entries that have status `moved`, `changed`, or `renamed`.
- Skips entries with status `gone` or `unknown`, listing them in an error summary at the end.
- Asks for confirmation before updating files, unless `-y`/`--yes` is supplied.
- Performs format-preserving frontmatter update (`ref`, `path`, `line`, `end_line`, `updated`).
- Re-indexes updated entries.

---

## 6. `kb code show`

Displays the code snippet and current drift status for a code link.

### Usage
```bash
kb code show <id> [--json]
```

### Text Output
Prints title, repo, path:line, pinned ref, drift status badge, and syntax-highlighted code snippet with line numbers and highlighted target line.

---

## 7. Exit Codes Summary

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Entry not found, ambiguous query, action unavailable, or drift detected by `kb code check` |
| `2` | Usage error, configuration syntax error, or I/O failure |
| `130` | Operation cancelled by user (e.g. Esc or Ctrl-C during prompt) |
| *Other* | In `kb run`, returns the child process exit code (or `128 + signal`) |
