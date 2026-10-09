# Research: Index and Search

Phase 0 for `002-index-search`. Technical Context had no open NEEDS CLARIFICATION items. A spike
(`rusqlite` 0.37 bundled, SQLite FTS5, release build) verified the behavior marked **[spike]**.

## R1. Storage engine: SQLite FTS5 via `rusqlite`

- **Decision**: `rusqlite` 0.37 with `bundled`, one FTS5 table with
  `tokenize='unicode61 remove_diacritics 2'` and `prefix='2 3'`.
- **Rationale**: mandated by the constitution (II and Technical Constraints). [spike] confirmed
  `configuracion` matches `Configuración`; `auth*` prefix, `"5xx by"` phrase and `title:auth*`
  column filters all work; a prefix query over 10,000 rows took 0.14 ms. `bundled` keeps the binary
  static and FTS5 always present on both OSes.
- **Alternatives considered**: `tantivy` (heavier, own file format, no gain at 10,000 entries),
  `grep`-style scanning in Rust (no ranking, too slow with 1 MB entries), a hand-built inverted
  index (re-implements what FTS5 gives for free).

## R2. Freshness check (FR-002)

- **Decision**: on every search, run M1's `walk()`, `stat` each file, and compare `(mtime_ns, size)`
  with the `files` table. Re-read and re-index only files that differ, are new, or are *racy*
  (mtime within 2 s of the previous sync's start time, the same rule git uses for its index). Rows
  with no file on disk are deleted.
- **Rationale**: no daemon (constitution III), no re-reading unchanged files, and correct when an
  editor saves twice within the filesystem's timestamp granularity.
- **Alternatives considered**: hashing every file each time (reads 10,000 files, blows the 50 ms
  budget); directory mtimes only (miss in-place edits); a watcher (out of scope, constitution III).

## R3. Renames and usage (FR-018, US2 scenario 5)

- **Decision**: the index treats a rename as delete + add. Usage carries over through
  `state.json`: each usage record stores the content hash of its entry. During sync, a usage
  record whose ID has no file is matched against *newly appeared* files with the same hash; on a
  match the record moves to the new ID. Records still unmatched are pruned only at a full rebuild
  (FR-018), so a file temporarily moved out and back keeps its history.
- **Rationale**: keeps the index free of anything that cannot be recomputed (SC-006 and
  constitution II), and works even when the index was deleted before the rename.
- **Hash**: FNV-1a 64-bit, implemented inline (about 6 lines), stable across Rust versions and
  platforms. It is for identity matching, not security; a collision could at worst carry usage to
  the wrong entry, and only between a vanished and a newly appeared file.
- **Hash refresh**: when sync re-reads a changed file that has a usage record, it updates the
  record's hash, so "edit, then rename later" still carries over. "Renamed and edited at once" is
  a new entry, as the spec states.
- **Alternatives considered**: a `sha2`/`blake3` dependency (a dependency for 6 lines, IX);
  `std::hash::DefaultHasher` (not guaranteed stable across releases, and the value is persisted).

## R4. Typo tolerance (FR-010, FR-011)

- **Decision**:
  1. Fold each term (lowercase, strip accents) with `unicode-normalization`.
  2. Prefix: `term*` (FTS5 prefix, with `prefix='2 3'` for speed).
  3. Typos, only for terms of 5+ characters: read the `vocab` table rows whose `len` is within ±1
     of the term's length, keep words within one edit (insert, delete, substitute, adjacent swap —
     optimal string alignment distance ≤ 1), and OR them into that term's match as exact words
     (at most 50, alphabetical). `vocab` is a plain table filled from `fts_vocab` at a full rebuild
     and with each new document's words afterwards; it is never pruned (a stale term costs one row
     and can only add an alternative that matches nothing).
  4. Skip step 3 when exact/prefix matches already fill the limit (`limit > 0`), since fuzzy-only
     entries are tier 2 and would not appear on the page anyway. With `--limit 0` it always runs.
- **Rationale**: FTS5 has no fuzzy matching. [spike] A length-filtered scan of `fts5vocab` took
  53 ms over a synthetic 378,000-term vocabulary. **Measured on the generated 10,000-entry corpus
  (60,012 distinct terms): 23 ms**, because `fts5vocab` walks all postings, not just terms. That
  made a typo query 61 ms against the 50 ms budget, so the persisted `vocab` table (the fallback
  this research first described) was built: the same lookup is now a few hundred microseconds and
  the typo query measures 37 ms. Step 4 still skips the lookup when exact matches fill the page.
- **Alternatives considered**: SymSpell-style deletion index (large, rebuilt on every change);
  trigram index (bigger and needs its own ranking); Levenshtein automata crates (a dependency
  for one operation).
- **Parity risk**: Rust folding must agree with SQLite's `remove_diacritics 2` on the words that
  matter. A unit test folds every word in the fixture KB both ways (via a throwaway FTS5 table) and
  compares.

## R5. Ranking (FR-012 to FR-015)

- **Decision**: results are ordered by this key, each part applied only to break ties in the
  previous one:
  1. **Title exact**: `fold(title) == fold(query)` (whitespace collapsed). Found through an indexed
     `title_fold` column so such entries are candidates even beyond the BM25 cut-off.
  2. **Match tier**: 1 if every term matched as exact or prefix; 2 if any term needed a typo
     variant. Computed by running the typo-free expression first and treating entries that appear
     only in the typo-expanded expression as tier 2.
  3. **Score** = `relevance × (1 + 0.25 × frecency)`, where `relevance = -bm25(weights)` with
     weights `title 10, tags 6, id 4, meta 3, payload 1.5, notes 1` (FR-012 order), plus a flat
     +10% bonus if every term matched exactly rather than as a prefix, and `frecency ∈ [0, 1)`.
  4. **ID ascending** (FR-015).
- **Why FR-014 holds**: frecency can raise a score by at most 25%, so it can never lift an entry
  above one whose relevance is more than 1.25× higher. That bound is the definition of "clearly
  higher" the tests assert. A frequently used partial match can never beat an exact title match
  because that is a separate, earlier sort key.
- **Frecency**: `f = (1 − e^(−count/5)) × 2^(−age_days/30)`. Count saturates, recency halves every
  30 days; both parts are needed so a heavily used but stale entry fades. `now` is passed in, never
  read inside the formula, so tests are deterministic.
- **Candidate cap**: the FTS query returns at most 1,000 best-by-BM25 candidates plus title-exact
  entries; tiering, frecency and tie-breaks run in Rust on those. Entries below rank 1,000 by BM25
  are not reachable by frecency alone; acceptable for a 10,000-entry KB where the visible page is
  20 results.
- **Empty query or filters only**: no text relevance; order by frecency descending, then ID.
- **Alternatives considered**: one blended float (hard to prove FR-014, hard to debug);
  Mozilla-style bucketed frecency (needs per-visit history, spec stores only count and last-used).

## R6. Warm path cost

**Measured (release build, macOS arm64, 10,000 generated entries, mtimes backdated, warm file cache):**
`kb reindex` 0.79 s (budget 5 s); warm `kb search` median 40 ms (budget 50 ms); search after one
edit median 43 ms (budget 100 ms); typo search median 63 ms (budget 100 ms); `kb --version` 6 ms.
The stat-walk is the dominant cost of a warm search, so 50 ms has little headroom: if a slower
disk or CI runner misses it, use the parallel-stat fallback below. Typo queries first measured 61 ms
(the `fts5vocab` scan alone is 23 ms at 60,012 terms, above T006's 15 ms limit), so the persisted
`vocab` table of R4 was built; they now measure 37 ms. After that change: warm search 40 ms, after
one edit 43 to 48 ms.

**Test note**: the racy rule (R2) makes any file modified in the last 2 s of the previous sync get re-read. Tests that assert "no file is re-read" or time the warm path must backdate file mtimes (`File::set_modified`, stable since Rust 1.75) or the constant must be overridable in tests.

- **Decision**: accept one `stat` per file on every search; measure in the perf test.
- **Rationale**: 10,000 `stat` calls on a warm cache are expected around 10 to 25 ms on both
  OSes; process start and SQLite open are a few ms (M1 measured `kb --version` at ~3 ms).
- **Fallback if SC-001 is missed**: stat in parallel with `std::thread::scope` (no dependency),
  and skip subtrees whose directory mtime and entry list are unchanged *only for add/delete
  detection* (edits still need stat). First step of the tasks is a spike that measures this before
  anything else is built on it.

## R7. Concurrency and atomicity (FR-004, SC-005)

- **Decision**:
  - Open with `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=1000`.
  - Sync applies changes in `BEGIN IMMEDIATE` batches of 200 files; each file's rows (entry, FTS,
    tags, vars) change in the same transaction, so a reader sees an entry fully old or fully new.
  - If a writer cannot get the lock within 1 s, the search continues read-only on the current
    snapshot; another process is already syncing.
  - **Full rebuild happens in place, never by renaming a new file over `index.db`.** In WAL mode
    `index.db-wal` and `index.db-shm` belong to the old file; a renamed-in file would be paired with
    a stale WAL and processes holding the old file would share its `-shm` (a documented corruption
    mode, sqlite.org/howtocorrupt). So: (a) file is valid SQLite (version mismatch, `reindex`):
    one `BEGIN IMMEDIATE` transaction drops and recreates the tables, inserts everything, sets
    `user_version`, and commits; WAL readers keep the old snapshot until the commit. (b) file is
    garbage or not SQLite: delete `index.db`, `-wal` and `-shm`, then build as in (a). (c) no
    tables and the write lock is not obtained within the busy timeout: another process is
    building; re-check once, then fall back to the in-memory index.
  - After ranking, `stat` each result on the returned page; drop any whose file is gone. This
    closes the window in which a file is deleted after the sync and before the answer, so a
    deleted entry is never returned (SC-005).
  - FTS5 `MATCH` input is escaped: embedded `"` doubled, terms with no word characters after
    `words()` dropped, so no query string can produce an FTS5 syntax error.
- **Learned while implementing**: (1) `std::fs::copy` keeps the source's old modification time
  on macOS, so a copied fixture is *not* racy; tests that exercise the racy rule must set the
  mtime to "now" first. (2) Two processes building a missing index at once: the loser's
  `BEGIN IMMEDIATE` times out after 1 s, and it falls back to a temporary in-memory index for that
  one search if the winner has not committed yet; both exit 0. (3) Parsing happens before the write
  lock is taken, so the lock is held only for the inserts.
- **Alternatives considered**: a lock file (stale-lock handling, and `std::fs::File::lock` needs
  Rust 1.89 while the MSRV is 1.88); one giant transaction (blocks other writers during a first
  sync).

## R8. Corruption, version mismatch, read-only `.kb/` (FR-003, edge cases)

- **Decision**: the index version is `PRAGMA user_version`. Opening runs `SELECT` on a meta row;
  any SQLite error, a missing file or a version mismatch means rebuild, with one stderr notice
  returned as data (`Notice::Rebuilt`). If an error occurs *mid-query* with a malformed-database
  code, rebuild once and retry once. If `.kb/` cannot be written (permission denied or read-only
  filesystem), build into `:memory:`, return `Notice::ReadOnly`, and never touch disk.
- **Rationale**: no migrations by constitution; the cost of a rebuild is a few seconds at most.
- **`state.json` problems**: unreadable, invalid JSON or newer `state_version` → copy to
  `state.json.bak`, start empty, return `Notice::StateReset` once. A missing file is not an
  error.

## R9. Query language (FR-006 to FR-008, FR-025)

- **Decision**: hand-written tokenizer, about 100 lines, producing `Query`. Grammar in
  [contracts/query-syntax.md](contracts/query-syntax.md). Flags and raw string produce the same
  `Query` value, tested by building both and comparing.
- **Stop words**: dropped from free text only (not from phrases). The list is deliberately short:
  `a an and are as at be by for in is it of on or the to`. A query left with no terms, filters or
  phrases is an empty query. Assumption recorded because the spec does not list stop words.
- **Shell quoting**: argv elements are joined with single spaces and then parsed, so
  `kb search '"role cache"'` is a phrase and `kb search role cache` is two terms. An element is
  never silently turned into a phrase.
- **Unknown key** `foo:bar`: the whole token is free text (FTS5 splits it into `foo` and `bar`,
  AND-ed), and a hint listing valid keys goes to stderr as `Notice::UnknownFilter`.
- **`in:` scope**: `in:title`, `in:payload`, `in:notes` apply to every free-text term and phrase in
  the query, as in the spec's wording; they map to FTS5 column filters.

## R10. Spec defect: `authz` → `authorizer` (resolved)

**Resolved 2026-10-08**: the spec example was changed to `authori` and an Assumption records that abbreviations are not matched. The finding below is kept for the reasoning.

- **Finding**: User Story 4 scenario 1 says "a term is a prefix of a word in an entry (`authz` →
  `authorizer`)". `authz` is not a prefix of `authorizer` (`authz…` vs `autho…`), and the edit
  distance is 5, so neither FR-010 nor FR-011 matches it. [spike] confirmed `authz*` returns 0
  rows.
- **Decision**: implement FR-010 and FR-011 as written. Do not add an abbreviation or subsequence
  rule: it would change the meaning of "prefix", add false positives, and was not asked for in the
  requirements (only in an example).
- **Action for the author**: change the example to `authori` (prefix) and optionally add
  `authroizer` (a transposition) to the same story. The fixture query table already uses those two.
  If an abbreviation match like `authz` is really wanted, it needs its own functional requirement
  and a `/speckit-clarify` pass.

## R11. Output, color and offsets (FR-019, FR-020, FR-023)

- **Decision**:
  - Highlight ranges come from SQLite's own `highlight()` auxiliary function run on the returned
    page only: it uses the same tokenizer and prefix logic as the match, so ranges agree with what
    matched. Core parses the marker bytes into ranges and strips them.
  - Ranges are `[start, end)` in **Unicode scalar values (characters)** of the original field text.
    JSON consumers and the TUI index into characters, not bytes, which avoids splitting accented
    letters.
  - Snippet: a window of at most 160 characters centered on the first matched range in the
    highest-weighted matching field, trimmed at word boundaries with `…` markers. For very long
    payloads only the matching region is read.
  - Color: plain ANSI (bold yellow for matches, dim for tags) in `crates/kb`, enabled only when
    stdout is a terminal and `NO_COLOR` is unset or empty. No color crate (IX). M1's "kb prints no
    color" doc comment in `output.rs` is updated.
  - `--ids` and `--json` never contain escape codes.
- **Alternatives considered**: tokenizing again in Rust to find ranges (duplicates logic and risks
  disagreeing with SQLite); byte offsets (error-prone for non-ASCII).

## R12. What is indexed (FR-005) and what is skipped

- **Decision**: one FTS5 row per valid entry with columns `title, tags, id, meta, payload, notes`.
  `id` holds the ID with `/`, `-`, `_` and `.` turned into spaces (path segments become words).
  `meta` holds variable names and descriptions plus `dialect`, `database`, `log_groups`, `region`,
  the `url` host and path, and `code.repo`, `code.path`, `code.symbol`. Plain tables hold `type`,
  tag rows, variable-name rows and `code_repo` for the filters.
- **Skipped**: a file is not indexed if M1's per-file checks produce any error (parse and field
  findings, secret findings, invalid UTF-8). A file over 1 MB is **not** skipped: M1's KB051
  file-too-large is a warning, and the spec says warning-only and 1 MB entries are indexed and
  fully searchable (the snippet reads only the matching region) or if its ID collides case-insensitively
  with another (KB053). Warnings and info do not skip. The skip list is stored so every search can
  print the count without re-checking.
- **Alternatives considered**: indexing everything and flagging bad entries (leaks secrets into
  `index.db`, violates VI's intent).

## R13. Dependencies, summarized

| Crate | Why | Rejected alternative |
|-------|-----|----------------------|
| `rusqlite` (bundled) | SQLite + FTS5, required by the constitution | `sqlx` (async, heavy), raw FFI (unsafe is forbidden by workspace lints) |
| `unicode-normalization` | accent folding on the query side, must agree with SQLite | hand-written Latin-only table (breaks on other scripts), asking SQLite to fold via a throwaway table per query (slow, awkward) |

`cargo tree` after implementation must show no other new direct dependency.
