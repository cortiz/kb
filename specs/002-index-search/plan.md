# Implementation Plan: Index and Search

**Branch**: `002-index-search` | **Date**: 2026-10-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/002-index-search/spec.md`

## Summary

M2 adds a disposable SQLite FTS5 index at `.kb/index.db`, a usage file at `.kb/state.json`, a query
language, a ranking function, and the commands `kb search`, `kb reindex` and `kb index status`.

Four decisions carry the design (details and rejected alternatives in [research.md](research.md)):

1. **Freshness by stat-walk.** Every search walks the KB, compares `(mtime, size)` per file with the
   index, and re-reads only files that differ (plus "racily clean" files modified within 2 s of the
   last sync). Renames need no special index logic: an old ID disappears and a new one appears.
2. **Usage survives renames through content hashes in `state.json`**, not through the index. The
   index stays purely derived, so deleting it can never lose usage (constitution II).
3. **Typo tolerance without a fuzzy index.** FTS5 answers exact and prefix matching. For terms of
   five or more characters, one-edit variants are found in a persisted `vocab` table in Rust and
   are added as extra exact terms. This is skipped when exact and prefix matches already fill the
   requested limit, because typo matches always rank below them.
4. **Ranking is a sort key, not one blended number**: exact-title match first, then "all terms
   exact or prefix" before "some term by typo", then relevance multiplied by a bounded frecency
   factor, then ID. The bounded factor is what makes FR-014 provable.

A full rebuild runs in place inside one write transaction, so concurrent readers keep their
snapshot until it commits and never see a half-built index. Incremental updates run in short `BEGIN IMMEDIATE`
transactions with a 1 s busy timeout (FR-004).

**Spec defect (resolved)**: User Story 4 originally used `authz` as a prefix example, which is
neither a prefix nor within one edit of `authorizer` ([research R10](research.md#r10-spec-defect-authz--authorizer)).
The spec now uses `authori` and records that abbreviations are not matched.

## Technical Context

**Language/Version**: Rust stable, edition 2024, `rust-version = "1.88"` (unchanged from M1).

**Primary Dependencies** (new in M2; each justified in [research.md](research.md)):

- `kb-core`: `rusqlite` 0.37 with `bundled` (SQLite with FTS5, mandated by constitution II and
  Technical Constraints), `unicode-normalization` 0.1 (accent and case folding of query text on
  the Rust side, to match SQLite's `unicode61` tokenizer).
- `kb`: nothing new. Color uses plain ANSI escapes and `std::io::IsTerminal`.
- Dev only: nothing new (`tempfile`, `assert_cmd` already present).

**Storage**: `.kb/index.db` (SQLite, WAL, `user_version` = index version) and `.kb/state.json`
(JSON, `state_version`). Both live under the `.kb/` directory that M1's `kb init` already
self-ignores with `.kb/.gitignore`. Entry files are never written.

**Testing**: `cargo test`. Unit tests beside the code. A search fixture KB in
`tests/fixtures/search/` with a query table `queries.tsv` (at least 40 rows, SC-004). CLI tests with
`assert_cmd`. A concurrency test (SC-005). An ignored-by-default performance test in release mode
(SC-001 to SC-003), run in CI as M1's is.

**Target Platform**: macOS arm64 and x86_64, Linux x86_64 and aarch64 (unchanged).

**Project Type**: Cargo workspace, library `kb-core` plus binary `kb` (unchanged).

**Performance Goals**: search ≤ 50 ms end to end on 10,000 unchanged entries; ≤ 100 ms with one
changed file; full rebuild ≤ 5 s (SC-001 to SC-003). The warm path is process start, one directory
walk with a `stat` per file, and one or two indexed queries. Risk and fallback in
[research.md R6](research.md#r6-warm-path-cost).

**Constraints**: no network, no daemon, no telemetry. Writes only inside `.kb/`. A read-only `.kb/`
degrades to an in-memory index. `kb search` never records usage (FR-017).

**Scale/Scope**: ~10,000 entries of up to 1 MB. 3 new commands, 1 new on-disk format
(`state.json`), 1 derived store (`index.db`).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| # | Principle | Status | How this plan complies |
|---|-----------|--------|------------------------|
| I | Plain Files Are the Source of Truth | Pass | Entry files are only read. M2 writes only `.kb/index.db` (+ `-wal`, `-shm`, temp build files) and `.kb/state.json` (+ `.bak`, temp file), all inside `.kb/`. |
| II | The Index Is Disposable | Pass | SQLite + FTS5 at `.kb/index.db`, git-ignored by `.kb/.gitignore`. No migrations: a `user_version` mismatch, corruption or absence triggers a rebuild. Usage lives only in `state.json`; the index holds no data that cannot be recomputed from files (SC-006). |
| III | Local-Only, No Server | Pass | No daemon: freshness is checked on demand. No network code. `rusqlite` bundled does no I/O beyond files. |
| IV | Terminal Only, One Core | Pass | Query parsing, index, sync, ranking, highlighting ranges, snippets, usage and status all live in `kb-core`. The `kb` crate parses flags, calls core, prints, and colors. Core returns notices as data instead of printing. Every command supports `--json`. |
| V | Portable by Construction | Pass | Nothing OS-specific is added: SQLite WAL works on both targets. Paths in the index are the `/`-separated relative paths M1 already produces. The in-memory fallback covers read-only mounts. |
| VI | Secrets Are Never Stored | Pass | Entries with secret findings are lint errors and are therefore not indexed, so a leaked secret is never copied into `index.db`. `state.json` stores counts, times and hashes only. |
| VII | Keyboard-First and Fast | Pass (budgeted) | Search ≤ 50 ms at 10,000 entries with a CI performance test. Fuzzy expansion is skipped when it cannot change the visible page. Risk R6 names the fallback. |
| VIII | Test-First Core | Pass | Tasks put failing tests first: query-parser table, ranking table (SC-004), freshness scenarios, concurrency. Fixture KB covers every type. CI unchanged (fmt, clippy `-D warnings`, tests on both OSes). |
| IX | Simplicity First | Pass | Two new dependencies, both justified. No watcher, no background work, no custom fuzzy index, no stemming, no embeddings. Usage is a plain JSON file. M2 alone is a usable search tool. |

**Gate result**: pass, no exceptions. Complexity Tracking stays empty.

**Post-design re-check**: still pass. The Phase 1 design adds no dependency beyond the two
above, no process, and no file outside `.kb/`.

**Post-implementation re-check (2026-10-09)**: still pass.

- I: entry files are only read; everything kb writes is inside `.kb/` (`cli_search.rs` asserts the
  root listing is unchanged apart from `.kb`).
- II: `ensure_kb_dir` creates `.kb/.gitignore` (`*`) whenever `.kb/` is created, including from
  `kb search` on a KB that never ran `kb init`. A different `user_version`, a garbage file or a
  missing file triggers a rebuild; a rebuild never renames a file over `index.db` (WAL safety).
- III: `cargo tree -e normal --depth 1 -p kb-core` shows `rusqlite` and `unicode-normalization` as
  the only new direct dependencies; no networking crate in the tree.
- IV: the CLI only parses flags, calls `Kb`, prints and colors; core returns `Notice`s as data.
- V: nothing OS-specific added; tests that need unix permissions are `cfg(unix)`.
- VI: files with secret findings are lint errors and are not indexed.
- VII (release build, macOS arm64, 10,000 entries): warm search 40 ms, after one edit 43 to 48
  ms, typo query 37 ms, full rebuild 0.8 s, `kb --version` 3 ms (after adding the `vocab` table;
  before it, typo queries took 61 ms). The warm path has only ~10 ms of headroom
  against the 50 ms budget (research R6 names the fallback).
- VIII: **not followed to the letter.** Unit tests were written alongside or just after the code in
  most modules, not seen failing first. The coverage the principle wants is there (fixture KB of
  every type, the 40+ row query table, freshness, concurrency, fuzzing, byte-for-byte round trips
  of the new fixtures), and `fmt --check`, `clippy -D warnings`, `cargo doc` with warnings denied
  and the full suite on stable and on 1.88 are clean. Linux CI has not run yet.
- IX: two dependencies, no watcher, no vocabulary table, no background work.

## Project Structure

### Documentation (this feature)

```text
specs/002-index-search/
├── plan.md              # This file
├── research.md          # Phase 0: decisions, rejected alternatives, spec defect
├── data-model.md        # Phase 1: entities, index schema, state file
├── quickstart.md        # Phase 1: runnable validation scenarios
├── contracts/
│   ├── cli.md                       # kb search / reindex / index status
│   ├── query-syntax.md              # grammar and semantics of the query language
│   ├── core-api.md                  # kb-core API used by the CLI, TUI (M4) and picker (M9)
│   ├── state-file.md                # .kb/state.json format
│   ├── search-report.schema.json    # kb search --json
│   └── index-status.schema.json     # kb index status --json, kb reindex --json
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
Cargo.toml                    # + rusqlite, unicode-normalization in [workspace.dependencies]

crates/kb-core/src/
├── lib.rs                    # + pub mod fold, query, index, usage, search
├── fold.rs                   # fold(): lowercase + strip accents; words(): split like unicode61
├── query.rs                  # Query, Filter, Field; parse(&str); Query::from_parts(flags)
├── usage.rs                  # State file: load, save atomically, record_use, rename carry-over, prune
├── index/
│   ├── mod.rs                # Index: open, version check, rebuild, status; IndexError
│   ├── schema.rs             # DDL, INDEX_VERSION, column weights
│   ├── sync.rs               # stat-walk diff, batched upserts, skip list, racy-clean rule
│   └── doc.rs                # Entry + lint verdict → row values (what FR-005 lists)
└── search.rs                 # Kb handle: search(), entry(), record_use(), reindex(), status();
                              # candidate query, fuzzy expansion, tiering, frecency, highlight, snippet

crates/kb/src/
├── main.rs                   # + Search, Reindex, Index{Status} subcommands
├── output.rs                 # + print_search (human, color), print_search_json, status/reindex output
└── color.rs                  # ANSI helper: enabled only if stdout is a TTY and NO_COLOR is unset

tests/
├── fixtures/search/          # fixture KB (every type, accents, typos targets, tags, vars) + queries.tsv
├── search_table.rs           # SC-004: runs queries.tsv through kb-core (registered as [[test]] in kb-core)
└── (crates/kb/tests/)
    ├── cli_search.rs         # exit codes, --ids, --json, color rules, notices
    ├── cli_index.rs          # reindex, index status, corrupt/missing/version-mismatch recovery
    ├── freshness.rs          # create/edit/rename/delete between searches; read-only .kb/
    ├── support/corpus.rs     # seeded generator of N valid entries (perf, concurrency)
    ├── concurrency.rs        # 50 concurrent searches during edits (SC-005)
    └── perf.rs               # extended: SC-001 to SC-003 (ignored by default)
```

**Structure Decision**: extend the two existing crates. `kb-core` gains the whole feature
behind one handle (`Kb`) so the TUI and picker call the same functions the CLI does; `kb` gains
only argument parsing and rendering. No new crate.

## Complexity Tracking

> Empty: no constitution violations.
