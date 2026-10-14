//! Secret scanning gates before commit and before push.

use crate::git::{GitError, GitRunner};
use crate::lint::RuleCode;
use crate::lint::secrets::scan_secrets;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    pub path: String,
    pub commit: Option<String>,
    pub line: usize,
    pub code: RuleCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedSecretSummary {
    pub path: String,
    pub commit: Option<String>,
    pub line: usize,
    pub code: RuleCode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretGateReport {
    pub blocked: Vec<SecretFinding>,
    pub allowed: Vec<AllowedSecretSummary>,
}

impl SecretGateReport {
    pub fn is_clean(&self) -> bool {
        self.blocked.is_empty()
    }
}

pub struct SecretGate;

impl SecretGate {
    pub fn check_text(
        path: &str,
        content: &str,
        commit: Option<&str>,
    ) -> (Vec<SecretFinding>, Vec<AllowedSecretSummary>) {
        let scan = scan_secrets(content);
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let get_line = |offset: usize| -> usize {
            match line_starts.binary_search(&offset) {
                Ok(idx) => idx + 1,
                Err(idx) => idx,
            }
        };

        let mut blocked = Vec::new();
        for hit in scan.hits {
            let line = get_line(hit.offset);
            blocked.push(SecretFinding {
                path: path.to_string(),
                commit: commit.map(str::to_string),
                line,
                code: hit.code,
                message: hit.message,
            });
        }

        let mut allowed = Vec::new();
        for (offset, code) in scan.suppressed {
            let line = get_line(offset);
            allowed.push(AllowedSecretSummary {
                path: path.to_string(),
                commit: commit.map(str::to_string),
                line,
                code,
            });
        }

        (blocked, allowed)
    }

    pub fn check_entries(root: &Path, files: &[PathBuf]) -> Result<SecretGateReport, GitError> {
        let mut report = SecretGateReport::default();
        for file in files {
            let full_path = root.join(file);
            if full_path.is_file()
                && let Ok(content) = std::fs::read_to_string(&full_path)
            {
                let path_str = file.to_string_lossy();
                let (blocked, allowed) = Self::check_text(&path_str, &content, None);
                report.blocked.extend(blocked);
                report.allowed.extend(allowed);
            }
        }
        Ok(report)
    }

    pub fn check_unpushed_history(
        root: &Path,
        git: &impl GitRunner,
    ) -> Result<SecretGateReport, GitError> {
        let mut report = SecretGateReport::default();
        let has_upstream = git
            .run_git(root, &["rev-parse", "--verify", "@{u}"])
            .is_ok();
        let range = if has_upstream { "@{u}..HEAD" } else { "HEAD" };
        let rev_list = git.run_git(root, &["rev-list", range]);
        let commits_str = match rev_list {
            Ok(c) => c,
            Err(_) => return Ok(report),
        };
        let commits: Vec<&str> = commits_str
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        for commit in commits {
            let diff_tree = git.run_git(
                root,
                &["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
            )?;
            for line in diff_tree.lines().map(str::trim).filter(|s| !s.is_empty()) {
                if line.ends_with(".md") && !line.starts_with(".kb/") {
                    let show_res = git.run_git(root, &["show", &format!("{commit}:{line}")]);
                    if let Ok(content) = show_res {
                        let (blocked, allowed) = Self::check_text(line, &content, Some(commit));
                        report.blocked.extend(blocked);
                        report.allowed.extend(allowed);
                    }
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FakeGitRunner, GitCommandResult};
    use std::fs;

    #[test]
    fn test_check_entries_clean_and_blocked_and_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let clean_path = root.join("clean.md");
        fs::write(
            &clean_path,
            "---\nid: clean\ntitle: Clean\ntype: cmd\n---\necho hello\n",
        )
        .unwrap();

        let leak_path = root.join("leak.md");
        fs::write(
            &leak_path,
            "---\nid: leak\ntitle: Leak\ntype: cmd\n---\nAKIAIOSFODNN7EXAMPLE\n",
        )
        .unwrap();

        let allowed_path = root.join("allowed.md");
        fs::write(
            &allowed_path,
            "---\nid: allowed\ntitle: Allowed\ntype: cmd\n---\nAKIAIOSFODNN7EXAMPLE # kb:allow-secret\n",
        )
        .unwrap();

        let report = SecretGate::check_entries(
            root,
            &[
                PathBuf::from("clean.md"),
                PathBuf::from("leak.md"),
                PathBuf::from("allowed.md"),
            ],
        )
        .unwrap();

        assert!(!report.is_clean());
        assert_eq!(report.blocked.len(), 1);
        assert_eq!(report.blocked[0].path, "leak.md");
        assert_eq!(report.blocked[0].code, RuleCode::SecretAwsAccessKey);

        assert_eq!(report.allowed.len(), 1);
        assert_eq!(report.allowed[0].path, "allowed.md");
        assert_eq!(report.allowed[0].code, RuleCode::SecretAwsAccessKey);
    }

    #[test]
    fn test_check_unpushed_history_detects_removed_secret() {
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"@{u}") && args.contains(&"--verify") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "origin/main\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"rev-list") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "commit2\ncommit1\n".into(),
                    stderr: String::new(),
                })
            } else if (args.contains(&"commit1") || args.contains(&"commit2"))
                && args.contains(&"--name-only")
            {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "cmd/leak.md\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"show") && args.contains(&"commit1:cmd/leak.md") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout:
                        "---\nid: cmd/leak\ntitle: Leak\ntype: cmd\n---\nAKIAIOSFODNN7EXAMPLE\n"
                            .into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"show") && args.contains(&"commit2:cmd/leak.md") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "---\nid: cmd/leak\ntitle: Fixed\ntype: cmd\n---\necho fixed\n".into(),
                    stderr: String::new(),
                })
            } else {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        });

        let report = SecretGate::check_unpushed_history(Path::new("/kb"), &fake).unwrap();
        assert!(!report.is_clean());
        assert_eq!(report.blocked.len(), 1);
        assert_eq!(report.blocked[0].commit, Some("commit1".to_string()));
        assert_eq!(report.blocked[0].path, "cmd/leak.md");
        assert_eq!(report.blocked[0].code, RuleCode::SecretAwsAccessKey);
    }
}
