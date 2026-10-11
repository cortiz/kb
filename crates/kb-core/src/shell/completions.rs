//! Index-backed tab completion queries.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Entries,
    Tags,
    Types,
    Vars,
    VarValues,
}

impl CompletionKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "entries" => Some(Self::Entries),
            "tags" => Some(Self::Tags),
            "types" => Some(Self::Types),
            "vars" => Some(Self::Vars),
            "var-values" => Some(Self::VarValues),
            _ => None,
        }
    }
}

pub fn complete_types(prefix: Option<&str>) -> Vec<String> {
    const TYPES: [&str; 7] = ["cmd", "sql", "sumo", "cw", "prompt", "link", "code"];
    TYPES
        .iter()
        .filter(|t| prefix.map_or(true, |p| t.starts_with(p)))
        .map(|&s| s.to_string())
        .collect()
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String)> {
    Ok((row.get(0)?, row.get(1)?))
}

pub fn complete_entries(root: &Path, prefix: Option<&str>) -> Vec<(String, String)> {
    let db_path = root.join(".kb").join("index.db");
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let p = prefix.unwrap_or("");
    let sql = if p.is_empty() {
        "SELECT id, title FROM entries ORDER BY id"
    } else {
        "SELECT id, title FROM entries WHERE id LIKE ?1 || '%' ORDER BY id"
    };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = if p.is_empty() {
        stmt.query_map([], map_entry)
    } else {
        stmt.query_map([p], map_entry)
    };

    match rows {
        Ok(mapped) => mapped.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

fn map_tag(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    row.get(0)
}

pub fn complete_tags(root: &Path, prefix: Option<&str>) -> Vec<String> {
    let db_path = root.join(".kb").join("index.db");
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let p = prefix.unwrap_or("");
    let sql = if p.is_empty() {
        "SELECT DISTINCT tag FROM entry_tags ORDER BY tag"
    } else {
        "SELECT DISTINCT tag FROM entry_tags WHERE tag LIKE ?1 || '%' ORDER BY tag"
    };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = if p.is_empty() {
        stmt.query_map([], map_tag)
    } else {
        stmt.query_map([p], map_tag)
    };

    match rows {
        Ok(mapped) => mapped.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

fn map_var(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    let name: String = row.get(0)?;
    Ok(format!("{name}="))
}

pub fn complete_vars(root: &Path, entry_id: &str, prefix: Option<&str>) -> Vec<String> {
    let db_path = root.join(".kb").join("index.db");
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let p = prefix.unwrap_or("");
    let sql = if p.is_empty() {
        "SELECT name FROM entry_vars WHERE id = ?1 ORDER BY name"
    } else {
        "SELECT name FROM entry_vars WHERE id = ?1 AND name LIKE ?2 || '%' ORDER BY name"
    };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = if p.is_empty() {
        stmt.query_map([entry_id], map_var)
    } else {
        stmt.query_map([entry_id, p], map_var)
    };

    match rows {
        Ok(mapped) => mapped.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

pub fn complete_var_values(
    root: &Path,
    entry_id: &str,
    var_name: &str,
    prefix: Option<&str>,
) -> Vec<String> {
    let db_path = root.join(".kb").join("index.db");
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let rel_path: String = match conn.query_row(
        "SELECT path FROM files WHERE id = ?1",
        [entry_id],
        |row| row.get(0),
    ) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let file_path = root.join(rel_path);
    let bytes = match std::fs::read(&file_path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };

    let parsed = crate::entry::parse_entry(entry_id, &file_path.to_string_lossy(), bytes);
    let Some(entry) = parsed.entry else {
        return Vec::new();
    };

    let Some(var) = entry.vars().iter().find(|v| v.name == var_name) else {
        return Vec::new();
    };

    var.options
        .iter()
        .filter(|opt| prefix.map_or(true, |p| opt.starts_with(p)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Kb;
    use std::path::PathBuf;
    use std::time::Instant;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        let root = d.path().join("kb");
        copy(&src, &root);
        // Build index
        let mut kb = Kb::open(&root).unwrap();
        let _ = kb.reindex().unwrap();
        (d, root)
    }

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }

    #[test]
    fn test_complete_types() {
        let all = complete_types(None);
        assert_eq!(
            all,
            vec!["cmd", "sql", "sumo", "cw", "prompt", "link", "code"]
        );

        let filtered = complete_types(Some("c"));
        assert_eq!(filtered, vec!["cmd", "cw", "code"]);
    }

    #[test]
    fn test_complete_entries_budget() {
        let (_d, root) = fixture();
        let start = Instant::now();
        let entries = complete_entries(&root, None);
        let elapsed = start.elapsed();

        assert!(!entries.is_empty(), "must return indexed entries");
        assert!(
            elapsed.as_millis() < 50,
            "completion must return within 50ms (took {}ms)",
            elapsed.as_millis()
        );

        let filtered = complete_entries(&root, Some("cmd/"));
        assert!(
            filtered.iter().all(|(id, _)| id.starts_with("cmd/")),
            "must filter by prefix"
        );
        assert!(
            filtered.iter().any(|(id, title)| id == "cmd/restart-worker" && title == "Restart worker"),
            "must include entry id and title"
        );
    }

    #[test]
    fn test_complete_tags() {
        let (_d, root) = fixture();
        let tags = complete_tags(&root, None);
        assert!(!tags.is_empty(), "must return tags");
        assert!(tags.contains(&"ops".to_string()));

        let filtered = complete_tags(&root, Some("op"));
        assert_eq!(filtered, vec!["ops"]);
    }

    #[test]
    fn test_complete_vars() {
        let (_d, root) = fixture();
        let vars = complete_vars(&root, "cmd/deploy-service", None);
        assert_eq!(vars, vec!["env=", "region="]);

        let filtered = complete_vars(&root, "cmd/deploy-service", Some("e"));
        assert_eq!(filtered, vec!["env="]);
    }

    #[test]
    fn test_complete_var_values() {
        let (_d, root) = fixture();
        let options = complete_var_values(&root, "cmd/deploy-service", "env", None);
        assert_eq!(options, vec!["prod", "stage"]);

        let filtered = complete_var_values(&root, "cmd/deploy-service", "env", Some("p"));
        assert_eq!(filtered, vec!["prod"]);
    }
}
