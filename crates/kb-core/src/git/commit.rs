//! Path-limited atomic auto-commits for entry writes, renames, and deletions.

use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::git::{GitError, GitRunner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCommitAction {
    Add,
    Edit,
    Mv,
    Rm,
    Repin,
    Restore,
}

impl AutoCommitAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutoCommitAction::Add => "add",
            AutoCommitAction::Edit => "edit",
            AutoCommitAction::Mv => "mv",
            AutoCommitAction::Rm => "rm",
            AutoCommitAction::Repin => "repin",
            AutoCommitAction::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoCommitOutcome {
    Committed {
        commit_hash: String,
        message: String,
    },
    SkippedNotGitRepo,
    SkippedDisabled,
    SkippedMissingIdentity {
        error: String,
    },
    Failed(String),
}

/// Checks whether commit signing (`commit.gpgsign`) is enabled in git config.
pub fn is_signing_configured(root: &Path, git: &impl GitRunner) -> bool {
    let out = git.run_git(root, &["config", "--get", "commit.gpgsign"]);
    match out {
        Ok(val) => val.trim().eq_ignore_ascii_case("true"),
        Err(_) => false,
    }
}

/// Checks whether author identity (`user.name` and `user.email`) is configured.
pub fn check_git_identity(root: &Path, git: &impl GitRunner) -> Result<(), String> {
    let name = git.run_git(root, &["config", "user.name"]);
    let email = git.run_git(root, &["config", "user.email"]);
    match (name, email) {
        (Ok(n), Ok(e)) if !n.trim().is_empty() && !e.trim().is_empty() => Ok(()),
        _ => Err("Git author identity not configured (user.name or user.email missing)".into()),
    }
}

/// Formats the commit message using the template from settings.
pub fn format_commit_message(template: &str, action: AutoCommitAction, id: &str) -> String {
    template
        .replace("{action}", action.as_str())
        .replace("{id}", id)
}

/// Performs a path-limited auto-commit for the specified action and paths.
pub fn auto_commit_operation(
    root: &Path,
    git: &impl GitRunner,
    settings: &Settings,
    action: AutoCommitAction,
    id: &str,
    paths: &[PathBuf],
) -> Result<AutoCommitOutcome, GitError> {
    if !settings.git_auto_commit() {
        return Ok(AutoCommitOutcome::SkippedDisabled);
    }

    // 1. Check if root is inside a git working tree
    let inside = git.run_git(root, &["rev-parse", "--is-inside-work-tree"]);
    match inside {
        Ok(out) if out.trim() == "true" => {}
        _ => return Ok(AutoCommitOutcome::SkippedNotGitRepo),
    }

    // 2. Check author identity
    if let Err(err) = check_git_identity(root, git) {
        return Ok(AutoCommitOutcome::SkippedMissingIdentity { error: err });
    }

    // 3. Format message
    let message = format_commit_message(settings.git_commit_template(), action, id);

    // 4. Stage specified paths with git add -A -- <path>
    let path_strs: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    for p in &path_strs {
        let add_res = git.run_git(root, &["add", "-A", "--", p]);
        if let Err(GitError::CommandFailed { stderr, .. }) = &add_res {
            if stderr.contains("did not match any files") {
                // The path was already removed/staged by git mv or git rm.
                continue;
            }
        }
        add_res?;
    }

    // 5. Commit specified paths with git commit --only -m <message> -- <paths>
    let mut commit_args = vec!["commit", "--only", "-m", &message, "--"];
    for p in &path_strs {
        commit_args.push(p.as_str());
    }
    git.run_git(root, &commit_args)?;

    // 6. Get commit hash
    let hash_out = git.run_git(root, &["rev-parse", "HEAD"])?;
    let commit_hash = hash_out.trim().to_string();

    Ok(AutoCommitOutcome::Committed {
        commit_hash,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::git::{FakeGitRunner, GitCommandResult};
    use std::path::{Path, PathBuf};

    #[test]
    fn test_auto_commit_operation_success() {
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"user.name") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "Alice\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"user.email") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "alice@example.com\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"HEAD") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "deadbeef0000\n".into(),
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

        let settings = Settings::default();
        let outcome = auto_commit_operation(
            Path::new("/kb"),
            &fake,
            &settings,
            AutoCommitAction::Add,
            "cw/authorizer",
            &[PathBuf::from("cw/authorizer.md")],
        )
        .unwrap();

        match outcome {
            AutoCommitOutcome::Committed {
                commit_hash,
                message,
            } => {
                assert_eq!(commit_hash, "deadbeef0000");
                assert_eq!(message, "kb: add cw/authorizer");
            }
            other => panic!("expected Committed, got {other:?}"),
        }

        let calls = fake.calls();
        assert!(calls.iter().any(|(_, args)| {
            args.contains(&"add".to_string()) && args.contains(&"cw/authorizer.md".to_string())
        }));
        assert!(calls.iter().any(|(_, args)| {
            args.contains(&"commit".to_string())
                && args.contains(&"--only".to_string())
                && args.contains(&"cw/authorizer.md".to_string())
        }));
    }

    #[test]
    fn test_auto_commit_operation_disabled() {
        let fake = FakeGitRunner::new();
        let mut settings = Settings::default();
        settings.set_text("git.auto_commit", "false");

        let outcome = auto_commit_operation(
            Path::new("/kb"),
            &fake,
            &settings,
            AutoCommitAction::Edit,
            "sql/query",
            &[PathBuf::from("sql/query.md")],
        )
        .unwrap();

        assert_eq!(outcome, AutoCommitOutcome::SkippedDisabled);
        assert_eq!(fake.calls().len(), 0);
    }

    #[test]
    fn test_auto_commit_operation_not_git_repo() {
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Err(crate::git::GitError::CommandFailed {
                    exit_code: Some(128),
                    stderr: "fatal: not a git repository".into(),
                })
            } else {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        });

        let settings = Settings::default();
        let outcome = auto_commit_operation(
            Path::new("/kb"),
            &fake,
            &settings,
            AutoCommitAction::Rm,
            "cmd/old",
            &[PathBuf::from("cmd/old.md")],
        )
        .unwrap();

        assert_eq!(outcome, AutoCommitOutcome::SkippedNotGitRepo);
    }

    #[test]
    fn test_auto_commit_operation_missing_identity() {
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"user.name") {
                Err(crate::git::GitError::CommandFailed {
                    exit_code: Some(1),
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

        let settings = Settings::default();
        let outcome = auto_commit_operation(
            Path::new("/kb"),
            &fake,
            &settings,
            AutoCommitAction::Add,
            "cmd/new",
            &[PathBuf::from("cmd/new.md")],
        )
        .unwrap();

        match outcome {
            AutoCommitOutcome::SkippedMissingIdentity { .. } => {}
            other => panic!("expected SkippedMissingIdentity, got {other:?}"),
        }
    }

    #[test]
    fn test_is_signing_configured() {
        let fake_signed = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"commit.gpgsign") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
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
        assert!(is_signing_configured(Path::new("/kb"), &fake_signed));

        let fake_unsigned = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"commit.gpgsign") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "false\n".into(),
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
        assert!(!is_signing_configured(Path::new("/kb"), &fake_unsigned));
    }
}
