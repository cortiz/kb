# Contract: `kb search`, `kb reindex`, `kb index status`

Global flags, root resolution and the exit-code base (`0`, `1`, `2`) are defined in
[M1's CLI contract](../../001-entry-format/contracts/cli.md#global). Errors causing exit 2 are
printed `kb: <message>` on stderr and, with `--json`, as `{"error": {"kind", "message"}}` on
stdout. Kinds: `usage`, `config`, `io`.

## `kb search [query...] [--type T]... [--tag T]... [--not-tag T]... [--limit N] [--ids | --json]`

- `query...`: any number of arguments, joined with spaces and parsed per
  [query-syntax.md](query-syntax.md).
- `--type` (repeatable, OR), `--tag` (repeatable, AND), `--not-tag` (repeatable). An invalid
  `--type` name is exit 2 (`usage`) listing the valid types.
- `--limit N`: default 20; `0` = no limit; a non-number is exit 2.
- `--ids` and `--json` are mutually exclusive (clap usage error, exit 2).
- Before answering, the index is brought up to date (see [research R2](../research.md#r2-freshness-check-fr-002)).
  `kb search` never records usage (FR-017) and never writes outside `.kb/`.

### Exit codes (FR-022)

| Code | When |
|------|------|
| 0 | at least one result |
| 1 | no results (including an empty KB) |
| 2 | usage, configuration or I/O error |

### Human output (stdout is a terminal or a pipe without `--ids`/`--json`)

One line per result:

```text
sumo  sumo/5xx-by-authorizer  5xx by authorizer  #auth #lambda
cw    cw/lambda-errors        Lambda errors       #prod #lambda
```

- Columns: type, ID, title, tags (as `#tag`), separated by two spaces. Types are padded to the
  widest type in the page so IDs align.
- Matched words are highlighted whole (a prefix match highlights the full word it matched) in the title, ID and tags columns: bold yellow, only when stdout is
  a terminal and `NO_COLOR` is unset or empty (FR-019). Never any escape code otherwise.
- Matches that occur only in the payload or notes are not shown in the line (the JSON form
  carries the snippet).

### `--ids`

One ID per line, no other output on stdout, no escape codes.

```sh
kb search auth --ids | fzf
```

### `--json`

One document on stdout, validated by [search-report.schema.json](search-report.schema.json):

```json
{
  "query": "5xx auth",
  "total": 1,
  "results": [
    {
      "id": "sumo/5xx-by-authorizer",
      "type": "sumo",
      "title": "5xx by authorizer",
      "tags": ["auth", "lambda"],
      "path": "sumo/5xx-by-authorizer.md",
      "score": 41.7,
      "tier": "exact",
      "matches": [
        {"field": "title", "ranges": [[0, 3], [7, 17]]},
        {"field": "tags", "ranges": [[0, 4]]}
      ],
      "snippet": {"field": "title", "text": "5xx by authorizer", "ranges": [[0, 3], [7, 17]]}
    }
  ]
}
```

- `ranges` are `[start, end)` in characters (Unicode scalar values) of the field text. Fields:
  `title`, `tags` (tags joined by single spaces), `id` (the ID with separators turned into
  spaces), `meta`, `payload`, `notes`.
- `snippet.text` is at most 160 characters. `snippet` is `null` for filter-only and empty
  queries.
- `total` is the number of matches before `--limit`, capped at 1,000.
- With zero results the document has `"results": []` and the exit code is still 1.

### stderr lines (never on stdout)

At most one line per kind, in this order:

1. `kb: index rebuilt (N entries)` — index was missing, corrupt or a different version
   (US2 scenarios 3, 4).
2. `kb: warning: .kb/ is not writable; …` — in-memory fallback.
3. `kb: warning: state.json was invalid; …` — usage reset.
4. `kb: unknown filter "foo:"; valid filters: type, tag, -tag, var, repo, in`
5. `kb: N entries skipped due to errors (run kb lint)` — only when N > 0 (edge case 1).
6. `kb: no matches` — zero results with a non-empty KB.
7. `kb: no entries in KB (run kb init)` — zero indexed and zero skipped files (exit 1).

## `kb reindex [--json]`

Deletes and rebuilds the index from the files, keeps `state.json` except that usage records for
entries that no longer exist are pruned (FR-018). Rebuilds in place in one transaction, so a
concurrent search never sees a partial index.

Human output on stdout: `Reindexed 284 entries (3 skipped due to errors) in 0.41 s`. With
`--json`: the document of [index-status.schema.json](index-status.schema.json) plus
`"duration_ms"`. Exit 0 on success, 2 on I/O error. Works when `.kb/` does not exist (it is
created) and fails with exit 2 when `.kb/` cannot be written.

## `kb index status [--json]`

Reports the index as it is on disk; does not sync and does not create the index.

```text
Index:      /home/carlos/kb/.kb/index.db
Version:    1
Entries:    284 indexed, 3 skipped due to errors
Size:       1.8 MiB
Last build: 2026-10-08 12:30:05 UTC
```

- No index file: `Index: none (it is built on the next search, or run kb reindex)`, counts `0`,
  exit 0.
- Corrupt or version-mismatched file: reported as `Index: unusable (<reason>); it is rebuilt on
  the next search`, exit 0.
- `--json`: [index-status.schema.json](index-status.schema.json). Exit 0, or 2 on I/O error.

## Documentation

`README.md` gains a "Searching" section with the query syntax summary and the three commands, in
the same change (constitution, Development Workflow).
