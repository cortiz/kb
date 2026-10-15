# Research & Architecture Decisions: Runbook Format

**Branch**: `011-runbook-format` | **Date**: 2026-10-15 | **Spec**: [specs/011-runbook-format/spec.md](spec.md)

---

## 1. Runbook Entry Format & Data Structures

### Decision: Dedicated `Runbook` Variant in `EntryType`
`EntryType::Runbook` will be added alongside `Command`, `Query`, `Link`, `Code`, and `Note`.
Like `Link`, `Code`, and `Note`, runbooks do not have an inline payload block in the frontmatter (`payload` field); the Markdown body represents free-form runbook notes/instructions.

### Frontmatter Schema:
```yaml
---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, lambda, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, description: "Release tag, e.g. v2.14.0" }
steps:
  - id: baseline
    title: Check the current error rate
    entry: sumo/5xx-by-authorizer
    vars: { minutes: 30 }
  - id: deploy
    title: Deploy {{version}} to {{env}}
    entry: cmd/deploy-authorizer
    confirm: true
  - id: watch
    title: Watch p99 latency for 10 minutes
    entry: cw/authorizer-p99-latency
  - id: rollback
    title: Roll back if p99 > 800 ms
    do: |
      Only if p99 stayed above 800 ms for 5 minutes.
      Post in #auth-oncall before rolling back.
    optional: true
---

Owner: platform security. Last rehearsed in stage on 2026-10-02.
```

### Slug & Implicit Step IDs:
If a step omits `id`, the canonical ID is derived by slugifying `title` (lowercased ASCII alphanumeric and hyphens, e.g., `check-the-current-error-rate`). Duplicate slugs within the same runbook append `-2`, `-3`, etc. Lint reports rule `KB084-implicit-step-id` (severity Info) to advise setting explicit IDs.

---

## 2. Variable Binding Table & Kind Compatibility

### Decision: Centralized Binding Evaluation in `kb-core`
Variable binding rules (FR-008, FR-009, FR-011) must be computed in a single shared function in `kb-core` used uniformly by `kb lint`, `kb show`, TUI preview, MCP `get`, and CLI `kb use`.

### Binding Rules:
1. **Implicit Binding by Name**: If a step's referenced entry declares variable `X`, and the runbook declares variable `X`, and step `vars` does not specify `X`, `X` is implicitly bound to runbook variable `X`.
2. **Explicit Binding**: Step `vars` maps entry variable name to either:
   - A literal value: e.g. `minutes: 30` or `env: stage`.
   - A runbook variable reference: `minutes: "{{watch_minutes}}"`.
3. **Kind Compatibility (FR-009)**:
   - `enum` -> `enum`: Runbook options must be a subset of the target entry variable's allowed options.
   - `enum` -> `text`: Valid (enum value is valid text).
   - `number` -> `number`: Valid.
   - `number` -> `text`: Valid.
   - `text` -> `number`: Invalid (unless bound to a valid numeric literal).
   - `text` -> `enum`: Invalid (unless bound to a valid enum literal).
   - `secret` -> `secret`: Valid.
   - `secret` <-> Any other kind: Invalid.
   - Literal values: Must be parseable/valid for the step variable's kind.
4. **Unbound Variables (FR-010)**: Any variable declared on the referenced entry that is not bound implicitly or explicitly remains unbound. Lint emits `KB083-unbound-step-var` (severity Info).

---

## 3. Reference Integrity & Relational Indexing

### Decision: Bump `INDEX_VERSION` to 4 and add `runbook_steps` table
In `.kb/index.db`:
```sql
CREATE TABLE IF NOT EXISTS runbook_steps (
    runbook_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    entry_id TEXT NOT NULL,
    PRIMARY KEY (runbook_id, position)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS runbook_steps_entry ON runbook_steps(entry_id);
```
Bumping `INDEX_VERSION` to 4 ensures older versions of the index are dropped and rebuilt transparently on first access without migration scripts (Constitution Principle II).

### Fast Lookups (FR-024, SC-004):
- **Used In**: `SELECT DISTINCT r.id, r.title FROM runbook_steps s JOIN entries r ON s.runbook_id = r.id WHERE s.entry_id = ? ORDER BY r.title`
- **Filter `runbook:<id>`**: `SELECT e.* FROM runbook_steps s JOIN entries e ON s.entry_id = e.id WHERE s.runbook_id = ? ORDER BY s.position`
- **Filter `used-by:runbook`**: `SELECT e.* FROM entries e WHERE e.id IN (SELECT DISTINCT entry_id FROM runbook_steps)`

All queries utilize indices and run in $<5$ ms on 10,000 entries.

---

## 4. Format-Preserving Edits on Rename & Delete Protection

### Decision: Frontmatter AST or Targeted YAML Modification for Reference Updates
When an entry is renamed (`kb mv <old> <new>` or TUI rename):
1. Query `runbook_steps` for all runbooks referencing `<old>`.
2. For each runbook, perform format-preserving updates replacing `entry: <old>` with `entry: <new>`.
3. Preserve comments, formatting, indentation, and trailing notes byte-for-byte.
4. If auto-commit is enabled (M9), stage the renamed entry file AND all updated runbook files in a single git commit. The commit message body lists each updated runbook.

### Delete Protection (FR-018):
When `kb rm <id>` or TUI delete is invoked:
1. Check `runbook_steps` for references to `<id>`.
2. If references exist and `--force` is not specified, abort with exit code 1, printing:
   `Error: Cannot delete '<id>' because it is referenced by N runbook(s):`
   followed by each runbook ID and step ID.

---

## 5. Lint Diagnostics & Fixes (`KB070`–`KB084`)

### Diagnostic Codes:
- `KB070-runbook-no-steps` (Error)
- `KB071-step-entry-and-do` (Error)
- `KB072-step-missing-entry` (Error)
- `KB073-step-is-runbook` (Error)
- `KB074-duplicate-step-id` (Error)
- `KB075-invalid-step-id` (Error)
- `KB076-invalid-binding` (Error)
- `KB077-binding-unknown-var` (Error)
- `KB078-binding-not-in-entry` (Error)
- `KB079-too-many-steps` (Error)
- `KB080-long-runbook` (Warning)
- `KB081-step-entry-has-errors` (Warning)
- `KB082-unused-runbook-var` (Warning)
- `KB083-unbound-step-var` (Info)
- `KB084-implicit-step-id` (Info)

### Rename Detection & Fuzzy Suggestions (FR-013, FR-015):
When an entry reference is broken:
1. Check `files` table for content-hash rename matches (M2 content-hash rename detection).
2. If no hash match, compute Damerau-Levenshtein edit distance against existing entry IDs.
3. Suggest the best match in the diagnostic message.
4. In `kb lint --fix`, prompt or auto-apply (`--yes`) the suggested reference replacement with format-preserving edits.

---

## 6. MCP Server Integration & Unreviewed Tracking

### Decision: Full Runbook Support in MCP Protocol
- MCP `get` returns `runbook` metadata, steps, and binding table. If `resolve: true`, entry steps include resolved payload and action links computed from runbook vars and step bindings.
- Hidden entries (`mcp.exclude_tags`, `mcp.exclude_types`) in a runbook are returned as `{ "hidden": true, "position": N }`, masking title and entry ID (FR-025, spec 012 FR-016).
- MCP `create` and `update` accept runbooks. Validation is strict (no "save anyway").
- Writes commit with `Kb-Source: mcp` git trailer and are flagged as `unreviewed` in the index and UI until Carlos reviews them.
- `get` and `update` on any entry include `"used_in": [ { "id": "...", "title": "..." } ]`.

---

## 7. Configurable Keybindings & TUI Marks

### Keymap Commands (FR-031, FR-032):
- `mark-entry`: toggles entry mark in the search and normal list views.
  - Emacs default: `Alt-M`
  - Vim normal default: `v`
- `new-runbook-from-marks`: launches runbook creation dialog/flow using marked entries in marking order.
  - Emacs default: `Alt-B`
  - Vim normal default: `B`
- Marks are tracked in TUI state and cleared on `Esc` (when search query is already empty), upon runbook creation, or upon exiting the TUI.
