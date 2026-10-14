//! History inspection, diff generation, and historical revision retrieval.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git::{GitError, GitRunner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeCategory {
    Payload,
    Tags,
    Variables,
    Frontmatter,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub change_categories: Vec<ChangeCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryHistory {
    pub id: String,
    pub commits: Vec<EntryCommit>,
}

#[derive(Debug)]
pub enum HistoryError {
    EntryNotFound(String),
    RevisionNotFound(String),
    Git(GitError),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntryNotFound(id) => write!(f, "no history for {id}"),
            Self::RevisionNotFound(r) => write!(f, "no such revision for this entry: {r}"),
            Self::Git(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HistoryError {}

impl From<GitError> for HistoryError {
    fn from(e: GitError) -> Self {
        Self::Git(e)
    }
}

pub struct HistoryEngine<'a, G: GitRunner> {
    root: &'a Path,
    git: &'a G,
}

/// Record and field separators in the `git log` format below.
const RS: char = '\u{1e}';
const FORMAT: &str = "--format=%x1e%H%x1f%h%x1f%an <%ae>%x1f%aI%x1f%s";

/// A commit's header and the entry's path before and after it.
struct Touch {
    commit: EntryCommit,
    old_path: Option<String>,
    path: String,
}

fn parse_touches(out: &str) -> Vec<Touch> {
    out.split(RS)
        .filter(|b| !b.trim().is_empty())
        .filter_map(|block| {
            let mut lines = block.lines();
            let mut f = lines.next()?.split('\u{1f}');
            let commit = EntryCommit {
                hash: f.next()?.into(),
                short_hash: f.next()?.into(),
                author: f.next()?.into(),
                date: f.next()?.into(),
                message: f.next()?.into(),
                change_categories: Vec::new(),
            };
            let status = lines.find(|l| !l.trim().is_empty())?;
            let cols: Vec<&str> = status.split('\t').collect();
            let (old_path, path) = match cols[..] {
                [s, old, new] if s.starts_with('R') => (Some(old.to_string()), new.to_string()),
                [_, p] => (None, p.to_string()),
                _ => return None,
            };
            Some(Touch {
                commit,
                old_path,
                path,
            })
        })
        .collect()
}

/// Top-level frontmatter keys with the raw text under each, and the body.
fn parts(text: &str) -> (Vec<(String, String)>, String) {
    let Ok(split) = crate::frontmatter::split(text) else {
        return (Vec::new(), text.to_string());
    };
    let mut keys: Vec<(String, String)> = Vec::new();
    for line in text[split.frontmatter.clone()].lines() {
        let top = !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('-');
        match line.split_once(':') {
            Some((k, _)) if top => keys.push((k.trim().to_string(), line.to_string())),
            _ => {
                if let Some((_, raw)) = keys.last_mut() {
                    raw.push('\n');
                    raw.push_str(line);
                }
            }
        }
    }
    (keys, text[split.body].to_string())
}

/// What changed between two versions of an entry's text.
pub fn categorize(before: Option<&str>, after: &str, renamed: bool) -> Vec<ChangeCategory> {
    let mut out = Vec::new();
    let Some(before) = before else {
        return vec![ChangeCategory::Payload];
    };
    let ((bk, bb), (ak, ab)) = (parts(before), parts(after));
    if bb != ab {
        out.push(ChangeCategory::Payload);
    }
    let get =
        |v: &[(String, String)], k: &str| v.iter().find(|(x, _)| x == k).map(|(_, r)| r.clone());
    let mut keys: Vec<&String> = bk.iter().chain(ak.iter()).map(|(k, _)| k).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        if get(&bk, k) != get(&ak, k) {
            match k.as_str() {
                "tags" => out.push(ChangeCategory::Tags),
                "vars" => out.push(ChangeCategory::Variables),
                "updated" => {}
                _ if !out.contains(&ChangeCategory::Frontmatter) => {
                    out.push(ChangeCategory::Frontmatter)
                }
                _ => {}
            }
        }
    }
    if renamed {
        out.push(ChangeCategory::Renamed);
    }
    out
}

impl<'a, G: GitRunner> HistoryEngine<'a, G> {
    pub fn new(root: &'a Path, git: &'a G) -> Self {
        Self { root, git }
    }

    fn touches(&self, id: &str, extra: &[&str]) -> Result<Vec<Touch>, HistoryError> {
        let path = format!("{id}.md");
        let mut args = vec!["log", "--follow", "-M", "--name-status", FORMAT];
        args.extend_from_slice(extra);
        args.extend(["--", &path]);
        match self.git.run_git(self.root, &args) {
            Ok(out) => Ok(parse_touches(&out)),
            Err(GitError::CommandFailed { .. }) => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn blob(&self, rev: &str, path: &str) -> Option<String> {
        self.git
            .run_git(self.root, &["show", &format!("{rev}:{path}")])
            .ok()
    }

    /// Commits that touched the entry, newest first, following renames.
    pub fn log(&self, id: &str) -> Result<EntryHistory, HistoryError> {
        let touches = self.touches(id, &[])?;
        if touches.is_empty() {
            return Err(HistoryError::EntryNotFound(id.to_string()));
        }
        let commits = touches
            .into_iter()
            .map(|mut t| {
                let after = self.blob(&t.commit.hash, &t.path).unwrap_or_default();
                let before_path = t.old_path.as_deref().unwrap_or(&t.path);
                let before = self.blob(&format!("{}^", t.commit.hash), before_path);
                t.commit.change_categories =
                    categorize(before.as_deref(), &after, t.old_path.is_some());
                t.commit
            })
            .collect();
        Ok(EntryHistory {
            id: id.to_string(),
            commits,
        })
    }

    /// The last touch of the entry at or before `rev`. The entry may have had another name by
    /// then, so this walks the entry's whole history instead of asking git about `rev` alone.
    fn touch_at(&self, id: &str, rev: &str) -> Result<Touch, HistoryError> {
        let not_found = || HistoryError::RevisionNotFound(rev.to_string());
        let full = self
            .git
            .run_git(
                self.root,
                &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
            )
            .map_err(|_| not_found())?;
        let full = full.trim();
        self.touches(id, &[])?
            .into_iter()
            .find(|t| {
                t.commit.hash == full
                    || self
                        .git
                        .run_git(
                            self.root,
                            &["merge-base", "--is-ancestor", &t.commit.hash, full],
                        )
                        .is_ok()
            })
            .ok_or_else(not_found)
    }

    /// The diff the entry got at `rev` (or at its last change before it).
    pub fn diff(&self, id: &str, rev: &str) -> Result<String, HistoryError> {
        let t = self.touch_at(id, rev)?;
        Ok(self.git.run_git(
            self.root,
            &["show", "-M", "--format=", &t.commit.hash, "--", &t.path],
        )?)
    }

    /// The entry's text as of `rev`.
    pub fn show(&self, id: &str, rev: &str) -> Result<String, HistoryError> {
        let t = self.touch_at(id, rev)?;
        self.blob(&t.commit.hash, &t.path)
            .ok_or_else(|| HistoryError::RevisionNotFound(rev.to_string()))
    }

    /// Short hash of `rev`, for commit messages.
    pub fn short(&self, rev: &str) -> String {
        self.git
            .run_git(self.root, &["rev-parse", "--short", rev])
            .map_or_else(|_| rev.to_string(), |s| s.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FakeGitRunner, GitCommandResult};

    const V1: &str = "---\nkb_format: 1\ntype: cmd\ntitle: A\ntags: [x]\nvars:\n  host: {}\n---\n\n```\nls\n```\n";

    fn change(from: &str, to: &str) -> String {
        V1.replacen(from, to, 1)
    }

    #[test]
    fn categorizes_what_changed() {
        use ChangeCategory::*;
        assert_eq!(
            categorize(Some(V1), &change("ls", "ls -l"), false),
            [Payload]
        );
        assert_eq!(
            categorize(Some(V1), &change("[x]", "[x, y]"), false),
            [Tags]
        );
        assert_eq!(
            categorize(Some(V1), &change("host: {}", "host: {default: a}"), false),
            [Variables]
        );
        assert_eq!(
            categorize(Some(V1), &change("title: A", "title: B"), false),
            [Frontmatter]
        );
        assert_eq!(categorize(Some(V1), V1, true), [Renamed]);
        assert_eq!(categorize(None, V1, false), [Payload]);
        let both = change("title: A", "title: B").replacen("ls", "pwd", 1);
        assert_eq!(categorize(Some(V1), &both, false), [Payload, Frontmatter]);
    }

    fn fake() -> FakeGitRunner {
        let log = "\u{1e}h2\u{1f}h2s\u{1f}Ann <a@x>\u{1f}2026-02-01T00:00:00Z\u{1f}kb: mv a b\n\nR100\ta.md\tb.md\n\
                   \u{1e}h1\u{1f}h1s\u{1f}Ann <a@x>\u{1f}2026-01-01T00:00:00Z\u{1f}kb: add a\n\nA\ta.md\n";
        FakeGitRunner::with_handler(move |_, args| {
            let out = match args {
                ["log", ..] => log.to_string(),
                ["rev-parse", "--verify", spec] => spec.trim_end_matches("^{commit}").to_string(),
                ["show", spec]
                    if *spec == "h2:b.md" || *spec == "h2^:a.md" || *spec == "h1:a.md" =>
                {
                    V1.to_string()
                }
                _ => {
                    return Ok(GitCommandResult {
                        exit_code: Some(128),
                        stdout: String::new(),
                        stderr: "bad".into(),
                    });
                }
            };
            Ok(GitCommandResult {
                exit_code: Some(0),
                stdout: out,
                stderr: String::new(),
            })
        })
    }

    #[test]
    fn log_follows_renames_and_summarizes() {
        let git = fake();
        let h = HistoryEngine::new(Path::new("/kb"), &git).log("b").unwrap();
        assert_eq!(h.commits.len(), 2);
        assert_eq!(h.commits[0].change_categories, [ChangeCategory::Renamed]);
        assert_eq!(h.commits[1].change_categories, [ChangeCategory::Payload]);
        assert_eq!(h.commits[0].short_hash, "h2s");
        assert!(
            git.calls()
                .iter()
                .any(|(_, a)| a.contains(&"--follow".to_string()))
        );
    }

    #[test]
    fn show_reads_the_path_the_entry_had_then() {
        let git = fake();
        let eng = HistoryEngine::new(Path::new("/kb"), &git);
        assert_eq!(eng.show("b", "h2").unwrap(), V1);
        let nothing = FakeGitRunner::new();
        assert!(matches!(
            HistoryEngine::new(Path::new("/kb"), &nothing).log("zzz"),
            Err(HistoryError::EntryNotFound(_))
        ));
    }
}
