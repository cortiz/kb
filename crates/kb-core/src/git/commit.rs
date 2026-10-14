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

/// Checks whether author identity (`user.name` and `user.email`) is configured. One git call:
/// every process costs ~15 ms and a save has a 100 ms budget on a big KB.
pub fn check_git_identity(root: &Path, git: &impl GitRunner) -> Result<(), String> {
    let out = git
        .run_git(root, &["config", "--get-regexp", r"^user\.(name|email)$"])
        .unwrap_or_default();
    let has = |key: &str| {
        out.lines().any(|l| {
            l.split_once(' ')
                .is_some_and(|(k, v)| k == key && !v.trim().is_empty())
        })
    };
    if has("user.name") && has("user.email") {
        Ok(())
    } else {
        Err("Git author identity not configured (user.name or user.email missing)".into())
    }
}

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

    // 1. Is the root inside a git working tree? A `.git` directory or file above it says so
    //    without a process (~15 ms of the 100 ms a save may cost on a large KB).
    if !root.ancestors().any(|d| d.join(".git").exists()) {
        return Ok(AutoCommitOutcome::SkippedNotGitRepo);
    }

    // 2. Check author identity
    if let Err(err) = check_git_identity(root, git) {
        return Ok(AutoCommitOutcome::SkippedMissingIdentity { error: err });
    }

    // 3. Format message
    let message = format_commit_message(settings.git_commit_template(), action, id);

    // 4. Commit exactly the given paths (`--only`), leaving everything else staged or not alone.
    //    New files must be added first; for the rest `--only` reads the working tree itself, so
    //    the `git add` process is only spent when git says it does not know a path.
    let path_strs: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let add = |paths: &[String]| -> Result<(), GitError> {
        let mut args = vec!["add", "-A", "--"];
        args.extend(paths.iter().map(String::as_str));
        match git.run_git(root, &args) {
            // already removed or staged by git mv / git rm
            Err(GitError::CommandFailed { stderr, .. })
                if stderr.contains("did not match any files") =>
            {
                Ok(())
            }
            other => other.map(|_| ()),
        }
    };
    let mut commit_args = vec!["commit", "--only", "-m", &message, "--"];
    commit_args.extend(path_strs.iter().map(String::as_str));
    if action == AutoCommitAction::Add {
        add(&path_strs)?;
    }
    let out = match git.run_git(root, &commit_args) {
        Err(GitError::CommandFailed { stderr, .. })
            if action != AutoCommitAction::Add && stderr.contains("did not match any file") =>
        {
            add(&path_strs)?;
            git.run_git(root, &commit_args)?
        }
        other => other?,
    };

    // 5. The new commit's hash is in git's own summary line: `[main abc1234] message`
    let from_summary = out
        .lines()
        .find_map(|l| l.strip_prefix('[')?.split_once(']'))
        .and_then(|(head, _)| head.split_whitespace().last().map(str::to_string));
    let commit_hash = match from_summary {
        Some(h) => h,
        None => git
            .run_git(root, &["rev-parse", "HEAD"])?
            .trim()
            .to_string(),
    };

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

    /// A directory that looks like a working tree to `auto_commit_operation`.
    fn work_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        dir
    }

    #[test]
    fn test_auto_commit_operation_success() {
        let tree = work_tree();
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"--get-regexp") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "user.name Alice\nuser.email alice@example.com\n".into(),
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
            tree.path(),
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
    fn untracked_file_on_edit_is_added_then_committed_once_more() {
        let tree = work_tree();
        use std::sync::atomic::{AtomicBool, Ordering};
        let first = AtomicBool::new(true);
        let fake = FakeGitRunner::with_handler(move |_, args| {
            let ok = |out: &str| {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: out.into(),
                    stderr: String::new(),
                })
            };
            match args {
                ["rev-parse", "--is-inside-work-tree"] => ok("true\n"),
                ["config", ..] => ok("user.name A\nuser.email a@x\n"),
                ["commit", ..] if first.swap(false, Ordering::SeqCst) => {
                    Err(crate::git::GitError::CommandFailed {
                        exit_code: Some(1),
                        stderr: "error: pathspec 'a.md' did not match any file(s) known to git"
                            .into(),
                    })
                }
                ["commit", ..] => ok("[main abc1234] kb: edit a\n 1 file changed\n"),
                _ => ok(""),
            }
        });
        let outcome = auto_commit_operation(
            tree.path(),
            &fake,
            &Settings::default(),
            AutoCommitAction::Edit,
            "a",
            &[PathBuf::from("a.md")],
        )
        .unwrap();
        assert!(
            matches!(outcome, AutoCommitOutcome::Committed { commit_hash, .. } if commit_hash == "abc1234")
        );
        let c: Vec<String> = fake
            .calls()
            .into_iter()
            .map(|(_, a)| a[0].clone())
            .collect();
        assert_eq!(c, ["config", "commit", "add", "commit"]);
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
        let fake = FakeGitRunner::new();
        let dir = tempfile::tempdir().unwrap();
        let outcome = auto_commit_operation(
            dir.path(),
            &fake,
            &Settings::default(),
            AutoCommitAction::Rm,
            "cmd/old",
            &[PathBuf::from("cmd/old.md")],
        )
        .unwrap();

        assert_eq!(outcome, AutoCommitOutcome::SkippedNotGitRepo);
        assert!(
            fake.calls().is_empty(),
            "no git process for a plain directory"
        );
    }

    #[test]
    fn test_auto_commit_operation_missing_identity() {
        let tree = work_tree();
        let fake = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: String::new(),
                })
            } else if args.contains(&"--get-regexp") {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "user.name Alice\n".into(),
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
            tree.path(),
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
