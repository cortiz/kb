//! Command execution plans, destructive pattern scanning, and secret environment isolation.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretEnvVar {
    pub env_key: String,
    pub secret_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub resolved_command: String,
    pub shell: String,
    pub cwd: Option<PathBuf>,
    pub secret_env_vars: Vec<SecretEnvVar>,
    pub destructive_warnings: Vec<String>,
    pub requires_typed_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub exit_code: i32,
    pub duration: Duration,
    pub killed_by_signal: Option<i32>,
}
