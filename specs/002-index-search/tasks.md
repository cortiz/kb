---
description: "Task list for M2: Index and Search"
---

# Tasks: Index and Search

**Input**: Design documents from `specs/002-index-search/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII makes red-green-refactor mandatory for `kb-core`: every
test task must be written and seen failing (`cargo test` red) before its implementation task
starts. Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they test.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested on
its own. US1 can ship alone: a first search builds the index from scratch; US2 adds incremental
freshness and recovery.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: The user story the task belongs to (US1 to US5)

## Path Conventions

Cargo workspace from [plan.md](plan.md): core in `crates/kb-core/src/`, binary in
`crates/kb/src/`, shared fixtures and core integration tests in `tests/` at the repository root,
CLI tests in `crates/kb/tests/`. FR/SC numbers refer to [spec.md](spec.md).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependencies, the search fixture KB, a corpus generator, and the two measurements the
plan flagged as risks.

- [X] T001 Add to `[workspace.dependencies]` in `Cargo.toml`: `rusqlite = { version = "0.37", features = ["bundled"] }` and `unicode-normalization = "0.1"`; add both to `[dependencies]` of `crates/kb-core/Cargo.toml`. No other new dependency (research R13). Check MSRV now: read `rust-version` of each new crate and its dependencies under `~/.cargo/registry/src/`, or run `cargo +1.88 check -p kb-core`; pick an older minor if anything needs more than 1.88.
- [X] T002 [P] Declare the new modules in `crates/kb-core/src/lib.rs` (`fold`, `query`, `usage`, `index`, `search`) as empty files (`index` as `index/mod.rs`, `index/schema.rs`, `index/sync.rs`, `index/doc.rs`) so the crate builds.
- [X] T003 [P] Create the search fixture KB in `tests/fixtures/search/` (valid entries only, each lint-clean with `kb lint --strict`): at least 30 entries covering all 7 types, including `sumo/5xx-by-authorizer` (title "5xx by authorizer", tags `auth`, `lambda`), a second authorizer entry with a different type, `cw/latency-prod` and `cw/latency-stage` (tags `prod` / `stage`), an entry whose notes contain the phrase "role cache" and another containing "cache role" (words reversed), an entry titled "Configuración de red" and one titled "Network configuration", entries using variables `region` and `env`, a `code` entry with `repo: acme/api`, a `link` entry with a URL host, an entry whose title equals another entry's frequently matched word (for the title-exact test), and one entry whose only match for the word `gateway` is in its notes. The over-1-MB entry is NOT committed: tests generate a valid 1,048,577-byte entry (unique word `zebrafish` near the end) in a temp dir. Add `tests/fixtures/search/README.md` listing the purpose of each entry.
- [X] T004 [P] Create `tests/fixtures/search/queries.tsv`: at least 40 rows `query<TAB>expected_first_id<TAB>expected_top3_ids_comma_separated<TAB>story` (the optional fourth column is `us1`, `us3` or `us4`, default `us1`), covering: plain terms, multi-term AND, prefixes (`authori`), one-edit typos of 5+ characters (`authroizer`, one per edit kind: insert, delete, substitute, swap), accent-less queries (`configuracion`), exact-title queries, phrase queries, and filter queries (`latency type:cw tag:prod`). Lines starting with `#` are comments.
- [X] T005 [P] Extract and extend the generator already in `crates/kb/tests/perf.rs` into `crates/kb/tests/support/mod.rs` (shared test helpers) (seeded xorshift, no new dependency; perf.rs then uses the shared module): `generate_kb(root, n)` writes `n` valid entries across all types with realistic word distribution (a vocabulary of about 40,000 distinct words drawn from a Zipf-like distribution, 1 to 30 lines of payload, 0 to 5 tags). Used by T006, T035 and T055. Include a unit test that 200 generated entries lint clean.
- [X] T006 Measurement spike (done through the release-mode perf tests; criterion (b) was missed at 23 ms for the `fts5vocab` scan, so the persisted `vocab` table was built; numbers in research R6) (record results in the "Risk R6" paragraph of `specs/002-index-search/research.md`, then delete the spike code): using T005's generator at 10,000 entries in release mode, time (a) `kb_core::walk::walk` plus one `std::fs::metadata` per file with a warm cache, and (b) a scan of `fts5vocab` filtered by `length(term) BETWEEN n-1 AND n+1` on the generated corpus. Pass criteria: (a) ≤ 25 ms, (b) ≤ 15 ms. If (a) fails, change the plan to parallel stat before continuing; if (b) fails, add the persisted `vocab` table from research R4 to T049's scope.

**Checkpoint**: `cargo build --workspace` passes; fixture KB lints clean; risk numbers recorded.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Folding, content hash, usage state, the index schema, document construction and a
from-scratch build, and the `Kb` handle. Every story depends on these.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Tests first (must fail before the matching implementation)

- [X] T007 [P] Write unit tests in `crates/kb-core/src/fold.rs`: `fold("Configuración") == "configuracion"`, `fold("ÀÉÎõü") == "aeiou"`, `fold("ß")` unchanged or `"ss"` (pick one and assert it), `fold` is idempotent; `words("5xx-by-authorizer")` yields `["5xx", "by", "authorizer"]` with char offsets `[(0,3),(4,6),(7,17)]`; `words` splits on any non-alphanumeric character; offsets count characters, not bytes (check `"año nuevo"`).
- [X] T008 [P] Write unit tests in `crates/kb-core/src/usage.rs` for the content hash and the state file: `content_hash(b"")` equals the FNV-1a 64 offset basis `0xcbf29ce484222325` and `content_hash(b"a") == 0xaf63dc4c8601ec8c`; `State::load` of a missing file is empty with no notice; invalid JSON, wrong shape and `state_version` 2 each produce an empty state, a `state.json.bak` copy and exactly one `Notice::StateReset`; unknown top-level keys survive `load` then `save`; `save` is atomic (the file is never partially written: write to a temp name in `.kb/`, then rename); `frecency(None, now) == 0.0`, `frecency` stays in `[0, 1)` for count 1 to 1,000,000, is monotonic in count, halves with each 30 days of age (assert `f(age=30d) ≈ f(age=0) / 2` within 1e-9), and takes `now` as a parameter.
- [X] T009 [P] Write unit tests in `crates/kb-core/src/index/doc.rs` for document construction (FR-005): for each entry type, the FTS columns hold exactly what [data-model.md](data-model.md#entry-row-construction-fr-005) lists (title, tags joined by spaces, ID with `/ - _ .` turned into spaces, `meta` with variable names and descriptions plus `dialect`, `database`, `log_groups`, `region`, the `url` host and path, `code.repo`, `code.path`, `code.symbol`, then `payload` and `notes`); a verdict of `Skipped` for each of: a parse error finding, a secret finding, invalid UTF-8; `Indexed` for an entry with only warnings and info findings, including one just over 1,048,576 bytes (KB051 is a warning: it is indexed and its snippet comes from the matching region).
- [X] T010 [P] Write tests in `crates/kb-core/src/index/mod.rs` for the schema and full build: building over the `tests/fixtures/search/` KB creates `.kb/index.db` with `PRAGMA user_version == INDEX_VERSION`; one `files` row per walked file; `entries`/`fts` rows only for `ok` files; a `meta` row `last_full_build`; a rebuild over an existing valid index runs in one transaction (a second connection that opened a read transaction before the rebuild still sees the old rows until it ends, and the new rows afterwards); a garbage `index.db` with stale `-wal`/`-shm` files is removed and rebuilt; invariants from data-model.md hold (every `ok` file has exactly one `entries` row and one `fts` row).
- [X] T011 [P] Write unit tests in `crates/kb-core/src/search.rs` for `Kb::open` and `Kb::entry`: `open` on a missing directory is `KbError::Io`; `open` does not create or touch `.kb/`; `entry("sql/example")` returns the M1 `Entry` for an existing file; `KbError::NotFound` for an unknown ID; `KbError::BadId` for `../x`, `/etc/passwd`, `a\\b` and the empty string; `entry` works with no index present (FR-024).

### Implementation

- [X] T012 [P] Implement `crates/kb-core/src/fold.rs`: `fold(&str) -> String` (NFD via `unicode-normalization`, drop combining marks, lowercase) and `words(&str) -> Vec<Word { text: String, start: usize, end: usize }>` (folded words with character offsets, splitting at any non-alphanumeric character).
- [X] T013 [P] Implement in `crates/kb-core/src/usage.rs`: `content_hash(&[u8]) -> u64` (FNV-1a 64, about six lines), `UsageRecord { count: u64, last_used: i64, hash: u64 }` (JSON: `count`, `last_used`, `hash` as 16 hex digits), `State` with `load(kb_dir) -> (State, Option<Notice>)`, `save(&self, kb_dir)` per [contracts/state-file.md](contracts/state-file.md) (preserve unknown keys with `#[serde(flatten)] extra: serde_json::Map`), and `frecency(Option<&UsageRecord>, SystemTime) -> f64 = (1 − e^(−count/5)) × 2^(−age_days/30)`.
- [X] T014 [P] Define `Notice` and `KbError` in `crates/kb-core/src/search.rs` exactly as in [data-model.md](data-model.md#notice) and [contracts/core-api.md](contracts/core-api.md#errors); implement `Display` for `Notice` producing the CLI texts listed there so the binary only prints.
- [X] T015 Implement `crates/kb-core/src/index/schema.rs`: `INDEX_VERSION = 1`, the DDL from data-model.md (`meta`, `files`, `entries` with indexes on `title_fold`, `type`, `code_repo`, `entry_tags`, `entry_vars`, `fts` with `tokenize='unicode61 remove_diacritics 2'` and `prefix='2 3'`, `fts_vocab`), and the BM25 column weights `title 10, tags 6, id 4, meta 3, payload 1.5, notes 1` as constants.
- [X] T016 Implement `crates/kb-core/src/index/doc.rs`: `build_doc(&WalkEntry, bytes) -> Verdict` where `Verdict::Indexed(Doc)` or `Verdict::Skipped`, reusing M1 (`parse_source`, `scan_secrets`, the size and UTF-8 checks as `lint_file` does; extract the shared per-file check from `crates/kb-core/src/lint/mod.rs` into a `pub(crate) fn file_findings(...)` rather than duplicating it) and producing the six FTS columns, `title_fold`, tags, variable names and `code_repo`. Also compute `content_hash(bytes)`. Depends on T009, T013.
- [X] T017 Implement `crates/kb-core/src/index/mod.rs`: `Index::open(kb_dir) -> Result<Index, OpenFailure>` (WAL, `synchronous=NORMAL`, `busy_timeout=1000`, version and sanity check by reading `meta`), `Index::build_full(root, kb_dir) -> Result<BuildReport>` (walk, `build_doc` per file, insert in batches of 200 inside transactions, all inside one `BEGIN IMMEDIATE` transaction that drops and recreates the tables, sets `user_version` and `last_full_build`, and commits; a non-SQLite file is deleted first together with `-wal` and `-shm`; never rename a file over `index.db`, see research R7), `Index::in_memory(root)` (same build into `:memory:`), and `IndexStatus` reads. Also treat KB053 case collisions from `crate::walk::case_collisions` as `Skipped` for both files. Depends on T010, T015, T016, T062.
- [X] T062 Implement `ensure_kb_dir(root) -> io::Result<PathBuf>` in `crates/kb-core/src/index/mod.rs` per [research R14](research.md#r14-kb-creation-and-git-ignore-constitution-ii): create `.kb/` and, if absent, `.kb/.gitignore` containing `*\n` using exclusive create; add a unit test that it never overwrites an existing `.gitignore`, is idempotent, and creates nothing when the root is read-only. Call it from `build_full`, `State::save` and `reindex`. Must be done before T017 builds on it (T017 depends on T062).
- [X] T018 Implement the `Kb` handle in `crates/kb-core/src/search.rs`: `Kb::open(root)` (canonicalize, no side effects), `Kb::root()`, `Kb::entry(id)` (validate ID, read `<root>/<id>.md`, `parse_entry`), and a private `ensure_index(&mut self) -> Vec<Notice>` that opens the index or builds it when missing (full sync logic comes in US2). Depends on T011, T014, T017.

- [X] T063 Implement `Kb::record_use(id, now)` and `Kb::usage(id)` in `crates/kb-core/src/search.rs` (FR-017, [contracts/core-api.md](contracts/core-api.md#usage-fr-016-fr-017-fr-018)): validate the ID, read the entry file (`KbError::NotFound` when missing), `State::load`, increment `count`, set `last_used`, set `hash = content_hash(bytes)`, `State::save` (which calls `ensure_kb_dir`). Write the tests first in the same file: first call creates a record with count 1; five calls give count 5; unknown ID is `NotFound` and writes nothing; `usage` returns `None` for an unused entry; the state file keeps unknown keys. Depends on T013, T018, T062.

**Checkpoint**: `cargo test -p kb-core` is green; a unit test can build an index over the fixture KB and read it back with `rusqlite`.

---

## Phase 3: User Story 1 - Find an entry from the command line (Priority: P1) 🎯 MVP

**Goal**: `kb search 5xx auth` returns the right entry first, with ranking, highlighting, `--ids`,
`--json`, `--limit`, and the exit-code convention.

**Independent Test**: run the `queries.tsv` rows that use only terms and phrases against the
fixture KB and check first result and top three; run the CLI tests for output modes and exit
codes.

### Tests for User Story 1 ⚠️ (write first, see them fail)

- [X] T019 [P] [US1] Write unit tests in `crates/kb-core/src/query.rs` for `Query::parse` over terms and phrases: `5xx auth` → terms `["5xx","auth"]`; `"role cache"` → one phrase `role cache`; an unterminated quote closes at end of input; stop words (`a an and are as at be by for in is it of on or the to`) are dropped from terms but kept inside phrases; a query of only stop words, only punctuation (`"???"`, `-`, `:`) or only whitespace is `is_empty()`; a leading `-` on a plain word is ignored (`-stage` → term `stage`); `limit` defaults to 20; parse never panics on 1,000 seeded random strings (including unbalanced quotes and non-ASCII).
- [X] T020 [P] [US1] Write unit tests in `crates/kb-core/src/search.rs` for ranking against the fixture KB with an injected `now`: `5xx auth` ranks `sumo/5xx-by-authorizer` first; all terms must match (AND, FR-006); a term may match in different fields; field weight order (a title hit outranks a notes hit for the same word, FR-012); title-exact entry ranks first even when another entry has a higher BM25 and higher usage (FR-013, FR-014); frecency lifts an entry among equal-relevance entries; frecency never lifts an entry over one with more than 1.25× the relevance (construct both cases from fixture entries and a large usage count); ties break by ID ascending and the same query twice gives the identical order (FR-015); the empty query orders by frecency then ID; `limit` caps and `limit 0` means no limit (FR-021); results never include an entry whose file was deleted after the sync (delete a file between sync and result assembly via a test hook that runs after the SQL query).
- [X] T021 [P] [US1] Write unit tests in `crates/kb-core/src/search.rs` for highlighting and snippets (FR-023): for `5xx auth`, `matches` lists `title` ranges `[0,3)` and `[7,17)` for "5xx by authorizer" (character offsets); an accented title ("Configuración de red") for query `configuracion` yields a range covering the whole accented word in characters; a prefix match highlights only the typed-prefix-matching word; the snippet is at most 160 characters, centered on the first matched range of the highest-weighted matching field, with `…` where cut, and its ranges are relative to `snippet.text`; a 1 MB payload entry produces a snippet from the matching region without reading beyond it more than necessary (assert it completes under 50 ms); filter-only and empty queries give `snippet: None`.
- [X] T022 [P] [US1] Write the table test (implemented as `query_table` in `crates/kb-core/src/search/tests.rs`, not a separate file): load `queries.tsv`, run each row through `Query::parse` and `Kb::search` on a copy of the fixture KB in a temp dir, and assert SC-004: expected entry first in at least 90% of rows and in the top three in 100%. Print each failing row. Rows whose `story` column is `us3` or `us4` are skipped until a `const ENABLED_STORIES: &[&str]` in the test file includes them (T045 and T050 add them).
- [X] T023 [P] [US1] Write CLI tests in `crates/kb/tests/cli_search.rs` with `assert_cmd` and `KB_ROOT` set to a copy of the fixture KB: `kb search 5xx auth` exits 0 and the first stdout line contains `sumo/5xx-by-authorizer`; piped stdout contains no `\x1b`; `--ids` prints only IDs one per line; `--json` stdout parses and matches the shape of `contracts/search-report.schema.json` (check required keys, types and `snippet.text` length ≤ 160 by hand-written assertions, no schema crate); `--ids --json` together exit 2; no match → stdout empty, stderr contains `no matches`, exit 1; empty KB → stderr `no entries in KB (run kb init)`, exit 1; a KB with one entry containing a secret and one valid entry → stderr `1 entries skipped due to errors (run kb lint)`; default limit 20 (generate 25 matching entries) and `--limit 3` and `--limit 0`; `--limit abc` exits 2; running `kb search` ten times leaves `.kb/state.json` absent or byte-identical (FR-017); a search on a KB with no `.kb/` creates `.kb/.gitignore` containing `*` and every file the search wrote is inside `.kb/` (constitution I, II); `--root` pointing at a nonexistent directory exits 2 with `kb:` on stderr (FR-022); `print_search` with color on emits bold-yellow escapes around the matched range, and with `NO_COLOR=1` set emits none (FR-019); with a pseudo-terminal-free run, `NO_COLOR=1` and non-TTY both give no escape codes.
- [X] T024 [P] [US1] Write a test in `crates/kb/src/color.rs` for the color gate: `enabled(is_tty, no_color_env)` is true only when `is_tty` and `NO_COLOR` is unset or empty; `highlight("5xx by", &[(0,3)], on)` wraps characters 0..3 in bold yellow when `on` and returns the text unchanged otherwise; ranges are in characters (check an accented string).

### Implementation for User Story 1

- [X] T025 [US1] Implement terms and phrases in `crates/kb-core/src/query.rs`: the `Query` struct from [data-model.md](data-model.md#query) (all fields, even those used only by later stories), `Query::parse(&str)` as a hand-written tokenizer per [contracts/query-syntax.md](contracts/query-syntax.md) (stop words, quotes, punctuation-only, leading `-`), `Query::is_empty()`, and `Query::from_parts(raw, types, tags, not_tags, limit)` handling `limit` only for now (filters arrive in US3). Depends on T019.
- [X] T026 [US1] Implement search in `crates/kb-core/src/search.rs` per [research R5](research.md#r5-ranking-fr-012-to-fr-015): build the FTS5 `MATCH` expression (each term as `"term"*` after folding, phrases as `"w1 w2"`), run it with `bm25(fts, 10, 6, 4, 3, 1.5, 1)` and a 1,000-row cap, add title-exact candidates via `title_fold`, apply the sort key (title-exact, tier, `relevance × (1 + 0.25 × frecency)` plus the +10% all-exact bonus, ID), load `State` for frecency, truncate to `limit`, then `stat` each returned file and drop vanished ones (refilling from the next candidates). Returns `SearchOutcome { results, total, notices }`. Escape the MATCH input: double any `"`, drop terms with no word characters after `words()`; a fuzz test pushes 1,000 seeded random strings (plus `AND`, `NEAR`, `5xx ???`, `foo"bar`) through `Kb::search` and asserts `Ok`. Empty queries skip FTS and order all `entries` by frecency then ID. Depends on T020, T018, T025.
- [X] T027 [US1] Add highlight ranges and snippets in `crates/kb-core/src/search.rs`: for the returned page only, call SQLite `highlight(fts, col, '\u{1}', '\u{2}')` per matched column, parse the markers into character ranges and strip them (`FieldMatch`), and build the `Snippet` per [research R11](research.md#r11-output-color-and-offsets-fr-019-fr-020-fr-023). Depends on T021, T026.
- [X] T028 [P] [US1] Implement `crates/kb/src/color.rs` per T024: `enabled()` using `std::io::IsTerminal` and `NO_COLOR`, and `highlight(text, ranges, on)`. Depends on T024.
- [X] T029 [US1] Add rendering to `crates/kb/src/output.rs`: `print_search(results, color)` (aligned columns type / ID / title / `#tags`, highlighting matched ranges in title, ID and tags per [contracts/cli.md](contracts/cli.md#human-output-stdout-is-a-terminal-or-a-pipe-without---ids---json)), `print_ids`, `print_search_json` (shape of `contracts/search-report.schema.json`, `tier` as `title_exact | exact | typo`), and `print_notice(&Notice)` writing the `Display` text to stderr; update the module comment that says kb prints no color. Depends on T023, T027, T028.
- [X] T030 [US1] Add the `search` subcommand to `crates/kb/src/main.rs`: `SearchArgs { query: Vec<String>, --limit, --ids, --json }` (`--ids` and `--json` with `conflicts_with`), join `query` with spaces, build the `Query`, call `Kb::open(&root)?.search(...)`, print notices, results, `no matches` / `no entries in KB (run kb init)` and return exit 0 / 1 / 2 per FR-022. `--type`, `--tag`, `--not-tag` are added in US3. Make the `--json` error path in `main` cover `search`. Depends on T026, T029.
- [X] T031 [US1] Make T022's `search_table` and T023's CLI tests pass; fix ranking weights only through the constants in `index/schema.rs`, never per query. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

**Checkpoint**: US1 is shippable: `kb search` builds the index on first use and answers correctly. The index is not yet refreshed after edits (US2).

---

## Phase 4: User Story 2 - The index is never stale (Priority: P1)

**Goal**: any create, edit, rename or delete is visible to the next search with no command; a
missing, corrupt or old index rebuilds silently; usage follows renames; concurrent use is safe.

**Independent Test**: the freshness and concurrency tests below.

### Tests for User Story 2 ⚠️

- [X] T032 [P] [US2] Write tests in `crates/kb-core/src/index/sync.rs` for the sync decision table in [data-model.md](data-model.md#fingerprint-and-sync-decision): new file → inserted; size change → re-read; mtime change with identical bytes → only mtime updated and the row's FTS content untouched; unchanged and not racy → the file is not opened (count reads through a `FileReader` trait object injected into `sync`, asserting zero reads on a second sync of the unchanged fixture KB); racy file (mtime within 2 s of `last_sync_start_ns`) → re-read; every other case backdates the fixture files' mtimes by 1 hour with `File::set_modified` first, otherwise all freshly written files are racy; deleted file → every row for the ID removed; case-colliding IDs → both skipped; a file that becomes invalid (secret added) → rows removed and counted as skipped; a skipped file that becomes valid → indexed.
- [X] T033 [P] [US2] Write tests in `crates/kb/tests/freshness.rs` (CLI level, `assert_cmd`): US2 scenarios 1 to 5 in order on a fixture copy: edit then search sees the new text; delete then search no longer lists it; delete `.kb/index.db` then search prints `index rebuilt` on stderr and returns correct results; overwrite `index.db` with garbage then search rebuilds silently (stderr has one `index rebuilt` line, no prompt); set `PRAGMA user_version` to 99 via `rusqlite` then search rebuilds; rename a file with unchanged content and check the entry appears under the new ID. Add: SC-006 (delete `.kb/` entirely, results equal except frecency order, compare ID sets and order after neutralizing usage); a read-only `.kb/` (`chmod 0o555`, unix only) still searches, warns about the in-memory index, and creates no file in `.kb/`; mid-query corruption (truncate `index.db` after open via a test hook) rebuilds once and retries.
- [X] T034 [P] [US2] Write tests in `crates/kb-core/src/usage.rs` (extend T008's module) for rename carry-over (FR-018, US2 scenario 5): with a usage record for `a/old` and the hash of its bytes, moving the file to `a/new` and running sync moves the record to `a/new` (count and `last_used` intact); a rename plus an edit does not carry over; editing then renaming later does (the hash was refreshed by sync); a record whose file is gone and has no matching new file stays until a full rebuild, and `reindex` prunes it; works when the index was deleted before the rename (the match uses `state.json`, not the index); two vanished IDs with the same content and one new file: the lexicographically first vanished ID wins (deterministic).
- [X] T035 [P] [US2] Write the concurrency test `crates/kb/tests/concurrency.rs` (SC-005, FR-004): on a 500-entry generated KB, spawn 50 `kb search` processes in batches of 10 while a writer thread edits, creates, renames and deletes files in a loop for the duration; assert every process exits 0 or 1 (never 2), no stderr line mentions `malformed` or `locked`, no deleted ID is ever returned after its deletion time (record deletion timestamps, compare with each result's process start time), and afterwards `PRAGMA integrity_check` returns `ok` and the invariants of data-model.md hold. Also assert a search started while another process holds `BEGIN IMMEDIATE` for 3 s still returns within 1.5 s using the current snapshot.

### Implementation for User Story 2

- [X] T036 [US2] Implement `crates/kb-core/src/index/sync.rs`: `sync(&mut Index, &Walk, &dyn FileReader, usage: &mut State) -> SyncReport`, applying the decision table (stat each walked file, compare `(mtime_ns, size)`, racy rule using `meta.last_sync_start_ns`, read and hash only when needed, `build_doc`, upsert or delete in `BEGIN IMMEDIATE` batches of 200, update `last_sync_start_ns`), recording `skipped` counts, refreshing the hash of changed files that have a usage record. If `BEGIN IMMEDIATE` times out (SQLITE_BUSY after 1 s), return `SyncReport::Busy` and let the search proceed on the current snapshot. Depends on T032, T017.
- [X] T037 [US2] Implement rename carry-over in `crates/kb-core/src/usage.rs` and call it from `sync`: after the walk, for each usage ID with no file, match its `hash` against files newly inserted in this sync (sorted by ID); move matching records; do not prune (pruning happens only in `reindex`, US5). Depends on T034, T036.
- [X] T038 [US2] Replace T018's `ensure_index` with the full open sequence in `crates/kb-core/src/search.rs`: open `.kb/index.db`; on missing file, SQLite error, or `user_version` mismatch run `build_full` and emit `Notice::Rebuilt { entries }`; on a write-permission failure create `Index::in_memory` and emit `Notice::ReadOnly`; otherwise run `sync`; after the query, if SQLite reports `SQLITE_CORRUPT` or `SQLITE_NOTADB`, rebuild once and retry once; emit `Notice::Skipped { count }` when the skip count is above zero and `Notice::StateReset` from `State::load`. Hold the `State` for the search and save it only when sync changed it. Depends on T033, T036, T037.
- [X] T039 [US2] Make T033 and T035 pass: fix whatever they expose (for example WAL checkpoint behavior, two processes racing on a missing index). Add a `CONCURRENCY` note to `specs/002-index-search/research.md` R7 for anything learned. Run `cargo test --workspace` on Linux and macOS in CI.

**Checkpoint**: US1 and US2 together: search is always current and self-healing.

---

## Phase 5: User Story 3 - Narrow results with filters (Priority: P2)

**Goal**: `type:`, `tag:`, `-tag:`, `var:`, `repo:`, `in:` filters, the matching flags, and the
empty/filter-only query.

**Independent Test**: filter rows of `queries.tsv` and the tests below.

### Tests for User Story 3 ⚠️

- [X] T040 [P] [US3] Extend the tests in `crates/kb-core/src/query.rs`: `latency type:cw tag:prod` parses to terms `["latency"]`, types `[Cw]`, tags `["prod"]`; keys are case-insensitive (`TYPE:cw`); `type:cw type:sql` is OR; `tag:prod tag:auth` is AND; `tag:#Prod` normalizes to `prod`; `-tag:stage` goes to `not_tags`; `var:region`, `repo:acme/api`, `in:title`; a second `in:` replaces the first; an empty value (`tag:`) is free text; `foo:bar` is free text `foo:bar` and records an `UnknownFilter` hint; `type:nonsense` records an unknown-type hint and matches nothing; `Query::from_parts("latency", &["cw"], &["prod"], &[], None) == Query::parse("latency type:cw tag:prod")` (FR-025), and the same for `--not-tag` ↔ `-tag:`; flags and inline filters combine; an invalid `--type` name returns `QueryError`.
- [X] T041 [P] [US3] Extend the ranking tests in `crates/kb-core/src/search.rs`: `latency type:cw` returns only `cw` entries; `tag:prod tag:auth` returns only entries with both tags; `-tag:stage` excludes them; `"role cache"` returns the entry with that phrase and not the one with the words reversed; `var:region` returns exactly the entries using `region` (declared or referenced); `repo:acme/api` returns the code entry only; `in:title gateway` does not match the entry that has `gateway` only in its notes while plain `gateway` does; filter-only queries return the filtered set ordered by frecency then ID (record usage first); an empty query lists all indexed entries ordered by frecency then ID (US3 scenario 6); filters combined with terms narrow the candidate set before the 1,000 cap.
- [X] T042 [P] [US3] Extend `crates/kb/tests/cli_search.rs`: `--type cw --tag prod` output equals the output of `'type:cw tag:prod'` (US3 scenario 5); repeated `--type` is OR; `--not-tag stage`; an unknown inline key prints the hint `valid filters: type, tag, -tag, var, repo, in` on stderr once; `kb search` with no arguments lists all entries and exits 0; `kb search '"role cache"'` quoting works at the shell level; `--type bogus` exits 2 with the valid types listed.

### Implementation for User Story 3

- [X] T043 [US3] Implement filter parsing in `crates/kb-core/src/query.rs` per [contracts/query-syntax.md](contracts/query-syntax.md): key recognition, tag normalization (lowercase, strip leading `#`), `in:` scope, unknown-key hints (surfaced through `Query::hints()` returning the data `search` turns into `Notice::UnknownFilter`), and the full `from_parts`. Depends on T040.
- [X] T044 [US3] Implement filter SQL in `crates/kb-core/src/search.rs`: `type` as `entries.type IN (...)`, each `tag` as `EXISTS (SELECT 1 FROM entry_tags ...)`, `not_tags` as `NOT EXISTS`, `var` via `entry_vars`, `repo` via `entries.code_repo` (compared case-insensitively), and `in:` via FTS5 column filters (`title:`, `payload:`, `notes:`); apply the filters in the same statement as `MATCH` so the 1,000 cap applies after filtering; the filter-only path selects from `entries` directly. Depends on T041, T043.
- [X] T045 [US3] Add `--type`, `--tag`, `--not-tag` (all repeatable) to `SearchArgs` in `crates/kb/src/main.rs`, build the query with `Query::from_parts`, print `UnknownFilter` hints through `print_notice`, and map `QueryError` to `CliError::new("usage", ...)`. Add `us3` to `ENABLED_STORIES` in T022. Depends on T042, T044.

**Checkpoint**: US1 to US3 work together; `queries.tsv` filter rows pass.

---

## Phase 6: User Story 4 - Forgiving search (Priority: P2)

**Goal**: typo tolerance for terms of 5+ characters, ranked below exact and prefix matches. Prefix
and accent/case insensitivity already work from US1; this phase proves them and adds typos.

**Independent Test**: the typo and prefix rows of `queries.tsv` plus the tests below.

### Tests for User Story 4 ⚠️

- [X] T046 [P] [US4] Write unit tests in `crates/kb-core/src/search.rs` (module `fuzzy`): `edits_within_one("authorizer", "authroizer")` (swap), insert, delete and substitute each true; two edits false; the first-character substitution case (`xuthorizer`) true; terms shorter than 5 characters never expand (`auth` does not match `aath`); candidates come only from words of length within ±1; the expansion of one term returns at most 50 variants (document the cap and assert it, picking the 50 most frequent via the vocab `doc` column); the fold parity test: for every word in the fixture KB, `fold(word)` equals the token SQLite's `unicode61 remove_diacritics 2` produces for it (compare against a one-row throwaway FTS5 table and `fts5vocab`) including the accented fixture words.
- [X] T047 [P] [US4] Write ranking tests in `crates/kb-core/src/search.rs`: `authori` finds the authorizer entries (prefix); `authroizer` finds them with `tier == Typo`; an exact-match entry for `authorizer` always ranks above a typo-only entry regardless of BM25 or usage (FR-011); a prefix match ranks with the exact tier; `configuracion`, `CONFIGURACION` and `configuración` all find "Configuración de red" and the reverse (query with accents, text without) works; typos inside phrases do not match; with `limit = 1` and an exact match present, the vocabulary scan does not run (count scans through a test counter on the `Kb`), while with `limit = 0` it does (research R4 step 4); the top-3 hits for `authroizer` include the authorizer entries.
- [X] T048 [P] [US4] Extend `crates/kb/tests/cli_search.rs`: `kb search authroizer --ids` lists the authorizer entries and exits 0; `--json` shows `"tier": "typo"` for them and `"tier": "exact"` for an exact query; `kb search authorizr` (deleted letter) works.

### Implementation for User Story 4

- [X] T049 [US4] Implement typo expansion in `crates/kb-core/src/search.rs`: for each term of 5+ characters (after folding), query the persisted `vocab` table (`SELECT term FROM vocab WHERE len BETWEEN ?-1 AND ?+1`; built because T006 criterion (b) failed), keep words with optimal-string-alignment distance ≤ 1 (insert, delete, substitute, adjacent swap), cap at 50 alphabetically, and OR them as exact tokens into that term's group. Run the exact/prefix expression first; run the expanded expression only if `limit == 0` or fewer than `limit` results came back; entries found only by the expanded expression get `Tier::Typo`. Depends on T046, T047.
- [X] T050 [US4] Add `us4` to `ENABLED_STORIES` in T022, run the fold parity test, and confirm SC-004 (≥ 90% first, 100% top three) over the full table. Tune only by adding query rows or fixing bugs, not by loosening the thresholds.

**Checkpoint**: US1 to US4 work; the full `queries.tsv` passes.

---

## Phase 7: User Story 5 - Inspect and rebuild the index (Priority: P3)

**Goal**: `kb index status` and `kb reindex`.

**Independent Test**: the CLI tests below.

### Tests for User Story 5 ⚠️

- [X] T051 [P] [US5] Write unit tests in `crates/kb-core/src/index/mod.rs` for `status` and `reindex`: status after a search reports the indexed and skipped counts, size (db plus `-wal`), `index_version`, and `last_full_build`; status does not create a missing index (`state: "missing"`, counts 0) and does not sync (edit a file, status still reports the old count); a garbage file reports `state: "unusable"` with a `reason`; `reindex` after recording usage keeps usage for existing entries and prunes usage for entries whose files are gone (FR-018); `reindex` changes `last_full_build`; `reindex` while another handle holds a read transaction leaves that handle with a consistent view.
- [X] T052 [P] [US5] Write CLI tests in `crates/kb/tests/cli_index.rs`: `kb index status` human output has the lines `Index:`, `Version:`, `Entries:`, `Size:`, `Last build:` per [contracts/cli.md](contracts/cli.md#kb-index-status---json); `--json` has exactly the keys of `contracts/index-status.schema.json` with the right types; no-index case prints `Index: none (…)` and exits 0; `kb reindex` prints `Reindexed N entries (M skipped due to errors) in X s`, exits 0, and a following `kb search` prints no `index rebuilt` notice; `kb reindex --json` includes `duration_ms`; `reindex` with an unwritable `.kb/` (unix, `0o555`) exits 2 with `kb:` on stderr.

### Implementation for User Story 5

- [X] T053 [US5] Implement `Kb::status()` and `Kb::reindex()` in `crates/kb-core/src/search.rs` and `crates/kb-core/src/index/mod.rs`: `status` opens read-only and never writes; `reindex` runs `build_full`, then prunes `State` usage for IDs with no file (the only place pruning happens) and saves. `ReindexReport { indexed, skipped, duration }`. Depends on T051, T038.
- [X] T054 [US5] Add `Command::Reindex(JsonArgs)` and `Command::Index(IndexCommand::Status(JsonArgs))` to `crates/kb/src/main.rs`, and the output functions in `crates/kb/src/output.rs` (`print_index_status`, `print_index_status_json`, `print_reindex`), sizes shown as `1.8 MiB`, times as `YYYY-MM-DD HH:MM:SS UTC` computed from unix seconds without a date crate (civil-from-days arithmetic, with a unit test for 2026-10-08 12:30:05 UTC = 1791462605). Add both subcommands to the `--json` error path in `main`. Depends on T052, T053.

**Checkpoint**: all five stories work.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: performance proof, documentation, CI, and the final constitution check.

- [X] T055 [P] Extend `crates/kb/tests/perf.rs` (ignored by default, release mode) using T005's generator at 10,000 entries: SC-003 full rebuild ≤ 5 s (`kb reindex`); SC-001 warm `kb search` with no changes ≤ 50 ms end to end, median of 20 runs including process start (backdate all generated files' mtimes by 1 hour and run one warm-up search first, so the racy rule does not force re-reads); `kb --version` ≤ 100 ms (constitution VII cold start); SC-002 after touching one file, median ≤ 100 ms; also assert a typo query (`limit 20` with few exact hits) stays ≤ 100 ms. Print the measured numbers.
- [X] T056 [P] Add a `search` section to `README.md` in the same change: the query syntax summary from [contracts/query-syntax.md](contracts/query-syntax.md), the three commands, exit codes, the shell-quoting note, and that `.kb/index.db` and `.kb/state.json` are disposable and git-ignored. Update the status line to "milestone M2 (entry format, search)". Remove the "kb prints no color" statement if it appears.
- [X] T057 [P] Update `.github/workflows/ci.yml` so the existing perf job also runs the ignored M2 perf tests and uploads nothing; keep fmt, clippy and tests on both OSes. Confirm the 1.88 MSRV job still builds with the two new dependencies.
- [X] T058 [P] Add `tests/fixtures/search/` to the round-trip and lint fixture tests (`tests/lint_fixtures.rs`, `tests/roundtrip.rs`) so the new fixtures also lint clean and round-trip byte for byte.
- [X] T059 Run `cargo tree -e normal --depth 1` and confirm the only new direct dependencies are `rusqlite` and `unicode-normalization`, and that no crate in the tree does network I/O (constitution III, IX). Record the result under "Post-implementation re-check" in `specs/002-index-search/plan.md` with the measured numbers from T055.
- [X] T060 Run every command in [quickstart.md](quickstart.md) against a release build and fix any drift between the document and the behavior (or the other way around); verify the fixture KB never gets files written outside `.kb/`.
- [X] T061 Final gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --workspace --release -- --ignored`. Re-check each constitution principle against the finished code and write the result in the plan's re-check section.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 first; T002 to T005 in parallel after it; T006 needs T005 and T001.
- **Foundational (Phase 2)**: needs Phase 1. Tests T007 to T011 in parallel, then T012 to T014 in parallel, then T015 → T016 → T062 → T017 → T018 → T063.
- **US1 (Phase 3)**: needs Phase 2. **US2 (Phase 4)** needs Phase 2 and the `Kb::search` entry point from US1 (T026, T030) to exercise it from the CLI; its core sync tests (T032, T034) can start after Phase 2.
- **US3, US4 (Phases 5, 6)**: need US1 (search and CLI); independent of each other and of US2, but both extend `crates/kb-core/src/search.rs`, so run them one after another unless two people coordinate on that file.
- **US5 (Phase 7)**: needs Phase 2 (index) and T038 (open sequence); independent of US3 and US4.
- **Polish (Phase 8)**: needs the stories you want shipped; T055 needs US1 to US4 and US5 for `reindex`.

### Within Each Story

- Tests are written and seen failing before implementation (Constitution VIII).
- Core before CLI; `query.rs` before `search.rs`; `search.rs` before `main.rs`/`output.rs`.
- A task that edits `search.rs` or `main.rs` or `output.rs` is not marked [P] against other tasks touching the same file.

### Parallel Opportunities

- Phase 1: T002, T003, T004, T005.
- Phase 2: T007, T008, T009, T010, T011 together; then T012, T013, T014. T062 (before T017) and T063 (after T018) were added after the first numbering, so their IDs are out of order.
- US1 tests: T019, T020, T021, T022, T023, T024 together (different files, except T020 and T021 share `search.rs`; write them in one sitting).
- US2 tests: T032, T033, T034, T035 together.
- US5 tests: T051, T052 together.
- Polish: T055 to T058 together.

### Parallel Example: User Story 1 tests

```text
Task: "Unit tests for Query::parse in crates/kb-core/src/query.rs"        (T019)
Task: "Table test in tests/search_table.rs over queries.tsv"              (T022)
Task: "CLI tests in crates/kb/tests/cli_search.rs"                        (T023)
Task: "Color gate tests in crates/kb/src/color.rs"                        (T024)
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 (including the T006 measurement) and Phase 2.
2. Phase 3: US1. **Stop and validate**: `kb search 5xx auth` on the fixture KB, `--ids`, `--json`, exit codes.
3. This alone is a usable search tool; the index is rebuilt from scratch only when absent.

### Incremental Delivery

1. Setup + Foundational → US1 (MVP).
2. US2 → search can be trusted after edits (the other P1). Ship here if you want the P1 set.
3. US3 → filters. US4 → typos. US5 → status/reindex.
4. Polish: performance numbers, README, CI.

### Notes

- The spec's `authz` example was replaced by `authori` (see
  [research R10](research.md#r10-spec-defect-authz--authorizer)); T004 and T047 use `authori` and
  `authroizer`.
- Commit after each task or logical group, Conventional Commits (`feat(core): …`,
  `feat(search): …`, `test: …`).
- Every task that changes behavior also keeps `cargo clippy -D warnings` green.
