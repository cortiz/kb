# CLI Contracts: Runbook Run Mode

This document details the command line interface additions for runbook execution.

---

## 1. `kb runbook run <id|query> [--new]`

Launches the TUI directly into run mode for the specified runbook.

### Arguments & Flags
- `<id|query>`: Target runbook entry ID (e.g. `runbook/deploy-authorizer`) or unique fuzzy query.
- `--new`: Bypass the active runs picker and immediately open the variable form to start a fresh run.

### Behavior
- If the runbook has active runs and `--new` is not specified, opens the TUI run picker listing active runs and "Start a new run".
- If no active runs exist or `--new` is passed, prompts for runbook variables and enters the run screen.
- If target is not a runbook, exits `1` with error message.

---

## 2. `kb runbook runs [<runbook>] [--all] [--json] [--prune [--older-than <days>]]`

Lists active runs or manages existing run files.

### Arguments & Flags
- `[<runbook>]`: Optional filter by runbook ID or query.
- `--all`: Include finished and abandoned runs (defaults to active only).
- `--json`: Output structured JSON.
- `--prune`: Delete finished and abandoned run files (never active runs).
- `--older-than <days>`: Used with `--prune` to restrict deletion to runs finished or abandoned more than N days ago.

### Output Shapes

#### Human-readable Output (Default)
```text
ACTIVE RUNS (2)

ID                                    RUNBOOK                       NAME                                                  UPDATED
deploy-authorizer-to-prod-1760549400  runbook/deploy-authorizer     Deploy authorizer to prod (stage) · 2/4 done          2m ago
deploy-authorizer-to-prod-1760549100  runbook/deploy-authorizer     Deploy authorizer to prod (prod) · 0/4 done           12m ago
```

#### JSON Output (`--json`)
```json
[
  {
    "id": "deploy-authorizer-to-prod-1760549400",
    "runbook_id": "runbook/deploy-authorizer",
    "display_name": "Deploy authorizer to prod (stage, v2.14.0) · Oct 15 17:30",
    "state": "active",
    "created_at": 1760549400,
    "updated_at": 1760549520,
    "finished_at": null,
    "steps_total": 4,
    "steps_done": 2,
    "steps_skipped": 0,
    "steps_failed": 0,
    "is_locked": false
  }
]
```

---

## 3. `kb runbook abandon <run-id> [--yes]`

Marks an active run as abandoned without opening the TUI.

### Arguments & Flags
- `<run-id>`: Full run identifier (e.g. `deploy-authorizer-to-prod-1760549400`).
- `-y`, `--yes`: Confirm abandonment non-interactively.

### Behavior
- Refuses with exit code 1 if run is locked by a living process.
- Sets run `state: "abandoned"`, sets `finished_at` to current timestamp, and writes atomically.
