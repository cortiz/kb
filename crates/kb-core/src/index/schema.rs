//! The index schema. Everything here is derived from the entry files; a different
//! [`INDEX_VERSION`] means "rebuild", never "migrate".

/// Bump when the schema or the way documents are built changes.
pub const INDEX_VERSION: u32 = 1;

/// BM25 weights for the FTS columns, in column order: title, tags, id, meta, payload, notes
/// (spec FR-012: title, tags, ID, variable names and type-specific fields, payload, notes).
pub const WEIGHTS: [f64; 6] = [10.0, 6.0, 4.0, 3.0, 1.5, 1.0];

/// FTS column names, in order.
pub const COLUMNS: [&str; 6] = ["title", "tags", "id", "meta", "payload", "notes"];

pub const DROP: &str = "
DROP TABLE IF EXISTS vocab;
DROP TABLE IF EXISTS fts_vocab;
DROP TABLE IF EXISTS fts;
DROP TABLE IF EXISTS entry_vars;
DROP TABLE IF EXISTS entry_tags;
DROP TABLE IF EXISTS entries;
DROP TABLE IF EXISTS files;
DROP TABLE IF EXISTS meta;
";

pub const CREATE: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE files(
    id TEXT PRIMARY KEY, path TEXT NOT NULL, mtime_ns INTEGER NOT NULL, size INTEGER NOT NULL,
    hash INTEGER NOT NULL, status TEXT NOT NULL, reason TEXT);
CREATE TABLE entries(
    id TEXT PRIMARY KEY, type TEXT NOT NULL, title TEXT NOT NULL, title_fold TEXT NOT NULL,
    tags TEXT NOT NULL, code_repo TEXT);
CREATE INDEX entries_title_fold ON entries(title_fold);
CREATE INDEX entries_type ON entries(type);
CREATE INDEX entries_code_repo ON entries(code_repo);
CREATE TABLE entry_tags(id TEXT NOT NULL, tag TEXT NOT NULL, PRIMARY KEY(tag, id)) WITHOUT ROWID;
CREATE INDEX entry_tags_id ON entry_tags(id);
CREATE TABLE entry_vars(id TEXT NOT NULL, name TEXT NOT NULL, PRIMARY KEY(name, id)) WITHOUT ROWID;
CREATE INDEX entry_vars_id ON entry_vars(id);
CREATE VIRTUAL TABLE fts USING fts5(
    title, tags, id, meta, payload, notes,
    tokenize='unicode61 remove_diacritics 2', prefix='2 3');
CREATE VIRTUAL TABLE fts_vocab USING fts5vocab(fts, 'row');
-- Every word ever indexed, for typo expansion. Scanning fts_vocab directly costs ~25 ms at 60,000
-- terms (it walks all postings); this plain table is indexed by length. Terms are only added
-- incrementally and rebuilt from fts_vocab at a full rebuild, so stale terms are harmless.
CREATE TABLE vocab(term TEXT PRIMARY KEY, len INTEGER NOT NULL) WITHOUT ROWID;
CREATE INDEX vocab_len ON vocab(len);
";
