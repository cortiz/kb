# Quickstart: Runbook Format Validation Guide

This guide outlines end-to-end scenarios to validate the implementation of `runbook` entries, variable bindings, reference integrity, search filters, and checklist generation.

---

## Prerequisites

1. Working Rust toolchain (Rust 1.88+).
2. Clean temporary knowledge base:
   ```bash
   export KB_DIR=$(mktemp -d)
   git init "$KB_DIR"
   ```
3. Built `kb` binary:
   ```bash
   cargo build --bin kb
   ```

---

## Scenario 1: Create a Runbook with `kb runbook new`

### 1.1 Create Test Entries
```bash
kb add cmd/deploy-authorizer --title "Deploy authorizer" --tag deploy \
  --var "env:enum[stage,prod]=stage" --var "version:text" \
  --body "deploy --env {{env}} --tag {{version}}"

kb add cw/authorizer-latency --title "Watch latency" --tag metrics \
  --var "env:enum[stage,prod]=stage" \
  --body "aws cloudwatch get-metric-data --namespace auth --dimension Env={{env}}"
```

### 1.2 Create Runbook
```bash
kb runbook new "Deploy authorizer to prod" cmd/deploy-authorizer cw/authorizer-latency --yes
```

### 1.3 Validate Structure & Variable Lifting
- Run `kb show runbook/deploy-authorizer-to-prod`:
  - Runbook title, tags, and lifted variables (`env`) are displayed.
  - Numbered steps (1. Deploy authorizer, 2. Watch latency) with type badges (`cmd`, `cw`) are shown.
- Run `kb lint`:
  - Passes with 0 errors.

---

## Scenario 2: Search & Filter Queries

### 2.1 Search by Terms
```bash
kb search "deploy authorizer"
```
Runbook appears with the `runbook` type badge.

### 2.2 Filter by `runbook:<id>`
```bash
kb search "runbook:runbook/deploy-authorizer-to-prod"
```
Returns only `cmd/deploy-authorizer` and `cw/authorizer-latency`, in step order.

### 2.3 Filter by `used-by:runbook`
```bash
kb search "used-by:runbook"
```
Returns all entries referenced by any runbook.

---

## Scenario 3: Reference Integrity across Renames & Deletes

### 3.1 Rename an Entry
```bash
kb mv cmd/deploy-authorizer cmd/authorizer/deploy
```
- Outputs message indicating `runbook/deploy-authorizer-to-prod` was updated.
- Verify git commit: single commit containing both the renamed entry and the modified runbook.
- Verify runbook content: `entry: cmd/authorizer/deploy` replaces the old ID byte-for-byte.

### 3.2 Attempt Safe Delete
```bash
kb rm cmd/authorizer/deploy
```
- Refuses to delete: exits code 1, listing `runbook/deploy-authorizer-to-prod` step 1.
- With `--force`: deletes entry. `kb lint` reports `KB072-step-missing-entry`.

---

## Scenario 4: Export to Checklist

### 4.1 Generate Checklist
```bash
kb show runbook/deploy-authorizer-to-prod --checklist
```
Outputs Markdown checklist:
```markdown
# Deploy authorizer to prod

- [ ] 1. Deploy authorizer (`cmd/authorizer/deploy`)
- [ ] 2. Watch latency (`cw/authorizer-latency`)
```

### 4.2 Resolve Checklist with Variables
```bash
kb show runbook/deploy-authorizer-to-prod --checklist --resolve
```
Prompts for `env` and `version`, then prints resolved checklist with code blocks containing resolved commands.
