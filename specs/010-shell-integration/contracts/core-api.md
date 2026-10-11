# Core API Contracts: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## 1. `kb_core::shell` Module

### 1.1 Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    Zsh,
    Bash,
    Fish,
}

impl ShellDialect {
    pub fn from_str(s: &str) -> Option<Self>;
    pub fn as_str(&self) -> &'static str;
    pub fn env_var_ref(&self, var_name: &str) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferMode {
    Query,
    Insert,
}

#[derive(Debug, Clone)]
pub struct WidgetRequest {
    pub shell: ShellDialect,
    pub buffer: String,
    pub cursor: usize,
    pub mode: BufferMode,
    pub default_type: Option<EntryType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetResult {
    pub status: String, // "picked", "cancelled", "refused"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
```

### 1.2 Functions

```rust
pub fn generate_shell_init(
    dialect: ShellDialect,
    widget_key: &str,
    save_key: &str,
) -> String;

pub fn generate_completions(
    dialect: ShellDialect,
) -> String;

pub fn complete_entries(
    index: &Index,
    prefix: &str,
    limit: usize,
) -> Result<Vec<(String, String)>, SearchError>;

pub fn complete_tags(
    index: &Index,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>, SearchError>;
```

---

## 2. Variable Resolution for Shell Targets

In `kb_core::varres`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionTarget {
    Execution,
    Clipboard,
    ShellWidget(ShellDialect),
}
```

When `ResolutionTarget::ShellWidget(dialect)` is provided:
- A secret variable with `env: Some(name)` resolves to `dialect.env_var_ref(name)` (e.g. `"$SUMO_TOKEN"`).
- A secret variable with `env: None` returns an error `VarError::SecretWithoutEnvSource(name)`.
