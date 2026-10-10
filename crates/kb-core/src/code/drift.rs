//! Git diff hunk line mapping and drift detection.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::clone::{GitError, GitRunner, LocalClone};
use crate::entry::{CodeRef, Entry, TypeFields};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftStatus {
    Current,
    Moved {
        new_line: usize,
        new_end_line: Option<usize>,
    },
    Changed {
        new_line: usize,
        diff_summary: String,
    },
    Renamed {
        new_path: String,
        new_line: usize,
        new_end_line: Option<usize>,
    },
    Gone {
        reason: String,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftConfidence {
    DiffMapped,
    SymbolMatched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftReport {
    pub entry_id: String,
    pub repo: String,
    pub status: DriftStatus,
    pub pinned_ref: String,
    pub pinned_path: String,
    pub pinned_line: usize,
    pub pinned_end_line: Option<usize>,
    pub tracking_branch: String,
    pub tracking_ref: Option<String>,
    pub confidence: DriftConfidence,
}

pub fn resolve_tracking_branch(
    clone_path: &Path,
    code_ref: &CodeRef,
    git_runner: &impl GitRunner,
) -> Result<String, GitError> {
    // 1. entry.code.branch override
    if let Some(ref b) = code_ref.branch {
        let b = b.trim();
        if !b.is_empty() {
            // Check if b resolves directly or with origin/
            if git_runner
                .run_git(clone_path, &["rev-parse", "--verify", b])
                .is_ok()
            {
                return Ok(b.to_string());
            }
            let origin_b = format!("origin/{b}");
            if git_runner
                .run_git(clone_path, &["rev-parse", "--verify", &origin_b])
                .is_ok()
            {
                return Ok(origin_b);
            }
            return Ok(b.to_string());
        }
    }

    // 2. origin/HEAD
    if let Ok(head_out) =
        git_runner.run_git(clone_path, &["rev-parse", "--abbrev-ref", "origin/HEAD"])
    {
        let branch = head_out.trim();
        if !branch.is_empty() && branch != "origin/HEAD" {
            return Ok(branch.to_string());
        }
    }

    // 3. origin/main
    if git_runner
        .run_git(clone_path, &["rev-parse", "--verify", "origin/main"])
        .is_ok()
    {
        return Ok("origin/main".to_string());
    }

    // 4. origin/master
    if git_runner
        .run_git(clone_path, &["rev-parse", "--verify", "origin/master"])
        .is_ok()
    {
        return Ok("origin/master".to_string());
    }

    // 5. fallback to main or master
    if git_runner
        .run_git(clone_path, &["rev-parse", "--verify", "main"])
        .is_ok()
    {
        return Ok("main".to_string());
    }

    Ok("origin/main".to_string())
}

pub fn check_drift_impl(
    entry: &Entry,
    clone: &LocalClone,
    git_runner: &impl GitRunner,
) -> Result<DriftReport, GitError> {
    let code_ref = match entry.fields() {
        TypeFields::Code { code: Some(c) } => c,
        _ => {
            return Ok(DriftReport {
                entry_id: entry.id().to_string(),
                repo: String::new(),
                status: DriftStatus::Unknown {
                    reason: "entry is not a valid code entry".to_string(),
                },
                pinned_ref: String::new(),
                pinned_path: String::new(),
                pinned_line: 0,
                pinned_end_line: None,
                tracking_branch: String::new(),
                tracking_ref: None,
                confidence: DriftConfidence::DiffMapped,
            });
        }
    };

    let tracking_branch = resolve_tracking_branch(&clone.local_path, code_ref, git_runner)?;
    let tracking_sha = match git_runner.run_git(&clone.local_path, &["rev-parse", &tracking_branch])
    {
        Ok(out) => out.trim().to_string(),
        Err(_) => {
            return Ok(DriftReport {
                entry_id: entry.id().to_string(),
                repo: code_ref.repo.clone(),
                status: DriftStatus::Unknown {
                    reason: format!("tracking branch '{tracking_branch}' could not be resolved"),
                },
                pinned_ref: code_ref.git_ref.clone(),
                pinned_path: code_ref.path.clone(),
                pinned_line: code_ref.line as usize,
                pinned_end_line: code_ref.end_line.map(|l| l as usize),
                tracking_branch,
                tracking_ref: None,
                confidence: DriftConfidence::DiffMapped,
            });
        }
    };

    let pinned_sha = match git_runner.run_git(&clone.local_path, &["rev-parse", &code_ref.git_ref])
    {
        Ok(out) => out.trim().to_string(),
        Err(_) => {
            return Ok(DriftReport {
                entry_id: entry.id().to_string(),
                repo: code_ref.repo.clone(),
                status: DriftStatus::Unknown {
                    reason: format!("pinned ref '{}' not found in repository", code_ref.git_ref),
                },
                pinned_ref: code_ref.git_ref.clone(),
                pinned_path: code_ref.path.clone(),
                pinned_line: code_ref.line as usize,
                pinned_end_line: code_ref.end_line.map(|l| l as usize),
                tracking_branch,
                tracking_ref: Some(tracking_sha),
                confidence: DriftConfidence::DiffMapped,
            });
        }
    };

    let p_line = code_ref.line as usize;
    let p_end = code_ref.end_line.map(|l| l as usize).unwrap_or(p_line);

    // Exact commit match -> Current
    if pinned_sha == tracking_sha {
        return Ok(DriftReport {
            entry_id: entry.id().to_string(),
            repo: code_ref.repo.clone(),
            status: DriftStatus::Current,
            pinned_ref: code_ref.git_ref.clone(),
            pinned_path: code_ref.path.clone(),
            pinned_line: p_line,
            pinned_end_line: code_ref.end_line.map(|l| l as usize),
            tracking_branch,
            tracking_ref: Some(tracking_sha),
            confidence: DriftConfidence::DiffMapped,
        });
    }

    // Check rename status via name-status
    let mut renamed_to: Option<String> = None;
    let mut file_deleted = false;
    if let Ok(name_status) = git_runner.run_git(
        &clone.local_path,
        &["diff", "-M", "--name-status", &pinned_sha, &tracking_sha],
    ) {
        for line in name_status.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status_code = parts[0];
                if status_code.starts_with('R') && parts.len() >= 3 {
                    let old_path = parts[1];
                    let new_path = parts[2];
                    if old_path == code_ref.path {
                        renamed_to = Some(new_path.to_string());
                    }
                } else if status_code == "D" && parts[1] == code_ref.path {
                    file_deleted = true;
                }
            }
        }
    }

    let target_path = renamed_to.as_deref().unwrap_or(&code_ref.path);

    // Diff -M -U0 between pinned_sha and tracking_sha
    let diff_output = git_runner.run_git(
        &clone.local_path,
        &[
            "diff",
            "-M",
            "-U0",
            &pinned_sha,
            &tracking_sha,
            "--",
            target_path,
            &code_ref.path,
        ],
    )?;

    // Parse hunks
    let hunk_re = Regex::new(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap();
    let mut line_delta: isize = 0;
    let mut lines_modified = false;
    let mut lines_deleted = file_deleted;
    let mut total_added: usize = 0;
    let mut total_deleted: usize = 0;

    for line in diff_output.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            total_added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            total_deleted += 1;
        }

        if let Some(caps) = hunk_re.captures(line) {
            let old_start: usize = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
            let old_count: usize = caps.get(2).map_or(1, |m| m.as_str().parse().unwrap_or(1));
            let _new_start: usize = caps.get(3).unwrap().as_str().parse().unwrap_or(1);
            let new_count: usize = caps.get(4).map_or(1, |m| m.as_str().parse().unwrap_or(1));

            let old_end = if old_count == 0 {
                old_start
            } else {
                old_start + old_count - 1
            };

            // Check if hunk precedes pinned range
            let precedes = if old_count == 0 {
                old_start < p_line
            } else {
                old_end < p_line
            };

            if precedes {
                let delta = (new_count as isize) - (old_count as isize);
                line_delta += delta;
            } else if old_count > 0 && old_start <= p_end && old_end >= p_line {
                // Hunk overlaps pinned range
                if new_count == 0 && old_start <= p_line && old_end >= p_end {
                    lines_deleted = true;
                } else {
                    lines_modified = true;
                }
            }
        }
    }

    let diff_summary = format!("+{total_added} -{total_deleted} on {tracking_branch}");

    let mut proposed_line = ((p_line as isize) + line_delta).max(1) as usize;
    let mut proposed_end_line = code_ref
        .end_line
        .map(|e| ((e as isize) + line_delta).max(1) as usize);
    let mut confidence = DriftConfidence::DiffMapped;

    // Symbol fallback if changed or deleted
    if (lines_modified || lines_deleted)
        && let Some(ref sym) = code_ref.symbol
        && !sym.is_empty()
    {
        // 1. Search in target file at tracking_sha
        if let Ok(grep_out) = git_runner.run_git(
            &clone.local_path,
            &["grep", "-n", "-F", sym, &tracking_sha, "--", target_path],
        ) && let Some(first_line) = grep_out.lines().next()
        {
            let parts: Vec<&str> = first_line.split(':').collect();
            if parts.len() >= 3
                && let Ok(num) = parts[2].parse::<usize>()
            {
                proposed_line = num;
                proposed_end_line = None;
                lines_deleted = false;
                confidence = DriftConfidence::SymbolMatched;
            }
        }

        // 2. If deleted, search anywhere in tracking_sha
        if lines_deleted
            && let Ok(grep_out) =
                git_runner.run_git(&clone.local_path, &["grep", "-n", "-F", sym, &tracking_sha])
            && let Some(first_line) = grep_out.lines().next()
        {
            let parts: Vec<&str> = first_line.split(':').collect();
            if parts.len() >= 3
                && let Ok(num) = parts[2].parse::<usize>()
            {
                let new_file = parts[1];
                renamed_to = Some(new_file.to_string());
                proposed_line = num;
                proposed_end_line = None;
                lines_deleted = false;
                confidence = DriftConfidence::SymbolMatched;
            }
        }
    }

    let status = if lines_deleted {
        DriftStatus::Gone {
            reason: if file_deleted {
                "file deleted".to_string()
            } else {
                "lines deleted".to_string()
            },
        }
    } else if let Some(new_path) = renamed_to {
        DriftStatus::Renamed {
            new_path,
            new_line: proposed_line,
            new_end_line: proposed_end_line,
        }
    } else if lines_modified {
        DriftStatus::Changed {
            new_line: proposed_line,
            diff_summary,
        }
    } else if line_delta != 0 {
        DriftStatus::Moved {
            new_line: proposed_line,
            new_end_line: proposed_end_line,
        }
    } else {
        DriftStatus::Current
    };

    Ok(DriftReport {
        entry_id: entry.id().to_string(),
        repo: code_ref.repo.clone(),
        status,
        pinned_ref: code_ref.git_ref.clone(),
        pinned_path: code_ref.path.clone(),
        pinned_line: p_line,
        pinned_end_line: code_ref.end_line.map(|l| l as usize),
        tracking_branch,
        tracking_ref: Some(tracking_sha),
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::clone::tests::SystemGitRunner;
    use crate::code::clone::{LocalClone, RemoteHost};
    use crate::entry::parse_entry;
    use std::path::PathBuf;

    fn setup_fixture_repo() -> (tempfile::TempDir, PathBuf, serde_json::Value) {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("repo");
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/fixtures/git_repos/scripted_repo.sh");

        let output = std::process::Command::new("bash")
            .arg(script)
            .arg(&target)
            .output()
            .expect("scripted_repo.sh run");
        assert!(
            output.status.success(),
            "script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let manifest_bytes = std::fs::read(target.join("repo_manifest.json")).expect("manifest");
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).expect("json");
        (dir, target, manifest)
    }

    #[test]
    fn test_drift_detection_across_all_branches() {
        let (_dir, repo_path, manifest) = setup_fixture_repo();
        let runner = SystemGitRunner;

        let pinned_commit = manifest["pinned_commit"].as_str().unwrap();

        // 1. Current (pinned commit == tracking commit)
        let clone_main = LocalClone {
            repo_identifier: "org/repo".to_string(),
            local_path: repo_path.clone(),
            remote_url: "git@github.com:org/repo.git".to_string(),
            host: RemoteHost::GitHub,
            tracking_branch: "main".to_string(),
            has_local_changes: false,
        };
        let raw_current = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: main\n---\nNotes.\n"
        );
        let entry_current = parse_entry("auth", "auth.md", raw_current.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_current = check_drift_impl(&entry_current, &clone_main, &runner).unwrap();
        assert_eq!(report_current.status, DriftStatus::Current);
        assert_eq!(report_current.confidence, DriftConfidence::DiffMapped);

        // 2. Moved (branch-moved has 5 lines inserted above line 8 -> now at line 13)
        let raw_moved = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: branch-moved\n---\nNotes.\n"
        );
        let entry_moved = parse_entry("auth", "auth.md", raw_moved.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_moved = check_drift_impl(&entry_moved, &clone_main, &runner).unwrap();
        assert_eq!(
            report_moved.status,
            DriftStatus::Moved {
                new_line: 13,
                new_end_line: Some(18),
            }
        );
        assert_eq!(report_moved.confidence, DriftConfidence::DiffMapped);

        // 3. Changed (branch-changed modified method body lines 9..10)
        let raw_changed = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: branch-changed\n---\nNotes.\n"
        );
        let entry_changed = parse_entry("auth", "auth.md", raw_changed.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_changed = check_drift_impl(&entry_changed, &clone_main, &runner).unwrap();
        match report_changed.status {
            DriftStatus::Changed { new_line, .. } => assert_eq!(new_line, 8),
            other => panic!("expected Changed, got {other:?}"),
        }
        assert_eq!(report_changed.confidence, DriftConfidence::DiffMapped);

        // 4. Renamed (branch-renamed renamed to TokenHandler.java)
        let raw_renamed = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: branch-renamed\n---\nNotes.\n"
        );
        let entry_renamed = parse_entry("auth", "auth.md", raw_renamed.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_renamed = check_drift_impl(&entry_renamed, &clone_main, &runner).unwrap();
        assert_eq!(
            report_renamed.status,
            DriftStatus::Renamed {
                new_path: "src/TokenHandler.java".to_string(),
                new_line: 8,
                new_end_line: Some(13),
            }
        );
        assert_eq!(report_renamed.confidence, DriftConfidence::DiffMapped);

        // 5. Gone (branch-gone removed the method)
        let raw_gone = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: branch-gone\n---\nNotes.\n"
        );
        let entry_gone = parse_entry("auth", "auth.md", raw_gone.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_gone = check_drift_impl(&entry_gone, &clone_main, &runner).unwrap();
        match report_gone.status {
            DriftStatus::Gone { .. } => {}
            other => panic!("expected Gone, got {other:?}"),
        }

        // 6. Symbol fallback (branch-symbol rewrote method signature, but symbol exists at line 9)
        let raw_symbol = format!(
            "---\nkb_format: 1\ntype: code\ntitle: AuthHandler\ncode:\n  repo: org/repo\n  path: src/AuthHandler.java\n  line: 8\n  end_line: 13\n  ref: {pinned_commit}\n  branch: branch-symbol\n  symbol: handleRequest\n---\nNotes.\n"
        );
        let entry_symbol = parse_entry("auth", "auth.md", raw_symbol.as_bytes().to_vec())
            .entry
            .unwrap();
        let report_symbol = check_drift_impl(&entry_symbol, &clone_main, &runner).unwrap();
        match report_symbol.status {
            DriftStatus::Changed { new_line, .. } => assert_eq!(new_line, 9),
            DriftStatus::Moved { new_line, .. } => assert_eq!(new_line, 9),
            other => panic!("expected symbol matched line 9, got {other:?}"),
        }
        assert_eq!(report_symbol.confidence, DriftConfidence::SymbolMatched);
    }
}
