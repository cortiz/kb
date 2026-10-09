# Contract: `kb use`, `kb vars`, `kb copy`

Shared argument group `VarArgs` (flattened into `use` and `copy`):

| Flag | Meaning |
|---|---|
| `--var NAME=VALUE` | repeatable; splits at the first `=`; last one for a name wins |
| `-y`, `--yes` | never ask; use pre-filled values |
| `--print` | write the resolved text to stdout instead of the clipboard |
| `--no-remember` | do not write memory (use is still recorded) |
| `--method M` | as M4 (`copy` and `use`) |
| `--json` | print the result as JSON |

## `kb use <id|query...>`

Resolves the target like `kb copy` (M4 FR-020: exact id, `.md`/path, search with clear winner). Close matches open
the picker when there is a terminal (stderr first says `--var/--yes are ignored when choosing interactively` if
they were given; the form collects values; `--print`, `--method`, `--no-remember` carry over); without a
terminal they are listed and the exit is 1; no match exits 1. Then:

1. Plan and pre-fill (FR-010) with flags and `std::env`.
2. Unknown `--var` name → stderr `kb: --var nope: the entry has no such variable; it uses: env, minutes`,
   exit 2. Malformed (no `=`) → exit 2.
3. Missing = no value after pre-fill (an invalid declared default does not count as a pre-fill). If any variable is missing or the flag value is invalid and
   (`--yes` or no terminal): print `kb: missing values:` then one line per variable
   (`  minutes  number  (no flag, env, remembered value or default)`), copy nothing, exit 1. Invalid
   flag values: `kb: invalid: …` lines (FR-006 text), exit 2.
4. Otherwise, for each variable not given by a flag, prompt on `/dev/tty` (R8):
   `env (enum: prod|stage|dev) [prod]: ` Enter accepts the pre-fill; an invalid answer re-asks with the
   reason; secrets are masked; Ctrl-C/EOF → exit 130, nothing copied.
5. Resolve (engine), copy via `clipboard::copy` (or print), record use + memory, report.

Output: the M4 confirmation line on stderr (`Copied "title" via pbcopy`). If a secret was substituted
and the text went to the clipboard, add: `The clipboard now holds a secret; clipboard managers may keep a history.`
`--var` for a `secret` variable adds, once: `kb: warning: --var token=… may be saved in your shell history; use the variable's env source instead.`

`--json`:
```json
{ "id": "…", "ok": true, "method": "native", "bytes": 123, "secret": false,
  "variables": [ { "name": "env", "kind": "enum", "source": "flag", "value": "prod" },
                 { "name": "token", "kind": "secret", "source": "env", "value": null } ],
  "text": "…only with --print…" }
```
`fields` holds the other resolved strings (`cwd`, `range`, `url`, `log_groups[i]`) with secrets masked.
With `--print --json` the document includes `text`; a secret substituted into it appears in `text`
only (that is the point of `--print`), never in `variables` or `fields`. The same holds when the
clipboard chain falls back to the `stdout` method (or `clipboard.method = "stdout"`): that method
prints the text, so the secret is on stdout (and the JSON `text`), and nowhere else; stderr says why
nothing was copied. Pinned by `tests/secrets_scan.rs`.

Exit codes (FR-024): `0` ok; `1` missing values non-interactively, not found, ambiguous, or copy
refused; `2` unknown/malformed/invalid `--var`; `130` cancelled.

## `kb vars <id|query...>`

Text, one block per variable in field order (an invalid declared default is shown as `default: x (invalid, ignored)`):
```
env      enum    prod|stage|dev   default: stage   env: KB_ENV (unset)   remembered: prod   → prod  (remembered)
minutes  number  default: 15                                                              → 15    (default)
token    secret  env: SUMO_TOKEN (set)                                                    → ••••  (from $SUMO_TOKEN)
```
`--json`: `{ "id", "variables": [ { "name", "kind", "options", "default", "env": {"name","set"},
"remembered", "prefill": {"value","source"}, "declared", "escape", "locations" } ] }`. For `secret`:
`default`, `remembered` and `prefill.value` are null. Exit 1 for not found/ambiguous. Does not record a
use. An entry with no variables prints `no variables` (exit 0).

## `kb copy`

Same flags and behavior as `use` for entries with variables (FR-021). `--raw` copies the unresolved
payload (M4 behavior), also when it opens the picker (picking an entry copies its template, no form),
and conflicts with `--var`, `--yes`, `--no-remember` (usage error, exit 2).
Entries without variables are unchanged. `kb copy` keeps its M4 `--json` shape plus the fields above
when variables were resolved.

## Post-pick from the UI

`PostPick::Copy` and `Print(Payload)` use the values from `Outcome::Resolved` and the same
copy/print/remember path; `Outcome::Picked` for an entry with no variables is unchanged.
