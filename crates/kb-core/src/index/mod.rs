//! The disposable search index at `.kb/index.db` (SQLite + FTS5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};

pub mod doc;
pub mod schema;
pub mod sync;

pub use schema::INDEX_VERSION;
use sync::{FileReader, FileRow, Scan};

use crate::search::{KbError, Notice};
use crate::walk::Walk;

const BUSY: Duration = Duration::from_millis(1000);
const BATCH: usize = 200;

/// Creates `.kb/` and, if absent, `.kb/.gitignore` containing `*`, so derived and per-machine
/// files are never committed (constitution II). Never overwrites.
pub fn ensure_kb_dir(root: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let dir = root.join(".kb");
    std::fs::create_dir_all(&dir)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(".gitignore"))
    {
        Ok(mut f) => f.write_all(b"*\n")?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(dir)
}

/// Why an index file can't be used as is.
#[derive(Debug)]
pub enum OpenFailure {
    Missing,
    /// A valid, empty database: another process is creating it.
    Empty,
    Version(i64),
    Busy,
    Garbage(String),
}

impl std::fmt::Display for OpenFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenFailure::Missing => write!(f, "no index file"),
            OpenFailure::Empty => write!(f, "the index is being created"),
            OpenFailure::Version(v) => {
                write!(f, "index version {v}, this kb writes {INDEX_VERSION}")
            }
            OpenFailure::Busy => write!(f, "the index is locked"),
            OpenFailure::Garbage(m) => write!(f, "{m}"),
        }
    }
}

fn classify(e: &rusqlite::Error) -> OpenFailure {
    match e.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => OpenFailure::Busy,
        _ => OpenFailure::Garbage(e.to_string()),
    }
}

/// An open index.
pub struct Index {
    pub(crate) conn: Connection,
    pub(crate) path: Option<PathBuf>,
}

pub fn index_path(root: &Path) -> PathBuf {
    root.join(".kb").join("index.db")
}

fn remove_files(path: &Path) -> std::io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match std::fs::remove_file(PathBuf::from(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl Index {
    /// Opens an existing index of the current version.
    pub fn open_existing(path: &Path, read_only: bool) -> Result<Index, OpenFailure> {
        if !path.exists() {
            return Err(OpenFailure::Missing);
        }
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
        };
        let conn = Connection::open_with_flags(path, flags).map_err(|e| classify(&e))?;
        conn.busy_timeout(BUSY).map_err(|e| classify(&e))?;
        if !read_only {
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(|e| classify(&e))?;
            conn.pragma_update(None, "synchronous", "NORMAL")
                .map_err(|e| classify(&e))?;
        }
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| classify(&e))?;
        if v == 0 {
            let tables: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                .map_err(|e| classify(&e))?;
            return Err(if tables == 0 {
                OpenFailure::Empty
            } else {
                OpenFailure::Version(0)
            });
        }
        if v != i64::from(INDEX_VERSION) {
            return Err(OpenFailure::Version(v));
        }
        conn.query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
            .map_err(|e| classify(&e))?;
        Ok(Index {
            conn,
            path: Some(path.to_path_buf()),
        })
    }

    fn in_memory(scan: &Scan) -> Result<Index, KbError> {
        let mut conn = Connection::open_in_memory().map_err(index_err)?;
        sync::apply_reset(&mut conn, scan).map_err(index_err)?;
        Ok(Index { conn, path: None })
    }

    pub(crate) fn is_in_memory(&self) -> bool {
        self.path.is_none()
    }

    pub fn list_tags(&self) -> rusqlite::Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT tag, COUNT(*) as cnt FROM entry_tags GROUP BY tag ORDER BY cnt DESC, tag ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let tag: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok((tag, count as usize))
        })?;
        rows.collect()
    }

    fn prev_rows(&self) -> rusqlite::Result<HashMap<String, FileRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, mtime_ns, size, hash, status, reason FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                FileRow {
                    mtime_ns: r.get(1)?,
                    size: r.get(2)?,
                    hash: u64::from_ne_bytes(r.get::<_, i64>(3)?.to_ne_bytes()),
                    status: r.get(4)?,
                    reason: r.get(5)?,
                },
            ))
        })?;
        rows.collect()
    }

    fn meta(&self, key: &str) -> Option<i64> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse().ok())
    }

    fn count(&self, status: &str) -> usize {
        self.conn
            .query_row(
                "SELECT count(*) FROM files WHERE status = ?1",
                [status],
                |r| r.get::<_, i64>(0),
            )
            .map_or(0, |n| usize::try_from(n).unwrap_or(0))
    }

    pub(crate) fn skipped(&self) -> usize {
        self.count("skipped")
    }

    pub(crate) fn indexed(&self) -> usize {
        self.count("ok")
    }
}

pub(crate) fn index_err(e: rusqlite::Error) -> KbError {
    KbError::Index(e.to_string())
}

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// What a sync learned about the files, for usage bookkeeping.
#[derive(Default)]
pub(crate) struct FileFacts {
    pub added: std::collections::BTreeMap<String, u64>,
    pub rehashed: std::collections::BTreeMap<String, u64>,
    pub present: std::collections::BTreeSet<String>,
}

pub(crate) struct Synced {
    pub index: Index,
    pub notices: Vec<Notice>,
    pub facts: FileFacts,
}

fn facts(scan: &Scan) -> FileFacts {
    FileFacts {
        added: scan.added.clone(),
        rehashed: scan.rehashed.clone(),
        present: scan.present.clone(),
    }
}

/// Opens the index and brings it up to date; rebuilds it when missing, corrupt or another
/// version; falls back to an in-memory index when `.kb/` can't be written.
pub(crate) fn open_synced(
    root: &Path,
    walk: &Walk,
    reader: &dyn FileReader,
    force_rebuild: bool,
) -> Result<Synced, KbError> {
    let path = index_path(root);
    if force_rebuild {
        let _ = remove_files(&path);
    }
    match Index::open_existing(&path, false) {
        Ok(index) => incremental(index, root, walk, reader),
        Err(OpenFailure::Busy) => {
            let scan = sync::scan(&HashMap::new(), 0, walk, reader);
            Ok(Synced {
                index: Index::in_memory(&scan)?,
                notices: Vec::new(),
                facts: facts(&scan),
            })
        }
        Err(OpenFailure::Garbage(_)) => {
            if remove_files(&path).is_err() {
                return read_only(walk, reader);
            }
            rebuild(root, walk, reader)
        }
        Err(OpenFailure::Missing | OpenFailure::Empty | OpenFailure::Version(_)) => {
            rebuild(root, walk, reader)
        }
    }
}

fn read_only(walk: &Walk, reader: &dyn FileReader) -> Result<Synced, KbError> {
    let scan = sync::scan(&HashMap::new(), 0, walk, reader);
    Ok(Synced {
        index: Index::in_memory(&scan)?,
        notices: vec![Notice::ReadOnly],
        facts: facts(&scan),
    })
}

fn incremental(
    mut index: Index,
    root: &Path,
    walk: &Walk,
    reader: &dyn FileReader,
) -> Result<Synced, KbError> {
    let prev = match index.prev_rows() {
        Ok(p) => p,
        Err(_) => {
            drop(index);
            let _ = remove_files(&index_path(root));
            return rebuild(root, walk, reader);
        }
    };
    let last = index.meta("last_sync_start_ns").unwrap_or(0);
    let scan = sync::scan(&prev, last, walk, reader);
    // Nothing to write and no racy file to settle: skip the write transaction entirely.
    if !(scan.changes.is_empty() && scan.racy == 0) {
        match sync::apply(&mut index.conn, &scan, BATCH) {
            Ok(()) => {}
            Err(e) if is_busy(&e) => {} // another process is syncing; use the current snapshot
            Err(_) => {
                let mut s = read_only(walk, reader)?;
                s.facts = facts(&scan);
                return Ok(s);
            }
        }
    }
    Ok(Synced {
        index,
        notices: Vec::new(),
        facts: facts(&scan),
    })
}

fn rebuild(root: &Path, walk: &Walk, reader: &dyn FileReader) -> Result<Synced, KbError> {
    let scan = sync::scan(&HashMap::new(), 0, walk, reader);
    let Ok(dir) = ensure_kb_dir(root) else {
        return Ok(Synced {
            index: Index::in_memory(&scan)?,
            notices: vec![Notice::ReadOnly],
            facts: facts(&scan),
        });
    };
    let path = dir.join("index.db");
    let opened = Connection::open(&path).and_then(|c| {
        c.busy_timeout(BUSY)?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(c)
    });
    let mut conn = match opened {
        Ok(c) => c,
        Err(e) if is_busy(&e) => return recheck(root, walk, reader, &scan),
        Err(_) => return read_only(walk, reader),
    };
    match sync::apply_reset(&mut conn, &scan) {
        Ok(()) => {
            let index = Index {
                conn,
                path: Some(path),
            };
            let entries = index.indexed();
            Ok(Synced {
                index,
                notices: vec![Notice::Rebuilt { entries }],
                facts: facts(&scan),
            })
        }
        Err(e) if is_busy(&e) => recheck(root, walk, reader, &scan),
        Err(_) => read_only(walk, reader),
    }
}

/// Someone else is building: use their index if it is ready, else a temporary one.
fn recheck(
    root: &Path,
    walk: &Walk,
    reader: &dyn FileReader,
    scan: &Scan,
) -> Result<Synced, KbError> {
    match Index::open_existing(&index_path(root), false) {
        Ok(index) => incremental(index, root, walk, reader),
        Err(_) => Ok(Synced {
            index: Index::in_memory(scan)?,
            notices: Vec::new(),
            facts: facts(scan),
        }),
    }
}

/// Rebuilds from scratch (`kb reindex`): the whole index is replaced in one transaction.
pub(crate) fn reindex(
    root: &Path,
    walk: &Walk,
    reader: &dyn FileReader,
) -> Result<Synced, KbError> {
    let path = index_path(root);
    match Index::open_existing(&path, false) {
        Ok(mut index) => {
            let scan = sync::scan(&HashMap::new(), 0, walk, reader);
            match sync::apply_reset(&mut index.conn, &scan) {
                Ok(()) => Ok(Synced {
                    index,
                    notices: Vec::new(),
                    facts: facts(&scan),
                }),
                Err(e) => Err(KbError::Io {
                    path,
                    message: e.to_string(),
                }),
            }
        }
        Err(OpenFailure::Garbage(_)) => {
            let _ = remove_files(&path);
            let s = rebuild(root, walk, reader)?;
            check_writable(s, path)
        }
        Err(_) => {
            let s = rebuild(root, walk, reader)?;
            check_writable(s, path)
        }
    }
}

fn check_writable(s: Synced, path: PathBuf) -> Result<Synced, KbError> {
    if s.index.is_in_memory() {
        return Err(KbError::Io {
            path,
            message: "the index can't be written (is .kb/ writable?)".into(),
        });
    }
    Ok(Synced {
        notices: Vec::new(),
        ..s
    })
}

/// State of the index file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexState {
    Ok,
    Missing,
    Unusable(String),
}

/// The index as it is on disk (`kb index status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStatus {
    pub path: PathBuf,
    pub state: IndexState,
    pub entries_indexed: usize,
    pub entries_skipped: usize,
    pub size_bytes: u64,
    pub index_version: u32,
    pub last_full_build: Option<i64>,
}

pub(crate) fn status(root: &Path) -> IndexStatus {
    let path = index_path(root);
    let mut s = IndexStatus {
        path: path.clone(),
        state: IndexState::Missing,
        entries_indexed: 0,
        entries_skipped: 0,
        size_bytes: 0,
        index_version: INDEX_VERSION,
        last_full_build: None,
    };
    for suffix in ["", "-wal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        s.size_bytes += std::fs::metadata(PathBuf::from(name)).map_or(0, |m| m.len());
    }
    match Index::open_existing(&path, true) {
        Ok(index) => {
            s.state = IndexState::Ok;
            s.entries_indexed = index.indexed();
            s.entries_skipped = index.skipped();
            s.last_full_build = index.meta("last_full_build");
        }
        Err(OpenFailure::Missing) => {}
        Err(e) => s.state = IndexState::Unusable(e.to_string()),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::walk;

    fn kb_with(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        for (rel, text) in files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        (d, root)
    }

    pub(crate) fn entry(title: &str, body: &str) -> String {
        format!(
            "---\nkb_format: 1\ntype: prompt\ntitle: {title}\ntags: [t1]\n---\n\n```\n{body}\n```\n"
        )
    }

    fn open(root: &Path) -> Synced {
        let w = walk(root, &[root.to_path_buf()]).unwrap();
        open_synced(root, &w, &sync::FsReader, false).unwrap()
    }

    #[test]
    fn ensure_kb_dir_is_idempotent_and_never_overwrites() {
        let (_d, root) = kb_with(&[]);
        let dir = ensure_kb_dir(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "*\n"
        );
        std::fs::write(dir.join(".gitignore"), "custom\n").unwrap();
        ensure_kb_dir(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "custom\n"
        );
    }

    #[test]
    fn first_open_builds_the_schema_and_rows() {
        let (_d, root) = kb_with(&[
            ("a/one.md", &entry("One", "hello")),
            ("a/two.md", &entry("Two", "world")),
            ("bad.md", "not an entry"),
        ]);
        let s = open(&root);
        assert_eq!(s.notices, vec![Notice::Rebuilt { entries: 2 }]);
        let v: i64 = s
            .index
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, i64::from(INDEX_VERSION));
        assert_eq!(s.index.indexed(), 2);
        assert_eq!(s.index.skipped(), 1);
        let (e, f): (i64, i64) = s
            .index
            .conn
            .query_row(
                "SELECT (SELECT count(*) FROM entries), (SELECT count(*) FROM fts)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((e, f), (2, 2));
        assert!(s.index.meta("last_full_build").is_some());
        assert_eq!(
            std::fs::read_to_string(root.join(".kb/.gitignore")).unwrap(),
            "*\n"
        );
        // second open: no rebuild
        drop(s);
        assert!(open(&root).notices.is_empty());
    }

    #[test]
    fn reset_keeps_readers_consistent_until_commit() {
        let (_d, root) = kb_with(&[("a.md", &entry("A", "x"))]);
        let s = open(&root);
        let reader = Index::open_existing(&index_path(&root), false).unwrap();
        reader.conn.execute_batch("BEGIN").unwrap();
        let n: i64 = reader
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        std::fs::write(root.join("b.md"), entry("B", "y")).unwrap();
        let w = walk(&root, std::slice::from_ref(&root)).unwrap();
        drop(s);
        reindex(&root, &w, &sync::FsReader).unwrap();
        let n: i64 = reader
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "old snapshot inside the open read transaction");
        reader.conn.execute_batch("COMMIT").unwrap();
        let n: i64 = reader
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn garbage_file_and_stale_wal_are_replaced() {
        let (_d, root) = kb_with(&[("a.md", &entry("A", "x"))]);
        ensure_kb_dir(&root).unwrap();
        std::fs::write(index_path(&root), "garbage garbage garbage").unwrap();
        std::fs::write(root.join(".kb/index.db-wal"), "stale").unwrap();
        let s = open(&root);
        assert_eq!(s.notices, vec![Notice::Rebuilt { entries: 1 }]);
        assert_eq!(s.index.indexed(), 1);
    }

    #[test]
    fn version_mismatch_rebuilds_in_place() {
        let (_d, root) = kb_with(&[("a.md", &entry("A", "x"))]);
        drop(open(&root));
        {
            let c = Connection::open(index_path(&root)).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        let s = open(&root);
        assert_eq!(s.notices, vec![Notice::Rebuilt { entries: 1 }]);
    }

    #[test]
    fn status_reports_without_creating() {
        let (_d, root) = kb_with(&[("a.md", &entry("A", "x")), ("bad.md", "x")]);
        assert_eq!(status(&root).state, IndexState::Missing);
        assert!(!index_path(&root).exists());
        drop(open(&root));
        let s = status(&root);
        assert_eq!(
            (s.state.clone(), s.entries_indexed, s.entries_skipped),
            (IndexState::Ok, 1, 1)
        );
        assert!(s.size_bytes > 0 && s.last_full_build.is_some());
        std::fs::write(index_path(&root), "garbage").unwrap();
        assert!(matches!(status(&root).state, IndexState::Unusable(_)));
    }
}
