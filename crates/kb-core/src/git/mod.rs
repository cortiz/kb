//! Git execution and repository abstraction.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub mod commit;
pub mod secrets;
pub mod status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl GitCommandResult {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    NotFound(String),
    CommandFailed { exit_code: Option<i32>, stderr: String },
    Io(String),
    TimedOut,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::CommandFailed { exit_code, stderr } => {
                write!(f, "git command failed ({:?}): {}", exit_code, stderr)
            }
            GitError::NotFound(s) => write!(f, "git not found: {s}"),
            GitError::Io(s) => write!(f, "git io error: {s}"),
            GitError::TimedOut => write!(f, "git operation timed out"),
        }
    }
}

impl std::error::Error for GitError {}

pub trait GitRunner: Send + Sync {
    /// Executes a git command in the specified directory, returning captured stdout if successful.
    fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError>;

    /// Executes a git command with timeout and returns exit code, stdout, and stderr.
    fn run_git_status(
        &self,
        cwd: &Path,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<GitCommandResult, GitError> {
        match self.run_git(cwd, args) {
            Ok(stdout) => Ok(GitCommandResult {
                exit_code: Some(0),
                stdout,
                stderr: String::new(),
            }),
            Err(GitError::CommandFailed { exit_code, stderr }) => Ok(GitCommandResult {
                exit_code,
                stdout: String::new(),
                stderr,
            }),
            Err(e) => Err(e),
        }
    }
}



type FakeHandler = dyn Fn(&Path, &[&str]) -> Result<GitCommandResult, GitError> + Send + Sync;

#[derive(Clone)]
pub struct FakeGitRunner {
    pub calls: Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>,
    handler: Option<Arc<FakeHandler>>,
}

impl FakeGitRunner {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            handler: None,
        }
    }

    pub fn with_stdout(stdout: &str) -> Self {
        let out = stdout.to_string();
        Self::with_handler(move |_, _| {
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: out.clone(),
                stderr: String::new(),
            })
        })
    }

    pub fn with_handler<F>(f: F) -> Self
    where
        F: Fn(&Path, &[&str]) -> Result<GitCommandResult, GitError> + Send + Sync + 'static,
    {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            handler: Some(Arc::new(f)),
        }
    }

    pub fn calls(&self) -> Vec<(PathBuf, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for FakeGitRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl GitRunner for FakeGitRunner {
    fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError> {
        let res = self.run_git_status(cwd, args, Duration::from_secs(30))?;
        if res.success() {
            Ok(res.stdout)
        } else {
            Err(GitError::CommandFailed {
                exit_code: res.exit_code,
                stderr: res.stderr,
            })
        }
    }

    fn run_git_status(
        &self,
        cwd: &Path,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<GitCommandResult, GitError> {
        self.calls.lock().unwrap().push((
            cwd.to_path_buf(),
            args.iter().map(|s| (*s).to_string()).collect(),
        ));
        if let Some(h) = &self.handler {
            h(cwd, args)
        } else {
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

pub fn parse_git_version(output: &str) -> Result<(u32, u32), String> {
    let trimmed = output.trim();
    let version_part = trimmed
        .strip_prefix("git version ")
        .or_else(|| trimmed.strip_prefix("git version"))
        .ok_or_else(|| format!("unexpected git version output: {output:?}"))?
        .trim();
    let num_part = version_part.split_whitespace().next().unwrap_or("");
    let mut segments = num_part.split('.');
    let major: u32 = segments
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("invalid major version in {output:?}"))?;
    let minor: u32 = segments
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("invalid minor version in {output:?}"))?;

    if major > 2 || (major == 2 && minor >= 28) {
        Ok((major, minor))
    } else {
        Err(format!("Git 2.28+ is required; found {major}.{minor}"))
    }
}

pub fn verify_git_version(runner: &impl GitRunner, cwd: &Path) -> Result<(u32, u32), GitError> {
    let stdout = runner.run_git(cwd, &["--version"])?;
    parse_git_version(&stdout).map_err(GitError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_fake_git_runner_and_version_check() {
        let fake = FakeGitRunner::new();
        assert_eq!(fake.calls().len(), 0);

        let v1 = parse_git_version("git version 2.39.3\n").unwrap();
        assert_eq!(v1, (2, 39));

        let v2 = parse_git_version("git version 2.39.3 (Apple Git-146)\n").unwrap();
        assert_eq!(v2, (2, 39));

        let v3 = parse_git_version("git version 2.27.0\n");
        assert!(v3.is_err());

        let fake_ok = FakeGitRunner::with_stdout("git version 2.34.1\n");
        assert!(verify_git_version(&fake_ok, Path::new("/tmp")).is_ok());

        let fake_old = FakeGitRunner::with_stdout("git version 2.25.0\n");
        assert!(verify_git_version(&fake_old, Path::new("/tmp")).is_err());
    }
}
