//! Local repository status inspection and dirty file detection.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::git::{GitError, GitRunner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncommittedEntry {
    pub id: String,
    pub rel_path: PathBuf,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitStatus {
    pub is_git: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub uncommitted: Vec<UncommittedEntry>,
    pub unmanaged_changes: Vec<PathBuf>,
}

impl GitStatus {
    pub fn not_a_repo() -> Self {
        Self {
            is_git: false,
            ..Default::default()
        }
    }

    pub fn query(runner: &impl GitRunner, cwd: &Path) -> Result<Self, GitError> {
        let inside = runner.run_git(cwd, &["rev-parse", "--is-inside-work-tree"]);
        match inside {
            Ok(out) if out.trim() == "true" => {}
            _ => return Ok(Self::not_a_repo()),
        }

        let porcelain = runner.run_git(
            cwd,
            &["status", "--porcelain=v1", "-b", "--untracked-files=all"],
        )?;
        Ok(Self::parse_porcelain(&porcelain))
    }

    pub fn parse_porcelain(output: &str) -> Self {
        let mut status = GitStatus {
            is_git: true,
            ..Default::default()
        };

        for line in output.lines() {
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("##") {
                parse_header(line, &mut status);
                continue;
            }

            if line.len() < 3 {
                continue;
            }

            let code = &line[0..2];
            let raw_path = line[3..].trim();

            let (target_path_str, is_rename) = if code.starts_with('R') || code.ends_with('R') {
                if let Some(arrow_idx) = raw_path.find(" -> ") {
                    (&raw_path[arrow_idx + 4..], true)
                } else {
                    (raw_path, true)
                }
            } else {
                (raw_path, false)
            };

            let path_clean = target_path_str.trim_matches('"');
            let path = PathBuf::from(path_clean);

            if is_internal_kb_path(&path) {
                continue;
            }

            let file_status = if is_rename {
                FileStatus::Renamed
            } else if code.contains('?') || code.contains('A') {
                FileStatus::Added
            } else if code.contains('D') {
                FileStatus::Deleted
            } else {
                FileStatus::Modified
            };

            if is_entry_path(&path) {
                let id = entry_id_from_path(&path);
                status.uncommitted.push(UncommittedEntry {
                    id,
                    rel_path: path,
                    status: file_status,
                });
            } else {
                status.unmanaged_changes.push(path);
            }
        }

        status
    }
}

/// [`GitStatus`] plus what `kb sync status` adds, all read locally (no network).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    #[serde(flatten)]
    pub git: GitStatus,
    /// When `.git/FETCH_HEAD` was last written, in epoch seconds.
    pub last_fetch: Option<u64>,
    pub in_progress: Option<crate::sync::SyncInProgressState>,
}

impl SyncStatus {
    pub fn query(runner: &impl GitRunner, root: &Path) -> Result<Self, GitError> {
        let git = GitStatus::query(runner, root)?;
        let last_fetch = if git.is_git {
            runner
                .run_git(root, &["rev-parse", "--git-path", "FETCH_HEAD"])
                .ok()
                .map(|p| root.join(p.trim()))
                .and_then(|p| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        } else {
            None
        };
        Ok(Self {
            git,
            last_fetch,
            in_progress: crate::sync::state::load(root),
        })
    }
}

fn parse_header(line: &str, status: &mut GitStatus) {
    let rest = line.strip_prefix("##").unwrap_or("").trim();
    if rest.starts_with("Initial commit on ") {
        status.branch = Some(
            rest.strip_prefix("Initial commit on ")
                .unwrap()
                .trim()
                .to_string(),
        );
        return;
    }
    if rest.starts_with("No commits yet on ") {
        status.branch = Some(
            rest.strip_prefix("No commits yet on ")
                .unwrap()
                .trim()
                .to_string(),
        );
        return;
    }
    if rest.starts_with("HEAD (no branch)") {
        status.branch = None;
        return;
    }

    let (branch_part, bracket_part) = if let Some(bracket_start) = rest.find('[') {
        let bracket_end = rest.rfind(']').unwrap_or(rest.len());
        (
            rest[..bracket_start].trim(),
            Some(&rest[bracket_start + 1..bracket_end]),
        )
    } else {
        (rest, None)
    };

    if let Some((local, upstream)) = branch_part.split_once("...") {
        status.branch = Some(local.trim().to_string());
        status.upstream = Some(upstream.trim().to_string());
    } else if !branch_part.is_empty() {
        status.branch = Some(branch_part.to_string());
    }

    if let Some(bracket) = bracket_part {
        for piece in bracket.split(',') {
            let piece = piece.trim();
            if let Some(ahead_str) = piece.strip_prefix("ahead ") {
                status.ahead = ahead_str.trim().parse().unwrap_or(0);
            } else if let Some(behind_str) = piece.strip_prefix("behind ") {
                status.behind = behind_str.trim().parse().unwrap_or(0);
            }
        }
    }
}

fn is_internal_kb_path(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy() == ".kb")
}

fn is_entry_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "md" && ext != "markdown" {
        return false;
    }

    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if file_stem.eq_ignore_ascii_case("readme") {
        return false;
    }

    for comp in path.components() {
        let name = comp.as_os_str().to_string_lossy();
        if name.starts_with('.') {
            return false;
        }
    }

    true
}

fn entry_id_from_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for comp in path.components() {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }
    let joined = parts.join("/");
    if let Some(stripped) = joined.strip_suffix(".md") {
        stripped.to_string()
    } else if let Some(stripped) = joined.strip_suffix(".markdown") {
        stripped.to_string()
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FakeGitRunner;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_parse_porcelain_branch_and_tracking() {
        let porcelain = "## main...origin/main [ahead 2, behind 1]\n";
        let status = GitStatus::parse_porcelain(porcelain);
        assert!(status.is_git);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 2);
        assert_eq!(status.behind, 1);
        assert!(status.uncommitted.is_empty());
        assert!(status.unmanaged_changes.is_empty());

        let porcelain_ahead_only = "## feature...origin/feature [ahead 3]\n";
        let s2 = GitStatus::parse_porcelain(porcelain_ahead_only);
        assert_eq!(s2.ahead, 3);
        assert_eq!(s2.behind, 0);

        let porcelain_behind_only = "## feature...origin/feature [behind 4]\n";
        let s3 = GitStatus::parse_porcelain(porcelain_behind_only);
        assert_eq!(s3.ahead, 0);
        assert_eq!(s3.behind, 4);

        let porcelain_no_upstream = "## local-branch\n";
        let s4 = GitStatus::parse_porcelain(porcelain_no_upstream);
        assert_eq!(s4.branch.as_deref(), Some("local-branch"));
        assert_eq!(s4.upstream, None);
        assert_eq!(s4.ahead, 0);
        assert_eq!(s4.behind, 0);
    }

    #[test]
    fn test_parse_porcelain_entries_and_unmanaged_files() {
        let porcelain = "\
## main...origin/main
 M cw/authorizer.md
?? cmd/ping.md
D  old/foo.md
R  old/id.md -> new/id.md
 M README.md
?? scripts/deploy.sh
?? .gitignore
 M .kb/index.db
";
        let status = GitStatus::parse_porcelain(porcelain);
        assert_eq!(status.uncommitted.len(), 4);
        assert_eq!(status.uncommitted[0].id, "cw/authorizer");
        assert_eq!(status.uncommitted[0].status, FileStatus::Modified);

        assert_eq!(status.uncommitted[1].id, "cmd/ping");
        assert_eq!(status.uncommitted[1].status, FileStatus::Added);

        assert_eq!(status.uncommitted[2].id, "old/foo");
        assert_eq!(status.uncommitted[2].status, FileStatus::Deleted);

        assert_eq!(status.uncommitted[3].id, "new/id");
        assert_eq!(status.uncommitted[3].status, FileStatus::Renamed);

        assert_eq!(
            status.unmanaged_changes,
            vec![
                PathBuf::from("README.md"),
                PathBuf::from("scripts/deploy.sh"),
                PathBuf::from(".gitignore"),
            ]
        );
    }

    #[test]
    fn test_query_status_not_a_git_repo() {
        let fake = FakeGitRunner::with_stdout("true\n");
        let status = GitStatus::query(&fake, Path::new("/tmp")).unwrap();
        assert!(status.is_git);

        let fake_fail = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                Err(crate::git::GitError::CommandFailed {
                    exit_code: Some(128),
                    stderr: "fatal: not a git repository".into(),
                })
            } else {
                Ok(crate::git::GitCommandResult {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        });
        let status_fail = GitStatus::query(&fake_fail, Path::new("/tmp")).unwrap();
        assert!(!status_fail.is_git);
    }

    #[test]
    fn sync_status_reads_counts_fetch_time_and_serializes_as_the_contract_says() {
        use crate::git::{FakeGitRunner, GitCommandResult};
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("FETCH_HEAD"), "x").unwrap();
        let git = FakeGitRunner::with_handler(|_, args| {
            let out = match args {
                ["rev-parse", "--is-inside-work-tree"] => "true\n",
                ["rev-parse", "--git-path", _] => "FETCH_HEAD\n",
                ["status", ..] => "## main...origin/main [ahead 2, behind 3]\n M cw/p99.md\n",
                _ => "",
            };
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: out.into(),
                stderr: String::new(),
            })
        });
        let st = SyncStatus::query(&git, dir.path()).unwrap();
        assert_eq!((st.git.ahead, st.git.behind), (2, 3));
        assert!(st.last_fetch.is_some());
        let v = serde_json::to_value(&st).unwrap();
        assert_eq!(v["upstream"], "origin/main");
        assert_eq!(v["uncommitted"][0]["id"], "cw/p99");
        assert_eq!(v["uncommitted"][0]["status"], "modified");
        assert!(v["in_progress"].is_null());
    }
}
