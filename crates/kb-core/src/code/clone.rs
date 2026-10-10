//! Local git clone discovery and caching.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::config::Settings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteHost {
    GitHub,
    GitLab,
    Bitbucket,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalClone {
    pub repo_identifier: String,
    pub local_path: PathBuf,
    pub remote_url: String,
    pub host: RemoteHost,
    pub tracking_branch: String,
    pub has_local_changes: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloneCache {
    #[serde(default)]
    pub entries: HashMap<String, CachedCloneEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedCloneEntry {
    pub path: PathBuf,
    pub remote_url: String,
    pub tracking_branch: String,
    pub checked_at: u64,
}

impl CloneCache {
    pub fn load(path: &Path) -> Self {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(cache) = serde_json::from_slice(&bytes)
        {
            return cache;
        }
        CloneCache::default()
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    CommandFailed { exit_code: Option<i32>, stderr: String },
    NotFound(String),
    Io(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::CommandFailed { exit_code, stderr } => {
                write!(f, "git command failed ({:?}): {}", exit_code, stderr)
            }
            GitError::NotFound(s) => write!(f, "not found: {s}"),
            GitError::Io(s) => write!(f, "io error: {s}"),
        }
    }
}

impl std::error::Error for GitError {}

pub trait GitRunner {
    fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError>;
}


/// Normalizes a git remote URL into RemoteHost and canonical `owner/repo` (or path).
pub fn normalize_remote_url(url: &str) -> (RemoteHost, String) {
    let mut s = url.trim();
    if s.ends_with(".git") {
        s = &s[..s.len() - 4];
    }
    if let Some(stripped) = s.strip_prefix("ssh://") {
        s = stripped;
    }
    let (host, path) = if let Some(at_idx) = s.find('@') {
        let after_at = &s[at_idx + 1..];
        if let Some(colon_idx) = after_at.find(':') {
            (&after_at[..colon_idx], &after_at[colon_idx + 1..])
        } else if let Some(slash_idx) = after_at.find('/') {
            (&after_at[..slash_idx], &after_at[slash_idx + 1..])
        } else {
            (after_at, "")
        }
    } else if let Some(proto_idx) = s.find("://") {
        let after_proto = &s[proto_idx + 3..];
        if let Some(slash_idx) = after_proto.find('/') {
            (&after_proto[..slash_idx], &after_proto[slash_idx + 1..])
        } else {
            (after_proto, "")
        }
    } else if let Some(colon_idx) = s.find(':') {
        (&s[..colon_idx], &s[colon_idx + 1..])
    } else if s.contains('/') && !s.split('/').next().unwrap_or("").contains('.') {
        ("github.com", s)
    } else if let Some(slash_idx) = s.find('/') {
        (&s[..slash_idx], &s[slash_idx + 1..])
    } else {
        ("", s)
    };

    let clean_path = path.trim_start_matches('/').to_string();
    let host_lower = host.to_lowercase();
    let remote_host = match host_lower.as_str() {
        "github.com" => RemoteHost::GitHub,
        "gitlab.com" => RemoteHost::GitLab,
        "bitbucket.org" => RemoteHost::Bitbucket,
        other => RemoteHost::Custom(other.to_string()),
    };

    (remote_host, clean_path)
}

pub fn resolve_default_tracking_branch(cwd: &Path, git_runner: &impl GitRunner) -> String {
    if let Ok(out) = git_runner.run_git(cwd, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        let branch = out.trim();
        if !branch.is_empty() && branch != "origin/HEAD" {
            return branch.to_string();
        }
    }
    if git_runner.run_git(cwd, &["rev-parse", "--verify", "origin/main"]).is_ok() {
        return "origin/main".to_string();
    }
    if git_runner.run_git(cwd, &["rev-parse", "--verify", "origin/master"]).is_ok() {
        return "origin/master".to_string();
    }
    "origin/main".to_string()
}

pub fn check_dirty(cwd: &Path, git_runner: &impl GitRunner) -> bool {
    git_runner
        .run_git(cwd, &["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub fn find_clone_impl(
    repo_id: &str,
    settings: &Settings,
    cache: &mut CloneCache,
    git_runner: &impl GitRunner,
) -> Result<Option<LocalClone>, GitError> {
    let clean_id = repo_id.trim().trim_end_matches(".git").trim_start_matches('/');
    let home = std::env::var_os("HOME").map(PathBuf::from);

    // 1. Explicit config mappings: [code.repos]
    for (cfg_repo, cfg_path) in settings.code_repos() {
        let clean_cfg_repo = cfg_repo.trim().trim_end_matches(".git");
        if clean_cfg_repo == clean_id
            || clean_cfg_repo.ends_with(&format!("/{clean_id}"))
            || clean_id.ends_with(&format!("/{clean_cfg_repo}"))
        {
            let path = crate::root::expand_tilde(Path::new(cfg_path), home.as_deref());
            if path.exists() && path.is_dir() {
                let remote_url = git_runner
                    .run_git(&path, &["remote", "get-url", "origin"])
                    .unwrap_or_else(|_| clean_cfg_repo.to_string())
                    .trim()
                    .to_string();
                let (host, _) = normalize_remote_url(&remote_url);
                let tracking_branch = resolve_default_tracking_branch(&path, git_runner);
                let has_local_changes = check_dirty(&path, git_runner);
                return Ok(Some(LocalClone {
                    repo_identifier: repo_id.to_string(),
                    local_path: path,
                    remote_url,
                    host,
                    tracking_branch,
                    has_local_changes,
                }));
            }
        }
    }

    // 2. Check cache
    if let Some(cached) = cache.entries.get(clean_id) {
        if cached.path.exists() && cached.path.is_dir() {
            let (host, _) = normalize_remote_url(&cached.remote_url);
            let has_local_changes = check_dirty(&cached.path, git_runner);
            return Ok(Some(LocalClone {
                repo_identifier: repo_id.to_string(),
                local_path: cached.path.clone(),
                remote_url: cached.remote_url.clone(),
                host,
                tracking_branch: cached.tracking_branch.clone(),
                has_local_changes,
            }));
        } else {
            cache.entries.remove(clean_id);
        }
    }

    // 3. One-level deep scan in code.search_paths
    for search_path_str in settings.code_search_paths() {
        let search_path = crate::root::expand_tilde(Path::new(search_path_str), home.as_deref());
        if !search_path.exists() || !search_path.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&search_path) {
            for entry in entries.flatten() {
                let sub_path = entry.path();
                if sub_path.is_dir()
                    && sub_path.join(".git").exists()
                    && let Ok(url_out) = git_runner.run_git(&sub_path, &["remote", "get-url", "origin"])
                {
                    let remote_url = url_out.trim().to_string();
                    let (host, canonical) = normalize_remote_url(&remote_url);
                    if canonical == clean_id
                        || clean_id.ends_with(&format!("/{canonical}"))
                        || canonical.ends_with(&format!("/{clean_id}"))
                    {
                        let tracking_branch = resolve_default_tracking_branch(&sub_path, git_runner);
                        let has_local_changes = check_dirty(&sub_path, git_runner);
                        let checked_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        cache.entries.insert(
                            clean_id.to_string(),
                            CachedCloneEntry {
                                path: sub_path.clone(),
                                remote_url: remote_url.clone(),
                                tracking_branch: tracking_branch.clone(),
                                checked_at,
                            },
                        );

                        return Ok(Some(LocalClone {
                            repo_identifier: repo_id.to_string(),
                            local_path: sub_path,
                            remote_url,
                            host,
                            tracking_branch,
                            has_local_changes,
                        }));
                    }
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[derive(Debug, Clone, Default)]
    pub struct SystemGitRunner;

    impl GitRunner for SystemGitRunner {
        fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError> {
            let output = std::process::Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .map_err(|e| GitError::Io(e.to_string()))?;
            if !output.status.success() {
                return Err(GitError::CommandFailed {
                    exit_code: output.status.code(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
    }

    #[test]
    fn test_normalize_remote_url_github_ssh_and_https() {
        let (host1, path1) = normalize_remote_url("git@github.com:octocat/Hello-World.git");
        assert_eq!(host1, RemoteHost::GitHub);
        assert_eq!(path1, "octocat/Hello-World");

        let (host2, path2) = normalize_remote_url("https://github.com/octocat/Hello-World.git");
        assert_eq!(host2, RemoteHost::GitHub);
        assert_eq!(path2, "octocat/Hello-World");

        let (host3, path3) = normalize_remote_url("ssh://git@github.com/octocat/Hello-World");
        assert_eq!(host3, RemoteHost::GitHub);
        assert_eq!(path3, "octocat/Hello-World");
    }

    #[test]
    fn test_normalize_remote_url_gitlab_and_bitbucket_and_custom() {
        let (host1, path1) = normalize_remote_url("https://gitlab.com/group/subgroup/project.git");
        assert_eq!(host1, RemoteHost::GitLab);
        assert_eq!(path1, "group/subgroup/project");

        let (host2, path2) = normalize_remote_url("git@bitbucket.org:atlassian/repo.git");
        assert_eq!(host2, RemoteHost::Bitbucket);
        assert_eq!(path2, "atlassian/repo");

        let (host3, path3) = normalize_remote_url("git@git.internal.corp:corp/tools.git");
        assert_eq!(host3, RemoteHost::Custom("git.internal.corp".to_string()));
        assert_eq!(path3, "corp/tools");
    }

    struct MockRunner {
        origin_url: String,
        branch: String,
    }

    impl GitRunner for MockRunner {
        fn run_git(&self, _cwd: &Path, args: &[&str]) -> Result<String, GitError> {
            match args {
                ["remote", "get-url", "origin"] => Ok(self.origin_url.clone()),
                ["rev-parse", "--abbrev-ref", "origin/HEAD"] => Ok(self.branch.clone()),
                ["status", "--porcelain"] => Ok(String::new()),
                _ => Err(GitError::NotFound("unknown command".to_string())),
            }
        }
    }

    #[test]
    fn test_find_clone_in_repos_config() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_dir = tmp.path().join("my-repo");
        std::fs::create_dir_all(&repo_dir).unwrap();

        let mut settings = Settings::default();
        settings.code_repos_mut().insert("org/repo".to_string(), repo_dir.display().to_string());

        let mut cache = CloneCache::default();
        let runner = MockRunner {
            origin_url: "git@github.com:org/repo.git".to_string(),
            branch: "origin/main".to_string(),
        };

        let clone = find_clone_impl("org/repo", &settings, &mut cache, &runner).unwrap();
        assert!(clone.is_some());
        let c = clone.unwrap();
        assert_eq!(c.local_path, repo_dir);
        assert_eq!(c.host, RemoteHost::GitHub);
        assert_eq!(c.tracking_branch, "origin/main");
    }

    #[test]
    fn test_find_clone_search_paths_and_caching() {
        let tmp = tempfile::tempdir().unwrap();
        let search_dir = tmp.path().join("projects");
        let repo_dir = search_dir.join("discovered-repo");
        std::fs::create_dir_all(repo_dir.join(".git")).unwrap();

        let mut settings = Settings::default();
        settings.code_search_paths_mut().push(search_dir.display().to_string());

        let mut cache = CloneCache::default();
        let runner = MockRunner {
            origin_url: "https://github.com/my-org/my-tool.git".to_string(),
            branch: "origin/main".to_string(),
        };

        // 1. First discovery scans search_paths and populates cache
        let clone = find_clone_impl("my-org/my-tool", &settings, &mut cache, &runner).unwrap();
        assert!(clone.is_some());
        assert_eq!(clone.unwrap().local_path, repo_dir);
        assert!(cache.entries.contains_key("my-org/my-tool"));

        // 2. Cache invalidation on missing path
        std::fs::remove_dir_all(&repo_dir).unwrap();
        let clone_after_rm = find_clone_impl("my-org/my-tool", &settings, &mut cache, &runner).unwrap();
        assert!(clone_after_rm.is_none());
        assert!(!cache.entries.contains_key("my-org/my-tool"));
    }
}
