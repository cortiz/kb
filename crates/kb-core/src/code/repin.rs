//! Format-preserving re-pinning of code entries.

use super::drift::{DriftReport, DriftStatus};
use crate::edit::FieldValue;
use crate::entry::parse_entry;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepinError {
    ConcurrentModification,
    UnsupportedFormat,
    EditFailed(String),
    Io(String),
}

impl std::fmt::Display for RepinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepinError::ConcurrentModification => {
                write!(f, "file was modified concurrently on disk")
            }
            RepinError::UnsupportedFormat => write!(f, "unsupported kb format"),
            RepinError::EditFailed(s) => write!(f, "edit failed: {s}"),
            RepinError::Io(s) => write!(f, "io error: {s}"),
        }
    }
}

impl std::error::Error for RepinError {}

pub fn repin_entry_impl(
    entry_file_path: &Path,
    current_content: &str,
    report: &DriftReport,
    today_date: &str,
) -> Result<String, RepinError> {
    if entry_file_path.exists() {
        let disk_bytes =
            std::fs::read(entry_file_path).map_err(|e| RepinError::Io(e.to_string()))?;
        let disk_content =
            String::from_utf8(disk_bytes).map_err(|e| RepinError::Io(e.to_string()))?;
        if disk_content != current_content {
            return Err(RepinError::ConcurrentModification);
        }
    }

    let mut parsed = parse_entry("repin", "repin.md", current_content.as_bytes().to_vec())
        .entry
        .ok_or(RepinError::UnsupportedFormat)?;

    let (new_path, new_line, new_end_line) = match &report.status {
        DriftStatus::Current => (
            report.pinned_path.clone(),
            report.pinned_line,
            report.pinned_end_line,
        ),
        DriftStatus::Moved {
            new_line,
            new_end_line,
        } => (report.pinned_path.clone(), *new_line, *new_end_line),
        DriftStatus::Changed { new_line, .. } => (report.pinned_path.clone(), *new_line, None),
        DriftStatus::Renamed {
            new_path,
            new_line,
            new_end_line,
        } => (new_path.clone(), *new_line, *new_end_line),
        DriftStatus::Gone { .. } | DriftStatus::Unknown { .. } => {
            return Err(RepinError::EditFailed(
                "cannot repin an entry with gone or unknown status".to_string(),
            ));
        }
    };

    let new_ref = report.tracking_ref.as_deref().unwrap_or(&report.pinned_ref);

    parsed
        .set_field(&["code", "ref"], FieldValue::Str(new_ref.to_string()))
        .map_err(|e| RepinError::EditFailed(e.to_string()))?;

    parsed
        .set_field(&["code", "path"], FieldValue::Str(new_path))
        .map_err(|e| RepinError::EditFailed(e.to_string()))?;

    parsed
        .set_field(&["code", "line"], FieldValue::Int(new_line as i64))
        .map_err(|e| RepinError::EditFailed(e.to_string()))?;

    if let Some(end) = new_end_line {
        parsed
            .set_field(&["code", "end_line"], FieldValue::Int(end as i64))
            .map_err(|e| RepinError::EditFailed(e.to_string()))?;
    }

    parsed
        .set_field(&["updated"], FieldValue::Str(today_date.to_string()))
        .map_err(|e| RepinError::EditFailed(e.to_string()))?;

    let updated_bytes = parsed.to_bytes();
    let updated_text =
        String::from_utf8(updated_bytes).map_err(|e| RepinError::EditFailed(e.to_string()))?;

    if let Some(parent) = entry_file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RepinError::Io(e.to_string()))?;
    }
    std::fs::write(entry_file_path, &updated_text).map_err(|e| RepinError::Io(e.to_string()))?;

    Ok(updated_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::drift::DriftConfidence;

    #[test]
    fn test_format_preserving_repinning_preserves_comments_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("code.md");

        let initial_content = r#"---
kb_format: 1
type: code
# Useful comment about auth
title: Auth Handler Link
tags: [security, core]
code:
  repo: org/repo
  path: src/AuthHandler.java
  line: 8
  end_line: 13
  ref: 1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a
  branch: main
---

# Notes on handler
Body content preserved completely.
"#;
        std::fs::write(&file_path, initial_content).unwrap();

        let report = DriftReport {
            entry_id: "code".to_string(),
            repo: "org/repo".to_string(),
            status: DriftStatus::Moved {
                new_line: 13,
                new_end_line: Some(18),
            },
            pinned_ref: "1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a".to_string(),
            pinned_path: "src/AuthHandler.java".to_string(),
            pinned_line: 8,
            pinned_end_line: Some(13),
            tracking_branch: "origin/main".to_string(),
            tracking_ref: Some("2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b".to_string()),
            confidence: DriftConfidence::DiffMapped,
        };

        let updated = repin_entry_impl(&file_path, initial_content, &report, "2026-10-10").unwrap();

        // Verify updated fields
        assert!(updated.contains("ref: 2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b"));
        assert!(updated.contains("line: 13"));
        assert!(updated.contains("end_line: 18"));
        assert!(updated.contains("updated: 2026-10-10"));

        // Verify preserved comment and body
        assert!(updated.contains("# Useful comment about auth"));
        assert!(updated.contains("Body content preserved completely."));

        // Verify concurrent modification rejection
        std::fs::write(&file_path, "modified externally").unwrap();
        let err = repin_entry_impl(&file_path, initial_content, &report, "2026-10-10").unwrap_err();
        assert_eq!(err, RepinError::ConcurrentModification);
    }
}
