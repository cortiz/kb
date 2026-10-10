# Quickstart & Verification Guide: Actions and Code Links

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

This guide provides end-to-end scenarios to verify the functionality of Milestone 7 (Actions and Code Links) using the CLI and TUI.

---

## Prerequisites

1. Set up a temporary test KB environment:
   ```bash
   export KB_DIR=$(mktemp -d)
   cargo run -p kb -- init --examples
   ```
2. Configure Sumo and AWS settings in `~/.config/kb/config.toml` (or `$KB_DIR/config.toml`):
   ```toml
   [sumo]
   base_url = "https://service.us2.sumologic.com"

   [aws]
   default_region = "us-east-1"

   [code]
   default_open = "web"
   ```

---

## Scenario 1: Open Links and Console Deep Links (US1)

### 1.1 Open a Standard Link
```bash
# Verify opening a link entry directly in the browser
cargo run -p kb -- open link/auth0-dashboard
```
*Expected*: System default browser opens the resolved URL and a use record is recorded in `.kb/state.json`.

### 1.2 Open a Parameterized Sumo Logic Query
```bash
# Open Sumo query passing variables non-interactively
cargo run -p kb -- open sumo/5xx-authorizer --var env=prod --var minutes=30 -y
```
*Expected*: Browser opens Sumo Logic search URL targeting the user's base deployment with the query and 30-minute time window preloaded.

### 1.3 Open a CloudWatch Insights Query
```bash
cargo run -p kb -- open cw/authorizer-p99 --var fn=authorizer --var ms=500 -y
```
*Expected*: Browser opens AWS CloudWatch Logs Insights console in `us-east-1` with log groups and query prefilled.

---

## Scenario 2: Safe Command Execution (US2)

### 2.1 Standard Command Run with Confirmation
```bash
# Run command interactively
cargo run -p kb -- run cmd/tail-logs --var env=prod
```
*Expected*:
1. kb displays the resolved command, shell (`sh`), and working directory.
2. Prompts: `Run this command? [y/N]: `.
3. Typing `y` runs the command in the foreground.
4. Exits with the command's exit code.

### 2.2 Destructive Command Guard
Create a test destructive command entry:
```bash
cat << 'EOF' > "$KB_DIR/cmd/purge-data.md"
---
kb_format: 1
title: Purge test data
type: cmd
tags: [admin]
---
```sh
rm -rf /tmp/kb-test-purge
```
EOF
cargo run -p kb -- reindex
```
Now attempt to run it:
```bash
cargo run -p kb -- run cmd/purge-data
```
*Expected*:
1. kb shows a warning: `Matches destructive pattern 'rm -rf'`.
2. Prompts: `Type 'run' to execute: `.
3. Typing anything other than `run` aborts execution.
4. When testing `--yes` without `--force`: exits with error explaining `--force` is required for destructive commands.

### 2.3 Secret Environment Isolation
```bash
cat << 'EOF' > "$KB_DIR/cmd/call-api.md"
---
kb_format: 1
title: Call Authenticated API
type: cmd
vars:
  token:
    kind: secret
---
```sh
curl -H "Authorization: Bearer $token" https://example.com/api
```
EOF
cargo run -p kb -- reindex
```
Run with secret provided via environment:
```bash
SUMO_TOKEN="super-secret-pass" cargo run -p kb -- run cmd/call-api -y
```
*Expected*:
- In confirmation and process arguments, the secret is mapped to `$KB_SECRET_TOKEN`.
- The secret string `super-secret-pass` never appears in `ps aux` or command strings.

---

## Scenario 3: Code Links & Navigation (US3)

### 3.1 Display Code View (Screen 3)
```bash
cargo run -p kb -- code show code/authorizer-handle-request
```
*Expected*:
- Displays repo identity, path with line, pinned commit SHA.
- Code preview renders surrounding lines with the target line marked and syntax highlighted.
- Drift status is reported.

### 3.2 Open Web Permalink
```bash
cargo run -p kb -- open code/authorizer-handle-request --with web
```
*Expected*: Opens browser at `https://github.com/<owner>/<repo>/blob/<sha>/<path>#L<line>`.

---

## Scenario 4: Drift Detection and Re-pinning (US4)

### 4.1 Check Repository Drift
```bash
cargo run -p kb -- code check
```
*Expected*:
- Inspects code entries and prints one line per entry with status: `current`, `moved`, `changed`, `renamed`, `gone`, or `unknown`.
- Exits 0 if all entries are current or moved; exits 1 if any entry is changed, renamed, or gone.

### 4.2 Re-pin a Moved Entry
```bash
cargo run -p kb -- code repin code/authorizer-handle-request -y
```
*Expected*:
- Updates the entry file on disk.
- Byte-for-byte check: only `ref`, `path`, `line`, `end_line`, and `updated` frontmatter values change.
- Re-indexes the entry.

---

## Scenario 5: Action Menu and Custom Actions (US5, US6)

### 5.1 Inspect Available Actions via CLI
```bash
cargo run -p kb -- actions cw/authorizer-p99 --json
```
*Expected*: JSON payload listing all available built-in actions and configured custom actions with single-letter keys.

### 5.2 Execute Custom Action
Add custom action to config:
```toml
[[actions.sql]]
name = "count-rows"
key = "c"
command = "wc -l"
input = "stdin"
```
Run custom action:
```bash
cargo run -p kb -- run sql/active-users --action count-rows -y
```
*Expected*: The SQL payload is piped to `wc -l` via stdin and output is displayed.
