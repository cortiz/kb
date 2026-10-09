# Feature Specification: Index and Search

**Feature Branch**: `002-index-search`
**Created**: 2026-10-08
**Status**: Draft
**Milestone**: M2
**Depends on**: M1 (`001-entry-format`)
**Input**: Build a disposable search index over the KB that stays fresh on its own, and give the user `kb search` with filters, typo tolerance and ranking that learns from use.

## Overview

M1 made entries readable and valid. M2 makes them findable. The index is derived data: it lives in `.kb/`, is never committed, and can be deleted at any time without losing anything except usage statistics. Search is exposed through `kb search` and through a core search API that the TUI (M4) and the picker (M9) will use, so result ranking and match highlighting are defined here once.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Find an entry from the command line (Priority: P1)

Mid-incident, Carlos types `kb search 5xx auth` and the Sumo query he wants is the first result.

**Why this priority**: finding things is the reason the tool exists. This story alone makes the KB useful from the shell.

**Independent Test**: load the fixture KB, run a table of queries against it, and check the top result and the order of the first five for each.

**Acceptance Scenarios**:

1. **Given** a KB containing `sumo/5xx-by-authorizer` (title "5xx by authorizer", tags `auth`, `lambda`), **When** the user runs `kb search 5xx auth`, **Then** that entry is the first result.
2. **Given** results exist, **When** output goes to a terminal, **Then** each result is one line showing type, ID, title and tags, with matched terms highlighted, and results are limited to 20 unless `--limit` is passed.
3. **Given** output is piped, **When** the user runs `kb search auth --ids | fzf`, **Then** only IDs are printed, one per line, with no color codes.
4. **Given** `--json`, **Then** the output is one JSON document listing each result's ID, type, title, tags, path, score, which fields matched, and a short snippet of the matching text.
5. **Given** no entry matches, **Then** kb prints nothing to stdout, prints "no matches" to stderr, and exits with code 1 (the same convention as `grep`).

---

### User Story 2 — The index is never stale and never needs attention (Priority: P1)

Carlos edits an entry in his editor, saves it, and immediately searches. The change is there. He never has to run a command to keep the index fresh.

**Why this priority**: a search index that can be out of date is worse than `grep`. Trust depends on freshness.

**Independent Test**: create, edit, rename and delete fixture files between searches, and check each search reflects the current files.

**Acceptance Scenarios**:

1. **Given** an entry's file was modified since the last search, **When** any search runs, **Then** the result reflects the new content.
2. **Given** an entry file was deleted, **When** any search runs, **Then** it no longer appears.
3. **Given** `.kb/index.db` was deleted, **When** the user runs a search, **Then** kb rebuilds the index, prints a one-line notice to stderr, and returns correct results.
4. **Given** the index file is corrupt or was created by a different index version, **When** a search runs, **Then** kb discards it and rebuilds without asking.
5. **Given** a file was renamed and its content is unchanged, **When** the next search runs, **Then** the entry appears under its new ID and keeps its usage history.

---

### User Story 3 — Narrow results with filters (Priority: P2)

Carlos searches `latency type:cw tag:prod` to get only CloudWatch queries tagged `prod`.

**Why this priority**: with hundreds of entries, free text alone returns too much. Filters by type and tag are the main way to cut results down.

**Independent Test**: run filter queries over the fixture KB and check that result sets contain exactly the expected entries.

**Acceptance Scenarios**:

1. **Given** the query `latency type:cw`, **Then** only `cw` entries are returned.
2. **Given** `tag:prod tag:auth`, **Then** only entries with both tags are returned.
3. **Given** `-tag:stage`, **Then** entries tagged `stage` are excluded.
4. **Given** `"role cache"` in quotes, **Then** only entries containing that exact phrase are returned.
5. **Given** the flags `--type cw --tag prod`, **Then** the result is the same as the equivalent query syntax.
6. **Given** an empty query (`kb search` with no terms), **Then** all entries are listed, most frequently and recently used first.

---

### User Story 4 — Forgiving search (Priority: P2)

Carlos types `authroizer` or `authori` and still finds the authorizer entries.

**Why this priority**: queries are typed fast and under pressure. Exact-match search fails exactly when it matters most.

**Independent Test**: a table of misspelled and partial queries over the fixture KB, each with its expected top result.

**Acceptance Scenarios**:

1. **Given** a term is a prefix of a word in an entry (`authori` → `authorizer`), **Then** the entry matches.
2. **Given** a term of five or more characters with one typo (one character inserted, removed, replaced or two adjacent characters swapped), **Then** entries with the correctly spelled word match, ranked below exact matches.
3. **Given** a query with accents missing or in different case (`configuracion` vs `Configuración`), **Then** it matches.

---

### User Story 5 — Inspect and rebuild the index (Priority: P3)

Carlos runs `kb index status` to see how many entries are indexed and when, and `kb reindex` to force a clean rebuild.

**Why this priority**: rarely needed, but essential when debugging.

**Independent Test**: build an index over the fixture KB, run `kb index status` and check each reported value; run `kb reindex` after recording usage and check usage is intact.

**Acceptance Scenarios**:

1. **Given** `kb index status`, **Then** kb shows the number of indexed entries, skipped entries with lint errors, index size on disk, index version, and the time of the last full build.
2. **Given** `kb reindex`, **Then** the index is rebuilt from scratch and usage statistics are kept.

---

### Edge Cases

- Entries with lint **errors** are left out of the index. Every search that skips entries prints one stderr line such as `3 entries skipped due to errors (run kb lint)`. Entries with only warnings are indexed.
- Two kb processes at once (for example the TUI open while a CLI search runs): neither may corrupt the index, and each sees either the old or the new state of an entry, never a partial one.
- `.kb/` is not writable (read-only mount): search still works by building a temporary in-memory index, with a warning that it will be slow.
- A KB with zero entries: search exits 1 with "no entries in KB (run kb init)".
- A query made only of filters with no free text: valid; results are sorted by frecency.
- A query containing only stop words or punctuation: treated like an empty query.
- Unknown filter key (`foo:bar`): treated as free text, and a hint lists the valid filter keys.
- Very long entries (up to the 1 MB lint limit): fully searchable; snippets come from the matching region only.
- `.kb/state.json` missing or invalid: kb starts with empty usage statistics, keeps a copy of the bad file as `state.json.bak`, and warns once.

## Requirements *(mandatory)*

### Functional Requirements

#### Index

- **FR-001**: The index MUST be stored at `.kb/index.db` under the KB root, and MUST be rebuildable from the entry files alone.
- **FR-002**: Before answering any query, kb MUST bring the index up to date with the files on disk: new, changed, deleted and renamed entries. Detection MUST be cheap when nothing changed (no re-reading of unchanged files).
- **FR-003**: Each index MUST record its index version. A missing, corrupt or version-mismatched index MUST be rebuilt automatically with a one-line notice on stderr.
- **FR-004**: Index updates MUST be atomic per entry, and concurrent kb processes MUST NOT corrupt the index or block each other for more than 1 second.
- **FR-005**: The index MUST include, for each valid entry: ID, path segments, type, title, tags, payload, notes, variable names and descriptions, and type-specific fields (`dialect`, `database`, `log_groups`, `region`, `url` host and path, `code.repo`, `code.path`, `code.symbol`).

#### Query language

- **FR-006**: Free-text terms MUST all match for an entry to be returned (AND semantics). Each term may match in any indexed field.
- **FR-007**: Filters MUST be supported, both as query syntax and as flags:
  - `type:<type>` / `--type` (repeatable; OR between types),
  - `tag:<tag>` / `--tag` (repeatable; AND between tags),
  - `-tag:<tag>` / `--not-tag` (exclusion),
  - `var:<name>` (entries that use that variable),
  - `repo:<owner/name>` (code entries),
  - `in:title`, `in:payload`, `in:notes` (restrict free text to one field).
- **FR-008**: Double-quoted text MUST match as an exact phrase.
- **FR-009**: Matching MUST be case-insensitive and accent-insensitive.
- **FR-010**: Terms MUST match as prefixes of words.
- **FR-011**: Terms of five or more characters MUST also match words within one edit (insert, delete, substitute or adjacent swap). Typo matches MUST rank below exact and prefix matches.

#### Ranking

- **FR-012**: Text relevance MUST weight fields in this order, highest first: title, tags, ID, variable names and type-specific fields, payload, notes.
- **FR-013**: An entry whose title equals the query exactly (ignoring case and accents) MUST rank first.
- **FR-014**: Usage MUST raise ranking through a frecency score that combines how often and how recently an entry was used. Frecency MUST NOT lift an entry above another whose text relevance is clearly higher (for example, an exact title match always outranks a frequently used partial match).
- **FR-015**: Ties MUST be broken by ID in ascending order, so identical queries over identical data always produce identical output.

#### Usage state

- **FR-016**: Usage statistics (per-entry count and last-used time) MUST be stored in `.kb/state.json`, which is git-ignored and versioned.
- **FR-017**: The core MUST expose an operation to record that an entry was used. In M2 only the API exists; later milestones (copy, run, open) call it. `kb search` itself MUST NOT record usage.
- **FR-018**: When an entry is renamed with unchanged content, its usage MUST carry over to the new ID. Usage for entries that no longer exist MUST be pruned at the next full rebuild.

#### Output

- **FR-019**: Human output (default) MUST show one line per result with type, ID, title and tags, use color only when stdout is a terminal, highlight matched terms, and respect `NO_COLOR`.
- **FR-020**: `--ids` MUST print only IDs, one per line. `--json` MUST print one JSON document with, per result: ID, type, title, tags, path, score, matched fields with character ranges, and a snippet of at most 160 characters.
- **FR-021**: `--limit N` MUST cap results (default 20; `0` means no limit).
- **FR-022**: Exit codes: `0` when at least one result is returned, `1` when there are none, `2` for usage or I/O errors.

#### Core API

- **FR-023**: The core MUST expose search returning ranked results with the matched fields and character ranges needed to highlight matches, so the TUI can render highlighting without re-running the match itself.
- **FR-024**: The core MUST expose lookup of a single entry by ID, returning the parsed entry from M1.
- **FR-025**: The search API MUST accept a structured query (terms, phrases, filters) as well as a raw query string, and parsing a raw string MUST produce the same structure as the equivalent flags.

#### Commands

- **FR-026**: `kb search [query...]` with `--type`, `--tag`, `--not-tag`, `--limit`, `--ids`, `--json`.
- **FR-027**: `kb reindex` rebuilds the index from scratch and keeps usage statistics.
- **FR-028**: `kb index status` reports index health as described in User Story 5, with `--json` support.

### Key Entities

- **Index**: derived, versioned store of searchable entry data, keyed by entry ID, with each entry's file fingerprint (modification time, size, content hash).
- **Query**: free-text terms, exact phrases, include and exclude filters, field restrictions, and a limit.
- **SearchResult**: entry ID, score, matched fields with character ranges, and a snippet.
- **UsageRecord**: entry ID, use count, last-used time, content hash (for carrying usage across renames).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a KB of 10,000 entries with a warm index and no changed files, `kb search` returns in 50 ms or less, measured end to end including process start.
- **SC-002**: When one file changed, the next search reflects it and still returns in 100 ms or less.
- **SC-003**: A full rebuild of 10,000 entries finishes in 5 seconds or less.
- **SC-004**: For the fixture query table (at least 40 realistic queries, including misspellings and partial words), the expected entry is the first result in at least 90% of cases and in the top three in 100%.
- **SC-005**: Running 50 concurrent searches while files are being edited never corrupts the index and never returns a deleted entry.
- **SC-006**: Deleting `.kb/` entirely and searching again returns the same results as before, except for frecency ordering.

## Out of Scope

- Copying, showing or acting on results (M3, M7).
- The TUI search screen (M4); M2 only provides the API it will use.
- Recording usage from any command (M3 onward).
- A file-system watcher or background process. Freshness is checked on demand, which fits the no-daemon principle.
- Semantic or embedding-based search.
- Saved searches.

## Assumptions

- The KB will stay under roughly 10,000 entries; the budgets above are set for that size.
- On-demand freshness checks are fast enough for this size, so a background watcher is never needed.
- "Stop words" are a short fixed list of English function words (`a an and are as at be by for in is it of on or the to`), dropped from free-text terms but kept inside quoted phrases.
- Abbreviations that are neither a prefix nor within one edit of a word (for example `authz` for `authorizer`) are not matched.
- Whenever kb creates `.kb/` it also creates `.kb/.gitignore` containing `*`, so derived and per-machine files are never committed.
- Renames are recognized only when the content is unchanged. A file that is both renamed and edited is treated as a new entry.
