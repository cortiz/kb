//! Remote sync engine, pipeline, and conflict handling.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git::secrets::{SecretFinding, SecretGate};
use crate::git::{GitError, GitRunner};

pub use crate::config::SyncStrategy;
pub use conflict::{
    Conflict, ConflictError, ConflictHandler, ConflictKind, Resolution, ResolutionChoice,
};
pub use pipeline::{SyncConfig, SyncEngine, SyncOptions};
pub use state::SyncInProgressState;

pub mod conflict;
pub mod pipeline;
pub mod state;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitSummary {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// `git log` format string that [`parse_log`] reads.
pub const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%aI%x1f%s";

pub fn parse_log(out: &str) -> Vec<CommitSummary> {
    out.lines()
        .filter_map(|l| {
            let mut f = l.split('\u{1f}');
            Some(CommitSummary {
                hash: f.next()?.to_string(),
                short_hash: f.next()?.to_string(),
                author: f.next()?.to_string(),
                date: f.next()?.to_string(),
                message: f.next()?.to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    /// HEAD before anything is committed or integrated.
    pub pre_sync_commit: String,
    /// Entry files with uncommitted changes, to be committed as one batch.
    pub hand_edits: Vec<PathBuf>,
    pub commits_to_push: Vec<CommitSummary>,
    pub commits_to_integrate: Vec<CommitSummary>,
    pub strategy: SyncStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncSummary {
    pub local_added: usize,
    pub local_modified: usize,
    pub local_deleted: usize,
    pub remote_added: usize,
    pub remote_modified: usize,
    pub remote_deleted: usize,
    pub conflicts_resolved: usize,
    pub warnings: Vec<String>,
    pub allowed_secrets: Vec<String>,
    pub unmanaged_files: Vec<PathBuf>,
    pub new_status: String,
}

#[derive(Debug)]
pub enum SyncError {
    NotGitRepository,
    NoUpstreamBranch,
    /// A sync is already waiting for `--continue` or `--abort`.
    InProgress,
    NothingInProgress,
    DirtyUnmanagedFiles(Vec<PathBuf>),
    SecretFound(Vec<SecretFinding>),
    HistoricalSecretFound(Vec<SecretFinding>),
    PushRejectedTwice,
    /// Conflicts were left for `kb sync --continue` (non-interactive run).
    ConflictsPending(Vec<PathBuf>),
    ConflictAborted,
    Git(GitError),
    Io(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |v: &[PathBuf]| {
            v.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            Self::NotGitRepository => write!(
                f,
                "the KB is not a git repository (try `kb sync setup <url>`)"
            ),
            Self::NoUpstreamBranch => {
                write!(f, "the branch has no upstream (try `kb sync setup <url>`)")
            }
            Self::InProgress => write!(
                f,
                "a sync is in progress; run `kb sync --continue` or `kb sync --abort`"
            ),
            Self::NothingInProgress => write!(f, "no sync is in progress"),
            Self::DirtyUnmanagedFiles(v) => write!(
                f,
                "commit or stash changes to non-entry files first: {}",
                list(v)
            ),
            Self::SecretFound(v) | Self::HistoricalSecretFound(v) => {
                write!(
                    f,
                    "secrets found, nothing was {}:",
                    if matches!(self, Self::SecretFound(_)) {
                        "committed"
                    } else {
                        "pushed"
                    }
                )?;
                for s in v {
                    write!(
                        f,
                        "\n  {}:{} {}{}",
                        s.path,
                        s.line,
                        s.message,
                        s.commit
                            .as_deref()
                            .map(|c| format!(" (commit {})", &c[..c.len().min(7)]))
                            .unwrap_or_default()
                    )?;
                }
                if matches!(self, Self::HistoricalSecretFound(_)) {
                    write!(
                        f,
                        "\nrewrite local history (git rebase -i / git reset) to remove them, then sync again"
                    )?;
                }
                Ok(())
            }
            Self::PushRejectedTwice => write!(
                f,
                "push rejected twice (the remote keeps moving); the KB is up to date with the last fetch, run `kb sync` again"
            ),
            Self::ConflictsPending(v) => write!(
                f,
                "conflicts need resolving ({}); run `kb sync --continue` or `kb sync --abort`",
                list(v)
            ),
            Self::ConflictAborted => write!(
                f,
                "sync aborted; the KB is back to where it was before the sync"
            ),
            Self::Git(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<GitError> for SyncError {
    fn from(e: GitError) -> Self {
        Self::Git(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    pub branch: String,
    /// False when everything was already committed.
    pub committed: bool,
}

/// Git init if needed, ignore `.kb/`, scan for secrets, commit everything as
/// `kb: initial import` (`kb init --git`, and the first half of `kb sync setup`).
pub fn init_repo(
    root: &Path,
    git: &impl GitRunner,
    branch: Option<&str>,
) -> Result<SetupReport, SyncError> {
    let in_repo = git
        .run_git(root, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|o| o.trim() == "true");
    let current = if in_repo {
        git.run_git(root, &["branch", "--show-current"])?
            .trim()
            .to_string()
    } else {
        String::new()
    };
    let branch = branch
        .map(str::to_string)
        .or_else(|| Some(current.clone()).filter(|b| !b.is_empty()))
        .unwrap_or_else(|| "main".into());
    if !in_repo {
        git.run_git(root, &["init", "-q", "-b", &branch])?;
    }

    let ignore = root.join(".gitignore");
    let existing = fs::read_to_string(&ignore).unwrap_or_default();
    if !existing.lines().any(|l| matches!(l.trim(), ".kb" | ".kb/")) {
        let sep = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        fs::write(&ignore, format!("{existing}{sep}.kb/\n"))
            .map_err(|e| SyncError::Io(e.to_string()))?;
    }

    let listed = git.run_git(
        root,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    )?;
    let entries: Vec<PathBuf> = listed
        .lines()
        .filter(|l| l.ends_with(".md"))
        .map(PathBuf::from)
        .collect();
    let report = SecretGate::check_entries(root, &entries)?;
    if !report.is_clean() {
        return Err(SyncError::SecretFound(report.blocked));
    }

    git.run_git(root, &["add", "-A"])?;
    let dirty = !git
        .run_git(root, &["status", "--porcelain"])?
        .trim()
        .is_empty();
    if dirty {
        git.run_git(root, &["commit", "-m", "kb: initial import"])?;
    }

    Ok(SetupReport {
        branch,
        committed: dirty,
    })
}

/// `kb sync setup`: [`init_repo`], then point `origin` at `remote_url` and push with upstream
/// tracking. Safe to run again.
pub fn setup(
    root: &Path,
    git: &impl GitRunner,
    remote_url: &str,
    branch: Option<&str>,
    timeout: Duration,
) -> Result<SetupReport, SyncError> {
    let report = init_repo(root, git, branch)?;
    let branch = report.branch.clone();
    let remote_exists = git.run_git(root, &["remote", "get-url", "origin"]).is_ok();
    git.run_git(
        root,
        &[
            "remote",
            if remote_exists { "set-url" } else { "add" },
            "origin",
            remote_url,
        ],
    )?;

    // no upstream yet, so this scans the whole branch history
    let history = SecretGate::check_unpushed_history(root, git)?;
    if !history.is_clean() {
        return Err(SyncError::HistoricalSecretFound(history.blocked));
    }
    let refspec = format!("HEAD:{branch}");
    let res = git.run_git_status(root, &["push", "-u", "origin", &refspec], timeout)?;
    if !res.success() {
        return Err(GitError::CommandFailed {
            exit_code: res.exit_code,
            stderr: res.stderr,
        }
        .into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FakeGitRunner, GitCommandResult};

    fn fake(in_repo: bool, files: &'static str, dirty: bool) -> FakeGitRunner {
        FakeGitRunner::with_handler(move |_, args| {
            let ok = |out: &str| {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: out.into(),
                    stderr: String::new(),
                })
            };
            let fail = || {
                Ok(GitCommandResult {
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "no".into(),
                })
            };
            match args {
                ["rev-parse", "--is-inside-work-tree"] if !in_repo => fail(),
                ["rev-parse", "--is-inside-work-tree"] => ok("true\n"),
                ["rev-parse", "--verify", _] => fail(),
                ["branch", "--show-current"] => ok("trunk\n"),
                ["ls-files", ..] => ok(files),
                ["status", ..] if dirty => ok(" M a.md\n"),
                ["remote", "get-url", _] => fail(),
                _ => ok(""),
            }
        })
    }

    fn cmds(git: &FakeGitRunner) -> Vec<String> {
        git.calls().into_iter().map(|(_, a)| a.join(" ")).collect()
    }

    #[test]
    fn setup_initializes_ignores_commits_and_pushes() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake(false, "a.md\nREADME.txt\n", true);
        let r = setup(
            dir.path(),
            &git,
            "git@h:u/kb.git",
            None,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(
            r,
            SetupReport {
                branch: "main".into(),
                committed: true
            }
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            ".kb/\n"
        );
        let c = cmds(&git);
        assert!(c.contains(&"init -q -b main".to_string()));
        assert!(c.contains(&"commit -m kb: initial import".to_string()));
        assert!(c.contains(&"remote add origin git@h:u/kb.git".to_string()));
        assert_eq!(c.last().unwrap(), "push -u origin HEAD:main");
    }

    #[test]
    fn setup_is_idempotent_and_follows_the_active_branch() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target\n.kb/\n").unwrap();
        let git = fake(true, "", false);
        let r = setup(dir.path(), &git, "u", None, Duration::from_secs(5)).unwrap();
        assert_eq!(
            r,
            SetupReport {
                branch: "trunk".into(),
                committed: false
            }
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            "target\n.kb/\n"
        );
        let c = cmds(&git);
        assert!(
            !c.iter()
                .any(|s| s.starts_with("init") || s.starts_with("commit"))
        );
    }

    #[test]
    fn secrets_stop_setup_before_any_commit() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("leak.md"), "AKIAIOSFODNN7EXAMPLE\n").unwrap();
        let git = fake(true, "leak.md\n", true);
        let err = setup(dir.path(), &git, "u", Some("dev"), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, SyncError::SecretFound(_)));
        assert!(
            !cmds(&git)
                .iter()
                .any(|s| s.starts_with("commit") || s.starts_with("push"))
        );
    }

    #[test]
    fn parse_log_reads_fields() {
        let c = parse_log("abc123\u{1f}abc\u{1f}Ann\u{1f}2026-01-01T00:00:00Z\u{1f}kb: add x\n");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].short_hash, "abc");
        assert_eq!(c[0].message, "kb: add x");
    }
}
