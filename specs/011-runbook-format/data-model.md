# Data Model: Runbook Format

**Branch**: `011-runbook-format` | **Date**: 2026-10-15 | **Spec**: [specs/011-runbook-format/spec.md](spec.md)

This document specifies the logical data model, in-memory structures, index schemas, and validation rules for `runbook` entries in kb.

---

## 1. Domain Entities

### 1.1 Runbook Entry (`kb_core::entry::RunbookEntry`)

A `Runbook` is an entry of type `runbook`. It defines an ordered sequence of steps (references to existing entries or manual instructions), with variables declared once for the entire runbook. The body represents free-form Markdown notes.

```rust
pub struct RunbookEntry {
    /// Canonical entry ID (e.g. "runbook/deploy-authorizer").
    pub id: String,
    /// Constant EntryType::Runbook.
    pub entry_type: EntryType,
    /// Human-readable title (up to 120 characters).
    pub title: String,
    /// Classification tags (e.g. ["deploy", "lambda", "auth"]).
    pub tags: Vec<String>,
    /// Runbook-level variable declarations.
    pub vars: BTreeMap<String, VarDecl>,
    /// Ordered sequence of steps (1 to 100 steps).
    pub steps: Vec<RunbookStep>,
    /// Creation timestamp (ISO 8601 or YYYY-MM-DD).
    pub created: Option<String>,
    /// Last update timestamp.
    pub updated: Option<String>,
    /// Free-form notes in the Markdown body.
    pub body: String,
}
```

---

### 1.2 Runbook Step (`kb_core::runbook::RunbookStep`)

Each step in a runbook represents either a referenced entry to execute or a manual instruction to follow.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunbookStep {
    /// Unique identifier within the runbook ([a-z0-9][a-z0-9-]*).
    /// If omitted in frontmatter, slugified from title.
    pub id: String,
    /// Whether id was explicitly provided in frontmatter (true) or derived (false).
    pub id_explicit: bool,
    /// Step title (1-120 characters, may include {{var}} references).
    pub title: String,
    /// Step target: either a referenced entry ID or manual instruction text.
    #[serde(flatten)]
    pub target: StepTarget,
    /// Optional guidance note (may include {{var}} references).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this step is optional (default false).
    #[serde(default)]
    pub optional: bool,
    /// Whether this step requires explicit confirmation in R2 (default false).
    #[serde(default)]
    pub confirm: bool,
    /// Explicit variable bindings for this step.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, StepVarBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepTarget {
    Entry { entry: String },
    Do { r#do: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepVarBinding {
    /// Reference to a runbook variable: "{{var_name}}"
    RunbookVar(String),
    /// Literal value (string, number, boolean)
    Literal(serde_json::Value),
}
```

---

### 1.3 Variable Binding Table (`kb_core::runbook::BindingTable`)

The core exposes a single authoritative binding table for every runbook.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingTable {
    pub runbook_id: String,
    pub steps: Vec<StepBindingRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBindingRow {
    pub step_id: String,
    pub step_index: usize,
    pub entry_id: Option<String>,
    pub bindings: Vec<VarBindingInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarBindingInfo {
    /// Variable name defined on the step entry.
    pub var_name: String,
    /// Expected kind of the step entry variable.
    pub expected_kind: VarKind,
    /// Target of the binding.
    pub target: BindingTarget,
    /// Validity determination.
    pub validity: BindingValidity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingTarget {
    /// Bound to a runbook-level variable.
    RunbookVar { name: String, kind: VarKind },
    /// Bound to a literal constant value.
    Literal { value: serde_json::Value },
    /// Not bound; will prompt user interactively in R2.
    Unbound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingValidity {
    Valid,
    Invalid { reason: String },
}
```

---

## 2. Validation & Diagnostic Rules

| Code | Severity | Description | Fix / Remediation |
|---|---|---|---|
| `KB070-runbook-no-steps` | Error | Runbook frontmatter contains no steps (`steps:` is empty or missing) | Add at least 1 step |
| `KB071-step-entry-and-do` | Error | Step defines both `entry` and `do`, or neither | Specify exactly one of `entry` or `do` |
| `KB072-step-missing-entry` | Error | Referenced entry ID does not exist | Suggest renamed ID or closest ID via edit distance |
| `KB073-step-is-runbook` | Error | Step references an entry that is itself a runbook | Runbooks cannot be nested in R1 |
| `KB074-duplicate-step-id` | Error | Step `id` is duplicated within the runbook | Make step `id` unique |
| `KB075-invalid-step-id` | Error | Step `id` violates `^[a-z0-9][a-z0-9-]*$` | Use valid lowercase alphanumeric and hyphens |
| `KB076-invalid-binding` | Error | Step binding type is incompatible with target var kind | Fix binding type or options |
| `KB077-binding-unknown-var` | Error | Step binds to a runbook var `{{name}}` not declared in runbook `vars:` | Declare var in runbook or fix reference |
| `KB078-binding-not-in-entry` | Error | Step binds a variable name that the referenced entry does not declare | Remove or fix binding name |
| `KB079-too-many-steps` | Error | Runbook exceeds maximum 100 steps | Split into multiple runbooks |
| `KB080-long-runbook` | Warning | Runbook exceeds 50 steps | Advisory warning for operational usability |
| `KB081-step-entry-has-errors` | Warning | Referenced step entry has lint errors | Fix referenced entry |
| `KB082-unused-runbook-var` | Warning | Runbook variable declared but never used in step bindings or step text | Remove unused variable declaration |
| `KB083-unbound-step-var` | Info | Step variable left unbound (will prompt at run time) | Advisory info |
| `KB084-implicit-step-id` | Info | Step lacks explicit `id` (slug derived from title) | Specify explicit `id:` |

---

## 3. SQLite Disposable Index Schema (`INDEX_VERSION = 4`)

### `runbook_steps` Table
```sql
CREATE TABLE IF NOT EXISTS runbook_steps (
    runbook_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    entry_id TEXT NOT NULL,
    PRIMARY KEY(runbook_id, position)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS runbook_steps_entry ON runbook_steps(entry_id);
```

### Search Column Mapping (FR-020):
- `title`: Runbook title
- `tags`: Runbook tags
- `id`: Runbook ID
- `meta`: Runbook variable names + step IDs + step titles
- `payload`: `do` manual step text + step notes
- `notes`: Markdown body of the runbook
*Note*: The content of referenced step entries is NOT indexed into the runbook row.

---

## 4. TUI Session Marks (`kb::tui::mark::MarkState`)

```rust
#[derive(Debug, Default, Clone)]
pub struct MarkState {
    /// Ordered list of marked entry IDs.
    marked: Vec<String>,
}

impl MarkState {
    pub fn toggle(&mut self, id: &str) -> Option<usize>;
    pub fn is_marked(&self, id: &str) -> Option<usize>;
    pub fn clear(&mut self);
    pub fn entries(&self) -> &[String];
}
```
