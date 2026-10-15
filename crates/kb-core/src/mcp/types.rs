//! Core transfer and view types for the MCP server.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Detailed MCP view of a KB entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryView {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub payload: String,
    pub notes: String,
    /// A note's whole Markdown body, without frontmatter and without escape sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    pub vars: BTreeMap<String, VarDeclarationView>,
    pub version: String,
    pub unreviewed: bool,
    pub used_in: Vec<String>,
    pub findings: Vec<FindingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_fields: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<EntryLinksView>,
}

/// Representation of a declared variable in an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VarDeclarationView {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Action links constructed for an entry (M7 console/web/code links).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EntryLinksView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
}

/// A lint finding or warning presented to an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FindingView {
    pub rule: String,
    pub message: String,
    pub line: u32,
    pub severity: String,
}

impl FindingView {
    pub fn from_finding(f: &crate::lint::Finding) -> Self {
        Self {
            rule: f.code.to_string(),
            message: f.message.clone(),
            line: f.line,
            severity: match f.severity {
                crate::lint::Severity::Error => "error".into(),
                crate::lint::Severity::Warning => "warning".into(),
                crate::lint::Severity::Info => "info".into(),
            },
        }
    }
}

/// Set of changes for `update_entry`. Supports both delta mutations and full replacements.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChangeSet {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// A note's new Markdown body.
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub fields: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub add_tags: Option<Vec<String>>,
    #[serde(default)]
    pub remove_tags: Option<Vec<String>>,
    #[serde(default)]
    pub vars: Option<BTreeMap<String, VarDeclarationView>>,
    #[serde(default)]
    pub set_vars: Option<BTreeMap<String, VarDeclarationView>>,
    #[serde(default)]
    pub remove_vars: Option<Vec<String>>,
}

/// Review state tracking for an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewState {
    pub id: String,
    pub unreviewed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_mcp_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reviewed_commit: Option<String>,
}

/// One audit log line written to `.kb/mcp.log`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub timestamp: String,
    pub client: String,
    pub tool: String,
    pub entry_id: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

/// Search result item returned by `search`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResultView {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub snippet: String,
    pub score: f32,
    pub unreviewed: bool,
}

/// Prompt offered to MCP clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptView {
    pub name: String,
    pub description: String,
    pub arguments: Vec<PromptArgumentView>,
}

/// Argument declaration for an MCP prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptArgumentView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
}

/// Request payload for creating an entry via MCP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CreateEntryRequest {
    #[serde(rename = "type", default)]
    pub entry_type: Option<String>,
    /// Optional for a note whose `content` starts with a `# heading`.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub notes: Option<String>,
    /// A note's Markdown body.
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, VarDeclarationView>,
    #[serde(default)]
    pub fields: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

/// Response payload from creating an entry via MCP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateEntryResponse {
    pub id: String,
    pub version: String,
    pub findings: Vec<FindingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run_content: Option<String>,
}

/// Request payload for updating an entry via MCP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UpdateEntryRequest {
    pub id: String,
    pub version: String,
    pub changes: ChangeSet,
    #[serde(default)]
    pub dry_run: bool,
}

/// Response payload from updating an entry via MCP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateEntryResponse {
    pub id: String,
    pub version: String,
    pub findings: Vec<FindingView>,
    pub used_in: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run_diff: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_serde_roundtrip() {
        let entry = EntryView {
            id: "sumo/5xx".into(),
            entry_type: "sumo".into(),
            title: "5xx query".into(),
            tags: vec!["auth".into()],
            fields: BTreeMap::new(),
            payload: "_sourceCategory=prod".into(),
            notes: "Notes".into(),
            content: None,
            created: None,
            updated: None,
            vars: BTreeMap::new(),
            version: "sha256hash".into(),
            unreviewed: false,
            used_in: vec![],
            findings: vec![],
            resolved_payload: None,
            resolved_fields: None,
            links: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: EntryView = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
