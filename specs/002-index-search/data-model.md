# Data Model: Index and Search

Types live in `kb-core`. M1 types (`Entry`, `EntryType`, `Finding`, `WalkEntry`) are reused as is.

## Query

| Field | Type | Notes |
|-------|------|-------|
| `terms` | `Vec<String>` | Free-text words, as typed. Stop words already removed. |
| `phrases` | `Vec<String>` | Exact phrases (FR-008). |
| `types` | `Vec<EntryType>` | OR between them (FR-007). |
| `tags` | `Vec<String>` | AND; normalized like entry tags (lowercase, no leading `#`). |
| `not_tags` | `Vec<String>` | Exclusion. |
| `vars` | `Vec<String>` | Entries that use every named variable. |
| `repos` | `Vec<String>` | `owner/name`; matches `code.repo`. |
| `scope` | `Option<Field>` | `Title`, `Payload` or `Notes`, from `in:`. Applies to terms and phrases. |
| `limit` | `usize` | Default 20; `0` means no limit. |

`is_empty()` is true when there are no terms and no phrases (filters may be present): the result
is ordered by frecency. `Query::parse(&str)` and `Query::from_parts(...)` (flags) build the same
value for the same meaning (FR-025). An unknown `key:value` token becomes free text and is also
reported as a `Notice::UnknownFilter`.

## SearchOutcome and SearchResult

```text
SearchOutcome { results: Vec<SearchResult>, total: usize, notices: Vec<Notice> }
```

`total` is the number of matches before `limit`, capped at the candidate cap (1,000); the CLI does
not print it, the TUI may.

| SearchResult field | Type | Notes |
|--------------------|------|-------|
| `id` | `String` | Entry ID. |
| `entry_type` | `EntryType` | |
| `title` | `String` | |
| `tags` | `Vec<String>` | |
| `path` | `String` | Relative path with `/`, including `.md`. |
| `score` | `f64` | Final score (text relevance × frecency factor). Not comparable across queries. |
| `tier` | `Tier` | `TitleExact`, `Exact`, `Typo`. |
| `matches` | `Vec<FieldMatch>` | One per field that matched, in weight order. |
| `snippet` | `Option<Snippet>` | ≤ 160 characters. `None` for filter-only queries. |

`FieldMatch { field: Field, ranges: Vec<Range<usize>> }` where `Field` is `Title | Tags | Id |
Meta | Payload | Notes` and ranges are `[start, end)` in characters of that field's text as
stored. `Snippet { field: Field, text: String, ranges: Vec<Range<usize>> }` with ranges relative
to `text`.

## Notice

Data the core returns so front ends decide how to show it (constitution IV). The CLI prints each
as one stderr line.

| Variant | Meaning | CLI text |
|---------|---------|----------|
| `Rebuilt { entries }` | Index was missing, corrupt or a different version | `kb: index rebuilt (N entries)` |
| `ReadOnly` | `.kb/` not writable; in-memory index used | `kb: warning: .kb/ is not writable; using a temporary in-memory index, searches will be slow` |
| `Skipped { count }` | Entries left out because of lint errors | `kb: N entries skipped due to errors (run kb lint)` |
| `UnknownType { name }` | `type:` value is not a type | `kb: unknown type "x"; valid types: sql, sumo, cw, cmd, prompt, link, code` |
| `UnknownFilter { key }` | `key:` not a filter | `kb: unknown filter "foo:"; valid filters: type, tag, -tag, var, repo, in` |
| `StateReset` | `state.json` invalid; reset | `kb: warning: state.json was invalid; usage statistics were reset (copy kept as state.json.bak)` |

## Index (`.kb/index.db`)

`PRAGMA user_version` = `INDEX_VERSION` (1 in M2). Any other value, or any SQLite error on open,
means rebuild. All tables are derived from the entry files.

```text
meta(key TEXT PRIMARY KEY, value TEXT)
    -- 'last_full_build' (unix seconds), 'last_sync_start_ns', 'kb_root'

files(id TEXT PRIMARY KEY, path TEXT, mtime_ns INTEGER, size INTEGER, hash INTEGER,
      status TEXT)            -- 'ok' | 'skipped'; skipped rows have no other table rows
      -- hash is FNV-1a 64 stored as signed integer; the fingerprint of FR-002 / Key Entities

entries(id TEXT PRIMARY KEY REFERENCES files(id), type TEXT, title TEXT, title_fold TEXT,
        code_repo TEXT)       -- INDEX on title_fold, type, code_repo
entry_tags(id TEXT, tag TEXT, PRIMARY KEY(tag, id))
entry_vars(id TEXT, name TEXT, PRIMARY KEY(name, id))

fts USING fts5(title, tags, id, meta, payload, notes,
               tokenize='unicode61 remove_diacritics 2', prefix='2 3')
    -- rowid = entries.rowid; content stored (needed by highlight())
fts_vocab USING fts5vocab(fts, 'row')   -- source for vocab at a full rebuild
vocab(term TEXT PRIMARY KEY, len INTEGER)   -- INDEX on len; every indexed word, for typo expansion;
                                            -- only ever added to between full rebuilds
```

Invariants (checked by a test after random create/edit/rename/delete sequences):

- every `files` row with `status='ok'` has exactly one `entries` row and one `fts` row;
- every `entries` row has a `files` row; no row exists for an ID without a file on disk after sync;
- rows for one ID change in a single transaction.

## Fingerprint and sync decision

For each walked file, with `prev` the `files` row if any:

| Condition | Action |
|-----------|--------|
| no `prev` | read, hash, parse, lint verdict, insert |
| `prev.size` or `prev.mtime_ns` differs | read, hash; if hash equals `prev.hash`, only update mtime; else re-parse and replace rows |
| equal but `mtime_ns ≥ last_sync_start_ns − 2 s` (racy) | read and hash, as the row above |
| equal and not racy | nothing |
| `prev` exists, no file on disk | delete all rows for the ID |
| case-insensitive duplicate ID (KB053) | both `skipped` |

## Entry row construction (FR-005)

| FTS column | Content |
|------------|---------|
| `title` | entry title |
| `tags` | tags joined by spaces |
| `id` | ID with `/ - _ .` replaced by spaces |
| `meta` | variable names and descriptions; `dialect`, `database`, `log_groups`, `region`; `url` host and path; `code.repo`, `code.path`, `code.symbol` (each only if present) |
| `payload` | payload text (with `\{{` unescaped, as M1 returns it) |
| `notes` | notes text |

## UsageRecord and State (`.kb/state.json`)

Format in [contracts/state-file.md](contracts/state-file.md).

| Field | Type | Notes |
|-------|------|-------|
| `count` | `u64` | Times recorded. |
| `last_used` | `i64` | Unix seconds, UTC. |
| `hash` | `u64` (hex string in JSON) | FNV-1a 64 of the entry's file bytes, refreshed by sync when the file changes. Used to follow renames. |

State transitions (all in `usage.rs`):

- `record_use(id)` → count +1, `last_used = now`, hash recomputed from the file.
- sync, vanished ID with a record + new file with equal hash → record moves to the new ID.
- full rebuild → records whose ID has no file are removed (FR-018).
- Unknown top-level keys in `state.json` are preserved on write (later milestones add variable
  memory there).

## Frecency

`frecency(record, now) = (1 − e^(−count/5)) × 2^(−(now − last_used)/(30 days))`, in `[0, 1)`;
`0` when there is no record. Pure function, `now` injected.

## IndexStatus

| Field | Type |
|-------|------|
| `entries_indexed` | `usize` |
| `entries_skipped` | `usize` |
| `size_bytes` | `u64` (db + wal) |
| `index_version` | `u32` |
| `last_full_build` | `Option<i64>` (unix seconds; `None` for an in-memory index) |
| `path` | `String` |
| `in_memory` | `bool` |

`kb index status` does not sync first; it reports the index as it is on disk (a missing index
reports zeros and a hint to search or reindex). It does not create the index.
