//! The `Kb` handle: search, lookup, usage and index management.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::entry::{Entry, EntryType, parse_entry};
use crate::fold::{fold, words};
use crate::index::schema::{COLUMNS, WEIGHTS};
use crate::index::sync::{FileReader, FsReader};
use crate::index::{self, IndexStatus, Synced};
use crate::query::{FILTER_KEYS, Field, Hint, Query};
use crate::usage::{State, UsageRecord, content_hash, frecency};

/// Candidates kept from the full-text query before ranking.
const CANDIDATE_CAP: usize = 1000;
/// Typo variants added per term.
const MAX_VARIANTS: usize = 50;
/// Terms shorter than this never match by typo.
const FUZZY_MIN_CHARS: usize = 5;
const SNIPPET_CHARS: usize = 160;
/// Frecency can raise a score by at most this fraction (FR-014).
const FRECENCY_BOOST: f64 = 0.25;
/// Bonus when every term matched whole words, not only prefixes.
const EXACT_BONUS: f64 = 1.10;

/// Something the front end should tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Rebuilt { entries: usize },
    ReadOnly,
    Skipped { count: usize },
    UnknownFilter { key: String },
    UnknownType { name: String },
    StateReset,
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::Rebuilt { entries } => write!(f, "index rebuilt ({entries} entries)"),
            Notice::ReadOnly => write!(
                f,
                "warning: .kb/ is not writable; using a temporary in-memory index, searches will be slow"
            ),
            Notice::Skipped { count } => write!(
                f,
                "{count} {} skipped due to errors (run kb lint)",
                if *count == 1 { "entry" } else { "entries" }
            ),
            Notice::UnknownFilter { key } => {
                write!(f, "unknown filter \"{key}:\"; valid filters: {FILTER_KEYS}")
            }
            Notice::UnknownType { name } => {
                let valid: Vec<_> = EntryType::ALL.iter().map(|t| t.name()).collect();
                write!(
                    f,
                    "unknown type \"{name}\"; valid types: {}",
                    valid.join(", ")
                )
            }
            Notice::StateReset => write!(
                f,
                "warning: state.json was invalid; usage statistics were reset (copy kept as state.json.bak)"
            ),
        }
    }
}

#[derive(Debug)]
pub enum KbError {
    NotFound(String),
    BadId(String),
    /// The entry file exists but can't be read as an entry.
    Unreadable(String),
    Io {
        path: PathBuf,
        message: String,
    },
    Index(String),
}

impl fmt::Display for KbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KbError::NotFound(id) => write!(f, "no entry with ID \"{id}\""),
            KbError::BadId(id) => write!(f, "\"{id}\" is not a valid entry ID"),
            KbError::Unreadable(id) => write!(f, "\"{id}\" can't be read as an entry"),
            KbError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            KbError::Index(m) => write!(f, "index error: {m}"),
        }
    }
}

impl std::error::Error for KbError {}

/// How well a result matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    TitleExact,
    Exact,
    Typo,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::TitleExact => "title_exact",
            Tier::Exact => "exact",
            Tier::Typo => "typo",
        }
    }
}

/// Matched ranges in one field, in characters of that field's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMatch {
    pub field: Field,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub field: Field,
    pub text: String,
    /// Relative to `text`.
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub id: String,
    /// Times the entry was used (from `.kb/state.json`; 0 when never).
    pub use_count: u64,
    /// The entry's `updated:` value as written, if it has one.
    pub updated: Option<String>,
    /// The file's modification time, unix seconds.
    pub modified: Option<i64>,
    pub entry_type: EntryType,
    pub title: String,
    pub tags: Vec<String>,
    pub path: String,
    pub score: f64,
    pub tier: Tier,
    pub matches: Vec<FieldMatch>,
    pub snippet: Option<Snippet>,
    pub unreviewed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutcome {
    pub results: Vec<SearchResult>,
    /// Matches before the limit (at most 1,000 for text queries).
    pub total: usize,
    /// Candidates per entry type (index = `EntryType::ALL` order) for the text and non-type
    /// filters, before the type filter and paging.
    pub type_counts: [usize; 7],
    /// Entries in the index.
    pub indexed: usize,
    /// Entries left out of the index because of errors.
    pub skipped: usize,
    /// No file in the KB was indexed or skipped.
    pub kb_empty: bool,
    pub notices: Vec<Notice>,
}

/// What a refresh found.
#[derive(Debug, Clone, PartialEq)]
pub struct Refresh {
    pub indexed: usize,
    pub skipped: usize,
    /// Rebuilt, read-only and state-reset notices (not the skipped count).
    pub notices: Vec<Notice>,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexReport {
    pub indexed: usize,
    pub skipped: usize,
    pub duration: Duration,
}

/// A knowledge base on disk.
pub struct Kb {
    root: PathBuf,
    reader: Box<dyn FileReader>,
    /// Runs after the database answered and before results are checked against the files.
    pub(crate) after_query: Option<Box<dyn Fn()>>,
    /// How many times the vocabulary was scanned for typo variants.
    pub(crate) vocab_scans: Cell<usize>,
    /// The synced index kept between `refresh` and `query`.
    held: Option<index::Index>,
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && id
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

fn is_corrupt(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
    )
}

enum Fail {
    Corrupt,
    Other(KbError),
}

impl From<rusqlite::Error> for Fail {
    fn from(e: rusqlite::Error) -> Fail {
        if is_corrupt(&e) {
            Fail::Corrupt
        } else {
            Fail::Other(KbError::Index(e.to_string()))
        }
    }
}

impl Kb {
    /// Opens the KB at `root`. Cheap: touches neither the index nor `.kb/`.
    pub fn open(root: &Path) -> Result<Kb, KbError> {
        let canonical = root
            .canonicalize()
            .ok()
            .filter(|r| r.is_dir())
            .ok_or_else(|| KbError::Io {
                path: root.to_path_buf(),
                message: "the KB root does not exist or is not a directory".into(),
            })?;
        Ok(Kb {
            root: canonical,
            reader: Box::new(FsReader),
            after_query: None,
            vocab_scans: Cell::new(0),
            held: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn with_reader(mut self, r: Box<dyn FileReader>) -> Kb {
        self.reader = r;
        self
    }

    fn walk(&self) -> Result<crate::walk::Walk, KbError> {
        crate::walk::walk(&self.root, std::slice::from_ref(&self.root)).map_err(|e| KbError::Io {
            path: e.path,
            message: e.message,
        })
    }

    fn entry_file(&self, id: &str) -> Result<(PathBuf, String), KbError> {
        if !valid_id(id) {
            return Err(KbError::BadId(id.to_string()));
        }
        let rel = format!("{id}.md");
        let path = self.root.join(&rel);
        if !path.is_file() {
            return Err(KbError::NotFound(id.to_string()));
        }
        Ok((path, rel))
    }

    /// The absolute path of the entry file, checking that it exists.
    pub fn entry_path(&self, id: &str) -> Result<PathBuf, KbError> {
        self.entry_file(id).map(|(path, _)| path)
    }

    /// Looks up one entry by ID (FR-024). Doesn't use the index.
    pub fn entry(&self, id: &str) -> Result<Entry, KbError> {
        let (path, rel) = self.entry_file(id)?;
        let bytes = std::fs::read(&path).map_err(|e| KbError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        parse_entry(id, &rel, bytes)
            .entry
            .ok_or_else(|| KbError::Unreadable(id.to_string()))
    }

    /// Lists all tags with their frequency counts from the index.
    pub fn list_tags(&mut self) -> Result<Vec<(String, usize)>, KbError> {
        self.refresh()?;
        let Some(index) = self.held.as_ref() else {
            return Err(KbError::Index("no index".into()));
        };
        index.list_tags().map_err(|e| KbError::Index(e.to_string()))
    }

    /// Counts one use of an entry (FR-017). Search never calls this.
    pub fn record_use(&self, id: &str, now: SystemTime) -> Result<(), KbError> {
        let (path, _) = self.entry_file(id)?;
        let bytes = std::fs::read(&path).map_err(|e| KbError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let (mut state, _) = State::load(&self.root);
        state.record(id, content_hash(&bytes), now);
        state.save(&self.root).map_err(|e| KbError::Io {
            path: self.root.join(".kb/state.json"),
            message: e.to_string(),
        })
    }

    /// Counts one use of `id` and remembers the variable values it was used with (spec 006),
    /// in one write. Returns the names of values that were not remembered because they look like
    /// credentials.
    pub fn record_resolved_use(
        &self,
        id: &str,
        now: SystemTime,
        values: &[(String, String)],
    ) -> Result<Vec<String>, KbError> {
        let (path, _) = self.entry_file(id)?;
        let bytes = std::fs::read(&path).map_err(|e| KbError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let (mut state, _) = State::load(&self.root);
        state.record(id, content_hash(&bytes), now);
        let skipped = state.vars.remember(id, values);
        state.save(&self.root).map_err(|e| KbError::Io {
            path: self.root.join(".kb/state.json"),
            message: e.to_string(),
        })?;
        Ok(skipped)
    }

    /// The values remembered for `id`.
    pub fn remembered(
        &self,
        id: &str,
    ) -> (BTreeMap<String, String>, BTreeMap<String, Vec<String>>) {
        let (state, _) = State::load(&self.root);
        (
            state.vars.entries.get(id).cloned().unwrap_or_default(),
            state.vars.recent,
        )
    }

    /// What `kb show --print <mode>` prints for `id` (and what picking it in the TUI prints).
    pub fn pick(&self, id: &str, mode: crate::entry_json::PrintMode) -> Result<String, KbError> {
        let (path, _) = self.entry_file(id)?;
        let entry = self.entry(id)?;
        crate::entry_json::pick_output(&entry, &path, mode)
            .map_err(|e| KbError::Unreadable(format!("{id}: {e}")))
    }

    pub fn usage(&self, id: &str) -> Option<UsageRecord> {
        State::load(&self.root).0.usage.get(id).cloned()
    }

    /// The index as it is on disk; does not sync or create it.
    pub fn status(&self) -> IndexStatus {
        index::status(&self.root)
    }

    /// Rebuilds the index from the files and drops usage of entries that no longer exist.
    pub fn reindex(&mut self) -> Result<ReindexReport, KbError> {
        let started = Instant::now();
        let walk = self.walk()?;
        let synced = index::reindex(&self.root, &walk, self.reader.as_ref())?;
        let (mut state, _) = State::load(&self.root);
        let mut changed = apply_facts(&mut state, &synced);
        let existing: BTreeSet<String> = synced.facts.present.clone();
        changed |= state.prune(&existing);
        if changed {
            let _ = state.save(&self.root);
        }
        let report = ReindexReport {
            indexed: synced.index.indexed(),
            skipped: synced.index.skipped(),
            duration: started.elapsed(),
        };
        self.held = Some(synced.index);
        Ok(report)
    }

    /// Walks the KB and brings the index up to date (FR-002); the synced index is kept for
    /// `query`. Re-opens the index file every time, so a file replaced by another process is
    /// noticed.
    pub fn refresh(&mut self) -> Result<Refresh, KbError> {
        self.refresh_with(false)
    }

    fn refresh_with(&mut self, force: bool) -> Result<Refresh, KbError> {
        let started = Instant::now();
        let walk = self.walk()?;
        let (mut state, state_notice) = State::load(&self.root);
        self.held = None;
        let synced = index::open_synced(&self.root, &walk, self.reader.as_ref(), force)?;
        if apply_facts(&mut state, &synced) {
            let _ = state.save(&self.root);
        }
        let mut notices = synced.notices.clone();
        notices.extend(state_notice);
        let refresh = Refresh {
            indexed: synced.index.indexed(),
            skipped: synced.index.skipped(),
            notices,
            duration: started.elapsed(),
        };
        self.held = Some(synced.index);
        Ok(refresh)
    }

    /// Searches (FR-023): `refresh` then `query`.
    pub fn search(&mut self, query: &Query, now: SystemTime) -> Result<SearchOutcome, KbError> {
        let refresh = self.refresh()?;
        let mut outcome = self.query(query, now)?;
        let mut notices = refresh.notices;
        notices.append(&mut outcome.notices);
        if refresh.skipped > 0 {
            notices.push(Notice::Skipped {
                count: refresh.skipped,
            });
        }
        outcome.notices = notices;
        Ok(outcome)
    }

    /// Searches the held index without walking the files (see `refresh`). Refreshes first when
    /// there is no held index yet; rebuilds once if the index turns out to be corrupt.
    pub fn query(&mut self, query: &Query, now: SystemTime) -> Result<SearchOutcome, KbError> {
        let mut notices = Vec::new();
        if self.held.is_none() {
            notices = self.refresh()?.notices;
        }
        let mut attempt = 0;
        loop {
            let (state, _) = State::load(&self.root);
            let Some(index) = self.held.as_ref() else {
                return Err(KbError::Index("no index".into()));
            };
            match self.run(index, query, &state, now) {
                Ok(mut outcome) => {
                    for h in &query.hints {
                        notices.push(match h {
                            Hint::UnknownFilter(k) => Notice::UnknownFilter { key: k.clone() },
                            Hint::UnknownType(n) => Notice::UnknownType { name: n.clone() },
                        });
                    }
                    outcome.notices = notices;
                    return Ok(outcome);
                }
                Err(Fail::Corrupt) if attempt == 0 => {
                    attempt += 1;
                    notices = self.refresh_with(true)?.notices;
                }
                Err(Fail::Corrupt) => return Err(KbError::Index("the index is corrupt".into())),
                Err(Fail::Other(e)) => return Err(e),
            }
        }
    }

    /// Tags in use with how many entries have each, most used first, then by name. Reads the
    /// index when one is held (after a search or refresh), else scans the files.
    pub fn tag_counts(&self) -> Vec<(String, u32)> {
        let from_index = self.held.as_ref().and_then(|index| {
            let mut stmt = index
                .conn
                .prepare(
                    "SELECT tag, count(*) FROM entry_tags GROUP BY tag ORDER BY count(*) DESC, tag",
                )
                .ok()?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))
                .ok()?;
            Some(rows.flatten().collect::<Vec<_>>())
        });
        if let Some(counts) = from_index {
            return counts;
        }
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for file in self.walk().map(|w| w.files).unwrap_or_default() {
            let Ok(bytes) = self.reader.read(&file.abs_path) else {
                continue;
            };
            if let Some(entry) = parse_entry(&file.id, &file.rel_path, bytes).entry {
                for tag in entry.tags() {
                    *counts.entry(tag.clone()).or_default() += 1;
                }
            }
        }
        let mut counts: Vec<_> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
    }

    /// The errors `kb lint` would report for this entry's file (empty when it is valid).
    pub fn lint_entry(&self, id: &str) -> Result<Vec<crate::lint::Finding>, KbError> {
        let (path, rel) = self.entry_file(id)?;
        let bytes = std::fs::read(&path).map_err(|e| KbError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let file = crate::walk::WalkEntry {
            id: id.to_string(),
            rel_path: rel,
            abs_path: path,
        };
        let mut findings = crate::lint::check_file(&file, bytes).findings;
        findings.retain(|f| f.severity == crate::lint::Severity::Error);
        findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        Ok(findings)
    }

    fn run(
        &self,
        index: &index::Index,
        q: &Query,
        state: &State,
        now: SystemTime,
    ) -> Result<SearchOutcome, Fail> {
        let conn = &index.conn;
        let (filter_sql, filter_params) = filters(q);
        let mut cands: Vec<Cand> = Vec::new();
        let mut expr_for_highlight: Option<String> = None;

        if q.is_empty() {
            let sql = format!("SELECT e.id, e.type FROM entries e WHERE {filter_sql}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(filter_params.iter()), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, ty) = row?;
                let Some(ty) = EntryType::from_name(&ty) else {
                    continue;
                };
                let f = frecency(state.usage.get(&id), now);
                cands.push(Cand {
                    id,
                    ty,
                    score: f,
                    tier: Tier::Exact,
                });
            }
        } else {
            let base = terms_expr(q, &HashMap::new(), true);
            let mut rel: HashMap<String, (EntryType, f64)> = HashMap::new();
            if let Some(expr) = &base {
                for (id, ty, s) in fts_query(conn, expr, &filter_sql, &filter_params)? {
                    rel.insert(id, (ty, s));
                }
                expr_for_highlight = Some(expr.clone());
            }
            // Whole-word matches get a bonus.
            let mut exact: HashSet<String> = HashSet::new();
            if let Some(expr) = terms_expr(q, &HashMap::new(), false) {
                for (id, _, _) in fts_query(conn, &expr, &filter_sql, &filter_params)? {
                    exact.insert(id);
                }
            }
            let mut typo: HashMap<String, (EntryType, f64)> = HashMap::new();
            let visible = rel
                .values()
                .filter(|(ty, _)| q.types.is_empty() || q.types.contains(ty))
                .count();
            if q.limit == 0 || visible < q.limit {
                let variants = self.variants(conn, q)?;
                if !variants.is_empty()
                    && let Some(expr) = terms_expr(q, &variants, true)
                {
                    for (id, ty, s) in fts_query(conn, &expr, &filter_sql, &filter_params)? {
                        if !rel.contains_key(&id) {
                            typo.insert(id, (ty, s));
                        }
                    }
                    expr_for_highlight = Some(expr);
                }
            }
            let title_key = fold(&q.raw_text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let mut title_hits: HashMap<String, EntryType> = HashMap::new();
            if !title_key.is_empty() {
                let sql = format!(
                    "SELECT e.id, e.type FROM entries e WHERE e.title_fold = ? AND {filter_sql}"
                );
                let mut p = vec![Value::Text(title_key)];
                p.extend(filter_params.iter().cloned());
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params_from_iter(p.iter()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (id, ty) = row?;
                    if let Some(ty) = EntryType::from_name(&ty) {
                        title_hits.insert(id, ty);
                    }
                }
            }
            let mut push =
                |id: String, ty: EntryType, relevance: f64, tier: Tier, all_exact: bool| {
                    let f = frecency(state.usage.get(&id), now);
                    let mut score = relevance * (1.0 + FRECENCY_BOOST * f);
                    if all_exact {
                        score *= EXACT_BONUS;
                    }
                    let tier = if title_hits.contains_key(&id) {
                        Tier::TitleExact
                    } else {
                        tier
                    };
                    cands.push(Cand {
                        id,
                        ty,
                        score,
                        tier,
                    });
                };
            for (id, (ty, r)) in &rel {
                push(id.clone(), *ty, *r, Tier::Exact, exact.contains(id));
            }
            for (id, (ty, r)) in &typo {
                push(id.clone(), *ty, *r, Tier::Typo, false);
            }
            for (id, ty) in &title_hits {
                if !rel.contains_key(id) && !typo.contains_key(id) {
                    push(id.clone(), *ty, 0.0, Tier::Exact, true);
                }
            }
        }
        let mut type_counts = [0usize; 7];
        for c in &cands {
            if let Some(i) = EntryType::ALL.iter().position(|t| *t == c.ty) {
                type_counts[i] += 1;
            }
        }
        if !q.types.is_empty() {
            cands.retain(|c| q.types.contains(&c.ty));
        }
        if let Some(want_unreviewed) = q.unreviewed {
            cands.retain(|c| state.is_unreviewed(&c.id) == want_unreviewed);
        }
        cands.sort_by(|a, b| {
            a.tier
                .cmp(&b.tier)
                .then(b.score.total_cmp(&a.score))
                .then(a.id.cmp(&b.id))
        });
        let total = cands.len();
        if let Some(hook) = &self.after_query {
            hook();
        }
        // Never return an entry whose file is gone (SC-005).
        let want = if q.limit == 0 { usize::MAX } else { q.limit };
        let mut results = Vec::new();
        for cand in cands {
            if results.len() >= want {
                break;
            }
            let Some(mut r) = load_result(conn, &cand, state)? else {
                continue;
            };
            if !self.root.join(&r.path).is_file() {
                continue;
            }
            if let Some(expr) = &expr_for_highlight {
                highlight(conn, expr, &mut r)?;
            }
            results.push(r);
        }
        let (indexed, skipped) = (index.indexed(), index.skipped());
        Ok(SearchOutcome {
            results,
            total,
            type_counts,
            indexed,
            skipped,
            kb_empty: indexed + skipped == 0,
            notices: Vec::new(),
        })
    }

    /// One-edit variants for each long single-word term (FR-011).
    fn variants(&self, conn: &Connection, q: &Query) -> Result<HashMap<String, Vec<String>>, Fail> {
        let mut out = HashMap::new();
        for term in &q.terms {
            let ws = words(term);
            if ws.len() != 1 {
                continue;
            }
            let w = &ws[0].text;
            let len = w.chars().count();
            if len < FUZZY_MIN_CHARS {
                continue;
            }
            self.vocab_scans.set(self.vocab_scans.get() + 1);
            let mut stmt =
                conn.prepare_cached("SELECT term FROM vocab WHERE len BETWEEN ?1 AND ?2")?;
            let rows =
                stmt.query_map([len as i64 - 1, len as i64 + 1], |r| r.get::<_, String>(0))?;
            let mut found: Vec<String> = Vec::new();
            for row in rows {
                let t = row?;
                if t != *w && within_one_edit(w, &t) {
                    found.push(t);
                }
            }
            found.sort();
            found.truncate(MAX_VARIANTS);
            if !found.is_empty() {
                out.insert(term.clone(), found);
            }
        }
        Ok(out)
    }
}

struct Cand {
    id: String,
    ty: EntryType,
    score: f64,
    tier: Tier,
}

fn apply_facts(state: &mut State, synced: &Synced) -> bool {
    let missing: BTreeSet<String> = state
        .usage
        .keys()
        .filter(|id| !synced.facts.present.contains(*id))
        .cloned()
        .collect();
    let mut changed = false;
    if !missing.is_empty() && !synced.facts.added.is_empty() {
        changed |= state.carry_renames(&missing, &synced.facts.added);
    }
    for (id, h) in synced.facts.rehashed.iter().chain(&synced.facts.added) {
        if synced.facts.present.contains(id) {
            changed |= state.refresh_hash(id, *h);
        }
    }
    changed
}

/// Optimal-string-alignment distance ≤ 1: one insert, delete, substitute or adjacent swap.
pub(crate) fn within_one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if prefix == a.len() && prefix == b.len() {
        return true;
    }
    let (ra, rb) = (&a[prefix..], &b[prefix..]);
    match ra.len().cmp(&rb.len()) {
        std::cmp::Ordering::Equal => {
            ra[1..] == rb[1..]
                || (ra.len() >= 2 && ra[0] == rb[1] && ra[1] == rb[0] && ra[2..] == rb[2..])
        }
        std::cmp::Ordering::Greater => ra[1..] == *rb,
        std::cmp::Ordering::Less => *ra == rb[1..],
    }
}

/// SQL conditions on `entries e` for the filters, and their parameters.
fn filters(q: &Query) -> (String, Vec<Value>) {
    let mut conds: Vec<String> = vec!["1".into()];
    let mut params: Vec<Value> = Vec::new();
    if q.types.is_empty() && !q.bad_types.is_empty() {
        conds.push("0".into());
    }
    for t in &q.tags {
        conds.push("EXISTS (SELECT 1 FROM entry_tags t WHERE t.id = e.id AND t.tag = ?)".into());
        params.push(Value::Text(t.clone()));
    }
    for t in &q.not_tags {
        conds
            .push("NOT EXISTS (SELECT 1 FROM entry_tags t WHERE t.id = e.id AND t.tag = ?)".into());
        params.push(Value::Text(t.clone()));
    }
    for v in &q.vars {
        conds.push("EXISTS (SELECT 1 FROM entry_vars v WHERE v.id = e.id AND v.name = ?)".into());
        params.push(Value::Text(v.clone()));
    }
    for r in &q.repos {
        conds.push("e.code_repo = ?".into());
        params.push(Value::Text(r.clone()));
    }
    (conds.join(" AND "), params)
}

/// The FTS5 expression for the free text of `q`: every term (prefix or whole word, with typo
/// variants where given) and every phrase must match. `None` when there is nothing to match.
fn terms_expr(q: &Query, variants: &HashMap<String, Vec<String>>, prefix: bool) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for term in &q.terms {
        let ws: Vec<String> = words(term).into_iter().map(|w| w.text).collect();
        if ws.is_empty() {
            continue;
        }
        let star = if prefix { "*" } else { "" };
        let base = format!("\"{}\"{star}", ws.join(" "));
        match variants.get(term) {
            Some(vs) if !vs.is_empty() => {
                let alts: Vec<String> = vs.iter().map(|v| format!("\"{v}\"")).collect();
                parts.push(format!("({base} OR {})", alts.join(" OR ")));
            }
            _ => parts.push(base),
        }
    }
    for p in &q.phrases {
        let ws: Vec<String> = words(p).into_iter().map(|w| w.text).collect();
        if !ws.is_empty() {
            parts.push(format!("\"{}\"", ws.join(" ")));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join(" AND ");
    Some(match q.scope {
        Some(f) => format!("{} : ({joined})", f.name()),
        None => joined,
    })
}

/// Runs an FTS5 match with the filters; returns `(id, relevance)` best first.
fn fts_query(
    conn: &Connection,
    expr: &str,
    filter_sql: &str,
    filter_params: &[Value],
) -> Result<Vec<(String, EntryType, f64)>, rusqlite::Error> {
    let w = WEIGHTS;
    let sql = format!(
        "SELECT e.id, e.type, -bm25(fts, {}, {}, {}, {}, {}, {}) AS rel \
         FROM fts JOIN entries e ON e.rowid = fts.rowid \
         WHERE fts MATCH ? AND {filter_sql} ORDER BY rel DESC, e.id LIMIT {CANDIDATE_CAP}",
        w[0], w[1], w[2], w[3], w[4], w[5]
    );
    let mut p = vec![Value::Text(expr.to_string())];
    p.extend(filter_params.iter().cloned());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(p.iter()), |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, ty, rel) = row?;
        if let Some(ty) = EntryType::from_name(&ty) {
            out.push((id, ty, rel));
        }
    }
    Ok(out)
}

fn load_result(
    conn: &Connection,
    cand: &Cand,
    state: &State,
) -> Result<Option<SearchResult>, Fail> {
    let row = conn.query_row(
        "SELECT e.type, e.title, e.tags, f.path, e.updated, f.mtime_ns FROM entries e \
         JOIN files f ON f.id = e.id WHERE e.id = ?1",
        [&cand.id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        },
    );
    let (ty, title, tags, path, updated, mtime_ns) = match row {
        Ok(r) => r,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let Some(entry_type) = EntryType::from_name(&ty) else {
        return Ok(None);
    };
    Ok(Some(SearchResult {
        id: cand.id.clone(),
        use_count: state.usage.get(&cand.id).map_or(0, |u| u.count),
        updated,
        modified: Some(mtime_ns / 1_000_000_000),
        entry_type,
        title,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        path,
        score: cand.score,
        tier: cand.tier,
        matches: Vec::new(),
        snippet: None,
        unreviewed: state.is_unreviewed(&cand.id),
    }))
}

/// Fills `matches` and `snippet` from SQLite's own highlighting, so the ranges agree with
/// what the full-text query matched.
fn highlight(conn: &Connection, expr: &str, r: &mut SearchResult) -> Result<(), Fail> {
    let sql = "SELECT highlight(fts,0,char(1),char(2)), highlight(fts,1,char(1),char(2)), \
               highlight(fts,2,char(1),char(2)), highlight(fts,3,char(1),char(2)), \
               highlight(fts,4,char(1),char(2)), highlight(fts,5,char(1),char(2)) \
               FROM fts WHERE fts MATCH ?1 AND rowid = (SELECT rowid FROM entries WHERE id = ?2)";
    let cols: Result<[String; 6], _> = conn.query_row(sql, [expr, &r.id], |row| {
        Ok([
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        ])
    });
    let cols = match cols {
        Ok(c) => c,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    debug_assert_eq!(COLUMNS.len(), 6);
    for (field, text) in Field::ALL.into_iter().zip(cols) {
        let (plain, ranges) = split_marks(&text);
        if ranges.is_empty() {
            continue;
        }
        if r.snippet.is_none() {
            r.snippet = Some(snippet(field, &plain, &ranges));
        }
        r.matches.push(FieldMatch { field, ranges });
    }
    Ok(())
}

/// Removes the `\u{1}…\u{2}` markers; returns the text and the marked ranges in characters.
fn split_marks(s: &str) -> (String, Vec<Range<usize>>) {
    let mut plain = String::with_capacity(s.len());
    let mut ranges = Vec::new();
    let mut pos = 0;
    let mut start = None;
    for c in s.chars() {
        match c {
            '\u{1}' => start = Some(pos),
            '\u{2}' => {
                if let Some(st) = start.take()
                    && st < pos
                {
                    ranges.push(st..pos);
                }
            }
            _ => {
                plain.push(c);
                pos += 1;
            }
        }
    }
    (plain, ranges)
}

fn snippet(field: Field, text: &str, ranges: &[Range<usize>]) -> Snippet {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= SNIPPET_CHARS {
        return Snippet {
            field,
            text: text.to_string(),
            ranges: ranges.to_vec(),
        };
    }
    let first = &ranges[0];
    let window = SNIPPET_CHARS - 2; // room for two ellipses
    let mut start = first
        .start
        .saturating_sub((window.saturating_sub(first.len())) / 3);
    let mut end = (start + window).min(chars.len());
    start = end.saturating_sub(window).min(start);
    // trim to word boundaries when that costs little
    if start > 0
        && let Some(off) = chars[start..end.min(start + 20)]
            .iter()
            .position(|c| c.is_whitespace())
        && start + off < first.start
    {
        start += off + 1;
    }
    if end < chars.len()
        && let Some(off) = chars[end.saturating_sub(20).max(start)..end]
            .iter()
            .rposition(|c| c.is_whitespace())
        && end.saturating_sub(20).max(start) + off > first.end
    {
        end = end.saturating_sub(20).max(start) + off;
    }
    let lead = usize::from(start > 0);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    let rel: Vec<Range<usize>> = ranges
        .iter()
        .filter(|r| r.start >= start && r.end <= end)
        .map(|r| r.start - start + lead..r.end - start + lead)
        .collect();
    Snippet {
        field,
        text: out,
        ranges: rel,
    }
}

#[cfg(test)]
mod tests;
