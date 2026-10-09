//! Bringing the index up to date with the files on disk (FR-002).
//!
//! Work is split in two so the write lock is held only briefly: `scan` reads the database and
//! the files (no lock), and `apply` writes the result in short transactions.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, TransactionBehavior, params};

use super::doc::{Built, Doc, Verdict, build_doc};
use super::schema;
use crate::walk::{Walk, WalkEntry};

/// A file modified within this long before the previous sync started might have been edited again
/// inside the same timestamp tick, so it is re-read (the rule git uses for its index).
const RACY_NS: i64 = 2_000_000_000;

/// How files are read; tests count reads through this.
pub trait FileReader {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
}

pub struct FsReader;

impl FileReader for FsReader {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// A row of the `files` table.
#[derive(Debug, Clone)]
pub struct FileRow {
    pub mtime_ns: i64,
    pub size: i64,
    pub hash: u64,
    pub status: String,
    pub reason: Option<String>,
}

pub(crate) enum Change {
    Upsert {
        file: WalkEntry,
        mtime_ns: i64,
        size: i64,
        built: Built,
        reason: Option<&'static str>,
    },
    /// Same bytes, new timestamp or size: only the fingerprint changes.
    Touch {
        id: String,
        mtime_ns: i64,
        size: i64,
    },
    Remove(String),
}

#[derive(Default)]
pub(crate) struct Scan {
    pub changes: Vec<Change>,
    /// Hashes of IDs that had no row before.
    pub added: BTreeMap<String, u64>,
    /// Hashes of IDs whose content changed.
    pub rehashed: BTreeMap<String, u64>,
    /// IDs on disk right now.
    pub present: BTreeSet<String>,
    /// How many unchanged files were close enough to the last sync to be re-checked.
    pub racy: usize,
    pub start_ns: i64,
}

pub(crate) fn now_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

fn stat(path: &Path) -> Option<(i64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    Some((mtime, i64::try_from(m.len()).unwrap_or(i64::MAX)))
}

/// Compares the walk with the stored rows and reads only the files that need it.
pub(crate) fn scan(
    prev: &HashMap<String, FileRow>,
    last_sync_start_ns: i64,
    walk: &Walk,
    reader: &dyn FileReader,
) -> Scan {
    let mut out = Scan {
        start_ns: now_ns(),
        ..Scan::default()
    };
    let mut lower: HashMap<String, usize> = HashMap::new();
    for f in &walk.files {
        *lower.entry(f.id.to_lowercase()).or_default() += 1;
    }
    for file in &walk.files {
        let Some((mtime_ns, size)) = stat(&file.abs_path) else {
            continue;
        };
        out.present.insert(file.id.clone());
        let collides = lower[&file.id.to_lowercase()] > 1;
        let old = prev.get(&file.id);
        let same = old.is_some_and(|o| o.mtime_ns == mtime_ns && o.size == size);
        let racy = same && mtime_ns >= last_sync_start_ns - RACY_NS;
        let was_collision = old.is_some_and(|o| o.reason.as_deref() == Some("collision"));
        if same && !racy && collides == was_collision {
            continue;
        }
        if racy {
            out.racy += 1;
        }
        let Ok(bytes) = reader.read(&file.abs_path) else {
            out.present.remove(&file.id);
            continue;
        };
        let mut built = build_doc(file, bytes);
        if collides {
            built.verdict = Verdict::Skipped;
        }
        if let Some(o) = old {
            let unchanged = o.hash == built.hash && collides == was_collision;
            if unchanged {
                if !same {
                    out.changes.push(Change::Touch {
                        id: file.id.clone(),
                        mtime_ns,
                        size,
                    });
                }
                continue;
            }
            out.rehashed.insert(file.id.clone(), built.hash);
        } else {
            out.added.insert(file.id.clone(), built.hash);
        }
        out.changes.push(Change::Upsert {
            file: file.clone(),
            mtime_ns,
            size,
            built,
            reason: collides.then_some("collision"),
        });
    }
    for id in prev.keys() {
        if !out.present.contains(id) {
            out.changes.push(Change::Remove(id.clone()));
        }
    }
    out
}

fn delete_entry(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let rowid: Option<i64> = conn
        .query_row("SELECT rowid FROM entries WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .ok();
    if let Some(rowid) = rowid {
        conn.execute("DELETE FROM fts WHERE rowid = ?1", [rowid])?;
    }
    conn.execute("DELETE FROM entries WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM entry_tags WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM entry_vars WHERE id = ?1", [id])?;
    Ok(())
}

fn insert_doc(conn: &Connection, id: &str, doc: &Doc, vocab: bool) -> rusqlite::Result<()> {
    let tags = serde_json::to_string(&doc.tags).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO entries(id, type, title, title_fold, tags, code_repo) VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, doc.entry_type, doc.title, doc.title_fold, tags, doc.code_repo],
    )?;
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fts(rowid, title, tags, id, meta, payload, notes) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![rowid, doc.cols[0], doc.cols[1], doc.cols[2], doc.cols[3], doc.cols[4], doc.cols[5]],
    )?;
    if vocab {
        let mut add =
            conn.prepare_cached("INSERT OR IGNORE INTO vocab(term, len) VALUES (?1, ?2)")?;
        for col in &doc.cols {
            for w in crate::fold::words(col) {
                add.execute(params![w.text, w.text.chars().count()])?;
            }
        }
    }
    for t in &doc.tags {
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags(id, tag) VALUES (?1,?2)",
            params![id, t],
        )?;
    }
    for v in &doc.vars {
        conn.execute(
            "INSERT OR IGNORE INTO entry_vars(id, name) VALUES (?1,?2)",
            params![id, v],
        )?;
    }
    Ok(())
}

fn apply_change(conn: &Connection, change: &Change, vocab: bool) -> rusqlite::Result<()> {
    match change {
        Change::Remove(id) => {
            delete_entry(conn, id)?;
            conn.execute("DELETE FROM files WHERE id = ?1", [id])?;
        }
        Change::Touch { id, mtime_ns, size } => {
            conn.execute(
                "UPDATE files SET mtime_ns = ?2, size = ?3 WHERE id = ?1",
                params![id, mtime_ns, size],
            )?;
        }
        Change::Upsert {
            file,
            mtime_ns,
            size,
            built,
            reason,
        } => {
            delete_entry(conn, &file.id)?;
            let status = match &built.verdict {
                Verdict::Indexed(doc) => {
                    insert_doc(conn, &file.id, doc, vocab)?;
                    "ok"
                }
                Verdict::Skipped => "skipped",
            };
            conn.execute(
                "INSERT OR REPLACE INTO files(id, path, mtime_ns, size, hash, status, reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    file.id,
                    file.rel_path,
                    mtime_ns,
                    size,
                    i64::from_ne_bytes(built.hash.to_ne_bytes()),
                    status,
                    reason
                ],
            )?;
        }
    }
    Ok(())
}

fn set_meta(conn: &Connection, key: &str, value: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
        params![key, value.to_string()],
    )?;
    Ok(())
}

/// Writes `scan` in `BEGIN IMMEDIATE` batches. The rows of one file change in one transaction.
pub(crate) fn apply(conn: &mut Connection, scan: &Scan, batch: usize) -> rusqlite::Result<()> {
    let mut chunks: Vec<&[Change]> = scan.changes.chunks(batch.max(1)).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.into_iter().enumerate() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for change in chunk {
            apply_change(&tx, change, true)?;
        }
        if i == last {
            set_meta(&tx, "last_sync_start_ns", scan.start_ns)?;
        }
        tx.commit()?;
    }
    Ok(())
}

/// Replaces the whole index in one transaction (readers keep their snapshot until it commits).
pub(crate) fn apply_reset(conn: &mut Connection, scan: &Scan) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(schema::DROP)?;
    tx.execute_batch(schema::CREATE)?;
    for change in &scan.changes {
        if matches!(change, Change::Upsert { .. }) {
            apply_change(&tx, change, false)?;
        }
    }
    tx.execute(
        "INSERT OR IGNORE INTO vocab(term, len) SELECT term, length(term) FROM fts_vocab",
        [],
    )?;
    set_meta(&tx, "last_sync_start_ns", scan.start_ns)?;
    set_meta(&tx, "last_full_build", scan.start_ns / 1_000_000_000)?;
    tx.pragma_update(None, "user_version", schema::INDEX_VERSION)?;
    tx.commit()
}
