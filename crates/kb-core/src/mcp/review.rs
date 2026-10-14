//! Review state tracking and approval/reversion workflows for agent changes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::git::GitRunner;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StateJson {
    #[serde(default)]
    unreviewed: BTreeMap<String, bool>,
    #[serde(default)]
    reviewed_commits: BTreeMap<String, String>,
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".kb").join("state.json")
}

fn load_state_map(root: &Path) -> serde_json::Map<String, serde_json::Value> {
    let path = state_path(root);
    if let Ok(content) = fs::read_to_string(&path)
        && let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&content)
    {
        return map;
    }
    serde_json::Map::new()
}

fn load_state(root: &Path) -> StateJson {
    let map = load_state_map(root);
    let unreviewed = map
        .get("unreviewed")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let reviewed_commits = map
        .get("reviewed_commits")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    StateJson {
        unreviewed,
        reviewed_commits,
    }
}

fn save_state(root: &Path, state: &StateJson) -> std::io::Result<()> {
    let kb_dir = root.join(".kb");
    if !kb_dir.exists() {
        fs::create_dir_all(&kb_dir)?;
    }
    let mut map = load_state_map(root);
    if !map.contains_key("state_version") {
        map.insert("state_version".to_string(), serde_json::json!(1));
    }
    if !map.contains_key("usage") {
        map.insert("usage".to_string(), serde_json::json!({}));
    }
    map.insert(
        "unreviewed".to_string(),
        serde_json::to_value(&state.unreviewed).unwrap_or_default(),
    );
    map.insert(
        "reviewed_commits".to_string(),
        serde_json::to_value(&state.reviewed_commits).unwrap_or_default(),
    );

    let path = state_path(root);
    let json = serde_json::to_string_pretty(&map).map_err(std::io::Error::other)?;
    fs::write(path, json)
}

/// Finds the relative path of an entry file by its ID under `root`.
pub fn find_entry_rel_path(root: &Path, id: &str) -> Option<PathBuf> {
    let direct = root.join(format!("{id}.md"));
    if direct.is_file() {
        return direct.strip_prefix(root).ok().map(Path::to_path_buf);
    }
    // Search recursively for matching entry filename
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && entry.path().extension().is_some_and(|ext| ext == "md")
            && let Ok(rel) = entry.path().strip_prefix(root)
        {
            let stem = rel.with_extension("");
            let candidate_id = stem.to_string_lossy().to_string();
            if candidate_id == id {
                return Some(rel.to_path_buf());
            }
        }
    }
    None
}

/// Tracks and manages the human review state of agent-authored entries.
pub struct ReviewStateTracker;

impl ReviewStateTracker {
    /// Determines whether an entry is unreviewed (from local state cache).
    pub fn is_unreviewed(root: &Path, id: &str) -> bool {
        let state = load_state(root);
        state.unreviewed.get(id).copied().unwrap_or(false)
    }

    /// Determines whether an entry is unreviewed using a custom git runner.
    pub fn is_unreviewed_with_git(root: &Path, id: &str, git: &(impl GitRunner + ?Sized)) -> bool {
        // 1. Check local state.json first if marked unreviewed
        let state = load_state(root);
        if let Some(&unrev) = state.unreviewed.get(id) {
            return unrev;
        }

        // 2. Check git history if in a git repository
        let is_git = git
            .run_git(root, &["rev-parse", "--is-inside-work-tree"])
            .is_ok_and(|out| out.trim() == "true");

        if !is_git {
            return false;
        }

        let rel_path = match find_entry_rel_path(root, id) {
            Some(p) => p,
            None => return state.unreviewed.get(id).copied().unwrap_or(false),
        };
        let rel_str = rel_path.to_string_lossy();

        // Check the latest commit modifying this file
        let log_out = match git.run_git(
            root,
            &["log", "-n", "1", "--format=%H%x00%B", "--", &rel_str],
        ) {
            Ok(out) => out,
            Err(_) => return state.unreviewed.get(id).copied().unwrap_or(false),
        };

        let trimmed = log_out.trim();
        if trimmed.is_empty() {
            return state.unreviewed.get(id).copied().unwrap_or(false);
        }

        let (commit_hash, commit_msg) = match trimmed.split_once('\0') {
            Some((h, m)) => (h.trim(), m),
            None => return state.unreviewed.get(id).copied().unwrap_or(false),
        };

        // If the latest commit was NOT from an MCP agent, it is considered human-reviewed.
        if !commit_msg.contains("Kb-Source: mcp") {
            return false;
        }

        // The latest commit was from an agent: check if an approval commit exists.
        let pattern = format!("Kb-Reviewed: {commit_hash}");
        let review_log = git.run_git(root, &["log", "-n", "50", "--grep", &pattern]);
        !matches!(review_log, Ok(reviews) if !reviews.trim().is_empty())
    }

    /// Returns all currently unreviewed entry IDs from local state cache.
    pub fn unreviewed_entries(root: &Path) -> Vec<String> {
        let state = load_state(root);
        let mut unreviewed = Vec::new();
        for (id, &is_unrev) in &state.unreviewed {
            if is_unrev {
                unreviewed.push(id.clone());
            }
        }
        unreviewed.sort();
        unreviewed
    }

    /// Returns all currently unreviewed entry IDs using a custom git runner.
    pub fn unreviewed_entries_with_git(
        root: &Path,
        git: &(impl GitRunner + ?Sized),
    ) -> Vec<String> {
        let mut unreviewed = Vec::new();
        let state = load_state(root);
        for (id, &is_unrev) in &state.unreviewed {
            if is_unrev {
                unreviewed.push(id.clone());
            }
        }

        // Scan entries in repository
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "md")
                && let Ok(rel) = entry.path().strip_prefix(root)
            {
                if rel.starts_with(".kb") || rel.starts_with(".git") {
                    continue;
                }
                let stem = rel.with_extension("");
                let id = stem.to_string_lossy().to_string();
                if !unreviewed.contains(&id) && Self::is_unreviewed_with_git(root, &id, git) {
                    unreviewed.push(id);
                }
            }
        }

        unreviewed.sort();
        unreviewed.dedup();
        unreviewed
    }

    /// Records an approval for the given entry ID in local state cache.
    pub fn approve(root: &Path, id: &str) -> std::io::Result<()> {
        let mut state = load_state(root);
        state.unreviewed.remove(id);
        save_state(root, &state)
    }

    /// Records an approval for the given entry ID using a custom git runner.
    pub fn approve_with_git(
        root: &Path,
        id: &str,
        git: &(impl GitRunner + ?Sized),
    ) -> std::io::Result<()> {
        let mut state = load_state(root);
        state.unreviewed.remove(id);

        let is_git = git
            .run_git(root, &["rev-parse", "--is-inside-work-tree"])
            .is_ok_and(|out| out.trim() == "true");

        if is_git && let Some(rel_path) = find_entry_rel_path(root, id) {
            let rel_str = rel_path.to_string_lossy();
            if let Ok(hash_out) =
                git.run_git(root, &["log", "-n", "1", "--format=%H", "--", &rel_str])
            {
                let commit_hash = hash_out.trim();
                if !commit_hash.is_empty() {
                    state
                        .reviewed_commits
                        .insert(id.to_string(), commit_hash.to_string());
                    let msg = format!("kb: review {id}\n\nKb-Reviewed: {commit_hash}\n");
                    let _ = git.run_git(root, &["commit", "--allow-empty", "-m", &msg]);
                }
            }
        }

        save_state(root, &state)
    }

    /// Reverts an entry using a custom git runner.
    pub fn revert_with_git(
        root: &Path,
        id: &str,
        git: &(impl GitRunner + ?Sized),
    ) -> std::io::Result<()> {
        let is_git = git
            .run_git(root, &["rev-parse", "--is-inside-work-tree"])
            .is_ok_and(|out| out.trim() == "true");

        let rel_path = find_entry_rel_path(root, id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("entry {id} not found"),
            )
        })?;
        let rel_str = rel_path.to_string_lossy();

        if is_git {
            // Find commits modifying this file
            let log_out = git
                .run_git(root, &["log", "-z", "--format=%H%x00%B", "--", &rel_str])
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            let mut commits = Vec::new();
            let parts: Vec<&str> = log_out.split('\0').collect();
            let mut i = 0;
            while i + 1 < parts.len() {
                let hash = parts[i].trim();
                let msg = parts[i + 1].trim();
                if !hash.is_empty() {
                    commits.push((hash.to_string(), msg.to_string()));
                }
                i += 2;
            }

            // Find last non-mcp commit
            let last_non_mcp = commits
                .iter()
                .find(|(_, msg)| !msg.contains("Kb-Source: mcp"));
            if let Some((hash, _)) = last_non_mcp {
                // Checkout that revision
                git.run_git(root, &["checkout", hash, "--", &rel_str])
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                git.run_git(root, &["add", "-A", "--", &rel_str])
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let msg = format!("kb: revert agent change to {id}\n");
                git.run_git(root, &["commit", "--only", "-m", &msg, "--", &rel_str])
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            } else {
                // Entry was created by agent, remove it
                git.run_git(root, &["rm", "-f", "--", &rel_str])
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let msg = format!("kb: revert agent change to {id}\n");
                git.run_git(root, &["commit", "-m", &msg])
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
        } else {
            // Non-git: remove file if we don't have older backup
            let abs_path = root.join(&rel_path);
            if abs_path.is_file() {
                fs::remove_file(abs_path)?;
            }
        }

        let mut state = load_state(root);
        state.unreviewed.remove(id);
        save_state(root, &state)
    }

    /// Computes the unified diff using a custom git runner.
    pub fn review_diff_with_git(
        root: &Path,
        id: &str,
        git: &(impl GitRunner + ?Sized),
    ) -> std::io::Result<String> {
        let rel_path = find_entry_rel_path(root, id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("entry {id} not found"),
            )
        })?;
        let rel_str = rel_path.to_string_lossy();

        let is_git = git
            .run_git(root, &["rev-parse", "--is-inside-work-tree"])
            .is_ok_and(|out| out.trim() == "true");

        if is_git {
            let log_out = git
                .run_git(root, &["log", "-z", "--format=%H%x00%B", "--", &rel_str])
                .unwrap_or_default();

            let mut commits = Vec::new();
            let parts: Vec<&str> = log_out.split('\0').collect();
            let mut i = 0;
            while i + 1 < parts.len() {
                let hash = parts[i].trim();
                let msg = parts[i + 1].trim();
                if !hash.is_empty() {
                    commits.push((hash.to_string(), msg.to_string()));
                }
                i += 2;
            }

            let last_non_mcp = commits
                .iter()
                .find(|(_, msg)| !msg.contains("Kb-Source: mcp"));
            if let Some((base_hash, _)) = last_non_mcp {
                match git.run_git(root, &["diff", base_hash, "HEAD", "--", &rel_str]) {
                    Ok(diff) => return Ok(diff),
                    Err(e) => return Err(std::io::Error::other(e.to_string())),
                }
            } else {
                // Entirely agent-created file: diff against empty/show full file
                let content = fs::read_to_string(root.join(&rel_path))?;
                let mut diff = format!(
                    "--- /dev/null\n+++ b/{rel_str}\n@@ -0,0 +1,{} @@\n",
                    content.lines().count()
                );
                for line in content.lines() {
                    diff.push('+');
                    diff.push_str(line);
                    diff.push('\n');
                }
                return Ok(diff);
            }
        }

        // Non-git fallback
        let content = fs::read_to_string(root.join(&rel_path))?;
        let mut diff = format!(
            "--- /dev/null\n+++ b/{rel_str}\n@@ -0,0 +1,{} @@\n",
            content.lines().count()
        );
        for line in content.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        Ok(diff)
    }

    /// Flags an entry as unreviewed in `.kb/state.json`.
    pub fn mark_unreviewed(root: &Path, id: &str) -> std::io::Result<()> {
        let mut state = load_state(root);
        state.unreviewed.insert(id.to_string(), true);
        save_state(root, &state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FakeGitRunner, GitCommandResult};

    #[test]
    fn test_review_state_tracker_state_json_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        let fake_git = FakeGitRunner::new(); // Not a git repo
        assert!(!ReviewStateTracker::is_unreviewed_with_git(
            root, "cmd/tail", &fake_git
        ));

        ReviewStateTracker::mark_unreviewed(root, "cmd/tail").unwrap();
        assert!(ReviewStateTracker::is_unreviewed_with_git(
            root, "cmd/tail", &fake_git
        ));

        let unreviewed = ReviewStateTracker::unreviewed_entries_with_git(root, &fake_git);
        assert_eq!(unreviewed, vec!["cmd/tail"]);

        ReviewStateTracker::approve_with_git(root, "cmd/tail", &fake_git).unwrap();
        assert!(!ReviewStateTracker::is_unreviewed_with_git(
            root, "cmd/tail", &fake_git
        ));
    }

    #[test]
    fn test_review_state_tracker_git_trailers() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let entry_path = root.join("sumo").join("5xx.md");
        fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
        fs::write(
            &entry_path,
            "---\ntitle: 5xx\ntags: [auth]\n---\n_sourceCategory=prod\n",
        )
        .unwrap();

        // 1. Git reports latest commit has Kb-Source: mcp
        let fake_git = FakeGitRunner::with_handler(|_, args| {
            if args.contains(&"--is-inside-work-tree") {
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: "".into(),
                });
            }
            if args.contains(&"log") && args.contains(&"--format=%H%x00%B") {
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "c0ffee\0kb: create sumo/5xx\n\nKb-Source: mcp\nKb-Client: claude\n"
                        .into(),
                    stderr: "".into(),
                });
            }
            if args.contains(&"--grep") {
                // No approval commit found yet
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "".into(),
                    stderr: "".into(),
                });
            }
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: "".into(),
                stderr: "".into(),
            })
        });

        assert!(ReviewStateTracker::is_unreviewed_with_git(
            root, "sumo/5xx", &fake_git
        ));
    }

    #[test]
    fn test_review_state_tracker_approve_and_revert() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let entry_path = root.join("cmd").join("tail.md");
        fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
        fs::write(&entry_path, "---\ntitle: Tail\n---\nkubectl logs\n").unwrap();

        // 1. Approve records Kb-Reviewed commit and updates state.json
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let fake_git = FakeGitRunner::with_handler(move |_, args| {
            calls_clone
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if args.contains(&"--is-inside-work-tree") {
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: "".into(),
                });
            }
            if args.contains(&"log") && args.contains(&"--format=%H") {
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "deadbeef\n".into(),
                    stderr: "".into(),
                });
            }
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: "".into(),
                stderr: "".into(),
            })
        });

        ReviewStateTracker::approve_with_git(root, "cmd/tail", &fake_git).unwrap();
        let call_list = calls.lock().unwrap().clone();
        assert!(call_list.iter().any(|args| {
            args.contains(&"-m".to_string())
                && args.iter().any(|a| {
                    a.contains("kb: review cmd/tail") && a.contains("Kb-Reviewed: deadbeef")
                })
        }));
        let state = load_state(root);
        assert_eq!(
            state.reviewed_commits.get("cmd/tail"),
            Some(&"deadbeef".to_string())
        );
        assert!(!state.unreviewed.contains_key("cmd/tail"));

        // 2. Revert when older human commit exists checks out older commit
        let revert_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let revert_calls_clone = revert_calls.clone();
        let fake_revert_git = FakeGitRunner::with_handler(move |_, args| {
            revert_calls_clone
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if args.contains(&"--is-inside-work-tree") {
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: "true\n".into(),
                    stderr: "".into(),
                });
            }
            if args.contains(&"log") && args.contains(&"--format=%H%x00%B") {
                // Latest commit is MCP agent; previous commit is human
                let log_str = "agentcommit\0kb: update cmd/tail\n\nKb-Source: mcp\0humancommit\0human author edit\0";
                return Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: log_str.into(),
                    stderr: "".into(),
                });
            }
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: "".into(),
                stderr: "".into(),
            })
        });

        ReviewStateTracker::revert_with_git(root, "cmd/tail", &fake_revert_git).unwrap();
        let r_calls = revert_calls.lock().unwrap().clone();
        assert!(
            r_calls
                .iter()
                .any(|args| args.contains(&"checkout".to_string())
                    && args.contains(&"humancommit".to_string()))
        );
        assert!(r_calls.iter().any(|args| {
            args.contains(&"-m".to_string())
                && args
                    .iter()
                    .any(|a| a.contains("kb: revert agent change to cmd/tail"))
        }));
    }
}
