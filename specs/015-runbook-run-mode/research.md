# Technical Research: Runbook Run Mode

## 1. Run Lifecycle & Step State Machine

### 1.1 Run Lifecycle States
A run operates across three primary states:
- `active`: The run is in progress. May be resumed, stepped through, or edited.
- `finished`: All non-optional steps are `done` or `skipped`, and the user explicitly executed `finish-run` (`F`). Kept for historical records (R3).
- `abandoned`: The user abandoned the run (`A` in TUI or `kb runbook abandon`). Kept until pruned.

### 1.2 Step Status Transitions

```text
               ┌───────────┐
               │  pending  │
               └─────┬─────┘
                     │
       ┌─────────────┼─────────────┬─────────────┐
       │ (action)    │ (copy 'y')  │ (skip 's')  │ (run cmd)
       ▼             ▼             ▼             ▼
  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐
  │ opened  │   │ copied  │   │ skipped │   │ running │
  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘
       │             │             │ (reset 'u') │
       │             │             └────────►    │
       │             │                           │ (exit 0)
       │             │                           ▼
       │             │ (mark 'd'            ┌─────────┐
       └─────────────┼─────────────────────►│  done   │
                     │  or auto_done)       └─────────┘
                     │                           ▲
                     │ (mark 'd')                │ (retry exit 0)
                     │                           │
                     │                      ┌────┴────┐
                     └─────────────────────►│ failed  │◄─── (exit != 0)
                                            └─────────┘
                                                 ▲
                                                 │ (interrupted)
                                            ┌────┴────────┐
                                            │ interrupted │◄─── (SIGINT/Ctrl-C or crash)
                                            └─────────────┘
```

- `pending`: Step has not been acted on. Status icon: `○`.
- `opened`: Step entry was opened in console, code view, link, or note preview. Status icon: `◐`.
- `copied`: Step query or prompt was copied to clipboard. Status icon: `◐`.
- `running`: `cmd` step process is executing in foreground. Status icon: `▶`.
- `done`: Step completed successfully. Status icon: `●`.
- `skipped`: Step intentionally bypassed (`s`). Status icon: `⊘`.
- `failed`: Command exited with non-zero code. Exit code and duration recorded. Status icon: `✕`.
- `interrupted`: Command was killed, SIGINT (Ctrl-C) received, or terminal closed during execution. Status icon: `!`.
- `removed`: Step was removed from the runbook definition file after the run began.

---

## 2. Process Locking & Concurrency (`RunLock`)

### 2.1 Problem
Multiple terminal windows or separate `kb` instances could attempt to modify the same run concurrently, resulting in race conditions or lost progress.

### 2.2 Solution
- Alongside `.kb/runs/<run-id>.json`, maintain a sidecar `.kb/runs/<run-id>.lock`.
- Lock file content:
  ```json
  {
    "pid": 48291,
    "started_at": 1730000000,
    "hostname": "carlos-mbp"
  }
  ```
- **Stale Lock Detection**:
  - When acquiring lock, read existing PID.
  - On Unix, call `kill(pid, 0)`:
    - If error `ESRCH` (No such process), the process has died without removing the lock file. The stale lock is safely broken and reacquired.
    - If return code 0 or `EPERM`, the process is alive. The run opens in **read-only mode** with a warning notice: `Run locked by PID 48291 (read-only)`.
- **Lock Release**: Cleanly removed via `Drop` guard on exit, or on explicit close.

---

## 3. Terminal Suspension & Step Execution

### 3.1 Command Execution Flow
When Enter is pressed on a `cmd` step:
1. Validate command with M7 safety checks (destructive command pattern matching, agent review status).
2. If confirmation required, prompt inline in TUI.
3. Write run state with step status `running`.
4. Drop TUI raw mode, leave alternate screen buffer (`DisableMouseCapture`, `LeaveAlternateScreen`), flush terminal.
5. Spawn child process in foreground with standard pipes (`stdin`, `stdout`, `stderr` inherited from parent terminal).
6. Capture exit status and wall-clock duration:
   - Exit 0: If `runbook.auto_done` is true, check `confirm: true`. If confirm is required, prompt user with fixed `Confirm '<title>' is complete? [y/N]`. If confirmed or confirm not required, mark `done`.
   - Exit > 128 / SIGINT (Ctrl-C): Mark status `interrupted`.
   - Exit non-zero: Mark status `failed` with exit code.
7. Re-enter alternate screen buffer, enable raw mode and mouse capture, redraw run screen.
8. Persist updated run state atomically.

### 3.2 Non-Command Step Actions
- `sumo`, `cw`: Call platform opener (`open_url`) with constructed query URL and time parameters. Mark step `opened`.
- `link`: Call platform opener (`open_url`). Mark step `opened`.
- `code`: Switch TUI state to M7 code view modal. On Esc, return to run view.
- `note`: Switch TUI state to full note preview modal. On Esc, return to run view.
- `sql`, `prompt`: Copy resolved text to clipboard via platform clipboard tool. Mark step `copied`.
- Manual (`do`): Enter prompts for confirmation if `confirm: true`, then marks `done`.

---

## 4. Variable Resolution Hierarchy & Secret Handling

### 4.1 Resolution Hierarchy
Variables needed for a step's payload or URL are resolved in order:
1. **Step-only Variables**: Variables declared on the referenced entry but not declared on the runbook. Asked on first step execution; remembered in `step_state.vars` for the lifetime of this run.
2. **Step Bindings**: Explicit overrides in runbook step frontmatter (`vars: { key: value }` or `vars: { key: "{{runbook_var}}" }`).
3. **Run Variables**: Global runbook variables asked once at run start, remembered in `run.vars`.

### 4.2 Constitution VI Secret Shielding
- Run files serialized to `.kb/runs/<run-id>.json` **MUST NEVER** contain secret values.
- For secret variables:
  - If source is an environment variable (e.g. `AWS_SECRET_ACCESS_KEY`), the variable is tagged as secret and its name recorded, but value is not serialized.
  - On run resume:
    - Attempt to read from current `std::env::var`.
    - If present, use silently.
    - If missing, prompt user via variable form before executing the step.
- An automated test fixture (`run_secrets_scan.rs`) scans all `.kb/runs/` files after runs with dummy API keys to enforce zero secret persistence.

---

## 5. Runbook Drift & Entry Evolution Detection

### 5.1 Runbook Definition Changed
- The run stores `runbook_content_hash` at start and at last resume.
- On resume, if the runbook on disk has a different content hash:
  - Display top banner: `Runbook modified since run started`.
  - Reconcile steps by `step.id`:
    - Steps in both runbook and run preserve their existing status and history.
    - New steps in runbook appear as `pending`.
    - Steps removed from runbook are flagged `removed` and grouped in a collapsed section.
  - If new variables were added to the runbook, prompt only for the new variables.

### 5.2 Referenced Entry Changed
- When a step is acted on, record `entry_content_hash`.
- If the entry has been edited since:
  - Display indicator: `changed since run start`.
  - If modified by an AI agent and not yet reviewed, display `agent` badge and enforce `run` confirmation.
