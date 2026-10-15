# Contract: MCP Server Interface (Runbook Format)

This document specifies the Model Context Protocol (MCP) behavior for `runbook` entries and runbook reference tracking across all entries.

---

## 1. Tools Overview

| Tool Name | Behavior for Runbooks |
|---|---|
| `search` | Returns matching runbooks. Supports filters `runbook:<id>` and `used-by:runbook`. |
| `get` | Returns runbook metadata, steps, notes, and the full binding table. If `resolve: true`, resolves step entry payloads. Masks hidden entry steps. Lists `used_in` on other entries. |
| `create` | Validates and creates a new runbook entry. Enforces lint rules strictly (no "save anyway"). |
| `update` | Validates and updates runbook steps via `changes.steps`. Enforces lint rules strictly. |

---

## 2. Tool Details

### 2.1 `get` on a Runbook
- **Parameters**:
  - `id`: Runbook ID (e.g. `"runbook/deploy-authorizer"`).
  - `resolve`: Boolean (optional, default `false`).
  - `vars`: Map of string keys to values (optional).
- **Response Format (`resolve: false`)**:
  ```json
  {
    "id": "runbook/deploy-authorizer",
    "type": "runbook",
    "title": "Deploy authorizer to prod",
    "tags": ["deploy", "lambda", "auth"],
    "vars": {
      "env": { "kind": "enum", "options": ["stage", "prod"], "default": "stage" },
      "version": { "kind": "text", "description": "Release tag" }
    },
    "steps": [
      {
        "id": "baseline",
        "title": "Check the current error rate",
        "entry": "sumo/5xx-by-authorizer",
        "vars": { "minutes": 30 }
      },
      {
        "id": "rollback",
        "title": "Roll back if p99 > 800 ms",
        "do": "Only if p99 stayed above 800 ms for 5 minutes.\nPost in #auth-oncall before rolling back.",
        "optional": true
      }
    ],
    "binding_table": [
      {
        "step_id": "baseline",
        "step_index": 0,
        "entry_id": "sumo/5xx-by-authorizer",
        "bindings": [
          {
            "var_name": "minutes",
            "expected_kind": "number",
            "target": { "Literal": 30 },
            "validity": "Valid"
          }
        ]
      }
    ],
    "notes": "Owner: platform security. Last rehearsed in stage on 2026-10-02."
  }
  ```

- **Hidden Step Masking (FR-025, spec 012 FR-016)**:
  If a referenced entry has a tag in `mcp.exclude_tags` (e.g. `private`) or type in `mcp.exclude_types`, the step in `steps` is replaced with:
  ```json
  {
    "hidden": true,
    "position": 2
  }
  ```
  Neither the entry ID nor its title is revealed to the agent.

- **Resolved Step Content (`resolve: true`)**:
  For entry steps that are not hidden, includes:
  - `resolved_payload`: Resolved query/command string after applying runbook vars + step bindings.
  - `action_links`: Action links evaluated for the step entry.

---

### 2.2 `create` with `type: runbook`
- **Request Parameters**:
  - `title`: String
  - `type`: `"runbook"`
  - `steps`: Array of step objects
  - `vars`: Optional variable declarations
  - `tags`: Optional string array
  - `notes`: Optional body notes string
- **Validation**:
  - Must satisfy all lint rules KB070–KB084 without any errors.
  - Secret scanning applies to step notes and body.
- **Git Commit**:
  - Created with trailer `Kb-Source: mcp`.
  - Entry flagged as `unreviewed` until user reviews in TUI / CLI.

---

### 2.3 `update` on a Runbook
- **Request Parameters**:
  - `id`: Runbook ID
  - `changes`:
    - `steps`: Array of step objects (full replacement)
    - `vars`: Optional updated vars
    - `tags`: Optional updated tags
    - `notes`: Optional updated body
- **Validation**:
  - Fully validated against lint rules.
  - Git commit includes `Kb-Source: mcp`. Flagged as `unreviewed`.

---

### 2.4 "Used In" on Any Entry
- Any `get` or `update` response for an entry referenced by one or more runbooks includes:
  ```json
  {
    "used_in": [
      {
        "id": "runbook/deploy-authorizer",
        "title": "Deploy authorizer to prod"
      }
    ]
  }
  ```
  Derived from the `runbook_steps` index table (FR-016, FR-026).
