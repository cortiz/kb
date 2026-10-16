# Quickstart: Runbook Run Mode Validation Guide

This guide walks through end-to-end scenarios to validate the execution of runbooks, multi-run concurrency, crash resilience, secret protection, and drift detection.

---

## Prerequisites

1. Rust toolchain (Rust 1.88+)
2. Build `kb` binary:
   ```bash
   cargo build --bin kb
   ```
3. Initialize temporary KB:
   ```bash
   export KB_DIR=$(mktemp -d)
   kb init "$KB_DIR" --no-examples
   ```

---

## Scenario 1: Start a Run & Step Execution (US1)

### 1.1 Setup Fixtures
```bash
kb add cmd/deploy --title "Deploy service" --var "env:enum[stage,prod]=stage" --body "echo 'Deploying to {{env}}' && exit 0"
kb add cw/metrics --title "Check error metrics" --var "env:enum[stage,prod]=stage" --body "https://console.aws.amazon.com/cw/home?env={{env}}"
kb runbook new "Service Deployment" cw/metrics cmd/deploy --yes
```

### 1.2 Start Run via CLI
```bash
kb runbook run "Service Deployment" --new
```
1. Variable prompt appears requesting `env` (defaults to `stage`). Press Enter.
2. Run view opens with two steps on the left pane:
   - `1. ○ [cw] Check error metrics` (focused)
   - `2. ○ [cmd] Deploy service`
3. Press Enter on step 1: browser/console opens the CloudWatch URL; step status becomes `◐ opened`.
4. Press `d`: step 1 status becomes `● done`; selection moves to step 2.
5. Press Enter on step 2: command confirmation prompt appears; press `y`.
6. Terminal suspends, prints `Deploying to stage`, and returns to TUI.
7. Step 2 status becomes `● done` (due to `runbook.auto_done = true`).
8. Run displays `complete`. Press `F` to finish the run.

---

## Scenario 2: Command Failure & Interruption (US2)

### 2.1 Add Failing Command
```bash
kb add cmd/failing-check --title "Check database replica" --body "echo 'Replica down' && exit 2"
kb runbook new "Health Verification" cmd/failing-check --yes
```

### 2.2 Execute
```bash
kb runbook run "Health Verification" --new
```
1. Press Enter, confirm execution.
2. Command executes and exits with code 2.
3. Step status updates to `✕ failed (exit 2)`.
4. Run is blocked from auto-finishing. Press `A` and confirm with `y` to abandon run.

---

## Scenario 3: Multiple Concurrent Runs & Resume (US4)

1. Start run for `stage`:
   ```bash
   kb runbook run "Service Deployment" --new
   ```
   Answer `env=stage`. Complete step 1 (`d`). Press `Esc` to return to list.
2. Start run for `prod`:
   ```bash
   kb runbook run "Service Deployment" --new
   ```
   Answer `env=prod`. Press `Esc`.
3. Check active runs list:
   ```bash
   kb runbook runs --json
   ```
   Output shows 2 active runs with different variables and independent step states.
4. Re-open runbook:
   ```bash
   kb runbook run "Service Deployment"
   ```
   Picker displays both active runs:
   - `Service Deployment (stage) · 1/2 done`
   - `Service Deployment (prod) · 0/2 done`
   - `Start a new run`
5. Select the `stage` run: resumes exactly at step 2 with step 1 still marked `done`.

---

## Scenario 4: Secret Shielding Verification (Constitution VI)

1. Start run with secret parameter:
   ```bash
   export DB_PASSWORD="super-secret-password-xyz"
   kb add cmd/auth-test --title "Auth Test" --var "token:secret" --body "curl -H 'Authorization: Bearer {{token}}' https://api.internal"
   kb runbook new "Auth Runbook" cmd/auth-test --yes
   kb runbook run "Auth Runbook" --new
   ```
2. Inspect run files in `.kb/runs/*.json`:
   ```bash
   grep -rn "super-secret-password-xyz" "$KB_DIR/.kb/runs/"
   ```
   Must return 0 matches. Secrets are never persisted to disk.
