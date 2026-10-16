# Data Model: Runbook Run Mode

This document defines the domain models, state enums, and JSON file schemas for run execution.

---

## 1. Domain Entities & Enums

### 1.1 Run
Represents an ongoing or completed execution of a runbook.

```rust
pub struct Run {
    pub id: String,
    pub runbook_id: String,
    pub display_name: String,
    pub state: RunState,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
    pub runbook_hash_start: String,
    pub runbook_hash_resume: String,
    pub vars: BTreeMap<String, String>,
    pub var_history: Vec<VarChange>,
    pub step_states: BTreeMap<String, StepState>,
    pub notes: Vec<RunNote>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Active,
    Finished,
    Abandoned,
}
```

### 1.2 StepState & Status
Tracks the state and execution history of a single step within a run.

```rust
pub struct StepState {
    pub step_id: String,
    pub status: StepStatus,
    pub status_history: Vec<StatusTransition>,
    pub last_action_at: Option<i64>,
    pub entry_hash_at_action: Option<String>,
    pub last_exit_code: Option<i32>,
    pub last_duration_ms: Option<u64>,
    pub step_vars: BTreeMap<String, String>,
    pub notes: Vec<StepNote>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Opened,
    Copied,
    Running,
    Done,
    Skipped,
    Failed,
    Interrupted,
    Removed,
}

pub struct StatusTransition {
    pub status: StepStatus,
    pub timestamp: i64,
}

pub struct StepNote {
    pub timestamp: i64,
    pub text: String,
}

pub struct RunNote {
    pub timestamp: i64,
    pub text: String,
}

pub struct VarChange {
    pub timestamp: i64,
    pub name: String,
    pub old_value: Option<String>,
    pub new_value: String,
}
```

### 1.3 RunLock
Advisory process lock ensuring only one process modifies a run at a time.

```rust
pub struct RunLock {
    pub pid: u32,
    pub started_at: i64,
    pub hostname: String,
}
```

---

## 2. JSON Serialization Format (`.kb/runs/<run-id>.json`)

```json
{
  "$schema": "https://kb.dev/schemas/run-v1.json",
  "version": 1,
  "id": "deploy-authorizer-to-prod-1760549400",
  "runbook_id": "runbook/deploy-authorizer-to-prod",
  "display_name": "Deploy authorizer to prod (stage, v2.14.0) · Oct 15 17:30",
  "state": "active",
  "created_at": 1760549400,
  "updated_at": 1760549520,
  "finished_at": null,
  "runbook_hash_start": "a1b2c3d4e5f67890",
  "runbook_hash_resume": "a1b2c3d4e5f67890",
  "vars": {
    "env": "stage",
    "version": "v2.14.0"
  },
  "var_history": [
    {
      "timestamp": 1760549400,
      "name": "env",
      "old_value": null,
      "new_value": "stage"
    }
  ],
  "step_states": {
    "baseline": {
      "step_id": "baseline",
      "status": "done",
      "status_history": [
        { "status": "pending", "timestamp": 1760549400 },
        { "status": "opened", "timestamp": 1760549420 },
        { "status": "done", "timestamp": 1760549450 }
      ],
      "last_action_at": 1760549420,
      "entry_hash_at_action": "098765fedcba4321",
      "last_exit_code": null,
      "last_duration_ms": null,
      "step_vars": {},
      "notes": [
        {
          "timestamp": 1760549440,
          "text": "Baseline error rate verified under 0.01%."
        }
      ]
    },
    "deploy": {
      "step_id": "deploy",
      "status": "pending",
      "status_history": [
        { "status": "pending", "timestamp": 1760549400 }
      ],
      "last_action_at": null,
      "entry_hash_at_action": null,
      "last_exit_code": null,
      "last_duration_ms": null,
      "step_vars": {},
      "notes": []
    }
  },
  "notes": [
    {
      "timestamp": 1760549405,
      "text": "Starting scheduled deployment window."
    }
  ]
}
```

---

## 3. Storage Hierarchy & Invariants

```text
.kb/
├── index.db              # Search index (rebuildable)
├── state.json            # Usage stats and everyday variable pre-fills
└── runs/                 # [NEW] Machine-local run storage
    ├── .gitignore        # Contains "*" (runs are never committed)
    ├── deploy-authorizer-to-prod-1760549400.json
    ├── deploy-authorizer-to-prod-1760549400.lock
    └── deploy-authorizer-to-prod-1760549400.json.bak (if recovered)
```

### Invariants
1. **Zero Secret Leakage**: Secret variables (`AWS_SECRET_ACCESS_KEY`, API tokens) are never stored in `vars` or `step_vars`.
2. **Atomic Writes**: Writes occur to `.kb/runs/.<run-id>.tmp` then rename to `.kb/runs/<run-id>.json`.
3. **Step Identity**: Steps are keyed by `step_id`, not array index.
4. **Isolated Memory**: Values in `step_vars` and `vars` never pollute `.kb/state.json`.
