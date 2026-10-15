# Contract: Runbook Entry Format

This document defines the file syntax, frontmatter schema, step attributes, and variable binding syntax for `runbook` entries (`kb_format: 1`).

---

## 1. File Structure

A runbook entry is a single UTF-8 file stored under `runbook/<slug>.md`.

```markdown
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

---

## 2. Frontmatter Schema (`YAML`)

| Key | Type | Requirement | Description |
|---|---|---|---|
| `kb_format` | Integer | Mandatory | Format revision; MUST be `1`. |
| `type` | String | Mandatory | MUST be `"runbook"`. |
| `title` | String | Mandatory | Runbook title (1–120 characters). |
| `tags` | Array of Strings | Optional | List of classification tags. |
| `vars` | Map | Optional | Declarations of runbook-level variables. Standard format from M1/M5. |
| `steps` | Array of Objects | Mandatory | 1 to 100 step definitions. |

---

## 3. Step Schema

| Key | Type | Requirement | Description |
|---|---|---|---|
| `id` | String | Optional | Unique ID (`^[a-z0-9][a-z0-9-]*$`). Slugified from title if omitted. |
| `title` | String | Mandatory | Non-empty string (up to 120 chars). May contain `{{var}}` placeholders. |
| `entry` | String | Exactly one of `entry` or `do` | ID of referenced entry (command, query, link, code, note). Cannot be a runbook. |
| `do` | String | Exactly one of `entry` or `do` | Manual instruction text (may be multi-line Markdown). |
| `note` | String | Optional | Additional step guidance (may be multi-line Markdown). |
| `optional` | Boolean | Optional | Default `false`. If `true`, step can be skipped. |
| `confirm` | Boolean | Optional | Default `false`. In R2, requires explicit confirmation before execution. |
| `vars` | Map | Optional | Explicit variable bindings for this step. |

---

## 4. Variable Binding Syntax

Step `vars` maps target entry variable names to either:
1. **Literal Value**:
   ```yaml
   vars:
     minutes: 30
     region: us-east-1
   ```
2. **Runbook Variable Reference**:
   ```yaml
   vars:
     target_env: "{{env}}"
   ```

### Binding Compatibility:
- Source and target types must be compatible per FR-009:
  - `enum` source -> `enum` target: source options must be a subset of target options.
  - `enum` source -> `text` target: valid.
  - `number` source -> `number` target: valid.
  - `number` source -> `text` target: valid.
  - `text` source -> `number` or `enum` target: invalid.
  - `secret` source -> `secret` target: valid.
  - `secret` source <-> any other kind: invalid.
