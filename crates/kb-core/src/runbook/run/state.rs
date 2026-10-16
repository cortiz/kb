//! Run data model (spec 015 data-model.md). Serialized to `.kb/runs/<run-id>.json`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The run file format version this build reads and writes.
pub const RUN_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Active,
    Finished,
    Abandoned,
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

impl StepStatus {
    /// Icon readable without color (FR-026).
    pub const fn icon(self) -> &'static str {
        match self {
            StepStatus::Pending => "○",
            StepStatus::Opened | StepStatus::Copied => "◐",
            StepStatus::Running => "▶",
            StepStatus::Done => "●",
            StepStatus::Skipped => "⊘",
            StepStatus::Failed => "✕",
            StepStatus::Interrupted => "!",
            StepStatus::Removed => "–",
        }
    }

    /// Done or skipped: the step needs nothing more.
    pub const fn is_settled(self) -> bool {
        matches!(self, StepStatus::Done | StepStatus::Skipped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusTransition {
    pub status: StepStatus,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepNote {
    pub timestamp: i64,
    pub text: String,
}

pub type RunNote = StepNote;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarChange {
    pub timestamp: i64,
    pub name: String,
    pub old_value: Option<String>,
    pub new_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepState {
    pub step_id: String,
    pub status: StepStatus,
    #[serde(default)]
    pub status_history: Vec<StatusTransition>,
    #[serde(default)]
    pub last_action_at: Option<i64>,
    #[serde(default)]
    pub entry_hash_at_action: Option<String>,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
    #[serde(default)]
    pub last_duration_ms: Option<u64>,
    #[serde(default)]
    pub step_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Vec<StepNote>,
}

impl StepState {
    pub fn new(step_id: &str, now: i64) -> StepState {
        StepState {
            step_id: step_id.to_string(),
            status: StepStatus::Pending,
            status_history: vec![StatusTransition {
                status: StepStatus::Pending,
                timestamp: now,
            }],
            last_action_at: None,
            entry_hash_at_action: None,
            last_exit_code: None,
            last_duration_ms: None,
            step_vars: BTreeMap::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub version: u64,
    pub id: String,
    pub runbook_id: String,
    pub display_name: String,
    pub state: RunState,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub finished_at: Option<i64>,
    pub runbook_hash_start: String,
    pub runbook_hash_resume: String,
    /// Non-secret runbook variable values only (constitution VI).
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub var_history: Vec<VarChange>,
    #[serde(default)]
    pub step_states: BTreeMap<String, StepState>,
    #[serde(default)]
    pub notes: Vec<RunNote>,
}

/// Advisory lock sidecar `.kb/runs/<run-id>.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLock {
    pub pid: u32,
    pub started_at: i64,
    pub hostname: String,
}
