//! The git badge in the header, and the thread that keeps it current without blocking the UI.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use kb_core::git::status::SyncStatus;
use kb_core::sync::{SyncConfig, SyncEngine, SyncStrategy};

use crate::platform::BatchGitRunner;

/// The badge text and whether it is a warning; `None` outside a git repository.
pub fn badge(status: &SyncStatus, fetch_failed: bool, now_secs: u64) -> Option<(String, bool)> {
    let g = &status.git;
    if !g.is_git {
        return None;
    }
    if status.in_progress.is_some() {
        return Some(("git ⚠ conflict".into(), true));
    }
    if fetch_failed {
        return Some(("git ⚠ fetch auth".into(), true));
    }
    let mut parts = Vec::new();
    if g.ahead > 0 {
        parts.push(format!("↑{}", g.ahead));
    }
    if g.behind > 0 {
        parts.push(format!("↓{}", g.behind));
    }
    if !g.uncommitted.is_empty() {
        parts.push(format!("● {} changed", g.uncommitted.len()));
    }
    if !parts.is_empty() {
        return Some((format!("git {}", parts.join(" ")), false));
    }
    let age = status.last_fetch.map(|t| {
        let m = now_secs.saturating_sub(t) / 60;
        if m < 1 {
            "just now".to_string()
        } else if m < 120 {
            format!("{m}m ago")
        } else {
            format!("{}h ago", m / 60)
        }
    });
    Some((
        match age {
            Some(a) => format!("git ✓ in sync ({a})"),
            None => "git ✓ in sync".into(),
        },
        false,
    ))
}

pub enum GitRequest {
    Status,
    /// `sync.fetch_on_start`: fetch, then report the status again.
    Fetch,
    Shutdown,
}

pub enum GitReply {
    Status(Box<SyncStatus>),
    FetchFailed,
}

pub struct GitWorker {
    pub tx: Sender<GitRequest>,
    pub rx: Receiver<GitReply>,
    handle: Option<JoinHandle<()>>,
}

/// Fetches are cut off after this long: the UI must stay quick.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

impl GitWorker {
    pub fn spawn(root: PathBuf) -> GitWorker {
        let (tx, requests) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let git = BatchGitRunner;
            let config = SyncConfig {
                strategy: SyncStrategy::Rebase,
                timeout: FETCH_TIMEOUT,
            };
            while let Ok(request) = requests.recv() {
                match request {
                    GitRequest::Shutdown => break,
                    GitRequest::Fetch => {
                        let has_upstream =
                            SyncStatus::query(&git, &root).is_ok_and(|s| s.git.upstream.is_some());
                        if has_upstream && SyncEngine::new(&root, &git, config).fetch().is_err() {
                            let _ = replies.send(GitReply::FetchFailed);
                        }
                    }
                    GitRequest::Status => {}
                }
                if let Ok(status) = SyncStatus::query(&git, &root) {
                    let _ = replies.send(GitReply::Status(Box::new(status)));
                }
            }
        });
        GitWorker {
            tx,
            rx,
            handle: Some(handle),
        }
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(GitRequest::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_core::git::status::GitStatus;

    fn status(parse: &str) -> SyncStatus {
        SyncStatus {
            git: GitStatus::parse_porcelain(parse),
            last_fetch: Some(1000),
            in_progress: None,
        }
    }

    #[test]
    fn badges_follow_the_contract() {
        let now = 1000 + 12 * 60;
        let text = |s: &SyncStatus, failed| badge(s, failed, now).unwrap().0;
        assert_eq!(
            text(&status("## main...origin/main\n"), false),
            "git ✓ in sync (12m ago)"
        );
        assert_eq!(
            text(&status("## main...origin/main [ahead 2]\n"), false),
            "git ↑2"
        );
        assert_eq!(
            text(&status("## main...origin/main [behind 3]\n"), false),
            "git ↓3"
        );
        assert_eq!(
            text(
                &status("## main...origin/main [ahead 2, behind 3]\n"),
                false
            ),
            "git ↑2 ↓3"
        );
        assert_eq!(
            text(&status("## main...origin/main\n M a.md\n M b.md\n"), false),
            "git ● 2 changed"
        );
        assert_eq!(
            text(&status("## main...origin/main\n"), true),
            "git ⚠ fetch auth"
        );
        let mut conflict = status("## main...origin/main\n");
        conflict.in_progress = kb_core::sync::state::load(std::path::Path::new("/nonexistent"));
        assert!(conflict.in_progress.is_none());
        assert!(
            badge(
                &SyncStatus {
                    git: GitStatus::not_a_repo(),
                    last_fetch: None,
                    in_progress: None
                },
                false,
                now
            )
            .is_none()
        );
    }
}
