# Contract: `kb-core` API (M2)

What the `kb` binary uses in M2, and what the TUI (M4), picker (M9) and every action command
(M3 onward) build on. Signatures are indicative; names may change during implementation, the
capabilities and guarantees may not.

## The handle

```rust
pub struct Kb { /* canonical root, index connection, state path */ }

impl Kb {
    pub fn open(root: &Path) -> Result<Kb, KbError>;      // cheap: does not sync or build
    pub fn root(&self) -> &Path;
}
```

`open` never touches the index. The first `search` (or `reindex`) does. A long-lived front end
(the TUI) keeps one `Kb` and calls `search` per keystroke; each call re-syncs, which costs a
stat-walk when nothing changed.

## Search (FR-023, FR-025)

```rust
impl Kb {
    pub fn search(&mut self, query: &Query, now: SystemTime) -> Result<SearchOutcome, KbError>;
}
```

- Brings the index up to date first (FR-002), rebuilding if missing, corrupt or a different
  version (FR-003), or using an in-memory index when `.kb/` is not writable.
- Returns ranked results with `matches` (field + character ranges) and a `snippet`, so a front end
  highlights without matching again. Order is deterministic for identical data and `now`
  (FR-015).
- `now` is injected for frecency, so tests are exact. Front ends pass `SystemTime::now()`.
- Never records usage (FR-017). Never returns an entry whose file no longer exists.
- Never panics on any query string.

```rust
impl Query {
    pub fn parse(raw: &str) -> Query;                       // see contracts/query-syntax.md
    pub fn from_parts(raw: &str, types: &[String], tags: &[String], not_tags: &[String],
                      limit: Option<usize>) -> Result<Query, QueryError>;  // CLI flags
    pub fn is_empty(&self) -> bool;
}
```

`Query::parse(s)` and `Query::from_parts(s, ...)` yield equal values for equal meaning. `QueryError`
covers an unknown `--type` name only.

## Lookup (FR-024)

```rust
impl Kb {
    pub fn entry(&self, id: &str) -> Result<Entry, KbError>;   // parsed M1 Entry
}
```

Reads and parses `<root>/<id>.md` with M1's `parse_entry`. Does not use the index, so it is exact
even if the index is stale. `KbError::NotFound` for an unknown ID; `KbError::BadId` for an ID with
`..`, a leading `/`, or a backslash. Returns the entry even when it has lint errors (callers can
inspect `ParseOutcome` findings through `parse_entry` themselves if they need them).

## Usage (FR-016, FR-017, FR-018)

```rust
impl Kb {
    pub fn record_use(&self, id: &str, now: SystemTime) -> Result<(), KbError>;
    pub fn usage(&self, id: &str) -> Option<UsageRecord>;
}
pub fn frecency(record: Option<&UsageRecord>, now: SystemTime) -> f64;   // pure, in [0, 1)
```

- `record_use` reads `state.json`, increments, rewrites atomically (temp + rename). Concurrent
  callers may lose an increment (last writer wins), which is acceptable for statistics.
- It fails with `KbError::NotFound` if the entry file does not exist, so usage is never recorded
  for nothing.
- M2's only caller is tests; M3 (copy), M7 (run) and the TUI open action call it.

## Index management

```rust
impl Kb {
    pub fn reindex(&mut self) -> Result<ReindexReport, KbError>;   // rebuild + prune usage
    pub fn status(&self) -> Result<IndexStatus, KbError>;          // no sync, no creation
}
```

## Errors

```rust
pub enum KbError {
    NotFound(String), BadId(String),
    Unreadable(String),                         // the file exists but is not parseable as an entry
    Io { path: PathBuf, message: String },      // exit 2
    Index(String),                              // unrecoverable SQLite failure after one rebuild
}
```

A corrupt or incompatible index is never an error: it is rebuilt and reported as
`Notice::Rebuilt`.

## Guarantees (tested)

1. A search after create, edit, rename or delete reflects the files (US2).
2. Deleting `.kb/` and searching again gives the same results except frecency order (SC-006).
3. Two `Kb` handles in two threads or processes searching while files change never corrupt the
   index and never return a deleted ID (SC-005).
4. `Query::parse` never panics; `search` returns the same order for the same inputs.
5. Frecency never lifts an entry above one whose text relevance is more than 25% higher
   (FR-014), and never above a title-exact entry.
6. The core prints nothing. All user-facing text is returned as `Notice` values or errors.
