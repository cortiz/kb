# Contract: CLI Interface (Runbook Format)

This document defines the CLI commands, flags, and behavioral contracts for runbooks.

---

## 1. Creating Runbooks: `kb runbook new`

### Synopsis
```bash
kb runbook new <TITLE> <ENTRY-ID...> [--path PATH] [--tag TAG]... [--yes] [--json]
```

### Behavior:
1. Validates that every provided `ENTRY-ID` exists and is not a runbook.
2. Extracts variables from all step entries:
   - If a variable name appears in 2 or more step entries with compatible kinds, it is lifted to the runbook `vars:`.
   - For `enum` variables, the lifted options are the intersection of the steps' options.
   - A default value is set on the runbook variable only if all steps share the exact same default value.
3. Constructs steps in the order entries were specified on the command line:
   - `id`: derived from entry ID slug.
   - `title`: taken from entry title.
   - `entry`: entry ID.
4. Generates frontmatter and saves to `runbook/<slug-of-title>.md` (or `--path` if provided).
5. Opens in editor for user review unless `--yes` is specified.
6. With `--json`, outputs JSON description of the created runbook.

---

## 2. Displaying Runbooks: `kb show` & `kb use`

### Synopsis
```bash
kb show <RUNBOOK-ID> [--checklist] [--resolve] [--json]
kb use <RUNBOOK-ID>
```

### Behavior:
1. **`kb show <runbook>`**:
   - Prints terminal-formatted preview of the runbook: metadata, variables, and numbered steps with entry type badges, titles, confirm/optional markers, and bindings.
2. **`kb show <runbook> --checklist`**:
   - Outputs Markdown checklist:
     ```markdown
     # Deploy authorizer to prod

     - [ ] 1. Check the current error rate (`sumo/5xx-by-authorizer`)
     - [ ] 2. Deploy {{version}} to {{env}} (`cmd/deploy-authorizer`)
     - [ ] 3. Watch p99 latency for 10 minutes (`cw/authorizer-p99-latency`)
     - [ ] 4. Roll back if p99 > 800 ms (optional)
       Only if p99 stayed above 800 ms for 5 minutes.
       Post in #auth-oncall before rolling back.
     ```
3. **`kb show <runbook> --checklist --resolve`**:
   - Prompts once for runbook variables.
   - Resolves step titles.
   - Inlines each referenced entry's resolved payload inside a fenced code block under that step.
   - Secret variables appear as `$ENV_VAR` or `{{name}}`, never as raw secret values.
4. **`kb use <runbook>`**:
   - Executes identical logic to `kb show <runbook> --checklist --resolve`.
   - Copies generated checklist to system clipboard.
   - Records a usage increment for the runbook.

---

## 3. Query Filters: `runbook:<id>` and `used-by:runbook`

### Synopsis
```bash
kb search "runbook:deploy/authorizer"
kb search "used-by:runbook"
kb [list] --filter runbook:deploy/authorizer
```

### Behavior:
1. `runbook:<id>`: Lists only the entries referenced by that specific runbook, in exact step order.
2. `used-by:runbook`: Lists all entries that are referenced by at least one runbook.
3. Both filters are supported across CLI `kb search`, TUI search bar, dynamic shell completion, and MCP `search`.

---

## 4. Renames & Deletions: `kb mv` & `kb rm`

### Rename (`kb mv <old> <new>`)
1. Atomically updates all `entry: <old>` occurrences in referencing runbooks to `entry: <new>`.
2. Format-preserving: retains all comments and indentation.
3. Auto-commit: commits rename and all updated runbooks in a single commit, listing updated runbooks in the commit message body.

### Delete (`kb rm <id>`)
1. If `<id>` is referenced by any runbooks and `--force` is not passed:
   - Exits 1 with error listing referencing runbooks and steps.
2. With `--force`: deletes the entry. Steps become broken references detected by `kb lint`.
