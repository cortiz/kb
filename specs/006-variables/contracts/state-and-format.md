# Contract: format, lint and state additions

## `escape` in `kb_format: 1`

```yaml
vars:
  branch: { kind: text, escape: shell }
  q:      { kind: text, escape: none }
```

- Optional; values `auto` (default), `none`, `shell`, `sql`. Allowed on any kind (on `secret` too).
- Lint **KB054 `invalid-escape`** (error): unknown value or a non-scalar. Suggestion: closest of the four.
- `escape` is carried in `entry_json` for each variable (additive; consumers that ignore it keep working).
- Round trip: the key is preserved byte for byte like every other (M1 FR-016); the parser does not
  rewrite files.
- No other lint rule changes. `KB048` (unused) and `KB047` (undeclared) keep working.

## `.kb/state.json`

```json
{
  "state_version": 1,
  "usage": { "…": { "count": 3, "last_used": 1760000000, "hash": "ab12…" } },
  "vars": {
    "entries": { "<id>": { "<name>": "<value>" } },
    "recent":  { "<name>": ["<newest>", "…"] }
  }
}
```

- `vars` is optional on read (absent or malformed → empty; `usage` is unaffected).
- Never contains `secret` variables, and never a value `scan_secrets` flags.
- Written atomically with `usage` in the same save (temp file + rename, as M2).
- A build that does not know `vars` keeps it (`extra`); deleting the file loses only usage and
  remembered values (constitution II).

## Config

New setting `vars.remember` (the eleventh; the M4 config contract and `kb doctor` list are updated with it) (bool, default `true`, env `KB_VARS_REMEMBER`). It goes in the M4 `SETTINGS`
table, so `config init|get|set|list` and the validation warnings cover it with no extra code.
`--no-remember` overrides it for one run.
