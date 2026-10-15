//! `kb init`: create a KB, or add what is missing to one. Never overwrites anything.

use std::fmt;
use std::path::{Path, PathBuf};

/// The content of `.kb/.gitignore`: ignore everything in `.kb/`, including this file.
pub const KB_GITIGNORE: &str = "*\n";

/// One example entry per type, as (path relative to the root, content).
pub const EXAMPLES: [(&str, &str); 9] = [
    (
        "sql/example-active-connections.md",
        include_str!("examples/sql.md"),
    ),
    (
        "sumo/example-5xx-by-authorizer.md",
        include_str!("examples/sumo.md"),
    ),
    (
        "cw/example-lambda-errors.md",
        include_str!("examples/cw.md"),
    ),
    (
        "cmd/example-tail-pod-logs.md",
        include_str!("examples/cmd.md"),
    ),
    (
        "prompt/example-summarize-incident.md",
        include_str!("examples/prompt.md"),
    ),
    ("link/example-runbook.md", include_str!("examples/link.md")),
    (
        "code/example-request-handler.md",
        include_str!("examples/code.md"),
    ),
    (
        "note/example-authorizer-role-cache.md",
        include_str!("examples/note.md"),
    ),
    (
        "runbook/example-deploy-authorizer.md",
        include_str!("examples/runbook.md"),
    ),
];

/// What happened to the example entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExamplesOutcome {
    Created,
    /// `--no-examples` was passed.
    SkippedFlag,
    /// The KB already holds entries.
    SkippedHasEntries,
}

impl ExamplesOutcome {
    /// The name used in `kb init --json`.
    pub fn as_str(self) -> &'static str {
        match self {
            ExamplesOutcome::Created => "created",
            ExamplesOutcome::SkippedFlag => "skipped-flag",
            ExamplesOutcome::SkippedHasEntries => "skipped-has-entries",
        }
    }
}

/// One step of an init plan. Paths are relative to the root (`.` is the root itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitAction {
    CreateDir(String),
    CreateFile(String, &'static str),
    Exists(String),
}

/// What `kb init` will do, worked out without changing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    pub root: PathBuf,
    pub actions: Vec<InitAction>,
    pub examples: ExamplesOutcome,
}

/// What `kb init` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Absolute path of the KB.
    pub root: PathBuf,
    /// Paths created, relative to the root; directories end with `/`, and `.` is the root.
    pub created: Vec<String>,
    /// Paths that already existed and were left untouched.
    pub existing: Vec<String>,
    pub examples: ExamplesOutcome,
    /// Every path in plan order, with whether it was created.
    pub steps: Vec<(String, bool)>,
}

/// Why `kb init` failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    NotADirectory(PathBuf),
    Io { path: PathBuf, message: String },
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::NotADirectory(p) => {
                write!(f, "{} exists and is not a directory", p.display())
            }
            InitError::Io { path, message } => {
                write!(f, "cannot create {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for InitError {}

/// Works out what `kb init` would do at `path`.
pub fn plan_init(path: &Path, examples: bool) -> Result<InitPlan, InitError> {
    let mut actions = Vec::new();
    let exists = |rel: &str| path.join(rel).exists();
    if path.exists() {
        if !path.is_dir() {
            return Err(InitError::NotADirectory(path.to_path_buf()));
        }
        actions.push(InitAction::Exists(".".into()));
    } else {
        actions.push(InitAction::CreateDir(".".into()));
    }
    if path.join(".kb").exists() && !path.join(".kb").is_dir() {
        return Err(InitError::NotADirectory(path.join(".kb")));
    }
    actions.push(if exists(".kb") {
        InitAction::Exists(".kb/".into())
    } else {
        InitAction::CreateDir(".kb/".into())
    });
    actions.push(if exists(".kb/.gitignore") {
        InitAction::Exists(".kb/.gitignore".into())
    } else {
        InitAction::CreateFile(".kb/.gitignore".into(), KB_GITIGNORE)
    });
    let outcome = if !examples {
        ExamplesOutcome::SkippedFlag
    } else if has_entries(path) {
        ExamplesOutcome::SkippedHasEntries
    } else {
        for (rel, content) in EXAMPLES {
            actions.push(if exists(rel) {
                InitAction::Exists(rel.into())
            } else {
                InitAction::CreateFile(rel.into(), content)
            });
        }
        ExamplesOutcome::Created
    };
    Ok(InitPlan {
        root: path.to_path_buf(),
        actions,
        examples: outcome,
    })
}

/// True when the walk finds at least one entry file.
fn has_entries(path: &Path) -> bool {
    let Ok(root) = path.canonicalize() else {
        return false;
    };
    crate::walk::walk(&root, std::slice::from_ref(&root)).is_ok_and(|w| !w.files.is_empty())
}

/// Carries out a plan. Every file is created with exclusive create, so nothing is overwritten.
pub fn apply_init(plan: &InitPlan) -> Result<InitReport, InitError> {
    let io = |path: &Path, e: std::io::Error| InitError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    let mut steps: Vec<(String, bool)> = Vec::new();
    for action in &plan.actions {
        match action {
            InitAction::Exists(rel) => steps.push((rel.clone(), false)),
            InitAction::CreateDir(rel) => {
                let path = if rel == "." {
                    plan.root.clone()
                } else {
                    plan.root.join(rel)
                };
                match std::fs::create_dir(&path) {
                    Ok(()) => steps.push((rel.clone(), true)),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {
                        steps.push((rel.clone(), false))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        std::fs::create_dir_all(&path).map_err(|e| io(&path, e))?;
                        steps.push((rel.clone(), true));
                    }
                    Err(e) => return Err(io(&path, e)),
                }
            }
            InitAction::CreateFile(rel, content) => {
                let path = plan.root.join(rel);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
                }
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(mut file) => {
                        use std::io::Write;
                        file.write_all(content.as_bytes())
                            .map_err(|e| io(&path, e))?;
                        steps.push((rel.clone(), true));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        steps.push((rel.clone(), false))
                    }
                    Err(e) => return Err(io(&path, e)),
                }
            }
        }
    }
    let root = plan.root.canonicalize().map_err(|e| io(&plan.root, e))?;
    let pick = |want: bool| {
        steps
            .iter()
            .filter(|(_, c)| *c == want)
            .map(|(p, _)| p.clone())
            .collect()
    };
    Ok(InitReport {
        root,
        created: pick(true),
        existing: pick(false),
        examples: plan.examples,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names(actions: &[InitAction]) -> Vec<String> {
        actions
            .iter()
            .map(|a| match a {
                InitAction::CreateDir(p) => format!("dir {p}"),
                InitAction::CreateFile(p, _) => format!("file {p}"),
                InitAction::Exists(p) => format!("exists {p}"),
            })
            .collect()
    }

    #[test]
    fn new_kb() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("kb");
        let plan = plan_init(&root, true).unwrap();
        assert_eq!(plan.examples, ExamplesOutcome::Created);
        let mut want = vec![
            "dir .".to_string(),
            "dir .kb/".into(),
            "file .kb/.gitignore".into(),
        ];
        want.extend(EXAMPLES.iter().map(|(p, _)| format!("file {p}")));
        assert_eq!(names(&plan.actions), want);
        assert!(!root.exists(), "planning must not create anything");

        let report = apply_init(&plan).unwrap();
        assert_eq!(report.root, root.canonicalize().unwrap());
        assert_eq!(report.created.len(), 12);
        assert!(report.existing.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(".kb/.gitignore")).unwrap(),
            "*\n"
        );
        assert!(
            !root.join(".gitignore").exists(),
            "never writes a root .gitignore"
        );
        for (p, content) in EXAMPLES {
            assert_eq!(fs::read_to_string(root.join(p)).unwrap(), content);
        }
    }

    #[test]
    fn empty_directory_gets_everything_but_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan_init(dir.path(), true).unwrap();
        assert_eq!(names(&plan.actions)[0], "exists .");
        assert_eq!(plan.examples, ExamplesOutcome::Created);
    }

    #[test]
    fn existing_kb_gets_only_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sql")).unwrap();
        fs::write(root.join("sql/mine.md"), "my entry").unwrap();
        fs::write(root.join(".gitignore"), "target\n").unwrap();
        fs::create_dir(root.join(".kb")).unwrap();
        let plan = plan_init(root, true).unwrap();
        assert_eq!(plan.examples, ExamplesOutcome::SkippedHasEntries);
        assert_eq!(
            names(&plan.actions),
            vec!["exists .", "exists .kb/", "file .kb/.gitignore"]
        );
        let report = apply_init(&plan).unwrap();
        assert_eq!(report.created, vec![".kb/.gitignore"]);
        assert_eq!(report.existing, vec![".", ".kb/"]);
        assert_eq!(
            fs::read_to_string(root.join("sql/mine.md")).unwrap(),
            "my entry"
        );
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            "target\n"
        );
    }

    #[test]
    fn existing_gitignore_in_kb_dir_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".kb")).unwrap();
        fs::write(dir.path().join(".kb/.gitignore"), "custom\n").unwrap();
        let report = apply_init(&plan_init(dir.path(), false).unwrap()).unwrap();
        assert!(report.created.is_empty());
        assert_eq!(report.examples, ExamplesOutcome::SkippedFlag);
        assert_eq!(
            fs::read_to_string(dir.path().join(".kb/.gitignore")).unwrap(),
            "custom\n"
        );
    }

    #[test]
    fn path_that_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        fs::write(&file, "x").unwrap();
        assert_eq!(plan_init(&file, true), Err(InitError::NotADirectory(file)));
    }

    #[test]
    fn file_appearing_after_planning_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan_init(dir.path(), true).unwrap();
        let (first, _) = EXAMPLES[0];
        fs::create_dir_all(dir.path().join(first).parent().unwrap()).unwrap();
        fs::write(dir.path().join(first), "raced").unwrap();
        let report = apply_init(&plan).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join(first)).unwrap(), "raced");
        assert!(report.existing.contains(&first.to_string()));
    }

    #[test]
    fn examples_lint_clean_and_cover_every_type() {
        let dir = tempfile::tempdir().unwrap();
        apply_init(&plan_init(dir.path(), true).unwrap()).unwrap();
        let report = crate::lint::lint(dir.path(), &[]).unwrap();
        assert_eq!(report.entries, 9);
        assert!(report.findings.is_empty(), "{:#?}", report.findings);
        let mut types: Vec<_> = EXAMPLES
            .iter()
            .map(|(p, c)| {
                crate::entry::parse_entry(p, p, c.as_bytes().to_vec())
                    .entry
                    .unwrap()
                    .entry_type()
                    .unwrap()
            })
            .collect();
        types.dedup();
        assert_eq!(types, crate::entry::EntryType::ALL.to_vec());
    }
}
