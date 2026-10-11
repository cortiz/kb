//! Shell integration, dialects, and completion helpers.

pub mod completions;
pub mod dialects;
pub mod templates;

pub use completions::CompletionKind;
pub use dialects::ShellDialect;
pub use templates::{KB_STARTUP_BLOCK_BEGIN, KB_STARTUP_BLOCK_END};

use serde::{Deserialize, Serialize};

use crate::entry::EntryType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferMode {
    Query,
    Insert,
}

impl BufferMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "query" => Some(BufferMode::Query),
            "insert" => Some(BufferMode::Insert),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            BufferMode::Query => "query",
            BufferMode::Insert => "insert",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WidgetRequest {
    pub shell: ShellDialect,
    pub buffer: String,
    pub cursor: usize,
    pub mode: BufferMode,
    pub default_type: Option<EntryType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WidgetResult {
    pub status: String, // "picked", "cancelled", "refused"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl WidgetResult {
    pub fn picked(command: impl Into<String>, cursor: usize) -> Self {
        Self {
            status: "picked".to_string(),
            command: Some(command.into()),
            cursor: Some(cursor),
            message: None,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            status: "cancelled".to_string(),
            command: None,
            cursor: None,
            message: None,
        }
    }

    pub fn refused(message: impl Into<String>) -> Self {
        Self {
            status: "refused".to_string(),
            command: None,
            cursor: None,
            message: Some(message.into()),
        }
    }
}

