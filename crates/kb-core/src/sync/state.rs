//! `.kb/sync-state.json`: the checkpoint that lets an interrupted sync be continued or rolled back.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git::{GitError, GitRunner};

use super::SyncStrategy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncInProgressState {
    /// HEAD just before integration started (after hand edits were committed).
    pub pre_sync_commit: String,
    pub remote: String,
    pub branch: String,
    /// `rebase` or `merge`.
    pub strategy: String,
    pub remaining_conflicts: Vec<PathBuf>,
    pub started_at: u64,
}

impl SyncInProgressState {
    pub fn strategy(&self) -> SyncStrategy {
        if self.strategy == "merge" {
            SyncStrategy::Merge
        } else {
            SyncStrategy::Rebase
        }
    }
}

fn path(root: &Path) -> PathBuf {
    root.join(".kb").join("sync-state.json")
}

pub fn save(root: &Path, state: &SyncInProgressState) -> std::io::Result<()> {
    fs::create_dir_all(root.join(".kb"))?;
    let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    fs::write(path(root), json)
}

pub fn load(root: &Path) -> Option<SyncInProgressState> {
    serde_json::from_str(&fs::read_to_string(path(root)).ok()?).ok()
}

pub fn clear(root: &Path) {
    let _ = fs::remove_file(path(root));
}

/// Puts the repository back to `pre_sync_commit`: ends any rebase or merge, then resets hard.
/// The hand-edit commit made before the checkpoint survives; nothing else is lost.
pub fn rollback(
    root: &Path,
    git: &impl GitRunner,
    state: &SyncInProgressState,
) -> Result<(), GitError> {
    let _ = git.run_git_status(root, &["rebase", "--abort"], Duration::from_secs(30));
    let _ = git.run_git_status(root, &["merge", "--abort"], Duration::from_secs(30));
    git.run_git(root, &["reset", "--hard", &state.pre_sync_commit])?;
    clear(root);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FakeGitRunner;

    fn sample() -> SyncInProgressState {
        SyncInProgressState {
            pre_sync_commit: "abc".into(),
            remote: "origin".into(),
            branch: "main".into(),
            strategy: "rebase".into(),
            remaining_conflicts: vec!["a.md".into()],
            started_at: 1,
        }
    }

    #[test]
    fn persists_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let s = sample();
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()), Some(s.clone()));

        let git = FakeGitRunner::new();
        rollback(dir.path(), &git, &s).unwrap();
        let calls: Vec<_> = git.calls().into_iter().map(|(_, a)| a.join(" ")).collect();
        assert!(calls.contains(&"reset --hard abc".to_string()));
        assert_eq!(load(dir.path()), None, "checkpoint removed");
    }
}
