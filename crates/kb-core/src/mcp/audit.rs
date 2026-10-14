//! Append-only audit logger for MCP operations.
//! Writes JSON lines to `.kb/mcp.log`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use super::types::AuditEntry;
use crate::author::render::rfc3339_utc;

/// Logs an MCP operation to `.kb/mcp.log` under the given KB root.
///
/// Guaranteed never to record secret contents: only records client, tool name,
/// entry ID, outcome ("ok" or "error"), and an optional error code.
pub fn log_mcp_operation(
    kb_root: &Path,
    client: Option<&str>,
    tool: &str,
    entry_id: &str,
    outcome: &str,
    error_code: Option<&str>,
) -> std::io::Result<()> {
    let kb_dir = kb_root.join(".kb");
    if !kb_dir.exists() {
        std::fs::create_dir_all(&kb_dir)?;
    }
    let log_path = kb_dir.join("mcp.log");

    let entry = AuditEntry {
        timestamp: rfc3339_utc(SystemTime::now()),
        client: client.unwrap_or("unknown").to_string(),
        tool: tool.to_string(),
        entry_id: entry_id.to_string(),
        outcome: outcome.to_string(),
        error_code: error_code.map(str::to_string),
    };

    let line = serde_json::to_string(&entry).map_err(std::io::Error::other)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    writeln!(file, "{}", line)?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_mcp_operation_appends_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        log_mcp_operation(root, Some("claude-code"), "get", "sumo/5xx", "ok", None).unwrap();
        log_mcp_operation(
            root,
            Some("claude-code"),
            "update",
            "sumo/5xx",
            "error",
            Some("version_mismatch"),
        )
        .unwrap();

        let log_path = root.join(".kb").join("mcp.log");
        assert!(log_path.exists());
        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let parsed1: AuditEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed1.client, "claude-code");
        assert_eq!(parsed1.tool, "get");
        assert_eq!(parsed1.entry_id, "sumo/5xx");
        assert_eq!(parsed1.outcome, "ok");
        assert_eq!(parsed1.error_code, None);

        let parsed2: AuditEntry = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(parsed2.outcome, "error");
        assert_eq!(parsed2.error_code.as_deref(), Some("version_mismatch"));
    }
}
