//! Conflicts found while integrating remote changes, and applying the user's choice per entry.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::secrets::SecretGate;
use crate::git::{GitError, GitRunner};
use crate::lint::{Severity, check_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictKind {
    BothModified,
    ModifiedVsDeleted,
    DeletedVsModified,
    BothAdded,
    RenamedDifferently,
}

/// "mine" is the local version, "theirs" the remote one, whatever the strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub id: String,
    pub rel_path: PathBuf,
    pub kind: ConflictKind,
    pub mine: Option<String>,
    pub theirs: Option<String>,
    pub base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionChoice {
    Mine,
    Theirs,
    EditMerged(String),
    /// Keep mine at the original ID and save theirs as `new_id`.
    KeepBoth {
        new_id: String,
    },
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub conflict: Conflict,
    pub choice: ResolutionChoice,
}

#[derive(Debug)]
pub enum ConflictError {
    /// The user quit: roll the sync back.
    UserAborted,
    /// No one to ask (non-interactive): leave the conflict for `--continue`.
    Deferred,
    /// The merged text failed lint or the secret scan.
    ValidationFailed(String),
    /// `KeepBoth` target already exists; ask for another ID.
    IdCollision(String),
    Io(String),
    Git(GitError),
}

impl std::fmt::Display for ConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserAborted => write!(f, "aborted"),
            Self::Deferred => write!(f, "conflict left unresolved"),
            Self::ValidationFailed(m) | Self::Io(m) => write!(f, "{m}"),
            Self::IdCollision(id) => write!(f, "an entry named {id} already exists"),
            Self::Git(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConflictError {}

impl From<GitError> for ConflictError {
    fn from(e: GitError) -> Self {
        Self::Git(e)
    }
}

pub trait ConflictHandler {
    /// Decides one conflict.
    fn resolve(&mut self, conflict: &Conflict) -> Result<ResolutionChoice, ConflictError>;

    /// A different ID for "keep both" when `taken` already exists.
    fn new_id(&mut self, _taken: &str) -> Result<String, ConflictError> {
        Err(ConflictError::UserAborted)
    }
}

/// Unmerged paths in `git status --porcelain` output, with their kind. Porcelain letters are
/// ours/theirs in git's terms; during a rebase "ours" is the remote, so `rebase` swaps them to
/// keep mine = local.
pub fn parse_unmerged(porcelain: &str, rebase: bool) -> Vec<(PathBuf, ConflictKind)> {
    let mut out = Vec::new();
    for line in porcelain.lines() {
        let b = line.as_bytes();
        if b.len() < 4 {
            continue;
        }
        let (x, y) = (b[0] as char, b[1] as char);
        let (mine, theirs) = if rebase { (y, x) } else { (x, y) };
        let kind = match (mine, theirs) {
            ('U', 'U') => ConflictKind::BothModified,
            ('A', 'A') | ('A', 'U') | ('U', 'A') => ConflictKind::BothAdded,
            ('D', 'D') => ConflictKind::RenamedDifferently,
            ('U', 'D') => ConflictKind::ModifiedVsDeleted,
            ('D', 'U') => ConflictKind::DeletedVsModified,
            _ => continue,
        };
        out.push((PathBuf::from(line[3..].trim().trim_matches('"')), kind));
    }
    out
}

pub fn entry_id(rel: &Path) -> String {
    let s = rel.to_string_lossy();
    s.strip_suffix(".md").unwrap_or(&s).to_string()
}

/// Reads the three sides of an unmerged path from the index.
pub fn load_conflict(
    root: &Path,
    git: &impl GitRunner,
    rel: &Path,
    kind: ConflictKind,
    rebase: bool,
) -> Conflict {
    let stage = |n: u8| {
        git.run_git(root, &["show", &format!(":{n}:{}", rel.display())])
            .ok()
    };
    let (ours, theirs) = (stage(2), stage(3));
    let (mine, theirs) = if rebase {
        (theirs, ours)
    } else {
        (ours, theirs)
    };
    Conflict {
        id: entry_id(rel),
        rel_path: rel.to_path_buf(),
        kind,
        mine,
        theirs,
        base: stage(1),
    }
}

/// Lint errors and unallowed secrets in `text`, as one message; `None` when it is fine to save.
pub fn validate_entry_text(id: &str, text: &str) -> Option<String> {
    let mut problems: Vec<String> = check_text(id, text.as_bytes().to_vec())
        .into_iter()
        .filter(|f| f.code.severity() == Severity::Error)
        .map(|f| format!("line {}: {}", f.line, f.message))
        .collect();
    let (blocked, _) = SecretGate::check_text(&format!("{id}.md"), text, None);
    problems.extend(
        blocked
            .into_iter()
            .map(|s| format!("line {}: {}", s.line, s.message)),
    );
    problems.sort();
    problems.dedup();
    (!problems.is_empty()).then(|| problems.join("\n"))
}

fn put(
    root: &Path,
    git: &impl GitRunner,
    rel: &Path,
    text: Option<&str>,
) -> Result<(), ConflictError> {
    let p = rel.display().to_string();
    match text {
        Some(t) => {
            let full = root.join(rel);
            if let Some(dir) = full.parent() {
                fs::create_dir_all(dir).map_err(|e| ConflictError::Io(e.to_string()))?;
            }
            fs::write(&full, t).map_err(|e| ConflictError::Io(e.to_string()))?;
            git.run_git(root, &["add", "--", &p])?;
        }
        None => {
            git.run_git(root, &["rm", "-f", "-q", "--", &p])?;
        }
    }
    Ok(())
}

/// Writes the chosen content to the working tree and stages it (or stages the deletion).
pub fn apply_resolution(
    root: &Path,
    git: &impl GitRunner,
    resolution: &Resolution,
) -> Result<(), ConflictError> {
    let c = &resolution.conflict;
    match &resolution.choice {
        ResolutionChoice::Mine => put(root, git, &c.rel_path, c.mine.as_deref()),
        ResolutionChoice::Theirs => put(root, git, &c.rel_path, c.theirs.as_deref()),
        ResolutionChoice::Delete => put(root, git, &c.rel_path, None),
        ResolutionChoice::EditMerged(text) => {
            if let Some(problem) = validate_entry_text(&c.id, text) {
                return Err(ConflictError::ValidationFailed(problem));
            }
            put(root, git, &c.rel_path, Some(text))
        }
        ResolutionChoice::KeepBoth { new_id } => {
            let rel = PathBuf::from(format!("{new_id}.md"));
            if root.join(&rel).exists() {
                return Err(ConflictError::IdCollision(new_id.clone()));
            }
            put(root, git, &c.rel_path, c.mine.as_deref())?;
            put(root, git, &rel, c.theirs.as_deref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FakeGitRunner;

    #[test]
    fn parses_all_five_kinds_in_merge_and_rebase() {
        let p = "UU a.md\nUD b.md\nDU c.md\nAA d.md\nDD e.md\n M f.md\n";
        let merge = parse_unmerged(p, false);
        let kinds: Vec<_> = merge.iter().map(|(_, k)| *k).collect();
        assert_eq!(
            kinds,
            [
                ConflictKind::BothModified,
                ConflictKind::ModifiedVsDeleted,
                ConflictKind::DeletedVsModified,
                ConflictKind::BothAdded,
                ConflictKind::RenamedDifferently
            ]
        );
        // in a rebase "ours" is the remote, so UD (ours modified, theirs deleted) flips
        let rebase = parse_unmerged("UD b.md\n", true);
        assert_eq!(rebase[0].1, ConflictKind::DeletedVsModified);
    }

    fn conflict() -> Conflict {
        Conflict {
            id: "a".into(),
            rel_path: "a.md".into(),
            kind: ConflictKind::BothModified,
            mine: Some("mine\n".into()),
            theirs: Some("theirs\n".into()),
            base: None,
        }
    }

    fn apply(
        root: &Path,
        git: &FakeGitRunner,
        choice: ResolutionChoice,
    ) -> Result<(), ConflictError> {
        apply_resolution(
            root,
            git,
            &Resolution {
                conflict: conflict(),
                choice,
            },
        )
    }

    fn cmds(git: &FakeGitRunner) -> Vec<String> {
        git.calls().into_iter().map(|(_, a)| a.join(" ")).collect()
    }

    #[test]
    fn choices_write_and_stage() {
        let dir = tempfile::tempdir().unwrap();
        let git = FakeGitRunner::new();
        apply(dir.path(), &git, ResolutionChoice::Theirs).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "theirs\n"
        );
        apply(dir.path(), &git, ResolutionChoice::Mine).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "mine\n"
        );
        apply(dir.path(), &git, ResolutionChoice::Delete).unwrap();
        let c = cmds(&git);
        assert_eq!(c[0], "add -- a.md");
        assert_eq!(c[2], "rm -f -q -- a.md");
    }

    #[test]
    fn edit_merged_must_pass_validation() {
        let dir = tempfile::tempdir().unwrap();
        let git = FakeGitRunner::new();
        let bad = apply(
            dir.path(),
            &git,
            ResolutionChoice::EditMerged("<<<<<<< no frontmatter\n".into()),
        );
        assert!(matches!(bad, Err(ConflictError::ValidationFailed(_))));
        assert!(git.calls().is_empty(), "nothing staged on failure");
        let good = "---\nkb_format: 1\ntype: cmd\ntitle: A\n---\n\n```\nls\n```\n";
        apply(dir.path(), &git, ResolutionChoice::EditMerged(good.into())).unwrap();
    }

    #[test]
    fn keep_both_saves_theirs_under_new_id_and_detects_collision() {
        let dir = tempfile::tempdir().unwrap();
        let git = FakeGitRunner::new();
        let keep = |id: &str| ResolutionChoice::KeepBoth { new_id: id.into() };
        apply(dir.path(), &git, keep("a-conflict-mac")).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "mine\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("a-conflict-mac.md")).unwrap(),
            "theirs\n"
        );
        let again = apply(dir.path(), &git, keep("a-conflict-mac"));
        assert!(matches!(again, Err(ConflictError::IdCollision(id)) if id == "a-conflict-mac"));
    }
}
