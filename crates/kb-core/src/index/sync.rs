//! Bringing the index up to date with the files on disk (FR-002).
//!
//! Work is split in two so the write lock is held only briefly: `scan` reads the database and
//! the files (no lock), and `apply` writes the result in short transactions.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    let mut seen = HashSet::with_capacity(walk.files.len());
    let mut collided = HashSet::new();
    for f in &walk.files {
        let low = f.id.to_lowercase();
        if !seen.insert(low.clone()) {
            collided.insert(low);
        }
    }
    for file in &walk.files {
        let Some((mtime_ns, size)) = stat(&file.abs_path) else {
            continue;
        };
        out.present.insert(file.id.clone());
        let collides = if collided.is_empty() {
            false
        } else {
            collided.contains(&file.id.to_lowercase())
        };
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
    conn.execute("DELETE FROM entry_links WHERE source_id = ?1", [id])?;
    conn.execute("DELETE FROM runbook_steps WHERE runbook_id = ?1", [id])?;
    Ok(())
}

/// The words of `doc` that feed typo expansion. A note's body is arbitrary prose and code; only
/// its title, tags, ID and headings count, so the vocabulary (and the typo scan) stays small.
fn vocab_words(doc: &Doc) -> impl Iterator<Item = crate::fold::Word> + '_ {
    let cols = if doc.entry_type == "note" {
        &doc.cols[..4]
    } else {
        &doc.cols[..]
    };
    cols.iter().flat_map(|col| crate::fold::words(col))
}

fn insert_doc(conn: &Connection, id: &str, doc: &Doc, vocab: bool) -> rusqlite::Result<()> {
    let tags = serde_json::to_string(&doc.tags).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO entries(id, type, title, title_fold, tags, code_repo, updated) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            id,
            doc.entry_type,
            doc.title,
            doc.title_fold,
            tags,
            doc.code_repo,
            doc.updated
        ],
    )?;
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fts(rowid, title, tags, id, meta, payload, notes) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![rowid, doc.cols[0], doc.cols[1], doc.cols[2], doc.cols[3], doc.cols[4], doc.cols[5]],
    )?;
    if vocab {
        let mut add =
            conn.prepare_cached("INSERT OR IGNORE INTO vocab(term, len) VALUES (?1, ?2)")?;
        for w in vocab_words(doc) {
            add.execute(params![w.text, w.text.chars().count() as i64])?;
        }
    }
    for target in &doc.links {
        conn.execute(
            "INSERT OR IGNORE INTO entry_links(source_id, target_id) VALUES (?1,?2)",
            params![id, target],
        )?;
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
    for (idx, step_id, entry_id) in &doc.runbook_steps {
        if let Some(target) = entry_id {
            conn.execute(
                "INSERT INTO runbook_steps(runbook_id, step_id, position, entry_id) VALUES (?1,?2,?3,?4)",
                params![id, step_id, *idx as i64, target],
            )?;
        }
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

/// Adds the titles of the entries a note links to into the note's `meta` column, so searching for
/// a linked entry's title finds the notes that mention it. Runs after all rows are written, when
/// the targets are known; a retitled target is picked up the next time the note is indexed.
fn add_link_titles(conn: &Connection, scan: &Scan) -> rusqlite::Result<()> {
    for change in &scan.changes {
        let Change::Upsert { file, built, .. } = change else {
            continue;
        };
        let Verdict::Indexed(doc) = &built.verdict else {
            continue;
        };
        if doc.links.is_empty() {
            continue;
        }
        let titles: Vec<String> = conn
            .prepare_cached(
                "SELECT t.title FROM entry_links l JOIN entries t ON t.id = l.target_id \
                 WHERE l.source_id = ?1 ORDER BY l.target_id",
            )?
            .query_map([&file.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if titles.is_empty() {
            continue;
        }
        conn.execute(
            "UPDATE fts SET meta = meta || ' ' || ?2 WHERE rowid = (SELECT rowid FROM entries WHERE id = ?1)",
            params![file.id, titles.join(" ")],
        )?;
    }
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
            add_link_titles(&tx, scan)?;
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
    // The vocabulary is built from the documents, not from `fts_vocab`: that would also hold the
    // words of every note body.
    let mut terms: HashSet<String> = HashSet::new();
    for change in &scan.changes {
        if let Change::Upsert { built, .. } = change
            && let Verdict::Indexed(doc) = &built.verdict
        {
            terms.extend(vocab_words(doc).map(|w| w.text));
        }
    }
    {
        let mut add = tx.prepare("INSERT OR IGNORE INTO vocab(term, len) VALUES (?1, ?2)")?;
        for term in &terms {
            add.execute(params![term, term.chars().count() as i64])?;
        }
    }
    add_link_titles(&tx, scan)?;
    set_meta(&tx, "last_sync_start_ns", scan.start_ns)?;
    set_meta(&tx, "last_full_build", scan.start_ns / 1_000_000_000)?;
    tx.pragma_update(None, "user_version", schema::INDEX_VERSION)?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NOTE: &str = "---\nkb_format: 1\ntype: note\ntags: [redis]\n---\n\n# Cache design\n\n## Eviction policy\n\nZebrafish eviction happens nightly. See [[sql/active]].\n";
    const SQL: &str = "---\nkb_format: 1\ntype: sql\ntitle: Active\n---\n\n```sql\nselect pgrepxyz from t;\n```\n";

    fn change(id: &str, text: &str) -> Change {
        let file = WalkEntry {
            id: id.into(),
            rel_path: format!("{id}.md"),
            abs_path: PathBuf::new(),
        };
        let built = build_doc(&file, text.as_bytes().to_vec());
        assert!(matches!(built.verdict, Verdict::Indexed(_)), "{id}");
        Change::Upsert {
            file,
            mtime_ns: 1,
            size: 1,
            built,
            reason: None,
        }
    }

    fn has_term(conn: &Connection, term: &str) -> bool {
        conn.query_row("SELECT count(*) FROM vocab WHERE term = ?1", [term], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
            > 0
    }

    fn check(conn: &Connection) {
        for term in [
            "cache", "design", "eviction", "policy", "redis", "note", "pgrepxyz",
        ] {
            assert!(has_term(conn, term), "{term} should be in the vocabulary");
        }
        for term in ["zebrafish", "nightly", "happens"] {
            assert!(!has_term(conn, term), "{term} is note body text");
        }
    }

    #[test]
    fn note_bodies_stay_out_of_the_typo_vocabulary() {
        let changes = vec![change("note/cache", NOTE), change("sql/active", SQL)];
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(schema::CREATE).unwrap();
        let scan = Scan {
            changes,
            ..Scan::default()
        };
        apply(&mut conn, &scan, 10).unwrap();
        check(&conn);
        // the body is still searchable, only the typo vocabulary leaves it out
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM fts WHERE fts MATCH 'zebrafish'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);

        let mut conn = Connection::open_in_memory().unwrap();
        apply_reset(&mut conn, &scan).unwrap();
        check(&conn);
    }

    #[test]
    fn links_are_recorded_and_their_titles_searchable() {
        let changes = vec![change("note/cache", NOTE), change("sql/active", SQL)];
        let scan = Scan {
            changes,
            ..Scan::default()
        };
        for reset in [false, true] {
            let mut conn = Connection::open_in_memory().unwrap();
            if reset {
                apply_reset(&mut conn, &scan).unwrap();
            } else {
                conn.execute_batch(schema::CREATE).unwrap();
                apply(&mut conn, &scan, 1).unwrap();
            }
            let links: Vec<(String, String)> = conn
                .prepare("SELECT source_id, target_id FROM entry_links")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                links,
                [("note/cache".to_string(), "sql/active".to_string())]
            );
            // "Active" is the linked entry's title; it is now in the note's meta column
            let found: i64 = conn
                .query_row(
                    "SELECT count(*) FROM fts WHERE fts MATCH 'meta : active' AND rowid = (SELECT rowid FROM entries WHERE id = 'note/cache')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "reset={reset}");
            delete_entry(&conn, "note/cache").unwrap();
            let left: i64 = conn
                .query_row("SELECT count(*) FROM entry_links", [], |r| r.get(0))
                .unwrap();
            assert_eq!(left, 0);
        }
    }
}
