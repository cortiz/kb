//! Finding entry files under the KB root.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::lint::{Finding, RuleCode, Severity};

/// One entry file found by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    /// The entry ID: the path relative to the root, without `.md`, with `/` separators.
    pub id: String,
    /// The path relative to the root, with `/` separators.
    pub rel_path: String,
    /// The path on disk.
    pub abs_path: PathBuf,
}

/// The files found, plus walk findings (KB052).
#[derive(Debug, Clone, Default)]
pub struct Walk {
    pub files: Vec<WalkEntry>,
    pub findings: Vec<Finding>,
}

/// The walk could not run.
#[derive(Debug)]
pub struct WalkError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for WalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for WalkError {}

/// Walks `starts` (files or directories, absolute, inside `root`). `root` must be canonical.
pub fn walk(root: &Path, starts: &[PathBuf]) -> Result<Walk, WalkError> {
    let mut w = Walk::default();
    let mut visited = HashSet::new();
    for start in starts {
        let meta = std::fs::metadata(start).map_err(|e| WalkError {
            path: start.clone(),
            message: e.to_string(),
        })?;
        if meta.is_dir() {
            walk_dir(root, start, &mut visited, &mut w)?;
        } else {
            add_file(root, start, &mut w);
        }
    }
    w.files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    w.files.dedup_by(|a, b| a.rel_path == b.rel_path);
    w.findings.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(w)
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// `path` relative to `root` with `/` separators, or `None` when it is outside the root.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

fn add_file(root: &Path, path: &Path, w: &mut Walk) {
    if path.extension().is_none_or(|e| e != "md") {
        return;
    }
    let Some(rel_path) = relative(root, path) else {
        return;
    };
    if rel_path.split('/').any(|c| c.starts_with('.')) {
        return;
    }
    let id = rel_path
        .strip_suffix(".md")
        .unwrap_or(&rel_path)
        .to_string();
    w.files.push(WalkEntry {
        id,
        rel_path,
        abs_path: path.to_path_buf(),
    });
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    visited: &mut HashSet<PathBuf>,
    w: &mut Walk,
) -> Result<(), WalkError> {
    if let Ok(canonical) = dir.canonicalize()
        && !visited.insert(canonical)
    {
        return Ok(());
    }
    let walker = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.file_name()));
    for entry in walker {
        let entry = entry.map_err(|e| WalkError {
            path: e
                .path()
                .map_or_else(|| dir.to_path_buf(), Path::to_path_buf),
            message: e.to_string(),
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.path();
        if entry.path_is_symlink() {
            follow_symlink(root, path, visited, w)?;
        } else if entry.file_type().is_file() {
            add_file(root, path, w);
        }
    }
    Ok(())
}

fn follow_symlink(
    root: &Path,
    link: &Path,
    visited: &mut HashSet<PathBuf>,
    w: &mut Walk,
) -> Result<(), WalkError> {
    let rel = relative(root, link).unwrap_or_else(|| link.to_string_lossy().into_owned());
    let problem = match link.canonicalize() {
        Ok(target) if target.starts_with(root) => {
            if target.is_dir() {
                return walk_dir(root, link, visited, w);
            }
            add_file(root, link, w);
            return Ok(());
        }
        Ok(_) => "points outside the KB root",
        Err(_) => "points to a file that does not exist",
    };
    w.findings.push(Finding {
        path: rel,
        id: None,
        line: 1,
        column: 1,
        severity: Severity::Warning,
        code: RuleCode::SymlinkOutsideRoot,
        message: format!("symlink {problem}; skipped"),
        fix: Some("move the target into the KB, or remove the link".into()),
    });
    Ok(())
}

/// KB053 findings for IDs that differ only by letter case.
pub fn case_collisions(files: &[WalkEntry]) -> Vec<Finding> {
    let mut groups: BTreeMap<String, Vec<&WalkEntry>> = BTreeMap::new();
    for file in files {
        groups.entry(file.id.to_lowercase()).or_default().push(file);
    }
    let mut out = Vec::new();
    for group in groups.values().filter(|g| g.len() > 1) {
        for file in group {
            let others: Vec<&str> = group
                .iter()
                .filter(|o| o.rel_path != file.rel_path)
                .map(|o| o.rel_path.as_str())
                .collect();
            out.push(Finding {
                path: file.rel_path.clone(),
                id: Some(file.id.clone()),
                line: 1,
                column: 1,
                severity: Severity::Error,
                code: RuleCode::IdCaseCollision,
                message: format!(
                    "the ID `{}` differs only in letter case from {}",
                    file.id,
                    others.join(", ")
                ),
                fix: Some("rename one of them so the IDs differ by more than case".into()),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn kb() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        (dir, root)
    }

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "x").unwrap();
    }

    fn ids(w: &Walk) -> Vec<&str> {
        w.files.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn finds_md_files_sorted_and_skips_hidden() {
        let (_d, root) = kb();
        for rel in [
            "sql/b.md",
            "sql/a.md",
            "top.md",
            "notes.txt",
            ".git/x.md",
            ".kb/y.md",
            "sql/.draft.md",
            "a/.hidden/z.md",
        ] {
            touch(&root, rel);
        }
        let w = walk(&root, std::slice::from_ref(&root)).unwrap();
        assert_eq!(ids(&w), vec!["sql/a", "sql/b", "top"]);
        assert_eq!(w.files[0].rel_path, "sql/a.md");
        assert_eq!(w.files[0].abs_path, root.join("sql/a.md"));
        assert!(w.findings.is_empty());
    }

    #[test]
    fn hidden_root_still_works() {
        let (_d, base) = kb();
        let root = base.join(".kb-data");
        touch(&root, "a.md");
        let w = walk(&root, std::slice::from_ref(&root)).unwrap();
        assert_eq!(ids(&w), vec!["a"]);
    }

    #[test]
    fn scoped_to_a_file_or_directory() {
        let (_d, root) = kb();
        for rel in ["sql/a.md", "sumo/b.md"] {
            touch(&root, rel);
        }
        let w = walk(&root, &[root.join("sumo")]).unwrap();
        assert_eq!(ids(&w), vec!["sumo/b"]);
        let w = walk(&root, &[root.join("sql/a.md")]).unwrap();
        assert_eq!(ids(&w), vec!["sql/a"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks() {
        use std::os::unix::fs::symlink;
        let (_d, root) = kb();
        let (_o, outside) = kb();
        touch(&root, "real/a.md");
        touch(&outside, "o.md");
        symlink(root.join("real/a.md"), root.join("alias.md")).unwrap();
        symlink(root.join("real"), root.join("linked")).unwrap();
        symlink(outside.join("o.md"), root.join("out.md")).unwrap();
        symlink(&root, root.join("real/loop")).unwrap();
        let w = walk(&root, std::slice::from_ref(&root)).unwrap();
        assert_eq!(ids(&w), vec!["alias", "linked/a", "real/a"]);
        let f: Vec<_> = w
            .findings
            .iter()
            .map(|f| (f.path.as_str(), f.code, f.severity))
            .collect();
        assert_eq!(
            f,
            vec![("out.md", RuleCode::SymlinkOutsideRoot, Severity::Warning)]
        );
    }

    #[test]
    fn case_collisions_reported_on_each_file() {
        let entry = |id: &str| WalkEntry {
            id: id.into(),
            rel_path: format!("{id}.md"),
            abs_path: PathBuf::new(),
        };
        let files = vec![entry("SQL/a"), entry("sql/a"), entry("sql/b")];
        let found = case_collisions(&files);
        let got: Vec<_> = found
            .iter()
            .map(|f| (f.path.as_str(), f.line, f.column, f.code))
            .collect();
        assert_eq!(
            got,
            vec![
                ("SQL/a.md", 1, 1, RuleCode::IdCaseCollision),
                ("sql/a.md", 1, 1, RuleCode::IdCaseCollision)
            ]
        );
        assert!(
            found[0].message.contains("sql/a.md"),
            "{}",
            found[0].message
        );
    }
}
