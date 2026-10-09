# Contract: `.kb/state.json`

Per-machine usage statistics. Git-ignored through `.kb/.gitignore`. Not an entry, not part of the
KB's portable content. Deleting it loses only usage history.

```json
{
  "state_version": 1,
  "usage": {
    "sumo/5xx-by-authorizer": {
      "count": 7,
      "last_used": 1791460800,
      "hash": "9ae16a3b2f90404f"
    }
  }
}
```

| Key | Type | Meaning |
|-----|------|---------|
| `state_version` | integer | `1` in M2. A larger value is treated as invalid (see below). |
| `usage` | object | Keyed by entry ID. |
| `usage.<id>.count` | integer ≥ 1 | Times recorded. |
| `usage.<id>.last_used` | integer | Unix seconds, UTC. |
| `usage.<id>.hash` | string, 16 hex digits | FNV-1a 64 of the entry file's bytes, to follow renames. |

## Rules

- **Unknown top-level keys are preserved** on every rewrite. Later milestones add their own keys
  (remembered variable values in M5); M2 must not drop them.
- **Missing file**: empty statistics, no warning.
- **Invalid JSON, wrong shape, or `state_version` greater than known**: copy the file to
  `state.json.bak` (overwriting an older backup), continue with empty statistics, and emit
  `Notice::StateReset`. It is reported once: while `state.json.bak` already holds the same bytes
  as the invalid file, later runs stay silent (and still use empty statistics). Nothing is written to `state.json` until the next
  `record_use`.
- **Writes** are temp file + atomic rename inside `.kb/`. A crash leaves the old file intact.
- **Entries with no file** are removed from `usage` at the next full rebuild; between rebuilds
  they stay (a file moved out and back keeps its history).
- Contains no entry content and, by construction, no secrets (constitution VI).
